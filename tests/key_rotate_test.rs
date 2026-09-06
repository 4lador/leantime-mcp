//! `key rotate` unit tests — mockito-backed, keyring redirected to a temp HOME.
//! Rotation is destructive for the stored key: every failure mode must leave
//! the previous key intact.

use mockito::Server;
use serde_json::{json, Value};

use leantmcp::client::LeantimeClient;
use leantmcp::config;
use leantmcp::tools;

fn scratch(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("leantmcp-rotate-{}-{}", tag, std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Serializes the env-mutating tests (parallel `cargo test` support).
/// Async-aware: these tests await while holding the guard.
static HOME_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

async fn set_home(dir: &std::path::PathBuf) -> tokio::sync::MutexGuard<'static, ()> {
    let guard = HOME_LOCK.lock().await;
    std::env::set_var("HOME", dir);
    #[cfg(windows)]
    if let Some(up) = dir.to_str() {
        std::env::set_var("USERPROFILE", up);
    }
    std::env::remove_var("LEANTIME_URL");
    std::env::remove_var("LEANTIME_API_KEY");
    std::env::remove_var("LEANTIME_INSTANCE");
    guard
}

fn rpc_ok(result: Value) -> String {
    json!({ "jsonrpc": "2.0", "result": result, "id": 1 }).to_string()
}

/// Mock the full happy-path rotation choreography for current key `lt_<user>_x`.
fn mock_happy_path(server: &mut Server, key_user: &str, relations: Value) {
    let _keys = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.Api.getAPIKeys"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([
            {"id": "11", "username": "otheruser0000000000000000000", "role": "50"},
            {"id": "12", "username": key_user, "role": "50"},
        ])))
        .create();

    let _create = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.Api.createAPIKey"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({
            "id": "99", "user": "newuser0000000000000000000", "passwordClean": "NEWPASS",
            "role": "50"
        })))
        .create();

    if relations.as_array().map(|a| !a.is_empty()).unwrap_or(false) {
        let _assigned = server.mock("POST", "/api/jsonrpc")
            .match_body(mockito::Matcher::PartialJsonString(
                json!({"method": "leantime.rpc.Projects.getProjectsAssignedToUser", "params": {"userId": "12"}}).to_string()))
            .with_status(200).with_header("Content-Type", "application/json")
            .with_body(rpc_ok(relations))
            .create();
        let _copy = server
            .mock("POST", "/api/jsonrpc")
            .match_body(mockito::Matcher::PartialJsonString(
                json!({"method": "leantime.rpc.Projects.editUserProjectRelations"}).to_string(),
            ))
            .with_status(200)
            .with_header("Content-Type", "application/json")
            .with_body(rpc_ok(json!(true)))
            .create();
    }

    // Live verification of the NEW key (header-matched).
    let _verify = server
        .mock("POST", "/api/jsonrpc")
        .match_header("x-api-key", "lt_newuser0000000000000000000_NEWPASS")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([{"id": "1"}])))
        .create();
}

#[tokio::test]
async fn rotate_happy_path_replaces_key_and_copies_relations() {
    let home = scratch("happy");
    let _guard = set_home(&home).await;
    config::write_key("lt_myuser0000000000000000000_OLDPASS").unwrap();

    let mut server = Server::new_async().await;
    mock_happy_path(
        &mut server,
        "myuser0000000000000000000",
        json!([{"id": "1"}, {"id": "2"}]),
    );

    let mut c = LeantimeClient::new(&server.url(), "lt_myuser0000000000000000000_OLDPASS");
    let msg = tools::key_rotate(
        &mut c,
        "lt_myuser0000000000000000000_OLDPASS",
        &server.url(),
        "rotated-key",
    )
    .await
    .expect("rotation should succeed");

    assert!(
        msg.contains("rotated-key") || msg.contains("role 50"),
        "{}",
        msg
    );
    assert!(msg.contains("relations copied (2)"), "{}", msg);
    // Keyring now holds the NEW key.
    assert_eq!(
        config::read_key().as_deref(),
        Some("lt_newuser0000000000000000000_NEWPASS")
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn rotate_without_relations_still_succeeds() {
    let home = scratch("no-relations");
    let _guard = set_home(&home).await;
    config::write_key("lt_myuser0000000000000000000_OLDPASS").unwrap();

    let mut server = Server::new_async().await;
    mock_happy_path(&mut server, "myuser0000000000000000000", json!([]));

    let mut c = LeantimeClient::new(&server.url(), "lt_myuser0000000000000000000_OLDPASS");
    let msg = tools::key_rotate(
        &mut c,
        "lt_myuser0000000000000000000_OLDPASS",
        &server.url(),
        "k",
    )
    .await
    .expect("rotation should succeed");
    assert!(!msg.contains("relations copied"), "{}", msg);
    assert_eq!(
        config::read_key().as_deref(),
        Some("lt_newuser0000000000000000000_NEWPASS")
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn rotate_verification_failure_leaves_keyring_untouched() {
    let home = scratch("verify-fail");
    let _guard = set_home(&home).await;
    config::write_key("lt_myuser0000000000000000000_OLDPASS").unwrap();

    let mut server = Server::new_async().await;
    let _keys = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!([{"id": "12", "username": "myuser0000000000000000000", "role": "50"}]),
        ))
        .create();
    let _create = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!({"id": "99", "user": "newuser0000000000000000000", "passwordClean": "NEWPASS"}),
        ))
        .create();
    // Verification of the new key FAILS (401).
    let _verify = server
        .mock("POST", "/api/jsonrpc")
        .match_header("x-api-key", "lt_newuser0000000000000000000_NEWPASS")
        .with_status(401)
        .create();

    let mut c = LeantimeClient::new(&server.url(), "lt_myuser0000000000000000000_OLDPASS");
    let err = tools::key_rotate(
        &mut c,
        "lt_myuser0000000000000000000_OLDPASS",
        &server.url(),
        "k",
    )
    .await
    .expect_err("verification failure must abort");

    assert!(err.contains("previous key untouched"), "{}", err);
    assert_eq!(
        config::read_key().as_deref(),
        Some("lt_myuser0000000000000000000_OLDPASS")
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn rotate_aborts_when_current_key_not_found() {
    let home = scratch("not-found");
    let _guard = set_home(&home).await;
    config::write_key("lt_myuser0000000000000000000_OLDPASS").unwrap();

    let mut server = Server::new_async().await;
    let _keys = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!([{"id": "77", "username": "someoneelse00000000000000", "role": "50"}]),
        ))
        .create();

    let mut c = LeantimeClient::new(&server.url(), "lt_myuser0000000000000000000_OLDPASS");
    let err = tools::key_rotate(
        &mut c,
        "lt_myuser0000000000000000000_OLDPASS",
        &server.url(),
        "k",
    )
    .await
    .expect_err("unknown key must abort");
    assert!(err.contains("Could not identify current key"), "{}", err);
    assert_eq!(
        config::read_key().as_deref(),
        Some("lt_myuser0000000000000000000_OLDPASS")
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn rotate_relation_copy_failure_warns_but_rotates() {
    let home = scratch("rel-fail");
    let _guard = set_home(&home).await;
    config::write_key("lt_myuser0000000000000000000_OLDPASS").unwrap();

    let mut server = Server::new_async().await;
    let _keys = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.Api.getAPIKeys"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!([{"id": "12", "username": "myuser0000000000000000000", "role": "50"}]),
        ))
        .create_async()
        .await;
    let _create = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!({"id": "99", "user": "newuser0000000000000000000", "passwordClean": "NEWPASS"}),
        ))
        .create_async()
        .await;
    let _assigned = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.Projects.getProjectsAssignedToUser"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([{"id": "1"}])))
        .create_async()
        .await;
    // The relation copy FAILS (500) — rotation must continue with a warning.
    let _copy_fail = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.Projects.editUserProjectRelations"}).to_string(),
        ))
        .with_status(500)
        .create_async()
        .await;
    let _verify = server
        .mock("POST", "/api/jsonrpc")
        .match_header("x-api-key", "lt_newuser0000000000000000000_NEWPASS")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([{"id": "1"}])))
        .create_async()
        .await;

    let mut c = LeantimeClient::new(&server.url(), "lt_myuser0000000000000000000_OLDPASS");
    let msg = tools::key_rotate(
        &mut c,
        "lt_myuser0000000000000000000_OLDPASS",
        &server.url(),
        "k",
    )
    .await
    .expect("rotation should still succeed");
    assert!(
        msg.contains("WARNING: could not copy project relations"),
        "{}",
        msg
    );
    assert_eq!(
        config::read_key().as_deref(),
        Some("lt_newuser0000000000000000000_NEWPASS")
    );
    let _ = std::fs::remove_dir_all(&home);
}

#[tokio::test]
async fn rotate_creation_failure_aborts_before_any_write() {
    let home = scratch("create-fail");
    let _guard = set_home(&home).await;
    config::write_key("lt_myuser0000000000000000000_OLDPASS").unwrap();

    let mut server = Server::new_async().await;
    let _keys = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!([{"id": "12", "username": "myuser0000000000000000000", "role": "50"}]),
        ))
        .create();
    let _create = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!(false))) // creation refused
        .create();

    let mut c = LeantimeClient::new(&server.url(), "lt_myuser0000000000000000000_OLDPASS");
    let err = tools::key_rotate(
        &mut c,
        "lt_myuser0000000000000000000_OLDPASS",
        &server.url(),
        "k",
    )
    .await
    .expect_err("creation failure must abort");
    assert!(err.contains("Key creation failed"), "{}", err);
    assert_eq!(
        config::read_key().as_deref(),
        Some("lt_myuser0000000000000000000_OLDPASS")
    );
    let _ = std::fs::remove_dir_all(&home);
}
