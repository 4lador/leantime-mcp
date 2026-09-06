//! Exhaustive local e2e — the Rust port of tests/e2e/local.test.ts.
//!
//! Opt-in: requires LEANTIME_E2E=local + LEANTIME_URL + LEANTIME_API_KEY
//! (typically the docker Leantime from the leantime-mcp repo at :8090).
//!
//! Safety rails (identical to the TS suite):
//! - capture-on-create: every delete path calls assert_captured() FIRST and
//!   refuses to delete anything this suite did not create;
//! - scoped reads only (searchCriteria.currentProject), never unscoped getAll;
//! - cleanup hides (state: -1) the scratch projects, never deletes them;
//! - final emptiness assertion: each scratch project must be left empty.

use std::collections::HashSet;

use serde_json::{json, Value};

use leantmcp::client::LeantimeClient;
use leantmcp::tools;

type ClientRef = std::sync::Arc<tokio::sync::Mutex<LeantimeClient>>;

struct CallResult {
    is_error: bool,
    text: String,
    parsed: Value,
}

struct E2e {
    client: ClientRef,
    registry: Vec<tools::Tool>,
    // scratch state (declaration order = execution order)
    projects: Vec<String>,
    sprint: Option<String>,
    milestone: Option<String>,
    ticket: Option<String>,
    subtask: Option<String>,
    // capture registry — the "only delete what we created" rail
    c_tickets: HashSet<String>,
    c_comments: HashSet<String>,
    c_timesheets: HashSet<String>,
    c_projects: HashSet<String>,
}

impl E2e {
    fn new(url: &str, key: &str) -> Self {
        Self {
            client: std::sync::Arc::new(tokio::sync::Mutex::new(LeantimeClient::new(url, key))),
            registry: tools::create_registry(),
            projects: Vec::new(),
            sprint: None,
            milestone: None,
            ticket: None,
            subtask: None,
            c_tickets: HashSet::new(),
            c_comments: HashSet::new(),
            c_timesheets: HashSet::new(),
            c_projects: HashSet::new(),
        }
    }

    fn handler(&self, name: &str) -> &tools::Tool {
        self.registry
            .iter()
            .find(|t| t.name == name)
            .unwrap_or_else(|| panic!("tool {} not found", name))
    }

    async fn call(&self, name: &str, args: Value) -> CallResult {
        let tool = self.handler(name);
        let r = (tool.handler)(args, self.client.clone()).await;
        let text = r["content"][0]["text"].as_str().unwrap_or("").to_string();
        let is_error = r["isError"] == json!(true);
        let parsed = serde_json::from_str::<Value>(&text)
            .unwrap_or_else(|_| json!(text.strip_prefix("Error: ").unwrap_or(&text)));
        CallResult {
            is_error,
            text,
            parsed,
        }
    }

    fn capture(&mut self, kind: &str, id: Value) -> String {
        let id_str = match &id {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        match kind {
            "tickets" => {
                self.c_tickets.insert(id_str.clone());
            }
            "comments" => {
                self.c_comments.insert(id_str.clone());
            }
            "timesheets" => {
                self.c_timesheets.insert(id_str.clone());
            }
            "projects" => {
                self.c_projects.insert(id_str.clone());
            }
            _ => panic!("unknown capture kind"),
        }
        id_str
    }

    fn assert_captured(&self, kind: &str, id: &str) {
        let present = match kind {
            "tickets" => self.c_tickets.contains(id),
            "comments" => self.c_comments.contains(id),
            "timesheets" => self.c_timesheets.contains(id),
            "projects" => self.c_projects.contains(id),
            _ => panic!("unknown capture kind"),
        };
        assert!(
            present,
            "SAFETY: refusing to delete {} {} — not created by this suite",
            kind, id
        );
    }
}

// ---------------------------------------------------------------- assertions

fn ok<'a>(r: &'a CallResult, label: &str) -> &'a CallResult {
    assert!(!r.is_error, "[{}] unexpected error: {}", label, r.text);
    r
}

fn err_contains(r: &CallResult, needle: &str, label: &str) {
    assert!(r.is_error, "[{}] expected an error, got: {}", label, r.text);
    assert!(
        r.text.contains(needle),
        "[{}] error should contain {:?}, got: {}",
        label,
        needle,
        r.text
    );
}

fn id_str(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------- the suite

#[tokio::test]
async fn local_e2e_exhaustive() {
    if std::env::var("LEANTIME_E2E").ok().as_deref() != Some("local") {
        eprintln!("  ⚠ SKIPPED: set LEANTIME_E2E=local (+ LEANTIME_URL, LEANTIME_API_KEY) to run");
        return;
    }
    let url = std::env::var("LEANTIME_URL").expect("LEANTIME_URL");
    let key = std::env::var("LEANTIME_API_KEY").expect("LEANTIME_API_KEY");
    std::env::set_var("LEANTIME_MCP_DESTRUCTIVE_POLICY", "ask");

    let mut e = E2e::new(&url, &key);
    let ts = chrono::Utc::now().timestamp_millis();
    let today = chrono::Utc::now().format("%Y-%m-%d").to_string();

    // ================================================================ 1. projects & clients
    println!("— projects & clients");

    let clients = ok(
        &e.call("leantime_list_clients", json!({})).await,
        "list_clients",
    )
    .parsed
    .clone();
    assert!(
        !clients.as_array().map(|a| a.is_empty()).unwrap_or(true),
        "no client to attach the project to — is this a fresh instance?"
    );

    let r = e
        .call(
            "leantime_create_project",
            json!({
                "name": format!("e2e-scratch-{}", ts), "clientId": "1",
                "details": "Scratch **e2e** project — auto-created",
            }),
        )
        .await;
    let pid = e.capture("projects", ok(&r, "create_project").parsed["id"].clone());
    e.projects.push(pid.clone());
    println!("  scratch project: {}", pid);

    let proj = ok(
        &e.call("leantime_get_project", json!({"projectId": pid}))
            .await,
        "get_project",
    )
    .parsed
    .clone();
    let details = proj["details"].as_str().unwrap_or("");
    assert!(
        details.contains("<strong>e2e</strong>"),
        "markdown not converted: {}",
        details
    );

    ok(
        &e.call("leantime_get_project_progress", json!({"projectId": pid}))
            .await,
        "get_project_progress",
    );
    ok(
        &e.call(
            "leantime_update_project",
            json!({"projectId": pid, "name": format!("e2e-scratch-{}-renamed", ts)}),
        )
        .await,
        "update_project",
    );

    // findProject mangles ids ("id-modified") — both sides normalize via split('-')
    let found = ok(
        &e.call("leantime_find_projects", json!({"term": "e2e-scratch"}))
            .await,
        "find_projects",
    )
    .parsed
    .clone();
    let found_match = found
        .as_array()
        .map(|arr| {
            arr.iter()
                .any(|p| id_str(&p["id"]).split('-').next().unwrap_or("") == pid)
        })
        .unwrap_or(false);
    assert!(found_match, "scratch project not found via find_projects");

    // fresh project: empty user list is legitimate — loud-skip philosophy
    ok(
        &e.call("leantime_list_project_users", json!({"projectId": pid}))
            .await,
        "list_project_users",
    );

    let projects = ok(
        &e.call("leantime_list_projects", json!({})).await,
        "list_projects",
    )
    .parsed
    .clone();
    let listed = projects
        .as_array()
        .map(|arr| arr.iter().any(|p| id_str(&p["id"]) == pid))
        .unwrap_or(false);
    assert!(listed, "scratch project not in list_projects");

    // ================================================================ 2. sprints CRUD + current
    println!("— sprints");
    let r = e
        .call(
            "leantime_create_sprint",
            json!({
                "projectId": pid, "name": "e2e Sprint",
                "startDate": "2026-01-01", "endDate": "2030-12-31", // spans "today"
            }),
        )
        .await;
    let sprint = e.capture("tickets", ok(&r, "create_sprint").parsed["id"].clone());
    e.sprint = Some(sprint.clone());

    ok(
        &e.call(
            "leantime_update_sprint",
            json!({"sprintId": sprint, "name": "e2e Sprint renamed"}),
        )
        .await,
        "update_sprint",
    );

    let current = ok(
        &e.call("leantime_get_current_sprint", json!({"projectId": pid}))
            .await,
        "get_current_sprint",
    )
    .parsed
    .clone();
    assert_eq!(
        id_str(&current["current"]["id"]),
        sprint,
        "current sprint should be the one spanning today"
    );

    let sprints = ok(
        &e.call("leantime_list_sprints", json!({"projectId": pid}))
            .await,
        "list_sprints",
    )
    .parsed
    .clone();
    let listed = sprints
        .as_array()
        .map(|arr| arr.iter().any(|s| id_str(&s["id"]) == sprint))
        .unwrap_or(false);
    assert!(listed, "sprint not in list_sprints");

    // ================================================================ 3. milestones
    println!("— milestones");
    let r = e
        .call(
            "leantime_create_milestone",
            json!({"projectId": pid, "headline": "no-assignment"}),
        )
        .await;
    err_contains(
        &r,
        "Assignment required",
        "create_milestone without assignment",
    );

    let r = e.call("leantime_create_milestone", json!({
        "projectId": pid, "editorId": "1", "headline": "Jalon e2e", "description": "Jalon **e2e**",
    })).await;
    let milestone = e.capture("tickets", ok(&r, "create_milestone").parsed["id"].clone());
    e.milestone = Some(milestone.clone());

    let milestones = ok(
        &e.call("leantime_list_milestones", json!({"projectId": pid}))
            .await,
        "list_milestones",
    )
    .parsed
    .clone();
    let all_milestones = milestones
        .as_array()
        .map(|arr| arr.iter().all(|m| m["type"] == json!("milestone")))
        .unwrap_or(false);
    assert!(all_milestones, "list_milestones returned a non-milestone");

    ok(
        &e.call(
            "leantime_update_milestone",
            json!({"milestoneId": milestone, "headline": "Jalon e2e renamed"}),
        )
        .await,
        "update_milestone",
    );

    let progress = ok(
        &e.call(
            "leantime_get_milestone_progress",
            json!({"milestoneId": milestone}),
        )
        .await,
        "get_milestone_progress",
    )
    .parsed
    .clone();
    assert!(
        progress["percentDone"].is_number(),
        "percentDone should be a number: {}",
        progress
    );

    // ================================================================ 4. tickets
    println!("— tickets");
    let r = e
        .call(
            "leantime_create_ticket",
            json!({"projectId": pid, "headline": "no-assignment"}),
        )
        .await;
    err_contains(
        &r,
        "Assignment required",
        "create_ticket without assignment",
    );
    let r = e
        .call(
            "leantime_create_ticket",
            json!({"projectId": pid, "headline": "bad-editor", "editorId": "999"}),
        )
        .await;
    err_contains(&r, "does not exist", "create_ticket with unknown editorId");

    let r = e
        .call(
            "leantime_create_ticket",
            json!({
                "projectId": pid, "headline": "Ticket e2e",
                "description": "## Contexte\n\n- [ ] étape\n- point",
                "sprintId": sprint, "milestoneId": milestone, "editorId": "1",
            }),
        )
        .await;
    let ticket = e.capture("tickets", ok(&r, "create_ticket").parsed["id"].clone());
    e.ticket = Some(ticket.clone());

    let t = ok(
        &e.call(
            "leantime_get_ticket",
            json!({"projectId": pid, "ticketId": ticket}),
        )
        .await,
        "get_ticket",
    )
    .parsed
    .clone();
    let desc = t["description"].as_str().unwrap_or("");
    assert!(
        desc.contains("<h2>Contexte</h2>"),
        "heading not converted: {}",
        desc
    );
    assert!(
        desc.contains("data-type=\"taskList\""),
        "task list not converted: {}",
        desc
    );
    assert_eq!(id_str(&t["editorId"]), "1");

    // Field-wiping regression: headline-only patch must NOT wipe editorId/sprint
    ok(
        &e.call(
            "leantime_update_ticket",
            json!({"ticketId": ticket, "headline": "Ticket e2e renamed"}),
        )
        .await,
        "update_ticket",
    );
    let t2 = ok(
        &e.call(
            "leantime_get_ticket",
            json!({"projectId": pid, "ticketId": ticket}),
        )
        .await,
        "get_ticket re-read",
    )
    .parsed
    .clone();
    assert_eq!(id_str(&t2["editorId"]), "1", "patch wiped editorId");
    assert!(
        t2["headline"].as_str().unwrap_or("").contains("renamed"),
        "headline not renamed"
    );
    assert_eq!(id_str(&t2["sprint"]), sprint, "patch wiped sprint");

    // Scoping regression: another project's tickets must NOT leak into ours
    let r = e
        .call(
            "leantime_create_project",
            json!({"name": format!("e2e-other-{}", ts), "clientId": "1"}),
        )
        .await;
    let other_pid = e.capture(
        "projects",
        ok(&r, "create other project").parsed["id"].clone(),
    );
    e.projects.push(other_pid.clone());
    let r = e
        .call(
            "leantime_create_ticket",
            json!({
                "projectId": other_pid, "headline": "other-project ticket", "unassigned": true,
            }),
        )
        .await;
    let other_tid = e.capture(
        "tickets",
        ok(&r, "create other ticket").parsed["id"].clone(),
    );

    let listed = ok(
        &e.call("leantime_list_tickets", json!({"projectId": pid}))
            .await,
        "list_tickets",
    )
    .parsed
    .clone();
    let arr = listed.as_array().expect("list_tickets array");
    assert!(
        arr.iter().any(|t| id_str(&t["id"]) == ticket),
        "our ticket missing from list"
    );
    assert!(
        !arr.iter().any(|t| id_str(&t["id"]) == other_tid),
        "SCOPING LEAK: other project's ticket visible"
    );
    for t in arr {
        assert_eq!(
            id_str(&t["projectId"]),
            pid,
            "SCOPING LEAK: foreign projectId in list"
        );
    }

    // Subtasks
    let r = e
        .call(
            "leantime_create_ticket",
            json!({
                "projectId": pid, "headline": "Sous-tâche e2e",
                "dependingTicketId": ticket, "unassigned": true,
            }),
        )
        .await;
    let subtask = e.capture("tickets", ok(&r, "create_subtask").parsed["id"].clone());
    e.subtask = Some(subtask.clone());
    let subtasks = ok(
        &e.call("leantime_list_subtasks", json!({"ticketId": ticket}))
            .await,
        "list_subtasks",
    )
    .parsed
    .clone();
    assert!(
        subtasks
            .as_array()
            .map(|a| a.iter().any(|s| id_str(&s["id"]) == subtask))
            .unwrap_or(false),
        "subtask not listed"
    );

    // Reads
    let mine = ok(
        &e.call(
            "leantime_my_tasks",
            json!({"userId": "1", "projectId": pid}),
        )
        .await,
        "my_tasks",
    )
    .parsed
    .clone();
    assert!(
        mine.as_array()
            .map(|a| a.iter().any(|t| id_str(&t["id"]) == ticket))
            .unwrap_or(false),
        "ticket not in my_tasks"
    );
    let options = ok(
        &e.call("leantime_get_ticket_options", json!({"projectId": pid}))
            .await,
        "get_ticket_options",
    )
    .parsed
    .clone();
    for k in ["priorities", "efforts", "kanban", "types"] {
        assert!(options.get(k).is_some(), "options missing {}", k);
    }
    let statuses = ok(
        &e.call("leantime_get_statuses", json!({"projectId": pid}))
            .await,
        "get_statuses",
    )
    .parsed
    .clone();
    assert!(
        statuses.as_object().map(|o| !o.is_empty()).unwrap_or(false),
        "statuses empty"
    );
    ok(
        &e.call("leantime_get_ticket_types", json!({"projectId": pid}))
            .await,
        "get_ticket_types",
    );
    let users = ok(
        &e.call("leantime_list_users", json!({})).await,
        "list_users",
    )
    .parsed
    .clone();
    assert!(
        users
            .as_array()
            .map(|a| a.iter().any(|u| id_str(&u["id"]) == "1"))
            .unwrap_or(false),
        "user 1 not in list_users"
    );

    // ================================================================ 5. comments + timesheets
    println!("— comments + timesheets");
    ok(
        &e.call("leantime_list_comments", json!({"ticketId": ticket}))
            .await,
        "list_comments (fresh: empty ok)",
    );

    ok(
        &e.call(
            "leantime_add_comment",
            json!({
                "ticketId": ticket, "text": "Commentaire **e2e** avec `code`",
            }),
        )
        .await,
        "add_comment",
    );
    let comments = ok(
        &e.call("leantime_list_comments", json!({"ticketId": ticket}))
            .await,
        "list_comments",
    )
    .parsed
    .clone();
    let carr = comments.as_array().expect("comments array");
    assert_eq!(carr.len(), 1, "expected exactly 1 comment");
    let ctext = carr[0]["text"].as_str().unwrap_or("");
    assert!(
        ctext.contains("<strong>e2e</strong>"),
        "comment markdown not converted: {}",
        ctext
    );
    let comment = e.capture("comments", carr[0]["id"].clone());

    ok(
        &e.call(
            "leantime_update_comment",
            json!({"commentId": comment, "text": "Commentaire **e2e** édité"}),
        )
        .await,
        "update_comment",
    );

    ok(&e.call("leantime_log_time", json!({
        "ticketId": ticket, "hours": 1.5, "kind": "DEVELOPMENT", "date": today, "description": "e2e work",
    })).await, "log_time add");
    ok(
        &e.call(
            "leantime_log_time",
            json!({
                "ticketId": ticket, "hours": 2.0, "kind": "TESTING", "date": today, "mode": "set",
            }),
        )
        .await,
        "log_time set",
    );

    let time = ok(
        &e.call("leantime_get_ticket_time", json!({"ticketId": ticket}))
            .await,
        "get_ticket_time",
    )
    .parsed
    .clone();
    let total = time["totalHours"]
        .as_f64()
        .or_else(|| time["totalHours"].as_str().and_then(|s| s.parse().ok()))
        .unwrap_or(f64::NAN);
    assert!(
        (total - 3.5).abs() < 0.01,
        "totalHours should be ~3.5, got {}",
        total
    );

    let sheets = ok(
        &e.call(
            "leantime_list_timesheets",
            json!({
                "dateFrom": today, "dateTo": today, "projectId": pid,
            }),
        )
        .await,
        "list_timesheets",
    )
    .parsed
    .clone();
    let sarr = sheets.as_array().expect("timesheets array");
    assert_eq!(
        sarr.len(),
        2,
        "expected exactly 2 entries today, got {} — dateTo end-of-day broken?",
        sarr.len()
    );
    let mut ts_entries = Vec::new();
    for s in sarr {
        ts_entries.push(e.capture("timesheets", s["id"].clone()));
    }

    // ================================================================ 6. bulk operations
    println!("— bulk (fresh client to isolate state)");
    // Direct RPC probe: the array-wrapped-id quirk at API level
    {
        let mut probe = LeantimeClient::new(&url, &key);
        let direct = probe.call("tickets.addTicket", json!({
            "values": {"headline": "bulk direct probe", "projectId": pid.parse::<i64>().unwrap_or_default(), "editorId": "1"},
        })).await.expect("direct RPC probe");
        if let Some(arr) = direct.as_array() {
            e.capture("tickets", arr[0].clone());
        }
    }

    let mut bulk_specs = Vec::new();
    for i in 0..9 {
        let mut spec = json!({
            "headline": format!("bulk ticket {}", i + 1),
            "description": "Description **bulk**",
            "type": "task",
        });
        if i % 2 == 0 {
            spec["editorId"] = json!("1");
        } else {
            spec["unassigned"] = json!(true);
        }
        bulk_specs.push(spec);
    }
    let bulk = ok(
        &e.call(
            "leantime_bulk_create_tickets",
            json!({"projectId": pid, "tickets": bulk_specs}),
        )
        .await,
        "bulk_create",
    )
    .parsed
    .clone();
    assert_eq!(
        bulk["summary"]["created"],
        json!(9),
        "bulk create summary: {}",
        bulk["summary"]
    );
    assert_eq!(bulk["summary"]["failed"], json!(0));
    let mut bulk_ids: Vec<String> = Vec::new();
    for r in bulk["results"].as_array().expect("results") {
        assert!(r["ok"] == json!(true), "BULK ERROR: {}", r);
        bulk_ids.push(e.capture("tickets", r["id"].clone()));
    }

    // Bulk subtask
    let bulk = ok(&e.call("leantime_bulk_create_tickets", json!({
        "projectId": pid,
        "tickets": [{"headline": "bulk subtask", "dependingTicketId": bulk_ids[0], "unassigned": true}],
    })).await, "bulk subtask").parsed.clone();
    let bulk_subtask = e.capture("tickets", bulk["results"][0]["id"].clone());

    // All-or-nothing validation
    let r = e
        .call(
            "leantime_bulk_create_tickets",
            json!({
                "projectId": pid,
                "tickets": [{"headline": "ok", "unassigned": true}, {"headline": "BAD"}],
            }),
        )
        .await;
    err_contains(&r, "NOTHING was created", "bulk all-or-nothing");

    // Bulk update: first 5 → status 0
    let updates: Vec<Value> = bulk_ids
        .iter()
        .take(5)
        .map(|id| {
            json!({
                "ticketId": id, "status": 0, "headline": "bulk ticket updated",
            })
        })
        .collect();
    let bulk = ok(
        &e.call(
            "leantime_bulk_update_tickets",
            json!({"projectId": pid, "updates": updates}),
        )
        .await,
        "bulk_update",
    )
    .parsed
    .clone();
    assert_eq!(
        bulk["summary"]["created"],
        json!(5),
        "bulk update summary: {}",
        bulk["summary"]
    );
    let t = ok(
        &e.call(
            "leantime_get_ticket",
            json!({"projectId": pid, "ticketId": bulk_ids[0]}),
        )
        .await,
        "verify bulk update",
    )
    .parsed
    .clone();
    assert_eq!(id_str(&t["status"]), "0", "status not updated");

    // Bulk schedule: first 3 → sprint
    let schedules: Vec<Value> = bulk_ids
        .iter()
        .take(3)
        .map(|id| {
            json!({
                "ticketId": id, "sprintId": sprint,
            })
        })
        .collect();
    let bulk = ok(
        &e.call(
            "leantime_bulk_schedule_tickets",
            json!({"projectId": pid, "schedules": schedules}),
        )
        .await,
        "bulk_schedule",
    )
    .parsed
    .clone();
    assert_eq!(
        bulk["summary"]["created"],
        json!(3),
        "bulk schedule summary: {}",
        bulk["summary"]
    );
    let t = ok(
        &e.call(
            "leantime_get_ticket",
            json!({"projectId": pid, "ticketId": bulk_ids[0]}),
        )
        .await,
        "verify bulk schedule",
    )
    .parsed
    .clone();
    assert_eq!(id_str(&t["sprint"]), sprint, "sprint not scheduled");

    // ================================================================ 7. destructive gating
    println!("— destructive gating");
    // (kind, id, tool, id-param name)
    let cycles: Vec<(&str, String, &str, &str)> = vec![
        (
            "tickets",
            e.subtask.clone().expect("subtask"),
            "leantime_delete_ticket",
            "ticketId",
        ),
        (
            "comments",
            comment.clone(),
            "leantime_delete_comment",
            "commentId",
        ),
        (
            "timesheets",
            ts_entries[0].clone(),
            "leantime_delete_timesheet_entry",
            "entryId",
        ),
    ];
    for (kind, id, tool_name, param) in &cycles {
        e.assert_captured(kind, id);
        let mut req = json!({ *param: id });
        let r = e.call(tool_name, req.clone()).await;
        err_contains(&r, "Confirmation required", tool_name);
        req["confirm"] = json!(true);
        let r = e.call(tool_name, req).await;
        ok(&r, tool_name);
        match *kind {
            "tickets" => {
                e.c_tickets.remove(id);
            }
            "comments" => {
                e.c_comments.remove(id);
            }
            "timesheets" => {
                e.c_timesheets.remove(id);
            }
            _ => {}
        }
    }

    // Milestone delete: refused then confirmed
    let milestone_id = e.milestone.clone().expect("milestone");
    e.assert_captured("tickets", &milestone_id);
    let r = e
        .call(
            "leantime_delete_milestone",
            json!({"milestoneId": milestone_id}),
        )
        .await;
    err_contains(&r, "Confirmation required", "delete_milestone refuse");
    let r = e
        .call(
            "leantime_delete_milestone",
            json!({"milestoneId": milestone_id, "confirm": true}),
        )
        .await;
    ok(&r, "delete_milestone confirm");
    e.c_tickets.remove(&milestone_id);

    // deny policy: refused even with confirm
    std::env::set_var("LEANTIME_MCP_DESTRUCTIVE_POLICY", "deny");
    let r = e
        .call(
            "leantime_delete_ticket",
            json!({"ticketId": bulk_subtask, "confirm": true}),
        )
        .await;
    err_contains(&r, "deny", "deny policy refuses");
    std::env::set_var("LEANTIME_MCP_DESTRUCTIVE_POLICY", "ask");

    // ================================================================ 8. cleanup
    println!("— cleanup (captured ids only)");
    let comment_ids: Vec<String> = e.c_comments.clone().into_iter().collect();
    for id in &comment_ids {
        e.assert_captured("comments", id);
        ok(
            &e.call(
                "leantime_delete_comment",
                json!({"commentId": id, "confirm": true}),
            )
            .await,
            "cleanup comment",
        );
        e.c_comments.remove(id);
    }
    let ts_ids: Vec<String> = e.c_timesheets.clone().into_iter().collect();
    for id in &ts_ids {
        e.assert_captured("timesheets", id);
        ok(
            &e.call(
                "leantime_delete_timesheet_entry",
                json!({"entryId": id, "confirm": true}),
            )
            .await,
            "cleanup timesheet",
        );
        e.c_timesheets.remove(id);
    }
    let ticket_ids: Vec<String> = e.c_tickets.clone().into_iter().collect();
    for id in &ticket_ids {
        e.assert_captured("tickets", id);
        let r = e
            .call(
                "leantime_delete_ticket",
                json!({"ticketId": id, "confirm": true}),
            )
            .await;
        ok(&r, &format!("cleanup ticket {}", id));
        e.c_tickets.remove(id);
    }

    // Emptiness assertion — scoped read only
    {
        let mut c = e.client.lock().await;
        for pid in &e.projects {
            let r = c
                .call(
                    "tickets.getAll",
                    json!({
                        "searchCriteria": {"currentProject": pid}, "limit": 500,
                    }),
                )
                .await
                .expect("cleanup emptiness check");
            let leftovers: Vec<String> = r
                .as_array()
                .map(|arr| {
                    arr.iter()
                        .map(|t| t["headline"].as_str().unwrap_or("?").to_string())
                        .collect()
                })
                .unwrap_or_default();
            assert!(
                leftovers.is_empty(),
                "scratch project {} not empty after cleanup — leftover: {:?}",
                pid,
                leftovers
            );
        }

        // Hide, don't delete
        for pid in &e.projects {
            c.call(
                "projects.editProject",
                json!({
                    "id": pid, "values": {"name": "e2e-scratch-hidden", "state": -1},
                }),
            )
            .await
            .expect("hide project");
        }
    }

    assert!(
        e.c_tickets.is_empty(),
        "captured tickets not empty after cleanup"
    );
    assert!(
        e.c_comments.is_empty(),
        "captured comments not empty after cleanup"
    );
    assert!(
        e.c_timesheets.is_empty(),
        "captured timesheets not empty after cleanup"
    );
    // (projects set intentionally not emptied — hiding is their cleanup)

    std::env::set_var("LEANTIME_MCP_DESTRUCTIVE_POLICY", "ask");
    println!("✓ e2e exhaustive: all sections green");
}
