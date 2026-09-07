use serde_json::Value;
use std::time::Duration;

const MAX_429_RETRIES: usize = 5;
const MAX_NETWORK_RETRIES: usize = 2;
const DEFAULT_RATE_LIMIT_PER_MIN: u32 = 10;

/// Items requested per `tickets.getAll` call on completeness paths
/// (backup, restore verification, milestone progress, project_context).
/// Deliberately high: these results are processed server-side and never
/// dumped into an LLM context whole. Override for huge instances via
/// `LEANTIME_MCP_FETCH_LIMIT`; a non-parsable value falls back to the
/// default. The real backstop is the 64 MB streaming response cap.
pub const DEFAULT_FETCH_LIMIT: usize = 10_000;

/// Effective completeness fetch limit (env override, no numeric clamp —
/// admin-controlled, same trust level as LEANTIME_URL).
pub fn fetch_limit() -> usize {
    std::env::var("LEANTIME_MCP_FETCH_LIMIT")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .filter(|n| *n > 0)
        .unwrap_or(DEFAULT_FETCH_LIMIT)
}

/// Leantime JSON-RPC client: adaptive 429 retry, 502/503/504 retry,
/// status-map and user caches, response size cap.
pub struct LeantimeClient {
    base_url: String,
    api_key: String,
    rpc_id: u64,
    http: reqwest::Client,
    discovered_rate_limit: Option<u32>,
    status_cache: std::collections::HashMap<String, Value>,
    user_cache: Option<(Vec<Value>, std::time::Instant)>,
}

/// Error returned by the Leantime JSON-RPC client. Every variant carries an
/// actionable, human-readable message (surfaced verbatim to tool callers).
#[derive(Debug, thiserror::Error)]
pub enum ApiError {
    /// The request could not be sent or the response not be read.
    #[error("Network error: {0}")]
    Network(String),
    /// A malformed JSON body came back with a successful status.
    #[error("Parse error: {0}")]
    Parse(String),
    /// Non-2xx HTTP status after retries.
    #[error("Leantime API error: {status} {reason}")]
    Http {
        /// HTTP status code.
        status: u16,
        /// Canonical reason phrase (best effort).
        reason: String,
    },
    /// JSON-RPC level error returned with a 200.
    #[error("Leantime RPC error [{code}]: {message}{data}")]
    Rpc {
        /// JSON-RPC error code.
        code: i64,
        /// Error message from the instance.
        message: String,
        /// Data suffix, already carrying its " — " separator ("" when absent).
        data: String,
    },
    /// 429 persisted beyond the retry budget.
    #[error("Rate limit exhausted after {retries} retries{waited} — the instance allows ~{limit} req/min. Wait ~60 seconds or reduce the batch size.")]
    RateLimit {
        /// Number of retries performed.
        retries: usize,
        /// Pre-built "(waited ~Ns per retry)" clause ("" when never waited).
        waited: String,
        /// Discovered (or default) requests-per-minute allowance.
        limit: u32,
    },
    /// Response body exceeded the size cap.
    #[error("Response too large ({0} bytes, max 64 MB)")]
    TooLarge(u64),
    /// Defensive: the retry loop exited without returning.
    #[error("Unreachable state — please report this bug")]
    Unreachable,
}

impl LeantimeClient {
    /// Build a client for one Leantime instance.
    pub fn new(base_url: &str, api_key: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_string(),
            api_key: api_key.to_string(),
            rpc_id: 0,
            http: reqwest::Client::builder()
                .timeout(Duration::from_secs(30))
                .build()
                // TLS builder failures are virtually impossible with rustls;
                // falling back to the default client beats panicking in a library.
                .unwrap_or_default(),
            discovered_rate_limit: None,
            status_cache: std::collections::HashMap::new(),
            user_cache: None,
        }
    }

    fn inter_request_delay_ms(&self) -> u64 {
        let limit = self
            .discovered_rate_limit
            .unwrap_or(DEFAULT_RATE_LIMIT_PER_MIN);
        (60_000 / limit as u64).max(1000)
    }

    /// Call a Leantime JSON-RPC method (`method` without the `leantime.rpc.` prefix).
    pub async fn call(&mut self, method: &str, params: Value) -> Result<Value, ApiError> {
        self.rpc_id += 1;
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "method": format!("leantime.rpc.{}", method),
            "params": params,
            "id": self.rpc_id,
        });

        let url = format!("{}/api/jsonrpc", self.base_url);
        let mut network_retries = 0u32;
        let mut last_429_delay = 0u64;

        for attempt in 0..=(MAX_429_RETRIES + MAX_NETWORK_RETRIES) {
            let mut resp = self
                .http
                .post(&url)
                .header("x-api-key", &self.api_key)
                .json(&body)
                .send()
                .await
                .map_err(|e| ApiError::Network(e.to_string()))?;

            let status = resp.status().as_u16();

            if [502, 503, 504].contains(&status) {
                if (network_retries as usize) < MAX_NETWORK_RETRIES {
                    network_retries += 1;
                    let delay = if network_retries == 1 { 500 } else { 1000 };
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    continue;
                }
                return Err(ApiError::Http {
                    status,
                    reason: format!(
                        "{} (retried {} times)",
                        resp.status().canonical_reason().unwrap_or("?"),
                        MAX_NETWORK_RETRIES
                    ),
                });
            }

            if status == 429 {
                if self.discovered_rate_limit.is_none() {
                    if let Some(limit) = resp
                        .headers()
                        .get("X-RateLimit-Limit")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|v| v.parse::<u32>().ok())
                        .filter(|v| *v > 0)
                    {
                        // 0 would cause a division by zero downstream
                        self.discovered_rate_limit = Some(limit);
                    }
                }

                if attempt < MAX_429_RETRIES {
                    // Retry-After: seconds or HTTP-date; X-RateLimit-Retry-After: seconds only.
                    let header_delay = Self::parse_retry_after(&resp);
                    let rate_delay = self.inter_request_delay_ms();
                    last_429_delay = header_delay.unwrap_or(rate_delay);
                    tokio::time::sleep(Duration::from_millis(last_429_delay)).await;
                    continue;
                }

                let limit = self
                    .discovered_rate_limit
                    .unwrap_or(DEFAULT_RATE_LIMIT_PER_MIN);
                return Err(ApiError::RateLimit {
                    retries: MAX_429_RETRIES,
                    waited: if last_429_delay > 0 {
                        format!(" (waited ~{}s per retry)", (last_429_delay + 500) / 1000)
                    } else {
                        String::new()
                    },
                    limit,
                });
            }

            if !resp.status().is_success() {
                return Err(ApiError::Http {
                    status,
                    reason: resp.status().canonical_reason().unwrap_or("?").to_string(),
                });
            }

            // Bound the buffered response: a hostile instance must not be able
            // to OOM the process with a giant body. Chunked responses have no
            // content-length, so the cap is enforced WHILE accumulating, not
            // after the fact.
            const MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
            if let Some(len) = resp.content_length() {
                if len as usize > MAX_RESPONSE_BYTES {
                    return Err(ApiError::TooLarge(len));
                }
            }
            let mut body = Vec::new();
            loop {
                let chunk = resp
                    .chunk()
                    .await
                    .map_err(|e| ApiError::Network(e.to_string()))?;
                let Some(chunk) = chunk else { break };
                if body.len() + chunk.len() > MAX_RESPONSE_BYTES {
                    return Err(ApiError::TooLarge((body.len() + chunk.len()) as u64));
                }
                body.extend_from_slice(&chunk);
            }
            let json: Value =
                serde_json::from_slice(&body).map_err(|e| ApiError::Parse(e.to_string()))?;

            if let Some(err) = json.get("error") {
                return Err(ApiError::Rpc {
                    code: err.get("code").and_then(|c| c.as_i64()).unwrap_or(0),
                    message: err
                        .get("message")
                        .and_then(|m| m.as_str())
                        .unwrap_or("?")
                        .to_string(),
                    data: match err.get("data").and_then(|d| d.as_str()) {
                        Some(d) if !d.is_empty() => format!(" — {}", d),
                        _ => String::new(),
                    },
                });
            }

            return Ok(json.get("result").cloned().unwrap_or(Value::Null));
        }

        Err(ApiError::Unreachable)
    }

    /// Parse Retry-After / X-RateLimit-Retry-After headers into milliseconds.
    /// Handles both "seconds" format and HTTP-date format (RFC 7231).
    /// Server-controlled values are capped so a hostile instance cannot
    /// freeze the whole server with an absurd delay.
    fn parse_retry_after(resp: &reqwest::Response) -> Option<u64> {
        const MAX_DELAY_MS: u64 = 60_000;
        let raw = if let Some(ra) = resp
            .headers()
            .get("Retry-After")
            .and_then(|v| v.to_str().ok())
        {
            if let Ok(secs) = ra.trim().parse::<u64>() {
                Some(secs.saturating_mul(1000))
            } else if let Ok(date) = chrono::DateTime::parse_from_rfc2822(ra.trim()) {
                // HTTP-date format: delay = max(0, date - now)
                let now_ms = chrono::Utc::now().timestamp_millis();
                Some((date.timestamp_millis() - now_ms).max(0) as u64)
            } else {
                None
            }
        } else {
            // X-RateLimit-Retry-After: seconds only
            resp.headers()
                .get("X-RateLimit-Retry-After")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.trim().parse::<u64>().ok())
                .map(|s| s.saturating_mul(1000))
        };
        raw.map(|ms| ms.min(MAX_DELAY_MS))
    }

    /// Status map of a project (cached permanently per client).
    pub async fn get_status_map(&mut self, project_id: &str) -> Result<Value, ApiError> {
        if let Some(cached) = self.status_cache.get(project_id) {
            return Ok(cached.clone());
        }
        let result = self
            .call(
                "tickets.getStatusLabels",
                serde_json::json!({"projectId": project_id}),
            )
            .await?;
        self.status_cache
            .insert(project_id.to_string(), result.clone());
        Ok(result)
    }

    /// User list (cached 5 minutes).
    pub async fn get_users(&mut self) -> Result<Vec<Value>, ApiError> {
        if let Some((users, expires)) = &self.user_cache {
            if expires > &std::time::Instant::now() {
                return Ok(users.clone());
            }
        }
        let result = self.call("users.getAll", serde_json::json!({})).await?;
        let users = result.as_array().cloned().unwrap_or_default();
        self.user_cache = Some((
            users.clone(),
            std::time::Instant::now() + Duration::from_secs(300),
        ));
        Ok(users)
    }

    /// Status map key for a ticket's `status` field — numbers and numeric
    /// strings must both resolve to the same key ("3", not "\"3\"").
    fn status_key(v: &Value) -> String {
        match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        }
    }

    /// Add statusLabel/statusType/statusColor to each item, in place.
    pub fn enrich_with_statuses(&self, items: &mut [Value], status_map: &Value) {
        for item in items.iter_mut() {
            if let Some(status) = item.get("status") {
                let key = Self::status_key(status);
                if let Some(info) = status_map.get(&key) {
                    if let Some(obj) = item.as_object_mut() {
                        obj.insert(
                            "statusLabel".into(),
                            info.get("name").cloned().unwrap_or(Value::Null),
                        );
                        obj.insert(
                            "statusType".into(),
                            info.get("statusType").cloned().unwrap_or(Value::Null),
                        );
                        obj.insert(
                            "statusColor".into(),
                            info.get("class").cloned().unwrap_or(Value::Null),
                        );
                    }
                }
            }
        }
    }

    /// Enrich a single ticket/milestone with statusLabel/statusType/statusColor.
    pub async fn enrich_single_with_statuses(&mut self, item: &mut Value, project_id: &str) {
        if let Ok(sm) = self.get_status_map(project_id).await {
            let mut vec = vec![std::mem::take(item)];
            self.enrich_with_statuses(&mut vec, &sm);
            *item = vec.remove(0);
        }
    }
}
