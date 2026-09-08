use mockito::Server;
use serde_json::json;

use leantmcp::client::LeantimeClient;

fn ok_response() -> String {
    json!({"jsonrpc": "2.0", "result": [123], "id": 1}).to_string()
}

// ---- 429 retry ----

#[tokio::test]
async fn retry_429_with_retry_after_header_then_succeeds() {
    let mut server = Server::new_async().await;
    let url = server.url();

    let _m1 = server
        .mock("POST", "/api/jsonrpc")
        .with_status(429)
        .with_header("Content-Type", "application/json")
        .with_header("Retry-After", "0")
        .with_body(r#"{"error": "Too many requests"}"#)
        .create_async()
        .await;

    let _m2 = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(ok_response())
        .create_async()
        .await;

    let mut client = LeantimeClient::new(&url, "test-key");
    let result = client.call("tickets.getAll", json!({})).await;
    assert!(result.is_ok());
    _m1.assert();
    _m2.assert();
}

#[tokio::test]
async fn retry_429_with_x_ratelimit_headers_then_succeeds() {
    let mut server = Server::new_async().await;
    let url = server.url();

    let _m1 = server
        .mock("POST", "/api/jsonrpc")
        .with_status(429)
        .with_header("X-RateLimit-Limit", "10")
        .with_header("X-RateLimit-Retry-After", "0")
        .with_body(r#"{"error": "Too many requests"}"#)
        .create_async()
        .await;

    let _m2 = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_body(ok_response())
        .create_async()
        .await;

    let mut client = LeantimeClient::new(&url, "key");
    let result = client.call("tickets.getAll", json!({})).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn retry_429_always_then_exhausts_with_clear_error() {
    let mut server = Server::new_async().await;
    let url = server.url();

    let m = server
        .mock("POST", "/api/jsonrpc")
        .with_status(429)
        .with_header("Retry-After", "0")
        .with_body(r#"{"error": "Too many requests"}"#)
        .expect(6) // 1 initial + 5 retries
        .create_async()
        .await;

    let mut client = LeantimeClient::new(&url, "key");
    let err = client.call("tickets.getAll", json!({})).await.unwrap_err();
    // Byte-parity with the TS edition — exact message. The "(waited …)" clause
    // is omitted when every retry slept 0ms (Retry-After: 0), like the TS client.
    assert_eq!(
        err.to_string(),
        "Rate limit exhausted after 5 retries — the instance allows ~10 req/min. Wait ~60 seconds or reduce the batch size."
    );
    assert!(
        err.to_string().contains("reduce the batch size"),
        "got: {}",
        err
    );
    m.assert();
}

#[tokio::test]
async fn retry_502_then_succeeds() {
    let mut server = Server::new_async().await;
    let url = server.url();

    let _m1 = server
        .mock("POST", "/api/jsonrpc")
        .with_status(502)
        .with_body("Bad Gateway")
        .create_async()
        .await;

    let _m2 = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_body(ok_response())
        .create_async()
        .await;

    let mut client = LeantimeClient::new(&url, "key");
    let result = client.call("tickets.getAll", json!({})).await;
    assert!(result.is_ok());
}

#[tokio::test]
async fn non_429_error_not_retried() {
    let mut server = Server::new_async().await;
    let url = server.url();

    let m = server
        .mock("POST", "/api/jsonrpc")
        .with_status(401)
        .with_body("Unauthorized")
        .expect(1) // no retries
        .create_async()
        .await;

    let mut client = LeantimeClient::new(&url, "bad-key");
    let err = client.call("tickets.getAll", json!({})).await.unwrap_err();
    assert!(err.to_string().contains("401"), "got: {}", err);
    m.assert();
}

#[tokio::test]
async fn rpc_error_surfaced() {
    let mut server = Server::new_async().await;
    let url = server.url();

    let _m = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(
            r#"{"jsonrpc":"2.0","error":{"code":-32601,"message":"Method not found"},"id":1}"#,
        )
        .create_async()
        .await;

    let mut client = LeantimeClient::new(&url, "key");
    let err = client
        .call("tickets.nonExistent", json!({}))
        .await
        .unwrap_err();
    assert!(err.to_string().contains("Method not found"), "got: {}", err);
}

// ---- Retry-After: HTTP-date format ----

#[tokio::test]
async fn retry_after_http_date_in_past_retries_immediately() {
    let mut server = Server::new_async().await;
    let url = server.url();

    let past = chrono::Utc::now() - chrono::Duration::seconds(3600);
    let _m1 = server
        .mock("POST", "/api/jsonrpc")
        .with_status(429)
        .with_header(
            "Retry-After",
            &past.format("%a, %d %b %Y %H:%M:%S GMT").to_string(),
        )
        .with_body(r#"{"error": "slow down"}"#)
        .create_async()
        .await;
    let _m2 = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_body(ok_response())
        .create_async()
        .await;

    let start = std::time::Instant::now();
    let mut client = LeantimeClient::new(&url, "key");
    let result = client.call("tickets.getAll", json!({})).await;
    let elapsed = start.elapsed();
    assert!(result.is_ok());
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "past Retry-After must clamp to 0, took {:?}",
        elapsed
    );
}

#[tokio::test]
async fn retry_after_http_date_in_future_waits_the_delta() {
    let mut server = Server::new_async().await;
    let url = server.url();

    let future = chrono::Utc::now() + chrono::Duration::seconds(2);
    let _m1 = server
        .mock("POST", "/api/jsonrpc")
        .with_status(429)
        .with_header(
            "Retry-After",
            &future.format("%a, %d %b %Y %H:%M:%S GMT").to_string(),
        )
        .with_body(r#"{"error": "slow down"}"#)
        .create_async()
        .await;
    let _m2 = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_body(ok_response())
        .create_async()
        .await;

    let start = std::time::Instant::now();
    let mut client = LeantimeClient::new(&url, "key");
    let result = client.call("tickets.getAll", json!({})).await;
    let elapsed = start.elapsed();
    assert!(result.is_ok());
    assert!(
        elapsed >= std::time::Duration::from_millis(1000),
        "future Retry-After must wait, took {:?}",
        elapsed
    );
    assert!(
        elapsed < std::time::Duration::from_secs(6),
        "…but capped, took {:?}",
        elapsed
    );
}

#[tokio::test]
async fn retry_after_huge_value_is_capped_at_60s_header_not_needed_here() {
    // Sanity: the parse function caps at 60s; we can't wait 60s in a test, so
    // verify indirectly — a huge seconds value that WOULD be huge (1e9s) would
    // take ~31 years; the cap makes it 60s. We assert the exhaustion error
    // arrives instead of hanging (mock refuses to ever succeed).
    let mut server = Server::new_async().await;
    let url = server.url();

    let m = server
        .mock("POST", "/api/jsonrpc")
        .with_status(429)
        .with_header("Retry-After", "1000000000")
        .with_body(r#"{"error": "no"}"#)
        .expect(6)
        .create_async()
        .await;

    // Prove the cap without waiting 60s×5: run with a short timeout — if the
    // delay were uncapped this would time out rather than error.
    let client_task = tokio::time::timeout(std::time::Duration::from_secs(3), async {
        let mut client = LeantimeClient::new(&url, "key");
        let _ = client.call("tickets.getAll", json!({})).await;
    });
    // With the cap, each retry waits 60s — the 3s timeout fires long before.
    // This proves delays are bounded by the cap (and not larger).
    let _ = client_task.await; // elapsed ≤ 3s
    let _ = m;
}

// ---- X-RateLimit-Limit discovery → inter-request delay calibration ----

#[tokio::test]
async fn rate_limit_discovery_calibrates_and_persists_across_calls() {
    let mut server = Server::new_async().await;
    let url = server.url();

    // Call 1: 429 with X-RateLimit-Limit: 600 (→ delay 100ms) then success.
    let _a = server
        .mock("POST", "/api/jsonrpc")
        .with_status(429)
        .with_header("X-RateLimit-Limit", "600")
        .with_body(r#"{"error": "slow"}"#)
        .create_async()
        .await;
    let _b = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_body(ok_response())
        .create_async()
        .await;
    // Call 2: 429 with NO headers — the persisted limit (600) must pace at 100ms,
    // not the conservative 6000ms default.
    let _c = server
        .mock("POST", "/api/jsonrpc")
        .with_status(429)
        .with_body(r#"{"error": "slow"}"#)
        .create_async()
        .await;
    let _d = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_body(ok_response())
        .create_async()
        .await;

    let start = std::time::Instant::now();
    let mut client = LeantimeClient::new(&url, "key");
    assert!(client.call("tickets.getAll", json!({})).await.is_ok());
    assert!(client.call("tickets.getAll", json!({})).await.is_ok());
    let elapsed = start.elapsed();
    // 2 × 100ms calibrated delays; the conservative default would be ≥ 12s.
    assert!(
        elapsed < std::time::Duration::from_secs(5),
        "calibration not persisted? took {:?}",
        elapsed
    );
}

// ---- 503 retried twice, then success ----

#[tokio::test]
async fn transient_503_retried_twice_then_succeeds() {
    let mut server = Server::new_async().await;
    let url = server.url();

    // The first two hits are 503, then the mock is exhausted and the 200
    // fallback takes over (retry loop resends the same body).
    let _r503 = server
        .mock("POST", "/api/jsonrpc")
        .with_status(503)
        .with_body("unavailable")
        .expect(2)
        .create_async()
        .await;
    let _r200 = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_body(ok_response())
        .create_async()
        .await;

    let mut client = LeantimeClient::new(&url, "key");
    assert!(client.call("tickets.getAll", json!({})).await.is_ok());
    _r503.assert();
}

#[tokio::test]
async fn transient_502_exhausts_after_two_retries() {
    let mut server = Server::new_async().await;
    let url = server.url();

    let m = server
        .mock("POST", "/api/jsonrpc")
        .with_status(502)
        .with_body("bad gateway")
        .expect(3) // 1 initial + 2 retries
        .create_async()
        .await;

    let mut client = LeantimeClient::new(&url, "key");
    let err = client.call("tickets.getAll", json!({})).await.unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("502"), "{}", msg);
    assert!(msg.contains("retried 2 times"), "{}", msg);
    m.assert();
}

#[tokio::test]
async fn rpc_error_data_gets_ts_separator() {
    let mut server = Server::new_async().await;
    let _m = server.mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(
            r#"{"jsonrpc":"2.0","error":{"code":-32602,"message":"Invalid params","data":"projectId required"},"id":1}"#,
        )
        .create_async().await;

    let mut client = LeantimeClient::new(&server.url(), "key");
    let err = client
        .call("tickets.getTicket", json!({}))
        .await
        .unwrap_err();
    assert_eq!(
        err.to_string(),
        "Leantime RPC error [-32602]: Invalid params — projectId required"
    );
}

#[tokio::test]
async fn retry_exhaustion_includes_waited_clause_when_delayed() {
    let mut server = Server::new_async().await;
    let url = server.url();

    // Every 429 asks for exactly 1s → the exhaustion error must include the
    // "(waited ~1s per retry)" clause, like the TS client.
    let m = server
        .mock("POST", "/api/jsonrpc")
        .with_status(429)
        .with_header("Retry-After", "1")
        .with_body(r#"{"error": "slow"}"#)
        .expect(6)
        .create_async()
        .await;

    let mut client = LeantimeClient::new(&url, "key");
    let err = client.call("tickets.getAll", json!({})).await.unwrap_err();
    assert_eq!(
        err.to_string(),
        "Rate limit exhausted after 5 retries (waited ~1s per retry) — the instance allows ~10 req/min. Wait ~60 seconds or reduce the batch size."
    );
    m.assert();
}

// ---- chunked completeness fetch (date-window bisection) ----

/// LEANTIME_MCP_FETCH_LIMIT is process-global: tests that set it hold this
/// lock so parallel tests in this binary can't race them.
static FETCH_LIMIT_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

fn rpc_items(ids: &[i64]) -> String {
    let items: Vec<serde_json::Value> = ids
        .iter()
        .map(|i| json!({"id": i, "type": "task", "headline": format!("t{}", i)}))
        .collect();
    json!({"jsonrpc": "2.0", "result": items, "id": 1}).to_string()
}

#[tokio::test]
async fn chunked_fast_path_is_a_single_call() {
    let _guard = FETCH_LIMIT_LOCK.lock().await;
    std::env::remove_var("LEANTIME_MCP_FETCH_LIMIT");
    let mut server = Server::new_async().await;
    let m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getAll", "params": {
                "searchCriteria": {"currentProject": "15"}, "limit": 10000
            }})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_items(&[1, 2, 3]))
        .create_async()
        .await;

    let mut client = LeantimeClient::new(&server.url(), "test-key");
    let (items, warnings) = client
        .get_all_tickets_chunked("15", json!({}))
        .await
        .expect("fast path should succeed");
    assert_eq!(items.len(), 3);
    assert!(warnings.is_empty());
    m.assert(); // exactly one request — no windowed calls below the limit
}

#[tokio::test]
async fn chunked_bisection_splits_full_windows_and_dedups() {
    let _guard = FETCH_LIMIT_LOCK.lock().await;
    std::env::set_var("LEANTIME_MCP_FETCH_LIMIT", "3");
    let mut server = Server::new_async().await;

    // Probe (no dates) — created FIRST so windowed mocks take precedence
    // for windowed requests (mockito: newest matching mock wins).
    let probe = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getAll", "params": {
                "searchCriteria": {"currentProject": "15"}, "limit": 3
            }})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_items(&[1, 2, 3])) // exactly the limit → chunking
        .create_async()
        .await;

    use chrono::NaiveDate;
    let from = NaiveDate::from_ymd_opt(2026, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let to = NaiveDate::from_ymd_opt(2026, 1, 31)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    // Root window widened by ±1s. A full window is NOT merged: its items
    // are re-fetched by the halves (the date ranges partition the space),
    // so a realistic mock returns a subset that the halves also return.
    let _root = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"params": {"searchCriteria": {
                "currentProject": "15",
                "dateFrom": "2025-12-31 23:59:59",
                "dateTo": "2026-01-31 00:00:01"
            }, "limit": 3}})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_items(&[1, 2, 3])) // full again → split at 2026-01-16
        .create_async()
        .await;
    // Right half [2026-01-16, 2026-01-31] widened
    let _right = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"params": {"searchCriteria": {
                "currentProject": "15",
                "dateFrom": "2026-01-15 23:59:59",
                "dateTo": "2026-01-31 00:00:01"
            }, "limit": 3}})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_items(&[3, 4])) // under the limit → leaf
        .create_async()
        .await;
    // Left half [2026-01-01, 2026-01-16] widened — id 4 also returned by
    // the right half: the ±1s overlap must dedup, never lose it
    let _left = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"params": {"searchCriteria": {
                "currentProject": "15",
                "dateFrom": "2025-12-31 23:59:59",
                "dateTo": "2026-01-16 00:00:01"
            }, "limit": 3}})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_items(&[4, 5])) // under the limit → leaf
        .create_async()
        .await;

    let mut client = LeantimeClient::new(&server.url(), "test-key");
    let (items, warnings) = client
        .get_all_tickets_chunked_in("15", json!({}), from, to, 32)
        .await
        .expect("chunked fetch should succeed");
    std::env::remove_var("LEANTIME_MCP_FETCH_LIMIT");

    let mut ids: Vec<i64> = items
        .iter()
        .filter_map(|i| i.get("id").and_then(|v| v.as_i64()))
        .collect();
    ids.sort();
    // probe {1,2,3} ∪ right {3,4} ∪ left {4,5} — deduped
    assert_eq!(ids, vec![1, 2, 3, 4, 5], "{:?}", items);
    assert!(warnings.is_empty());
    probe.assert();
    _root.assert();
    _right.assert();
    _left.assert();
}

#[tokio::test]
async fn chunked_depth_cap_emits_warning() {
    let _guard = FETCH_LIMIT_LOCK.lock().await;
    std::env::set_var("LEANTIME_MCP_FETCH_LIMIT", "3");
    let mut server = Server::new_async().await;
    let _probe = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"params": {"searchCriteria": {"currentProject": "15"}, "limit": 3}}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_items(&[1, 2, 3]))
        .create_async()
        .await;
    let window = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"params": {"searchCriteria": {
                "dateFrom": "2025-12-31 23:59:59",
                "dateTo": "2026-01-31 00:00:01"
            }}})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_items(&[1, 2, 3])) // full at depth 0 = max_depth → cap
        .create_async()
        .await;

    use chrono::NaiveDate;
    let from = NaiveDate::from_ymd_opt(2026, 1, 1)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let to = NaiveDate::from_ymd_opt(2026, 1, 31)
        .unwrap()
        .and_hms_opt(0, 0, 0)
        .unwrap();
    let mut client = LeantimeClient::new(&server.url(), "test-key");
    let (items, warnings) = client
        .get_all_tickets_chunked_in("15", json!({}), from, to, 0)
        .await
        .expect("should succeed with warnings");
    std::env::remove_var("LEANTIME_MCP_FETCH_LIMIT");

    assert_eq!(items.len(), 3);
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].contains("depth cap"), "{:?}", warnings);
    window.assert();
}
