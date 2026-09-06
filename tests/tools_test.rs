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
    assert_eq!(parsed, json!({"ok": true, "id": "9"}));
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
