//! End-to-end wiring of MCP progress notifications (#632): a real server
//! process speaking stdio JSON-RPC against a mocked Leantime. A tools/call
//! carrying `_meta.progressToken` must see `notifications/progress` lines
//! on stdout BEFORE the final response; without the token, none.

use mockito::Server;
use serde_json::{json, Value};
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

fn request_line(id: u64, method: &str, params: Value) -> String {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params }).to_string() + "\n"
}

async fn spawn_server(url: &str) -> tokio::process::Child {
    tokio::process::Command::new(env!("CARGO_BIN_EXE_leantmcp"))
        .arg("serve")
        .env("LEANTIME_URL", url)
        .env("LEANTIME_API_KEY", "test-key")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("spawn leantmcp serve")
}

/// Read stdout lines until the JSON-RPC response for `id`, collecting any
/// progress notifications seen on the way. Every read is time-boxed so a
/// wiring bug fails the test instead of hanging CI.
async fn read_until_response(
    reader: &mut BufReader<tokio::process::ChildStdout>,
    id: u64,
) -> (Vec<Value>, Value) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(30);
    let mut notifications = Vec::new();
    let mut line = String::new();
    loop {
        line.clear();
        let n = tokio::time::timeout_at(deadline, reader.read_line(&mut line))
            .await
            .expect("timed out waiting for a stdout line")
            .expect("stdout read error");
        assert!(n > 0, "server closed stdout before responding");
        let v: Value = serde_json::from_str(line.trim())
            .unwrap_or_else(|e| panic!("non-JSON line: {:?} ({})", line, e));
        if v.get("method").and_then(|m| m.as_str()) == Some("notifications/progress") {
            notifications.push(v);
            continue;
        }
        if v.get("id") == Some(&json!(id)) {
            return (notifications, v);
        }
        panic!("unexpected stdout line: {}", line.trim());
    }
}

#[tokio::test]
async fn progress_token_yields_notifications_before_response() {
    let mut server = Server::new_async().await;
    let users = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.users.getAll"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(json!({"jsonrpc": "2.0", "result": [], "id": 1}).to_string())
        .create_async()
        .await;
    let add = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.addTicket"}).to_string(),
        ))
        .expect(3) // 2 items + the silent (token-less) call
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(json!({"jsonrpc": "2.0", "result": ["42"], "id": 1}).to_string())
        .create_async()
        .await;

    let mut child = spawn_server(&server.url()).await;
    let mut stdin = child.stdin.take().expect("stdin");
    let mut reader = BufReader::new(child.stdout.take().expect("stdout"));

    // Handshake.
    stdin
        .write_all(
            request_line(1, "initialize", json!({"protocolVersion": "2025-06-18"})).as_bytes(),
        )
        .await
        .unwrap();
    let (_, init) = read_until_response(&mut reader, 1).await;
    assert!(init["result"]["protocolVersion"].is_string());
    stdin
        .write_all(request_line(2, "notifications/initialized", json!({})).as_bytes())
        .await
        .unwrap();

    // 2-item bulk create → users.getAll + 2 × addTicket = 3 progress steps.
    let tickets: Vec<Value> = (1..=2)
        .map(|i| json!({ "headline": format!("stdio progress {}", i), "unassigned": true }))
        .collect();
    stdin
        .write_all(
            request_line(
                3,
                "tools/call",
                json!({
                    "name": "leantime_bulk_create_tickets",
                    "arguments": {"projectId": "5", "tickets": tickets},
                    "_meta": {"progressToken": "tok-stdio"},
                }),
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let (notifications, response) = read_until_response(&mut reader, 3).await;

    assert_eq!(
        notifications.len(),
        3,
        "all notifications: {:?}",
        notifications
    );
    for n in &notifications {
        assert_eq!(n["params"]["progressToken"], json!("tok-stdio"));
    }
    assert_eq!(notifications[0]["params"]["progress"], json!(1));
    assert_eq!(notifications[0]["params"]["total"], json!(3));
    assert!(notifications[0]["params"]["message"]
        .as_str()
        .unwrap()
        .contains("users.getAll"));
    assert_eq!(notifications[2]["params"]["progress"], json!(3));
    assert!(notifications[2]["params"]["message"]
        .as_str()
        .unwrap()
        .contains("addTicket"));
    // The response arrives AFTER the notifications and reports success.
    let text = response["result"]["content"][0]["text"].as_str().unwrap();
    assert!(text.contains("\"created\": 2"), "{}", text);
    assert_eq!(response["error"], Value::Null);

    // Without a token: same call shape, zero notifications.
    stdin
        .write_all(
            request_line(
                4,
                "tools/call",
                json!({
                    "name": "leantime_bulk_create_tickets",
                    "arguments": {"projectId": "5",
                        "tickets": [{"headline": "stdio silent", "unassigned": true}]},
                }),
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    let (notifications, response) = read_until_response(&mut reader, 4).await;
    assert!(notifications.is_empty(), "no token → no notifications");
    assert!(response["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("\"created\": 1"));

    users.assert();
    add.assert();
    drop(stdin);
    let _ = child.kill().await;
}
