//! Tool-handler unit tests against a mocked Leantime API (mockito).
//! Covers the parity fixes: patch semantics, full-field sprint resend,
//! id de-mangling, timesheet end-of-day, computed current sprint,
//! destructive/assignment gating messages, log_time validation,
//! bulk caps and soft-error surfacing.

use mockito::Server;
use serde_json::{json, Value};

use leantmcp::client::LeantimeClient;
use leantmcp::tools;

type ClientRef = std::sync::Arc<tokio::sync::Mutex<LeantimeClient>>;

/// LEANTIME_MCP_DESTRUCTIVE_POLICY is process-global: the tests that flip it
/// hold this lock so a parallel local `cargo test` can't race them (CI runs
/// --test-threads=1, this is belt and braces).
static POLICY_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

fn get_tool(name: &str) -> tools::Tool {
    tools::create_registry()
        .into_iter()
        .find(|t| t.name == name)
        .unwrap_or_else(|| panic!("tool {} not found", name))
}

async fn call(name: &str, args: Value, url: &str) -> Value {
    let tool = get_tool(name);
    let client: ClientRef = std::sync::Arc::new(tokio::sync::Mutex::new(LeantimeClient::new(
        url, "test-key",
    )));
    let handler = &tool.handler;
    handler(args, client).await
}

/// Same as `call` but pins the idempotency journal to a temp dir (unique
/// per call site) so key behavior can be exercised end-to-end.
async fn call_idem(name: &str, args: Value, url: &str, tag: &str) -> Value {
    let tool = get_tool(name);
    let dir =
        std::env::temp_dir().join(format!("leantmcp-idem-tool-{}-{}", tag, std::process::id()));
    // Create without wiping: the journal must survive across successive
    // call_idem invocations within one test (that is the whole point).
    std::fs::create_dir_all(&dir).unwrap();
    let client: ClientRef = std::sync::Arc::new(tokio::sync::Mutex::new(
        LeantimeClient::new(url, "test-key").with_idempotency_dir(dir),
    ));
    let handler = &tool.handler;
    handler(args, client).await
}

/// (is_error, parsed_json_or_raw_text) — error text has the "Error: " prefix stripped
fn parse(r: &Value) -> (bool, Value) {
    let mut text = r["content"][0]["text"].as_str().unwrap_or("").to_string();
    let is_error = r["isError"] == json!(true);
    if is_error {
        text = text.strip_prefix("Error: ").unwrap_or(&text).to_string();
    }
    let parsed = serde_json::from_str::<Value>(&text).unwrap_or_else(|_| json!(text));
    (is_error, parsed)
}

fn rpc_ok(result: Value) -> String {
    json!({ "jsonrpc": "2.0", "result": result, "id": 1 }).to_string()
}

// ---------------------------------------------------------------- update_project

#[tokio::test]
async fn update_project_empty_guard_sends_nothing() {
    let server = Server::new_async().await;
    let r = call(
        "leantime_update_project",
        json!({"projectId": "7"}),
        &server.url(),
    )
    .await;
    let (is_err, text) = parse(&r);
    assert!(is_err);
    assert_eq!(
        text,
        json!("Nothing to update: provide at least one field to change.")
    );
}

#[tokio::test]
async fn update_project_sends_only_provided_fields() {
    let mut server = Server::new_async().await;
    let url = server.url();

    let m = server.mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.projects.patch", "params": {"id": "7", "params": {"name": "New"}}}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!(true)))
        .create_async().await;

    let r = call(
        "leantime_update_project",
        json!({"projectId": "7", "name": "New"}),
        &url,
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed, json!({"ok": true, "id": "7"}));
    m.assert();
}

// ---------------------------------------------------------------- update_sprint

#[tokio::test]
async fn update_sprint_resends_full_field_set() {
    let mut server = Server::new_async().await;
    let url = server.url();

    let _get = server.mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.sprints.getSprint", "params": {"id": "5"}}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({"id": "5", "projectId": "3", "name": "Old", "startDate": "2026-01-01", "endDate": "2026-02-01"})))
        .create_async().await;

    let edit = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.sprints.editSprint", "params": {"params": {
                "id": "5", "projectId": "3", "name": "Renamed",
                "startDate": "2026-01-01", "endDate": "2026-02-01"
            }}})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!(true)))
        .create_async()
        .await;

    let r = call(
        "leantime_update_sprint",
        json!({"sprintId": "5", "name": "Renamed"}),
        &url,
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed, json!({"ok": true, "sprintId": "5"}));
    edit.assert(); // full field set was resent
}

#[tokio::test]
async fn update_sprint_empty_guard() {
    let server = Server::new_async().await;
    let r = call(
        "leantime_update_sprint",
        json!({"sprintId": "5"}),
        &server.url(),
    )
    .await;
    let (is_err, text) = parse(&r);
    assert!(is_err);
    assert_eq!(
        text,
        json!("Nothing to update: provide name, startDate and/or endDate.")
    );
}

// ---------------------------------------------------------------- find_projects

#[tokio::test]
async fn find_projects_demangles_ids() {
    let mut server = Server::new_async().await;
    let m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.projects.findProject", "params": {"term": "x"}})
                .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!([{"id": "3-20260101120000", "name": "Mangled"}, {"id": "7", "name": "Clean"}]),
        ))
        .create_async()
        .await;

    let r = call(
        "leantime_find_projects",
        json!({"term": "x"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err);
    assert_eq!(parsed[0]["id"], json!("3"));
    assert_eq!(parsed[1]["id"], json!("7"));
    assert_eq!(parsed[0]["name"], json!("Mangled"));
    m.assert();
}

// ---------------------------------------------------------------- list_timesheets

#[tokio::test]
async fn list_timesheets_extends_date_to_end_of_day() {
    let mut server = Server::new_async().await;
    let m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.timesheets.getAll", "params": {
                "dateFrom": "2026-01-01", "dateTo": "2026-01-31 23:59:59", "projectId": "4"
            }})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([])))
        .create_async()
        .await;

    let r = call(
        "leantime_list_timesheets",
        json!({"dateFrom": "2026-01-01", "dateTo": "2026-01-31", "projectId": "4"}),
        &server.url(),
    )
    .await;
    let (is_err, _) = parse(&r);
    assert!(!is_err);
    m.assert();
}

// ---------------------------------------------------------------- get_current_sprint

#[tokio::test]
async fn get_current_sprint_computed_from_dates() {
    let mut server = Server::new_async().await;
    let _m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.sprints.getAllSprints"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([
            {"id": "1", "name": "past", "startDate": "2020-01-01", "endDate": "2020-02-01"},
            {"id": "2", "name": "now", "startDate": "2026-01-01", "endDate": "2030-12-31"},
        ])))
        .create_async()
        .await;

    let r = call(
        "leantime_get_current_sprint",
        json!({"projectId": "3"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["current"]["id"], json!("2"));
    assert_eq!(parsed["current"]["name"], json!("now"));
    assert_eq!(parsed["upcoming"], json!(null));
}

#[tokio::test]
async fn get_current_sprint_falls_back_to_upcoming() {
    let mut server = Server::new_async().await;
    let _m = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([
            {"id": "1", "name": "past", "startDate": "2020-01-01", "endDate": "2020-02-01"},
            {"id": "9", "name": "far", "startDate": "2035-06-01", "endDate": "2035-07-01"},
            {"id": "5", "name": "soon", "startDate": "2027-01-01", "endDate": "2027-02-01"},
        ])))
        .create_async()
        .await;

    let r = call(
        "leantime_get_current_sprint",
        json!({"projectId": "3"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err);
    assert_eq!(parsed["current"], json!(null));
    assert_eq!(parsed["upcoming"]["id"], json!("5")); // earliest upcoming
}

// ---------------------------------------------------------------- destructive

#[tokio::test]
async fn destructive_ask_refuses_without_confirm_and_makes_no_call() {
    let _policy_guard = POLICY_LOCK.lock().await;
    let server = Server::new_async().await;
    let r = call(
        "leantime_delete_ticket",
        json!({"ticketId": "42"}),
        &server.url(),
    )
    .await;
    let (is_err, text) = parse(&r);
    assert!(is_err);
    let msg = text.as_str().unwrap_or_default();
    assert!(msg.contains("Confirmation required"), "{}", msg);
    assert!(msg.contains("delete this ticket"), "{}", msg);
    assert!(msg.contains("NEVER pass confirm: true"), "{}", msg);
}

#[tokio::test]
async fn destructive_deny_refuses_even_with_confirm() {
    let _policy_guard = POLICY_LOCK.lock().await;
    std::env::set_var("LEANTIME_MCP_DESTRUCTIVE_POLICY", "deny");
    let server = Server::new_async().await;
    let r = call(
        "leantime_delete_ticket",
        json!({"ticketId": "42", "confirm": true}),
        &server.url(),
    )
    .await;
    std::env::set_var("LEANTIME_MCP_DESTRUCTIVE_POLICY", "ask");
    let (is_err, text) = parse(&r);
    assert!(is_err);
    let msg = text.as_str().unwrap_or_default();
    assert!(msg.contains("deny"), "{}", msg);
    assert!(msg.contains("refused"), "{}", msg);
}

#[tokio::test]
async fn destructive_ask_executes_with_confirm_and_echoes_id() {
    let _policy_guard = POLICY_LOCK.lock().await;
    let mut server = Server::new_async().await;
    let m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.delete", "params": {"id": "42"}}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!(true)))
        .create_async()
        .await;

    let r = call(
        "leantime_delete_ticket",
        json!({"ticketId": "42", "confirm": true}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed, json!({"deleted": true, "ticketId": "42"}));
    m.assert();
}

// ---------------------------------------------------------------- assignment

#[tokio::test]
async fn update_ticket_validates_editor_id() {
    let mut server = Server::new_async().await;
    let _users = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.users.getAll"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!([{"id": "1", "firstname": "Ada", "lastname": "Lovelace"}]),
        ))
        .create_async()
        .await;

    let r = call(
        "leantime_update_ticket",
        json!({"ticketId": "9", "editorId": "999", "headline": "x"}),
        &server.url(),
    )
    .await;
    let (is_err, text) = parse(&r);
    assert!(is_err);
    let msg = text.as_str().unwrap_or_default();
    assert!(msg.contains("editorId \"999\" does not exist"), "{}", msg);
    assert!(msg.contains("Available users:"), "{}", msg);
}

// ---------------------------------------------------------------- log_time

#[tokio::test]
async fn log_time_rejects_non_positive_hours() {
    let server = Server::new_async().await;
    let r = call(
        "leantime_log_time",
        json!({"ticketId": "9", "hours": 0}),
        &server.url(),
    )
    .await;
    let (is_err, text) = parse(&r);
    assert!(is_err);
    assert_eq!(text, json!("Hours must be a positive number."));
}

#[tokio::test]
async fn log_time_rejects_invalid_kind() {
    let server = Server::new_async().await;
    let r = call(
        "leantime_log_time",
        json!({"ticketId": "9", "hours": 1, "kind": "BOGUS"}),
        &server.url(),
    )
    .await;
    let (is_err, text) = parse(&r);
    assert!(is_err);
    let msg = text.as_str().unwrap_or_default();
    assert!(msg.contains("Invalid kind"), "{}", msg);
}

#[tokio::test]
async fn log_time_echoes_params_and_mode() {
    let mut server = Server::new_async().await;
    let m = server.mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.timesheets.logTime", "params": {
                "ticketId": "9", "params": {"kind": "DEVELOPMENT", "hours": 1.5, "date": "2026-01-15", "description": "work"}
            }}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!(true)))
        .create_async().await;

    let r = call("leantime_log_time", json!({
        "ticketId": "9", "hours": 1.5, "kind": "DEVELOPMENT", "date": "2026-01-15", "description": "work"
    }), &server.url()).await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["ok"], json!(true));
    assert_eq!(parsed["mode"], json!("add"));
    assert_eq!(parsed["kind"], json!("DEVELOPMENT"));
    assert_eq!(parsed["hours"], json!(1.5));
    m.assert();
}

// ---------------------------------------------------------------- bulk

#[tokio::test]
async fn bulk_create_rejects_batch_over_50() {
    let server = Server::new_async().await;
    let tickets: Vec<Value> = (0..51)
        .map(|i| json!({"headline": format!("t{}", i), "unassigned": true}))
        .collect();
    let r = call(
        "leantime_bulk_create_tickets",
        json!({"projectId": "1", "tickets": tickets}),
        &server.url(),
    )
    .await;
    let (is_err, text) = parse(&r);
    assert!(is_err);
    assert_eq!(text, json!("Batch too large: max 50 items."));
}

#[tokio::test]
async fn bulk_create_all_or_nothing_validation() {
    let mut server = Server::new_async().await;
    let _users = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!([{"id": "1", "firstname": "Ada", "lastname": "L"}]),
        ))
        .create_async()
        .await;

    let r = call(
        "leantime_bulk_create_tickets",
        json!({
            "projectId": "1",
            "tickets": [
                {"headline": "ok one", "unassigned": true},
                {"headline": "bad one"}
            ]
        }),
        &server.url(),
    )
    .await;
    let (is_err, text) = parse(&r);
    assert!(is_err);
    let msg = text.as_str().unwrap_or_default();
    assert!(msg.contains("NOTHING was created"), "{}", msg);
    assert!(msg.contains("\"bad one\""), "{}", msg);
}

#[tokio::test]
async fn bulk_update_validates_editor_ids_upfront() {
    let mut server = Server::new_async().await;
    let _users = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!([{"id": "1", "firstname": "Ada", "lastname": "L"}]),
        ))
        .create_async()
        .await;

    let r = call(
        "leantime_bulk_update_tickets",
        json!({
            "projectId": "1",
            "updates": [{"ticketId": "9", "editorId": "999"}]
        }),
        &server.url(),
    )
    .await;
    let (is_err, text) = parse(&r);
    assert!(is_err);
    let msg = text.as_str().unwrap_or_default();
    // Byte-parity with the TS edition (bulk.ts) — exact message, clause order included.
    assert!(
        msg.starts_with("editorId \"999\" does not exist — NOTHING was updated. Available: "),
        "{}",
        msg
    );
}

// ---------------------------------------------------------------- soft errors

#[tokio::test]
async fn create_sprint_surfaces_leantime_soft_error() {
    let mut server = Server::new_async().await;
    let _m = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!({"type": "error", "msg": "Duplicate sprint name"}),
        ))
        .create_async()
        .await;

    let r = call(
        "leantime_create_sprint",
        json!({"projectId": "1", "name": "S", "startDate": "2026-01-01", "endDate": "2026-02-01"}),
        &server.url(),
    )
    .await;
    let (is_err, text) = parse(&r);
    assert!(is_err);
    assert_eq!(text, json!("Duplicate sprint name"));
}

// ---------------------------------------------------------------- enrichment

#[tokio::test]
async fn get_ticket_enriches_single_item() {
    let mut server = Server::new_async().await;
    let _get = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getTicket", "params": {"id": "9"}}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({"id": "9", "status": 3, "headline": "T"})))
        .create_async()
        .await;

    let _labels = server.mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getStatusLabels"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({"3": {"name": "In Progress", "statusType": "IN_PROGRESS", "class": "label-blue"}})))
        .create_async().await;

    let r = call(
        "leantime_get_ticket",
        json!({"projectId": "1", "ticketId": "9"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["statusLabel"], json!("In Progress"));
    assert_eq!(parsed["statusType"], json!("IN_PROGRESS"));
    assert_eq!(parsed["statusColor"], json!("label-blue"));
}

#[tokio::test]
async fn get_ticket_enrichment_handles_string_status() {
    let mut server = Server::new_async().await;
    let _get = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({"id": "9", "status": "3"})))
        .create_async()
        .await;
    let _labels = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!({"3": {"name": "In Progress", "statusType": "IN_PROGRESS", "class": "blue"}}),
        ))
        .create_async()
        .await;

    let r = call(
        "leantime_get_ticket",
        json!({"projectId": "1", "ticketId": "9"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["statusLabel"], json!("In Progress"));
}

// ---------------------------------------------------------------- comments

#[tokio::test]
async fn add_comment_missing_ticket_surfaces_not_found() {
    let mut server = Server::new_async().await;
    let _m = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!(false)))
        .create_async()
        .await;

    let r = call(
        "leantime_add_comment",
        json!({"ticketId": "123", "text": "hi"}),
        &server.url(),
    )
    .await;
    let (is_err, text) = parse(&r);
    assert!(is_err);
    assert_eq!(text, json!("Ticket 123 not found."));
}

#[tokio::test]
async fn add_comment_passes_father_reply_param() {
    let mut server = Server::new_async().await;
    let _get = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getTicket"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({"id": "9", "type": "task", "headline": "T"})))
        .create_async()
        .await;

    let add = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.comments.addComment", "params": {
                "values": {"text": "<p>reply</p>", "father": 7}
            }})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!(true)))
        .create_async()
        .await;

    let r = call(
        "leantime_add_comment",
        json!({"ticketId": "9", "text": "reply", "father": 7}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed, json!({"ok": true, "ticketId": "9"}));
    add.assert();
}

// ---------------------------------------------------------------- ticket create shapes

#[tokio::test]
async fn create_ticket_normalizes_array_id_and_keeps_string_project_id() {
    let mut server = Server::new_async().await;
    let _users = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!([{"id": "1", "firstname": "Ada", "lastname": "L"}]),
        ))
        .create_async()
        .await;

    let add = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.addTicket", "params": {
                "values": {"headline": "T", "projectId": "4", "editorId": "1"}
            }})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([55])))
        .create_async()
        .await;

    let r = call(
        "leantime_create_ticket",
        json!({"projectId": "4", "headline": "T", "editorId": "1"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed, json!({"id": 55}));
    add.assert();
}

// ---------------------------------------------------------------- destructive policy matrix

#[tokio::test]
async fn destructive_allow_executes_without_confirm() {
    let _policy_guard = POLICY_LOCK.lock().await;
    std::env::set_var("LEANTIME_MCP_DESTRUCTIVE_POLICY", "allow");
    let mut server = Server::new_async().await;
    let m = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!(true)))
        .create_async()
        .await;
    let r = call(
        "leantime_delete_ticket",
        json!({"ticketId": "7"}),
        &server.url(),
    )
    .await;
    std::env::set_var("LEANTIME_MCP_DESTRUCTIVE_POLICY", "ask");
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed, json!({"deleted": true, "ticketId": "7"}));
    m.assert(); // executed without any confirm
}

#[tokio::test]
async fn destructive_invalid_policy_falls_back_to_ask() {
    let _policy_guard = POLICY_LOCK.lock().await;
    std::env::set_var("LEANTIME_MCP_DESTRUCTIVE_POLICY", "nonsense");
    let server = Server::new_async().await;
    let r = call(
        "leantime_delete_ticket",
        json!({"ticketId": "7"}),
        &server.url(),
    )
    .await;
    std::env::set_var("LEANTIME_MCP_DESTRUCTIVE_POLICY", "ask");
    let (is_err, text) = parse(&r);
    assert!(is_err);
    assert_eq!(text, json!("Confirmation required: ask the user for EXPLICIT approval to delete this ticket, then retry with confirm: true. NEVER pass confirm: true without the user's explicit consent."));
}

#[tokio::test]
async fn every_destructive_tool_is_gated() {
    let _policy_guard = POLICY_LOCK.lock().await;
    // No mock server URL is ever contacted: the gate must refuse before any call.
    let cases = [
        ("leantime_delete_ticket", json!({"ticketId": "1"})),
        ("leantime_delete_milestone", json!({"milestoneId": "1"})),
        ("leantime_delete_comment", json!({"commentId": "1"})),
        ("leantime_delete_timesheet_entry", json!({"entryId": "1"})),
    ];
    for (name, args) in cases {
        let r = call(name, args, "http://127.0.0.1:1").await; // unroutable: any call would error differently
        let (is_err, text) = parse(&r);
        assert!(is_err, "{} not gated", name);
        let msg = text.as_str().unwrap_or_default();
        assert!(msg.contains("Confirmation required"), "{}: {}", name, msg);
    }
}

// ---------------------------------------------------------------- searchCriteria mapping

#[tokio::test]
async fn list_tickets_wraps_all_filters_in_search_criteria() {
    let mut server = Server::new_async().await;
    let m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getAll", "params": {
                "searchCriteria": {
                    "currentProject": "4", "status": "3", "milestone": "9",
                    "sprint": "2", "users": "1", "type": "task", "term": "search me"
                },
                "limit": 500
            }})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([])))
        .create_async()
        .await;

    let r = call(
        "leantime_list_tickets",
        json!({
            "projectId": "4", "status": "3", "milestoneId": "9",
            "sprintId": "2", "userId": "1", "type": "task", "search": "search me"
        }),
        &server.url(),
    )
    .await;
    assert!(!parse(&r).0);
    m.assert();
}

#[tokio::test]
async fn list_tickets_omits_absent_filters() {
    let mut server = Server::new_async().await;
    let m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"params": {"searchCriteria": {"currentProject": "4"}, "limit": 500}})
                .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([])))
        .create_async()
        .await;

    let r = call(
        "leantime_list_tickets",
        json!({"projectId": "4"}),
        &server.url(),
    )
    .await;
    assert!(!parse(&r).0);
    m.assert();
}

#[tokio::test]
async fn update_ticket_sends_only_changed_fields() {
    let mut server = Server::new_async().await;
    let _get = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getTicket", "params": {"id": "9"}}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!({"id": "9", "projectId": "3", "headline": "Old", "status": "3"}),
        ))
        .create_async()
        .await;
    let _st = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getStatusLabels"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({"3": {"name": "Done", "statusType": "DONE"}})))
        .create_async()
        .await;
    let m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.patch", "params": {
                "id": "9", "params": {"headline": "New"}
            }})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!(true)))
        .create_async()
        .await;

    let r = call(
        "leantime_update_ticket",
        json!({"ticketId": "9", "headline": "New"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["ok"], json!(true));
    assert_eq!(parsed["id"], json!("9"));
    // envelope: headline changed Old → New; status not provided → absent
    let changed = parsed["changed"].as_array().unwrap();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0]["field"], json!("headline"));
    assert_eq!(changed[0]["from"], json!("Old"));
    assert_eq!(changed[0]["to"], json!("New"));
    m.assert();
}

#[tokio::test]
async fn create_ticket_maps_renamed_fields() {
    let mut server = Server::new_async().await;
    let _users = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!([{"id": "1", "firstname": "Ada", "lastname": "L"}]),
        ))
        .create_async()
        .await;

    let m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.addTicket", "params": {"values": {
                "headline": "T", "projectId": "4", "editorId": "1",
                "milestoneid": "9", "sprint": "2"
            }}})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([77])))
        .create_async()
        .await;

    let r = call(
        "leantime_create_ticket",
        json!({
            "projectId": "4", "headline": "T", "editorId": "1",
            "milestoneId": "9", "sprintId": "2"
        }),
        &server.url(),
    )
    .await;
    assert!(!parse(&r).0);
    m.assert(); // milestoneId → milestoneid, sprintId → sprint
}

// ---------------------------------------------------------------- bulk unit tests

#[tokio::test]
async fn bulk_create_happy_path_markdown_and_subtask_link() {
    let mut server = Server::new_async().await;
    let _users = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!([{"id": "1", "firstname": "Ada", "lastname": "L"}]),
        ))
        .create_async()
        .await;

    let _t1 = server.mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"params": {"values": {"headline": "one", "description": "<p>desc <strong>b</strong></p>"}}}).to_string()))
        .with_status(200).with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([101]))).create_async().await;
    let _t2 = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"params": {"values": {"headline": "sub", "dependingTicketId": "101"}}})
                .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([102])))
        .create_async()
        .await;

    let r = call(
        "leantime_bulk_create_tickets",
        json!({
            "projectId": "4",
            "tickets": [
                {"headline": "one", "editorId": "1", "description": "desc **b**"},
                {"headline": "sub", "unassigned": true, "dependingTicketId": "101"},
            ]
        }),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(
        parsed["summary"],
        json!({"total": 2, "created": 2, "failed": 0})
    );
    assert_eq!(parsed["results"][0]["id"], json!("101"));
}

#[tokio::test]
async fn bulk_create_partial_failure_summary() {
    let mut server = Server::new_async().await;
    let _users = server
        .mock("POST", "/api/jsonrpc")
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!([{"id": "1", "firstname": "Ada", "lastname": "L"}]),
        ))
        .create_async()
        .await;

    let _ok1 = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"params": {"values": {"headline": "good one"}}}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([201])))
        .create_async()
        .await;
    let _bad = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"params": {"values": {"headline": "bad one"}}}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(
            json!({"jsonrpc": "2.0", "error": {"code": -32000, "message": "boom"}, "id": 2})
                .to_string(),
        )
        .create_async()
        .await;
    let _ok2 = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"params": {"values": {"headline": "good two"}}}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([203])))
        .create_async()
        .await;

    let r = call(
        "leantime_bulk_create_tickets",
        json!({
            "projectId": "4",
            "tickets": [
                {"headline": "good one", "unassigned": true},
                {"headline": "bad one", "unassigned": true},
                {"headline": "good two", "unassigned": true},
            ]
        }),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(
        parsed["summary"],
        json!({"total": 3, "created": 2, "failed": 1})
    );
    assert!(parsed["results"][1]["error"]
        .as_str()
        .unwrap_or("")
        .contains("boom"));
}

#[tokio::test]
async fn bulk_schedule_sends_sprint_and_dates_patch() {
    let mut server = Server::new_async().await;
    let m = server.mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"params": {"id": "9", "params": {"sprint": "2", "editFrom": "2026-01-05", "editTo": "2026-01-10"}}}).to_string()))
        .with_status(200).with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!(true))).create_async().await;

    let r = call("leantime_bulk_schedule_tickets", json!({
        "projectId": "4",
        "schedules": [{"ticketId": "9", "sprintId": "2", "editFrom": "2026-01-05", "editTo": "2026-01-10"}]
    }), &server.url()).await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["summary"]["created"], json!(1));
    m.assert();
}

// ---------------------------------------------------------------- comment crash recovery

#[tokio::test]
async fn add_comment_recovers_from_post_insert_notification_crash() {
    let mut server = Server::new_async().await;
    let _get = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getTicket"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({"id": "9", "type": "task", "headline": "T"})))
        .create_async()
        .await;

    // Leantime v3.7.3: the comment row is inserted, then the notification build
    // crashes — the HTTP call fails although the comment landed.
    let _add = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.comments.addComment"}).to_string(),
        ))
        .with_status(500)
        .create_async()
        .await;

    let html = markdown_to_html("landed **anyway**");
    let _verify = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.comments.getComments"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([{"id": "5", "text": html}])))
        .create_async()
        .await;

    let r = call(
        "leantime_add_comment",
        json!({"ticketId": "9", "text": "landed **anyway**"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "crash recovery failed: {:?}", parsed);
    assert_eq!(parsed, json!({"ok": true, "ticketId": "9"}));
}

use leantmcp::markdown::markdown_to_html;

// ---------------------------------------------------------------- log_time daily cap

#[tokio::test]
async fn log_time_rejects_over_24_hours() {
    let server = Server::new_async().await;
    let r = call(
        "leantime_log_time",
        json!({"ticketId": "9", "hours": 25}),
        &server.url(),
    )
    .await;
    let (is_err, text) = parse(&r);
    assert!(is_err);
    assert_eq!(
        text,
        json!("Hours must be at most 24 for a single day's entry (got 25). Split the entry across days or fix the value.")
    );
}

#[tokio::test]
async fn log_time_rejects_absurd_hours() {
    let server = Server::new_async().await;
    let r = call(
        "leantime_log_time",
        json!({"ticketId": "9", "hours": 1e308}),
        &server.url(),
    )
    .await;
    let (is_err, text) = parse(&r);
    assert!(is_err);
    let msg = text.as_str().unwrap_or_default();
    assert!(msg.contains("at most 24"), "{}", msg);
}

#[tokio::test]
async fn log_time_accepts_exactly_24_hours() {
    let mut server = Server::new_async().await;
    let m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.timesheets.logTime", "params": {
                "ticketId": "9", "params": {"hours": 24.0}
            }})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!(true)))
        .create_async()
        .await;

    let r = call(
        "leantime_log_time",
        json!({"ticketId": "9", "hours": 24.0, "date": "2026-01-15"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    m.assert();
}

// ---------------------------------------------------------------- project_context

/// Status map used by the project_context tests: 3=Done, 2=In Progress, 4=Blocked.
fn ctx_status_map() -> Value {
    json!({
        "3": {"name": "Done", "statusType": "DONE"},
        "2": {"name": "In Progress", "statusType": "IN_PROGRESS"},
        "4": {"name": "Blocked", "statusType": "BLOCKED"}
    })
}

/// Wire up the six happy-path mocks. Creation order matters: the milestone
/// tickets.getAll mock is created AFTER the main one so it takes precedence
/// for the request carrying `"type": "milestone"` (mockito: newest wins).
async fn ctx_happy_mocks(
    server: &mut Server,
    sprints: Value,
    milestones: Value,
) -> tokio::sync::MutexGuard<'static, ()> {
    // Hold the fetch-limit lock: parallel env mutation in the override test
    // would change fetch_limit() and break these limit-pinned mocks.
    let _guard = FETCH_LIMIT_LOCK.lock().await;
    let _p = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.projects.getProject", "params": {"id": "3"}})
                .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!({"id": 3, "name": "Vision", "state": "active"}),
        ))
        .create_async()
        .await;
    let _prog = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.projects.getProjectProgress"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({"percentdone": "62.5"})))
        .create_async()
        .await;
    // Main ticket set — 4 tickets exercising every health/milestone path:
    // 101 done (score 4.0), 102 overdue+unassigned (4.5), 103 blocked via
    // camelCase milestoneId (6.0), 104 done with a past due date (excluded).
    let _main = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getAll", "params": {
                "searchCriteria": {"currentProject": "3"}, "limit": 10000
            }})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([
            {"id": "101", "type": "task", "status": "3", "editorId": "1", "sprint": "7",
             "headline": "Done thing", "date": "2026-09-05 10:00:00",
             "milestoneid": "260", "storypoints": "2", "priority": "1"},
            {"id": "102", "type": "task", "status": "2", "editorId": "", "sprint": "7",
             "headline": "Overdue unassigned", "date": "2026-09-06 10:00:00",
             "dateToFinish": "2020-01-01", "milestoneid": 260, "storypoints": "", "priority": ""},
            {"id": "103", "type": "task", "status": "4", "editorId": "2", "sprint": "7",
             "headline": "Blocked item", "date": "2026-09-04 10:00:00",
             "milestoneId": "260", "storypoints": "4", "priority": "3"},
            {"id": "104", "type": "bug", "status": "3", "editorId": "2",
             "headline": "Done with past due date", "date": "2026-09-01 10:00:00",
             "dateToFinish": "2020-01-01"}
        ])))
        .create_async()
        .await;
    let _ms = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getAll", "params": {
                "searchCriteria": {"currentProject": "3", "type": "milestone"}
            }})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(milestones))
        .create_async()
        .await;
    let _sp = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.sprints.getAllSprints"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(sprints))
        .create_async()
        .await;
    let _st = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getStatusLabels"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(ctx_status_map()))
        .create_async()
        .await;
    _guard
}

#[tokio::test]
async fn project_context_happy_path_full_shape() {
    let mut server = Server::new_async().await;
    let _ctx_guard = ctx_happy_mocks(
        &mut server,
        json!([
            {"id": "7", "name": "Sprint 4", "startDate": "2026-01-01", "endDate": "2030-12-31"},
            {"id": "8", "name": "Far future", "startDate": "2035-06-01", "endDate": "2035-07-01"}
        ]),
        json!([{"id": "260", "headline": "Phase 1", "type": "milestone", "status": "3"}]),
    )
    .await;

    let r = call(
        "leantime_project_context",
        json!({"projectId": "3"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);

    assert!(parsed["generatedAt"].is_string());
    assert_eq!(parsed["project"]["name"], json!("Vision"));
    assert_eq!(parsed["project"]["state"], json!("active"));
    assert_eq!(parsed["project"]["progress"]["percentDone"], json!(62.5));
    assert_eq!(parsed["project"]["progress"]["ticketsTotal"], json!(4));
    assert_eq!(parsed["project"]["progress"]["ticketsDone"], json!(2));

    assert_eq!(parsed["health"]["blocked"], json!(1));
    assert_eq!(parsed["health"]["overdue"], json!(1)); // 102 only — 104 is DONE
    assert_eq!(parsed["health"]["unassigned"], json!(1));
    assert_eq!(parsed["health"]["openTotal"], json!(2));

    assert_eq!(parsed["currentSprint"]["name"], json!("Sprint 4"));
    assert_eq!(parsed["currentSprint"]["status"], json!("current"));
    assert!(parsed["currentSprint"]["daysRemaining"].is_u64());
    assert!(parsed["currentSprint"].get("daysUntilStart").is_none());
    assert_eq!(parsed["currentSprint"]["openTickets"], json!(2));

    let ms = &parsed["milestones"];
    assert_eq!(ms.as_array().map(|a| a.len()), Some(1));
    assert_eq!(ms[0]["id"], json!("260"));
    assert_eq!(ms[0]["name"], json!("Phase 1"));
    assert_eq!(ms[0]["status"], json!("done"));
    // Weighted: done 2×2.0=4.0 of 4.0+4.5+6.0=14.5 → 27.6%
    assert_eq!(ms[0]["percentDone"], json!(27.6));
    assert_eq!(ms[0]["tickets"], json!(3)); // includes camelCase milestoneId

    assert_eq!(
        parsed["ticketSummary"]["byStatus"],
        json!({"Done": 2, "In Progress": 1, "Blocked": 1})
    );
    assert_eq!(
        parsed["ticketSummary"]["byType"],
        json!({"task": 3, "bug": 1})
    );

    let activity = parsed["recentActivity"].as_array().unwrap();
    assert_eq!(activity.len(), 4);
    assert_eq!(
        activity[0]["what"],
        json!("Ticket #102 — Overdue unassigned")
    );
    assert_eq!(activity[0]["when"], json!("2026-09-06"));
}

#[tokio::test]
async fn project_context_output_under_4kb() {
    let mut server = Server::new_async().await;
    // 25 milestones → cap at 20 + note; size must stay < 4096 bytes.
    let milestones: Vec<Value> = (0..25)
        .map(|i| json!({"id": format!("{}", 260 + i), "headline": format!("Phase {} — a reasonably long milestone name for sizing", i), "type": "milestone", "status": "1"}))
        .collect();
    let _ctx_guard = ctx_happy_mocks(&mut server, json!([]), Value::Array(milestones)).await;

    let r = call(
        "leantime_project_context",
        json!({"projectId": "3"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["milestones"].as_array().map(|a| a.len()), Some(15));
    assert_eq!(
        parsed["milestonesNote"],
        json!("25 total, showing first 15")
    );
    let size = serde_json::to_string_pretty(&parsed).unwrap().len();
    assert!(size < 4096, "output is {} bytes", size);
}

#[tokio::test]
async fn project_context_no_sprints_current_null() {
    let mut server = Server::new_async().await;
    let _ctx_guard = ctx_happy_mocks(&mut server, json!([]), json!([])).await;

    let r = call(
        "leantime_project_context",
        json!({"projectId": "3"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["currentSprint"], json!(null));
    assert_eq!(parsed["milestones"], json!([]));
}

#[tokio::test]
async fn project_context_upcoming_sprint_shape() {
    let mut server = Server::new_async().await;
    let _ctx_guard = ctx_happy_mocks(
        &mut server,
        json!([
            {"id": "1", "name": "past", "startDate": "2020-01-01", "endDate": "2020-02-01"},
            {"id": "9", "name": "far", "startDate": "2035-06-01", "endDate": "2035-07-01"},
            {"id": "5", "name": "soon", "startDate": "2027-01-01", "endDate": "2027-02-01"}
        ]),
        json!([]),
    )
    .await;

    let r = call(
        "leantime_project_context",
        json!({"projectId": "3"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["currentSprint"]["name"], json!("soon")); // earliest upcoming
    assert_eq!(parsed["currentSprint"]["status"], json!("upcoming"));
    assert!(parsed["currentSprint"]["daysUntilStart"].is_u64());
    assert!(parsed["currentSprint"].get("daysRemaining").is_none());
}

#[tokio::test]
async fn project_context_overdue_excludes_done() {
    let mut server = Server::new_async().await;
    let _ctx_guard = ctx_happy_mocks(&mut server, json!([]), json!([])).await;

    let r = call(
        "leantime_project_context",
        json!({"projectId": "3"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    // 104 has dateToFinish 2020-01-01 but status DONE → not overdue.
    // (Also asserted in the happy path; this pins the rule in isolation.)
    assert_eq!(parsed["health"]["overdue"], json!(1));
}

#[tokio::test]
async fn project_context_include_milestones_false_omits_section() {
    let mut server = Server::new_async().await;
    // No milestone mock: the flag must prevent the milestone fetch entirely.
    let _ctx_guard = ctx_happy_mocks(&mut server, json!([]), json!([])).await;

    let r = call(
        "leantime_project_context",
        json!({"projectId": "3", "includeMilestones": false, "includeRecentActivity": false}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert!(parsed.get("milestones").is_none());
    assert!(parsed.get("recentActivity").is_none());
    assert!(parsed.get("ticketSummary").is_some());
}

#[tokio::test]
async fn project_context_computes_progress_when_official_absent() {
    let mut server = Server::new_async().await;
    let _ctx_guard = ctx_happy_mocks(&mut server, json!([]), json!([])).await;
    // The progress mock in the helper answers 62.5; override the computation
    // path by asserting the official value flows through — the fallback is
    // exercised implicitly by the no-data tests (0 tickets → 0.0).
    let r = call(
        "leantime_project_context",
        json!({"projectId": "3"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["project"]["progress"]["percentDone"], json!(62.5));
}

#[tokio::test]
async fn project_context_project_not_found() {
    let mut server = Server::new_async().await;
    let _m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.projects.getProject"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!(false)))
        .create_async()
        .await;

    let r = call(
        "leantime_project_context",
        json!({"projectId": "999"}),
        &server.url(),
    )
    .await;
    let (is_err, text) = parse(&r);
    assert!(is_err);
    assert_eq!(text, json!("Project 999 not found."));
}

#[tokio::test]
async fn project_context_reads_both_milestone_spellings() {
    // The happy path already mixes "milestoneid" (string + number) and
    // camelCase "milestoneId" — this pins the regression: all three
    // tickets must be attributed to milestone 260's progress.
    let mut server = Server::new_async().await;
    let _ctx_guard = ctx_happy_mocks(
        &mut server,
        json!([]),
        json!([{"id": "260", "headline": "Phase 1", "type": "milestone", "status": "3"}]),
    )
    .await;

    let r = call(
        "leantime_project_context",
        json!({"projectId": "3"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["milestones"][0]["tickets"], json!(3));
    assert_eq!(parsed["milestones"][0]["percentDone"], json!(27.6));
}

#[tokio::test]
async fn project_context_zero_storypoints_fall_back_to_default_effort() {
    // Restored/unestimated tickets carry storypoints: 0 — they must weigh
    // the default 3.0, not zero the whole milestone's progress.
    let _guard = FETCH_LIMIT_LOCK.lock().await; // mocks pin the default limit
    let mut server = Server::new_async().await;
    // Replace the main ticket set: two tasks under milestone 260, both DONE,
    // storypoints 0 → each weighs 3.0 × 1.5 = 4.5 → 100%.
    let _main = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getAll", "params": {
                "searchCriteria": {"currentProject": "3"}, "limit": 10000
            }})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([
            {"id": "201", "type": "task", "status": "3", "editorId": "1",
             "headline": "A", "date": "2026-09-05 10:00:00",
             "milestoneid": "260", "storypoints": 0, "priority": 0},
            {"id": "202", "type": "task", "status": "3", "editorId": "1",
             "headline": "B", "date": "2026-09-06 10:00:00",
             "milestoneid": "260", "storypoints": "0", "priority": ""}
        ])))
        .create_async()
        .await;
    let _p = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.projects.getProject", "params": {"id": "3"}})
                .to_string(),
        ))
        .with_body(rpc_ok(
            json!({"id": 3, "name": "Vision", "state": "active"}),
        ))
        .create_async()
        .await;
    let _prog = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.projects.getProjectProgress"}).to_string(),
        ))
        .with_body(rpc_ok(json!({"percentdone": "100.0"})))
        .create_async()
        .await;
    let _ms = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getAll", "params": {
                "searchCriteria": {"currentProject": "3", "type": "milestone"}
            }})
            .to_string(),
        ))
        .with_body(rpc_ok(
            json!([{"id": "260", "headline": "Phase 1", "type": "milestone", "status": "3"}]),
        ))
        .create_async()
        .await;
    let _sp = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.sprints.getAllSprints"}).to_string(),
        ))
        .with_body(rpc_ok(json!([])))
        .create_async()
        .await;
    let _st = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getStatusLabels"}).to_string(),
        ))
        .with_body(rpc_ok(ctx_status_map()))
        .create_async()
        .await;

    let r = call(
        "leantime_project_context",
        json!({"projectId": "3"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["milestones"][0]["percentDone"], json!(100.0));
    assert_eq!(parsed["milestones"][0]["tickets"], json!(2));
}

// ---------------------------------------------------------------- dry-run

/// users.getAll mock with a single user: id "1" = Ada Lovelace.
async fn dr_users_mock(server: &mut Server) {
    let _u = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.users.getAll"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!([{"id": "1", "firstname": "Ada", "lastname": "Lovelace"}]),
        ))
        .create_async()
        .await;
}

#[tokio::test]
async fn dry_run_create_ticket_valid_no_write() {
    let mut server = Server::new_async().await;
    dr_users_mock(&mut server).await;
    // No addTicket mock: if the handler tried to write, mockito would reject
    // the request and the result would be an error.

    let r = call(
        "leantime_create_ticket",
        json!({"projectId": "3", "headline": "Dry run me", "editorId": "1", "dryRun": true}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["dryRun"], json!(true));
    assert_eq!(parsed["valid"], json!(true));
    assert_eq!(parsed["errors"], json!([]));
    let fields: Vec<String> = parsed["changes"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|c| c["field"].as_str().map(|s| s.to_string()))
        .collect();
    assert!(fields.contains(&"headline".to_string()), "{:?}", fields);
    assert!(fields.contains(&"editorId".to_string()), "{:?}", fields);
    assert!(fields.contains(&"projectId".to_string()), "{:?}", fields);
}

#[tokio::test]
async fn dry_run_create_ticket_invalid_editor_id() {
    let mut server = Server::new_async().await;
    dr_users_mock(&mut server).await;

    let r = call(
        "leantime_create_ticket",
        json!({"projectId": "3", "headline": "X", "editorId": "999", "dryRun": true}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed); // failed validation ≠ MCP error
    assert_eq!(parsed["valid"], json!(false));
    let msg = parsed["errors"][0].as_str().unwrap_or_default();
    assert!(msg.contains("editorId \"999\" does not exist"), "{}", msg);
    assert!(msg.contains("Available users"), "{}", msg);
}

#[tokio::test]
async fn dry_run_create_ticket_missing_assignment() {
    let mut server = Server::new_async().await;
    dr_users_mock(&mut server).await;

    let r = call(
        "leantime_create_ticket",
        json!({"projectId": "3", "headline": "X", "dryRun": true}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["valid"], json!(false));
    let msg = parsed["errors"][0].as_str().unwrap_or_default();
    assert!(msg.contains("Assignment required"), "{}", msg);
}

#[tokio::test]
async fn dry_run_update_ticket_from_to_with_status_label() {
    let mut server = Server::new_async().await;
    let _t = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getTicket", "params": {"id": "123"}})
                .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!({"id": "123", "projectId": "3", "headline": "Old", "status": "3"}),
        ))
        .create_async()
        .await;
    let _st = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getStatusLabels"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({
            "3": {"name": "Done", "statusType": "DONE"},
            "2": {"name": "In Progress", "statusType": "IN_PROGRESS"}
        })))
        .create_async()
        .await;
    // No tickets.patch mock: a write attempt would fail the test.

    let r = call(
        "leantime_update_ticket",
        json!({"ticketId": "123", "status": 2, "dryRun": true}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["valid"], json!(true));
    assert_eq!(parsed["changes"][0]["field"], json!("status"));
    assert_eq!(parsed["changes"][0]["from"], json!("3"));
    assert_eq!(parsed["changes"][0]["to"], json!(2));
    assert_eq!(parsed["changes"][0]["label"], json!("Done → In Progress"));
    assert_eq!(parsed["warnings"], json!([]));
}

#[tokio::test]
async fn dry_run_update_ticket_same_value_warns() {
    let mut server = Server::new_async().await;
    let _t = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getTicket", "params": {"id": "123"}})
                .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({"id": "123", "projectId": "3", "status": 3})))
        .create_async()
        .await;
    let _st = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getStatusLabels"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({"3": {"name": "Done", "statusType": "DONE"}})))
        .create_async()
        .await;

    let r = call(
        "leantime_update_ticket",
        json!({"ticketId": "123", "status": "3", "dryRun": true}), // "3" vs 3 — loose equality
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["valid"], json!(true));
    let w = parsed["warnings"][0].as_str().unwrap_or_default();
    assert!(w.contains("status already has this value"), "{}", w);
}

#[tokio::test]
async fn dry_run_absent_executes_mutation() {
    let mut server = Server::new_async().await;
    dr_users_mock(&mut server).await;
    let m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.addTicket"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([777])))
        .create_async()
        .await;

    let r = call(
        "leantime_create_ticket",
        json!({"projectId": "3", "headline": "Real write", "editorId": "1"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed, json!({"id": 777}));
    m.assert(); // the write DID happen
}

#[tokio::test]
async fn dry_run_log_time_accumulates_local_errors() {
    let server = Server::new_async().await;
    // No mocks at all: log_time dry-run must be 100% local.
    let r = call(
        "leantime_log_time",
        json!({"ticketId": "9", "hours": 30, "kind": "NOPE", "dryRun": true}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["valid"], json!(false));
    let errors: Vec<String> = parsed["errors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e.as_str().map(|s| s.to_string()))
        .collect();
    assert_eq!(errors.len(), 2, "{:?}", errors); // both problems, not just the first
    assert!(
        errors.iter().any(|e| e.contains("at most 24")),
        "{:?}",
        errors
    );
    assert!(
        errors.iter().any(|e| e.contains("Invalid kind")),
        "{:?}",
        errors
    );
}

#[tokio::test]
async fn dry_run_log_time_valid_echoes_entry() {
    let server = Server::new_async().await;
    let r = call(
        "leantime_log_time",
        json!({"ticketId": "9", "hours": 2.5, "kind": "TESTING", "date": "2026-09-07", "dryRun": true}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["valid"], json!(true));
    assert_eq!(parsed["changes"][0]["field"], json!("timesheet entry"));
    assert_eq!(parsed["changes"][0]["to"]["hours"], json!(2.5));
    assert_eq!(parsed["changes"][0]["to"]["kind"], json!("TESTING"));
}

#[tokio::test]
async fn dry_run_bulk_create_reports_items_without_writing() {
    let mut server = Server::new_async().await;
    dr_users_mock(&mut server).await;

    let r = call(
        "leantime_bulk_create_tickets",
        json!({"projectId": "3", "dryRun": true, "tickets": [
            {"headline": "Good one", "editorId": "1"},
            {"headline": "Bad editor", "editorId": "999"},
            {"headline": "No assignment"}
        ]}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["valid"], json!(false));
    let errors: Vec<String> = parsed["errors"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|e| e.as_str().map(|s| s.to_string()))
        .collect();
    assert_eq!(errors.len(), 2, "{:?}", errors);
    assert!(errors[0].contains("Item 2"), "{}", errors[0]);
    assert!(errors[0].contains("does not exist"), "{}", errors[0]);
    assert!(errors[1].contains("Item 3"), "{}", errors[1]);
    assert!(errors[1].contains("assignment required"), "{}", errors[1]);
}

#[tokio::test]
async fn dry_run_bulk_update_items_preview() {
    let server = Server::new_async().await;
    // No editorId in any item → no users fetch; no patch mock: no writes.
    let r = call(
        "leantime_bulk_update_tickets",
        json!({"projectId": "3", "dryRun": true, "updates": [
            {"ticketId": "10", "status": 3},
            {"ticketId": "11"}
        ]}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["valid"], json!(false));
    assert_eq!(parsed["items"][0]["id"], json!("10"));
    assert_eq!(parsed["items"][0]["valid"], json!(true));
    assert_eq!(parsed["items"][0]["fields"][0]["field"], json!("status"));
    assert_eq!(parsed["items"][1]["valid"], json!(false));
    assert_eq!(
        parsed["items"][1]["errors"][0],
        json!("no fields to update")
    );
}

#[tokio::test]
async fn dry_run_guidance_present_in_mutation_descriptions() {
    // The guidance in tool descriptions is what makes agents dry-run
    // spontaneously — pin it so a refactor can't silently drop it.
    let expect: &[(&str, &str)] = &[
        (
            "leantime_create_ticket",
            "Prefer dryRun: true first when you chose or inferred any value",
        ),
        (
            "leantime_update_ticket",
            "Prefer dryRun: true first when you interpreted the request or chose values yourself",
        ),
        (
            "leantime_create_milestone",
            "Prefer dryRun: true first when you chose or inferred any value",
        ),
        (
            "leantime_update_milestone",
            "Prefer dryRun: true first when you interpreted the request or chose values yourself",
        ),
        (
            "leantime_bulk_create_tickets",
            "ALWAYS call with dryRun: true first",
        ),
        (
            "leantime_bulk_update_tickets",
            "ALWAYS call with dryRun: true first",
        ),
    ];
    for (name, needle) in expect {
        let t = get_tool(name);
        assert!(
            t.description.contains(needle),
            "{} description lost its dry-run guidance",
            name
        );
    }
    // log_time stays guidance-free by design.
    let lt = get_tool("leantime_log_time");
    assert!(!lt.description.contains("dryRun: true first"));
}

// ---------------------------------------------------------------- fetch limit

/// Env vars are process-global: tests that set LEANTIME_MCP_FETCH_LIMIT
/// hold this lock (same pattern as POLICY_LOCK).
static FETCH_LIMIT_LOCK: std::sync::LazyLock<tokio::sync::Mutex<()>> =
    std::sync::LazyLock::new(|| tokio::sync::Mutex::new(()));

#[tokio::test]
async fn fetch_limit_env_override_reaches_the_wire() {
    let _guard = FETCH_LIMIT_LOCK.lock().await;
    std::env::set_var("LEANTIME_MCP_FETCH_LIMIT", "777");
    let mut server = Server::new_async().await;
    // Support mocks (limit-independent)
    let _p = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.projects.getProject"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!({"id": 3, "name": "Vision", "state": "active"}),
        ))
        .create_async()
        .await;
    let _prog = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.projects.getProjectProgress"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({"percentdone": "0"})))
        .create_async()
        .await;
    let _sp = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.sprints.getAllSprints"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([])))
        .create_async()
        .await;
    let _st = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getStatusLabels"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({})))
        .create_async()
        .await;
    let m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getAll", "params": {
                "searchCriteria": {"currentProject": "3"}, "limit": 777
            }})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([])))
        .create_async()
        .await;
    // The milestone-fetch mock must ALSO expect 777 (mockito: newest wins,
    // but keeping them coherent documents the behavior).
    let _ms = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getAll", "params": {
                "searchCriteria": {"currentProject": "3", "type": "milestone"}, "limit": 777
            }})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([])))
        .create_async()
        .await;

    let r = call(
        "leantime_project_context",
        json!({"projectId": "3"}),
        &server.url(),
    )
    .await;
    // Remove BEFORE any assertion so a panic can't leak the env var.
    std::env::remove_var("LEANTIME_MCP_FETCH_LIMIT");
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    m.assert();
}

#[tokio::test]
async fn fetch_limit_default_10000_on_the_wire() {
    let _guard = FETCH_LIMIT_LOCK.lock().await; // serialize with the override test
    std::env::remove_var("LEANTIME_MCP_FETCH_LIMIT");
    let mut server = Server::new_async().await;
    let _p = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.projects.getProject"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!({"id": 3, "name": "Vision", "state": "active"}),
        ))
        .create_async()
        .await;
    let _prog = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.projects.getProjectProgress"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({"percentdone": "0"})))
        .create_async()
        .await;
    let _st = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getStatusLabels"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({})))
        .create_async()
        .await;
    let m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getAll", "params": {
                "searchCriteria": {"currentProject": "3"}, "limit": 10000
            }})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([])))
        .create_async()
        .await;

    let r = call(
        "leantime_project_context",
        json!({"projectId": "3", "includeMilestones": false, "includeRecentActivity": false}),
        &server.url(),
    )
    .await;
    let (is_err, _) = parse(&r);
    assert!(!is_err);
    m.assert();
}

#[tokio::test]
async fn list_tickets_notes_truncation_at_cap() {
    let mut server = Server::new_async().await;
    // Exactly LIST_TICKETS_LIMIT (500) items → response becomes
    // {tickets: [...], note: "showing first 500 — refine filters…"}.
    let items: Vec<Value> = (0..500)
        .map(|i| json!({"id": format!("{}", i), "type": "task", "status": "3", "headline": "x"}))
        .collect();
    let m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getAll", "params": {
                "searchCriteria": {"currentProject": "3"}, "limit": 500
            }})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(Value::Array(items)))
        .create_async()
        .await;
    let _st = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getStatusLabels"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({"3": {"name": "Done", "statusType": "DONE"}})))
        .create_async()
        .await;

    let r = call(
        "leantime_list_tickets",
        json!({"projectId": "3"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    assert!(parsed.get("tickets").is_some(), "wrapped shape expected");
    assert_eq!(parsed["tickets"].as_array().map(|a| a.len()), Some(500));
    assert_eq!(parsed["returned"], json!(500));
    assert_eq!(parsed["truncated"], json!(true));
    m.assert();
}

// ---------------------------------------------------------------- status filter resolution

#[tokio::test]
async fn list_tickets_resolves_status_label_to_id() {
    let mut server = Server::new_async().await;
    let _st = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getStatusLabels"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({
            "3": {"name": "New", "statusType": "NEW"},
            "4": {"name": "In Progress", "statusType": "IN_PROGRESS"},
            "0": {"name": "Done", "statusType": "DONE"}
        })))
        .create_async()
        .await;
    let m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getAll", "params": {
                "searchCriteria": {"currentProject": "15", "status": "3"}, "limit": 500
            }})
            .to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([])))
        .create_async()
        .await;

    let r = call(
        "leantime_list_tickets",
        json!({"projectId": "15", "status": "New"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    m.assert(); // the wire carried the resolved ID, not the label
}

#[tokio::test]
async fn list_tickets_status_resolution_is_case_insensitive() {
    let mut server = Server::new_async().await;
    let _st = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getStatusLabels"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({
            "3": {"name": "New", "statusType": "NEW"},
            "4": {"name": "In Progress", "statusType": "IN_PROGRESS"}
        })))
        .create_async()
        .await;
    let m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"params": {"searchCriteria": {"status": "4"}, "limit": 500}}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([])))
        .create_async()
        .await;

    let r = call(
        "leantime_list_tickets",
        json!({"projectId": "15", "status": "in progress"}),
        &server.url(),
    )
    .await;
    let (is_err, _) = parse(&r);
    assert!(!is_err);
    m.assert();
}

#[tokio::test]
async fn list_tickets_resolves_csv_of_labels() {
    let mut server = Server::new_async().await;
    let _st = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getStatusLabels"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({
            "3": {"name": "New", "statusType": "NEW"},
            "0": {"name": "Done", "statusType": "DONE"}
        })))
        .create_async()
        .await;
    let m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"params": {"searchCriteria": {"status": "3,0"}, "limit": 500}}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([])))
        .create_async()
        .await;

    let r = call(
        "leantime_list_tickets",
        json!({"projectId": "15", "status": "New, Done"}),
        &server.url(),
    )
    .await;
    let (is_err, _) = parse(&r);
    assert!(!is_err);
    m.assert();
}

#[tokio::test]
async fn list_tickets_magic_done_passes_through() {
    // "done"/"not_done" are resolved server-side by statusType — they must
    // reach the wire untouched (and must NOT require a status-map call).
    let mut server = Server::new_async().await;
    let _st = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getStatusLabels"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({"0": {"name": "Done", "statusType": "DONE"}})))
        .create_async()
        .await;
    let m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"params": {"searchCriteria": {"status": "done"}, "limit": 500}}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([])))
        .create_async()
        .await;

    let r = call(
        "leantime_list_tickets",
        json!({"projectId": "15", "status": "done"}),
        &server.url(),
    )
    .await;
    let (is_err, _) = parse(&r);
    assert!(!is_err);
    m.assert();
}

#[tokio::test]
async fn list_tickets_unknown_status_label_errors_without_fetching() {
    let mut server = Server::new_async().await;
    let _st = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getStatusLabels"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({
            "3": {"name": "New", "statusType": "NEW"},
            "0": {"name": "Done", "statusType": "DONE"}
        })))
        .create_async()
        .await;
    // No tickets.getAll mock: any fetch attempt would 501 and change the
    // error message — the resolution error is asserted instead.

    let r = call(
        "leantime_list_tickets",
        json!({"projectId": "15", "status": "Frozen"}),
        &server.url(),
    )
    .await;
    let (is_err, text) = parse(&r);
    assert!(is_err);
    let msg = text.as_str().unwrap_or_default();
    assert!(msg.contains("Unknown status \"Frozen\""), "{}", msg);
    assert!(msg.contains("Valid statuses:"), "{}", msg);
    assert!(msg.contains("3 (New)"), "{}", msg);
    assert!(msg.contains("0 (Done)"), "{}", msg);
    assert!(msg.contains("\"done\" and \"not_done\""), "{}", msg);
}

// ---------------------------------------------------------------- idempotency keys

#[tokio::test]
async fn idempotency_replay_returns_cached_result_without_second_write() {
    let mut server = Server::new_async().await;
    let users = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.users.getAll"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!([{"id": "1", "firstname": "Ada", "lastname": "L"}]),
        ))
        .create_async()
        .await;
    // exactly ONE mutation across both calls — the replay must not write
    let mutation = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.addTicket"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([777])))
        .expect(1)
        .create_async()
        .await;

    let args =
        json!({"projectId": "3", "headline": "Once", "editorId": "1", "idempotencyKey": "op-42"});
    let first = call_idem(
        "leantime_create_ticket",
        args.clone(),
        &server.url(),
        "replay",
    )
    .await;
    let (is_err, parsed) = parse(&first);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["id"], json!(777));
    assert!(
        parsed.get("idempotentReplay").is_none(),
        "first run is not a replay"
    );

    let second = call_idem("leantime_create_ticket", args, &server.url(), "replay").await;
    let (is_err, replayed) = parse(&second);
    assert!(!is_err, "{:?}", replayed);
    assert_eq!(replayed["id"], json!(777));
    assert_eq!(replayed["idempotentReplay"], json!(true));

    mutation.assert(); // one write, not two
    users.assert();
}

#[tokio::test]
async fn idempotency_failed_mutation_does_not_consume_the_key() {
    let mut server = Server::new_async().await;
    let _users = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.users.getAll"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!([{"id": "1", "firstname": "Ada", "lastname": "L"}]),
        ))
        .create_async()
        .await;
    // first attempt: server error → key NOT journaled; retry executes
    let _fail = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.addTicket"}).to_string(),
        ))
        .with_status(500)
        .with_body("boom")
        .expect(1)
        .create_async()
        .await;
    let ok = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.addTicket"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([888])))
        .expect(1)
        .create_async()
        .await;

    let args = json!({"projectId": "3", "headline": "Retry me", "editorId": "1", "idempotencyKey": "op-43"});
    let first = call_idem(
        "leantime_create_ticket",
        args.clone(),
        &server.url(),
        "failretry",
    )
    .await;
    assert!(parse(&first).0, "first attempt must fail (HTTP 500)");

    let second = call_idem("leantime_create_ticket", args, &server.url(), "failretry").await;
    let (is_err, parsed) = parse(&second);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["id"], json!(888));
    ok.assert();
}

#[tokio::test]
async fn idempotency_key_reused_across_tools_is_an_error() {
    let mut server = Server::new_async().await;
    let users = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.users.getAll"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!([{"id": "1", "firstname": "Ada", "lastname": "L"}]),
        ))
        .create_async()
        .await;
    let _create = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.addTicket"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([5])))
        .expect(1)
        .create_async()
        .await;

    // Journal the key under create_ticket…
    call_idem(
        "leantime_create_ticket",
        json!({"projectId": "3", "headline": "A", "editorId": "1", "idempotencyKey": "shared"}),
        &server.url(),
        "mismatch",
    )
    .await;
    // …then reuse it under create_milestone — must be refused, no write.
    let r = call_idem(
        "leantime_create_milestone",
        json!({"projectId": "3", "headline": "M", "editorId": "1", "idempotencyKey": "shared"}),
        &server.url(),
        "mismatch",
    )
    .await;
    let (is_err, text) = parse(&r);
    assert!(is_err);
    let msg = text.as_str().unwrap_or_default();
    assert!(
        msg.contains("already used by leantime_create_ticket"),
        "{}",
        msg
    );
    _create.assert(); // still exactly one mutation overall
    users.assert();
}

#[tokio::test]
async fn idempotency_dry_run_does_not_consume_the_key() {
    let mut server = Server::new_async().await;
    let _users = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.users.getAll"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!([{"id": "1", "firstname": "Ada", "lastname": "L"}]),
        ))
        .create_async()
        .await;
    let mutation = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.addTicket"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([9])))
        .expect(1) // the dry-run makes no mutation call; the real one executes once
        .create_async()
        .await;

    // dry-run with the key: validates, writes nothing, does not consume it
    let dr = call_idem("leantime_create_ticket",
        json!({"projectId": "3", "headline": "X", "editorId": "1", "idempotencyKey": "dry", "dryRun": true}),
        &server.url(), "dryrun").await;
    let (is_err, parsed) = parse(&dr);
    assert!(!is_err && parsed["dryRun"] == json!(true), "{:?}", parsed);

    // real call with the same key: executes (the dry-run consumed nothing)
    let real = call_idem(
        "leantime_create_ticket",
        json!({"projectId": "3", "headline": "X", "editorId": "1", "idempotencyKey": "dry"}),
        &server.url(),
        "dryrun",
    )
    .await;
    let (is_err, parsed) = parse(&real);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["id"], json!(9));
    mutation.assert();
}

#[tokio::test]
async fn idempotency_bulk_create_replays_whole_batch() {
    let mut server = Server::new_async().await;
    let users = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.users.getAll"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!([{"id": "1", "firstname": "Ada", "lastname": "L"}]),
        ))
        .create_async()
        .await;
    let mutation = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.addTicket"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!([31])))
        .expect(2) // two items in the batch — once. The replay adds nothing.
        .create_async()
        .await;

    let args = json!({"projectId": "3", "idempotencyKey": "batch-1", "tickets": [
        {"headline": "T1", "editorId": "1"},
        {"headline": "T2", "editorId": "1"}
    ]});
    let first = call_idem(
        "leantime_bulk_create_tickets",
        args.clone(),
        &server.url(),
        "bulk",
    )
    .await;
    let (is_err, parsed) = parse(&first);
    assert!(!is_err, "{:?}", parsed);
    assert_eq!(parsed["summary"]["created"], json!(2));

    let second = call_idem("leantime_bulk_create_tickets", args, &server.url(), "bulk").await;
    let (is_err, replayed) = parse(&second);
    assert!(!is_err, "{:?}", replayed);
    assert_eq!(replayed["idempotentReplay"], json!(true));
    assert_eq!(replayed["summary"]["created"], json!(2));
    mutation.assert();
    users.assert();
}

// ---------------------------------------------------------------- result envelopes

#[tokio::test]
async fn update_ticket_envelope_reports_changed_and_unchanged() {
    let mut server = Server::new_async().await;
    let _get = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getTicket", "params": {"id": "12"}}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!({"id": "12", "projectId": "3", "headline": "Same", "priority": 2, "status": "0"}),
        ))
        .create_async()
        .await;
    let _st = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getStatusLabels"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({"0": {"name": "New", "statusType": "NEW"}, "3": {"name": "Done", "statusType": "DONE"}})))
        .create_async()
        .await;
    let m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.patch"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!(true)))
        .expect(1)
        .create_async()
        .await;

    // headline "Same" = already that value (unchanged); status 0→3 changes
    let r = call(
        "leantime_update_ticket",
        json!({"ticketId": "12", "headline": "Same", "status": 3}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);

    let changed = parsed["changed"].as_array().unwrap();
    assert_eq!(changed.len(), 1, "{:?}", changed);
    assert_eq!(changed[0]["field"], json!("status"));
    assert_eq!(changed[0]["from"], json!("0"));
    assert_eq!(changed[0]["to"], json!(3));
    assert_eq!(changed[0]["label"], json!("New → Done"));

    let unchanged = parsed["unchanged"].as_array().unwrap();
    assert_eq!(unchanged.len(), 1, "{:?}", unchanged);
    assert_eq!(unchanged[0]["field"], json!("headline"));
    assert_eq!(unchanged[0]["value"], json!("Same"));

    let warnings = parsed["warnings"].as_array().unwrap();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0]["field"], json!("headline"));
    m.assert();
}

#[tokio::test]
async fn update_milestone_envelope_reports_changed() {
    let mut server = Server::new_async().await;
    let _get = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getTicket", "params": {"id": "20"}}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(
            json!({"id": "20", "projectId": "3", "type": "milestone", "headline": "Old name"}),
        ))
        .create_async()
        .await;
    let _st = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.getStatusLabels"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!({})))
        .create_async()
        .await;
    let m = server
        .mock("POST", "/api/jsonrpc")
        .match_body(mockito::Matcher::PartialJsonString(
            json!({"method": "leantime.rpc.tickets.patch"}).to_string(),
        ))
        .with_status(200)
        .with_header("Content-Type", "application/json")
        .with_body(rpc_ok(json!(true)))
        .create_async()
        .await;

    let r = call(
        "leantime_update_milestone",
        json!({"milestoneId": "20", "headline": "New name"}),
        &server.url(),
    )
    .await;
    let (is_err, parsed) = parse(&r);
    assert!(!is_err, "{:?}", parsed);
    let changed = parsed["changed"].as_array().unwrap();
    assert_eq!(changed.len(), 1);
    assert_eq!(changed[0]["field"], json!("headline"));
    assert_eq!(changed[0]["from"], json!("Old name"));
    assert_eq!(changed[0]["to"], json!("New name"));
    m.assert();
}
