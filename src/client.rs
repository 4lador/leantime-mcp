use serde_json::Value;
use std::time::Duration;

const MAX_429_RETRIES: usize = 5;
const MAX_NETWORK_RETRIES: usize = 2;
const DEFAULT_RATE_LIMIT_PER_MIN: u32 = 10;

/// RPC methods that only read state — safe to retry on transient 5xx,
/// because replaying them has no side effects. Default-deny: any method
/// not listed here is treated as a mutation and is not retried on 5xx.
const READ_METHODS: &[&str] = &[
    "comments.getComments",
    "projects.findProject",
    "projects.getProject",
    "projects.getProjectProgress",
    "sprints.getAllSprints",
    "sprints.getSprint",
    "tickets.getAll",
    "tickets.getAllOpenUserTickets",
    "tickets.getAllSubtasks",
    "tickets.getEffortLabels",
    "tickets.getKanbanColumns",
    "tickets.getPriorityLabels",
    "tickets.getStatusLabels",
    "tickets.getTicket",
    "tickets.getTicketTypes",
    "timesheets.getAll",
    "users.getAll",
];

/// True when the request body carries a known read-only method.
fn is_read_request(body: &Value) -> bool {
    body.get("method")
        .and_then(|m| m.as_str())
        .and_then(|m| m.strip_prefix("leantime.rpc."))
        .is_some_and(|m| READ_METHODS.contains(&m))
}

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

/// Stable identity key for a ticket across fetches — ids arrive as strings
/// or numbers depending on the API version.
fn id_key(item: &Value) -> String {
    match item.get("id") {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
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
    /// A transient 5xx arrived after a mutation was sent: the instance may
    /// or may not have applied the change. Deliberately not retried — a
    /// blind retry can duplicate it.
    #[error("Ambiguous outcome: the instance returned HTTP {status} after the request was sent — the change may or may not have been applied. Verify the result (re-read the entity) before retrying; a blind retry can duplicate the change.")]
    Ambiguous {
        /// The transient 5xx status the instance returned.
        status: u16,
    },
    /// Defensive: the retry loop exited without returning.
    #[error("Unreachable state — please report this bug")]
    Unreachable,
}

/// Outcome of ONE HTTP round trip — retry policy stays with the caller
/// ([`request_with_retries`]) so the sequential and concurrent paths can
/// never diverge.
enum RoundTrip {
    Ok(Value),
    /// 429 with the server's delay hint (Retry-After / X-RateLimit-Retry-After,
    /// already capped) and rate-limit hint, when present.
    TooManyRequests {
        header_delay_ms: Option<u64>,
        limit_hint: Option<u32>,
    },
    /// 502/503/504 — transient server trouble, worth retrying.
    ServerError {
        status: u16,
        reason: String,
    },
    /// Terminal: network error, parse error, oversized body, non-retryable
    /// HTTP status, or a JSON-RPC error object.
    Fail(ApiError),
}

/// One HTTP attempt: send, classify, cap and parse the response.
async fn rpc_roundtrip(
    http: &reqwest::Client,
    url: &str,
    api_key: &str,
    body: &Value,
) -> RoundTrip {
    let mut resp = match http
        .post(url)
        .header("x-api-key", api_key)
        .json(body)
        .send()
        .await
    {
        Ok(r) => r,
        Err(e) => return RoundTrip::Fail(ApiError::Network(e.to_string())),
    };

    let status = resp.status().as_u16();
    let reason = resp.status().canonical_reason().unwrap_or("?").to_string();

    if [502, 503, 504].contains(&status) {
        return RoundTrip::ServerError { status, reason };
    }

    if status == 429 {
        let header_delay_ms = LeantimeClient::parse_retry_after(&resp);
        let limit_hint = resp
            .headers()
            .get("X-RateLimit-Limit")
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|v| *v > 0); // 0 would cause a division by zero downstream
        return RoundTrip::TooManyRequests {
            header_delay_ms,
            limit_hint,
        };
    }

    if !resp.status().is_success() {
        return RoundTrip::Fail(ApiError::Http { status, reason });
    }

    // Bound the buffered response: a hostile instance must not be able
    // to OOM the process with a giant body. Chunked responses have no
    // content-length, so the cap is enforced WHILE accumulating, not
    // after the fact.
    const MAX_RESPONSE_BYTES: usize = 64 * 1024 * 1024;
    if let Some(len) = resp.content_length() {
        if len as usize > MAX_RESPONSE_BYTES {
            return RoundTrip::Fail(ApiError::TooLarge(len));
        }
    }
    let mut raw = Vec::new();
    loop {
        let chunk = match resp.chunk().await {
            Ok(c) => c,
            Err(e) => return RoundTrip::Fail(ApiError::Network(e.to_string())),
        };
        let Some(chunk) = chunk else { break };
        if raw.len() + chunk.len() > MAX_RESPONSE_BYTES {
            return RoundTrip::Fail(ApiError::TooLarge((raw.len() + chunk.len()) as u64));
        }
        raw.extend_from_slice(&chunk);
    }
    let json: Value = match serde_json::from_slice(&raw) {
        Ok(v) => v,
        Err(e) => return RoundTrip::Fail(ApiError::Parse(e.to_string())),
    };

    if let Some(err) = json.get("error") {
        return RoundTrip::Fail(ApiError::Rpc {
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

    RoundTrip::Ok(json.get("result").cloned().unwrap_or(Value::Null))
}

/// Drive one logical request through the shared retry policy:
/// 429 ≤ [`MAX_429_RETRIES`] with server-hinted or rate-derived delays,
/// 502/503/504 ≤ [`MAX_NETWORK_RETRIES`]. `discovered_rate_limit` persists
/// the instance's allowance when shared (sequential path) and stays
/// per-request when spawned (concurrent path) — safe either way because
/// the policy is reactive: the instance's limiter governs throughput.
async fn request_with_retries(
    http: &reqwest::Client,
    url: &str,
    api_key: &str,
    body: &Value,
    discovered_rate_limit: &mut Option<u32>,
) -> Result<Value, ApiError> {
    let mut network_retries = 0u32;
    let mut last_429_delay = 0u64;

    for attempt in 0..=(MAX_429_RETRIES + MAX_NETWORK_RETRIES) {
        match rpc_roundtrip(http, url, api_key, body).await {
            RoundTrip::Ok(v) => return Ok(v),
            RoundTrip::ServerError { status, reason } => {
                // A 5xx after a mutation is ambiguous: the instance may have
                // applied the change before the response was lost. Reads are
                // retried transparently; mutations surface an explicit error
                // instead of risking a duplicate.
                if !is_read_request(body) {
                    return Err(ApiError::Ambiguous { status });
                }
                if (network_retries as usize) < MAX_NETWORK_RETRIES {
                    network_retries += 1;
                    let delay = if network_retries == 1 { 500 } else { 1000 };
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    continue;
                }
                return Err(ApiError::Http {
                    status,
                    reason: format!("{} (retried {} times)", reason, MAX_NETWORK_RETRIES),
                });
            }
            RoundTrip::TooManyRequests {
                header_delay_ms,
                limit_hint,
            } => {
                if discovered_rate_limit.is_none() {
                    *discovered_rate_limit = limit_hint;
                }
                if attempt < MAX_429_RETRIES {
                    let limit = discovered_rate_limit.unwrap_or(DEFAULT_RATE_LIMIT_PER_MIN);
                    let rate_delay = (60_000 / limit as u64).max(1000);
                    last_429_delay = header_delay_ms.unwrap_or(rate_delay);
                    tokio::time::sleep(Duration::from_millis(last_429_delay)).await;
                    continue;
                }
                let limit = discovered_rate_limit.unwrap_or(DEFAULT_RATE_LIMIT_PER_MIN);
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
            RoundTrip::Fail(e) => return Err(e),
        }
    }

    Err(ApiError::Unreachable)
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
        request_with_retries(
            &self.http,
            &url,
            &self.api_key,
            &body,
            &mut self.discovered_rate_limit,
        )
        .await
    }

    /// Fetch `comments.getComments` for many tickets with bounded concurrency.
    /// Results come back in INPUT ORDER (deterministic backups); errors are
    /// per-ticket so one failure never drops the whole batch. Concurrency is
    /// an in-flight cap only — the retry policy stays reactive (429 backoff),
    /// so the instance's rate limit always governs aggregate throughput.
    pub async fn get_comments_for_tickets(
        &mut self,
        ticket_ids: &[String],
        concurrency: usize,
    ) -> Vec<Result<Value, ApiError>> {
        if ticket_ids.is_empty() {
            return Vec::new();
        }
        let start_id = self.rpc_id + 1;
        self.rpc_id += ticket_ids.len() as u64;
        let http = self.http.clone();
        let url = format!("{}/api/jsonrpc", self.base_url);
        let api_key = self.api_key.clone();
        let sem = std::sync::Arc::new(tokio::sync::Semaphore::new(concurrency.max(1)));

        let mut handles = Vec::with_capacity(ticket_ids.len());
        for (i, tid) in ticket_ids.iter().enumerate() {
            let http = http.clone();
            let url = url.clone();
            let api_key = api_key.clone();
            let tid = tid.clone();
            let sem = sem.clone();
            handles.push(tokio::spawn(async move {
                let _permit = sem
                    .acquire()
                    .await
                    .map_err(|e| ApiError::Network(e.to_string()))?;
                let body = serde_json::json!({
                    "jsonrpc": "2.0",
                    "method": "leantime.rpc.comments.getComments",
                    "params": {"module": "ticket", "entityId": tid},
                    "id": start_id + i as u64,
                });
                // Local limiter state per request: the shared &mut client
                // cannot cross the spawn boundary, and the policy is reactive
                // (429-driven) so per-request state is safe — the instance's
                // rate limit governs everyone regardless.
                let mut discovered = None;
                request_with_retries(&http, &url, &api_key, &body, &mut discovered).await
            }));
        }
        let mut out = Vec::with_capacity(handles.len());
        for h in handles {
            match h.await {
                Ok(r) => out.push(r),
                Err(e) => out.push(Err(ApiError::Network(e.to_string()))),
            }
        }
        out
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

    /// Completeness fetch of every ticket in a project (optionally filtered
    /// by extra searchCriteria such as `{"type": "milestone"}`), immune to
    /// the API's per-call limit. Fast path: a single unwindowed call —
    /// projects smaller than [`fetch_limit`] cost exactly one request (same
    /// as before chunking existed). When that call comes back full, the
    /// fetch falls back to date-window bisection over [1970, now + 2 days].
    /// Returns `(deduplicated items, warnings)`.
    pub async fn get_all_tickets_chunked(
        &mut self,
        project_id: &str,
        extra_criteria: Value,
    ) -> Result<(Vec<Value>, Vec<String>), ApiError> {
        let to = chrono::Local::now()
            .naive_local()
            .checked_add_signed(chrono::Duration::days(2))
            .unwrap_or_else(|| {
                chrono::NaiveDate::from_ymd_opt(2100, 1, 1)
                    .unwrap()
                    .and_hms_opt(0, 0, 0)
                    .unwrap()
            });
        let from = chrono::NaiveDate::from_ymd_opt(1970, 1, 1)
            .unwrap()
            .and_hms_opt(0, 0, 0)
            .unwrap();
        self.get_all_tickets_chunked_in(project_id, extra_criteria, from, to, 32)
            .await
    }

    /// Injectable core of [`get_all_tickets_chunked`] — fixed range and
    /// depth cap keep the bisection windows deterministic for tests.
    pub async fn get_all_tickets_chunked_in(
        &mut self,
        project_id: &str,
        extra_criteria: Value,
        from: chrono::NaiveDateTime,
        to: chrono::NaiveDateTime,
        max_depth: u32,
    ) -> Result<(Vec<Value>, Vec<String>), ApiError> {
        let limit = fetch_limit();
        let mut criteria = serde_json::json!({"currentProject": project_id});
        if let (Some(base), Some(ext)) = (criteria.as_object_mut(), extra_criteria.as_object()) {
            for (k, v) in ext {
                base.insert(k.clone(), v.clone());
            }
        }

        // Fast path: one unwindowed call. Below the limit the result is
        // complete by definition — zero extra requests for normal projects.
        let probe = self
            .call(
                "tickets.getAll",
                serde_json::json!({"searchCriteria": criteria.clone(), "limit": limit}),
            )
            .await?;
        // Order-preserving dedup: first occurrence wins, output order stays
        // the fetch order (deterministic for a given dataset).
        let mut out: Vec<Value> = Vec::new();
        let mut seen: std::collections::HashSet<String> = std::collections::HashSet::new();
        for item in probe.as_array().cloned().unwrap_or_default() {
            if seen.insert(id_key(&item)) {
                out.push(item);
            }
        }
        if out.len() < limit {
            return Ok((out, Vec::new()));
        }

        // Truncation suspected — bisect [from, to] by modification date.
        // Ascending-order invariants that make this loss-free:
        // - a ticket's date only moves forward, so a ticket modified during
        //   the fetch reappears in a LATER window → duplicate, never loss
        //   (the id-keyed map absorbs duplicates);
        // - each call widens its window by ±1s because the API compares
        //   strictly (`date > from AND date < to`): without the overlap, a
        //   ticket exactly on a split boundary would fall in NO window.
        let mut warnings = Vec::new();
        let fmt = |d: chrono::NaiveDateTime| d.format("%Y-%m-%d %H:%M:%S").to_string();
        let mut stack: Vec<(chrono::NaiveDateTime, chrono::NaiveDateTime, u32)> =
            vec![(from, to, 0)];
        while let Some((w_from, w_to, depth)) = stack.pop() {
            let lo = w_from - chrono::Duration::seconds(1);
            let hi = w_to + chrono::Duration::seconds(1);
            let mut sc = criteria.clone();
            sc["dateFrom"] = serde_json::json!(fmt(lo));
            sc["dateTo"] = serde_json::json!(fmt(hi));
            let batch = self
                .call(
                    "tickets.getAll",
                    serde_json::json!({"searchCriteria": sc, "limit": limit}),
                )
                .await?;
            let items = batch.as_array().cloned().unwrap_or_default();
            if items.len() < limit {
                for item in items {
                    if seen.insert(id_key(&item)) {
                        out.push(item);
                    }
                }
                continue;
            }
            if depth >= max_depth {
                for item in items {
                    if seen.insert(id_key(&item)) {
                        out.push(item);
                    }
                }
                warnings.push(format!(
                    "pagination depth cap reached for window [{} → {}] — tickets beyond the API limit inside this window may be missing (bulk import sharing one timestamp?); raise LEANTIME_MCP_FETCH_LIMIT",
                    fmt(w_from),
                    fmt(w_to)
                ));
                continue;
            }
            match w_to
                .signed_duration_since(w_from)
                .num_seconds()
                .checked_div(2)
            {
                Some(half) if half > 0 => {
                    if let Some(mid) = w_from.checked_add_signed(chrono::Duration::seconds(half)) {
                        stack.push((w_from, mid, depth + 1));
                        stack.push((mid, w_to, depth + 1));
                        continue;
                    }
                    warnings.push(format!(
                        "pagination window arithmetic failed at [{} → {}] — this slice may be incomplete",
                        fmt(w_from),
                        fmt(w_to)
                    ));
                }
                _ => {
                    warnings.push(format!(
                        "pagination depth cap reached for window [{} → {}] — tickets beyond the API limit inside this window may be missing; raise LEANTIME_MCP_FETCH_LIMIT",
                        fmt(w_from),
                        fmt(w_to)
                    ));
                }
            }
        }
        Ok((out, warnings))
    }
}
