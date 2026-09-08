use std::future::Future;
use std::pin::Pin;

use serde_json::{json, Value};

use crate::markdown::markdown_to_html;

use super::shared::*;
use super::{error_result, ok_result, rpc, ClientRef, Tool, ToolAnnotations};

/// Agent-facing cap for leantime_list_tickets. Unlike the completeness
/// paths (backup, restore verification…), this tool dumps its result into
/// the LLM context, so the limit exists to protect the context window.
const LIST_TICKETS_LIMIT: usize = 500;

/// True for status tokens that need the status map to resolve (labels).
/// Numeric IDs and the server's magic "done"/"not_done" pass through.
fn status_token_needs_map(t: &str) -> bool {
    let t = t.trim();
    !t.is_empty() && !t.bytes().all(|b| b.is_ascii_digit()) && t != "done" && t != "not_done"
}

/// True when the `status` argument contains at least one label token.
fn status_needs_map(arg: &Value) -> bool {
    match arg {
        Value::String(s) => s.split(',').any(status_token_needs_map),
        _ => false,
    }
}

/// Resolve the user-provided `status` argument into the wire value the API
/// expects: status IDs. The API `intval()`s whatever it receives, so an
/// unresolved label would silently filter on status 0 (the wrong result
/// set). Numeric tokens, "done" and "not_done" (server magic values,
/// resolved by statusType) pass through; labels resolve case-insensitively
/// against the project's status map; comma-separated lists resolve token
/// by token.
fn resolve_status_filter(arg: &Value, sm: &Value, pid: &str) -> Result<Value, String> {
    let s = match arg {
        Value::String(s) => s.clone(),
        other => return Ok(other.clone()),
    };
    let mut resolved: Vec<String> = Vec::new();
    for tok in s.split(',').map(str::trim).filter(|t| !t.is_empty()) {
        if !status_token_needs_map(tok) {
            resolved.push(tok.to_string());
            continue;
        }
        let lower = tok.to_lowercase();
        let hit = sm.as_object().and_then(|obj| {
            obj.iter().find(|(_, info)| {
                info.get("name")
                    .and_then(|n| n.as_str())
                    .map(|n| n.to_lowercase() == lower)
                    .unwrap_or(false)
            })
        });
        match hit {
            Some((id, _)) => resolved.push(id.clone()),
            None => {
                let valid: Vec<String> = sm
                    .as_object()
                    .map(|obj| {
                        obj.iter()
                            .filter_map(|(id, info)| {
                                info.get("name")
                                    .and_then(|n| n.as_str())
                                    .map(|n| format!("{} ({})", id, n))
                            })
                            .collect()
                    })
                    .unwrap_or_default();
                return Err(format!(
                    "Unknown status \"{}\" in project {}. Valid statuses: {} (by ID). The values \"done\" and \"not_done\" are also accepted.",
                    tok,
                    pid,
                    valid.join(", ")
                ));
            }
        }
    }
    Ok(json!(resolved.join(",")))
}

fn h_list_tickets(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let pid = a.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
        // Leantime expects filters inside a `searchCriteria` object — flat
        // params are silently ignored and would return every ticket.
        let mut sc = json!({ "currentProject": pid });
        if let Some(v) = a.get("status") {
            if status_needs_map(v) {
                let sm = match c.get_status_map(pid).await {
                    Ok(m) => m,
                    Err(e) => {
                        return error_result(&format!(
                            "Could not load status labels to resolve the status filter: {}",
                            e
                        ))
                    }
                };
                match resolve_status_filter(v, &sm, pid) {
                    Ok(r) => sc["status"] = r,
                    Err(e) => return error_result(&e),
                }
            } else {
                sc["status"] = v.clone();
            }
        }
        if let Some(v) = a.get("milestoneId") {
            sc["milestone"] = v.clone();
        }
        if let Some(v) = a.get("sprintId") {
            sc["sprint"] = v.clone();
        }
        if let Some(v) = a.get("userId") {
            sc["users"] = v.clone();
        }
        if let Some(v) = a.get("type") {
            sc["type"] = v.clone();
        }
        if let Some(v) = a.get("search") {
            sc["term"] = v.clone();
        }

        match c
            .call(
                "tickets.getAll",
                json!({ "searchCriteria": sc, "limit": LIST_TICKETS_LIMIT }),
            )
            .await
        {
            Ok(r) => {
                let sm = c.get_status_map(pid).await.unwrap_or(json!({}));
                let mut items = r.as_array().cloned().unwrap_or_default();
                c.enrich_with_statuses(&mut items, &sm);
                // The limit protects the agent's context window (unlike the
                // completeness paths, which use the configurable fetch limit).
                if items.len() >= LIST_TICKETS_LIMIT {
                    let mut out = json!({ "tickets": items });
                    out["note"] = json!(format!(
                        "showing first {} — refine filters (status, milestoneId, sprintId, type, search) to narrow the result",
                        LIST_TICKETS_LIMIT
                    ));
                    return ok_result(&out);
                }
                ok_result(&json!(items))
            }
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_get_ticket(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let pid = a.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
        let tid = a.get("ticketId").and_then(|v| v.as_str()).unwrap_or("");
        match c.call("tickets.getTicket", json!({"id": tid})).await {
            Ok(mut r) => {
                c.enrich_single_with_statuses(&mut r, pid).await;
                ok_result(&r)
            }
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_create_ticket(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let users = match get_users_simplified(&mut c).await {
            Ok(u) => u,
            Err(e) => return error_result(&e),
        };
        if let Err(e) = check_assignment(&a, &users) {
            if wants_dry_run(&a) {
                return dry_run_result(false, vec![e], vec![], vec![]);
            }
            return error_result(&e);
        }

        let mut v = json!({ "headline": a.get("headline"), "projectId": a.get("projectId") });
        if let Some(d) = a.get("description").and_then(|x| x.as_str()) {
            v["description"] = json!(markdown_to_html(d));
        }
        for (k, dst) in [
            ("type", "type"),
            ("priority", "priority"),
            ("status", "status"),
            ("editorId", "editorId"),
            ("tags", "tags"),
            ("storypoints", "storypoints"),
            ("dateToFinish", "dateToFinish"),
            ("planHours", "planHours"),
            ("dependingTicketId", "dependingTicketId"),
        ] {
            if let Some(x) = a.get(k) {
                v[dst] = x.clone();
            }
        }
        if let Some(x) = a.get("milestoneId") {
            v["milestoneid"] = x.clone();
        }
        if let Some(x) = a.get("sprintId") {
            v["sprint"] = x.clone();
        }

        if wants_dry_run(&a) {
            let changes: Vec<Value> = v
                .as_object()
                .map(|o| {
                    o.iter()
                        .map(|(k, val)| json!({"field": k, "to": val}))
                        .collect()
                })
                .unwrap_or_default();
            return dry_run_result(true, vec![], changes, vec![]);
        }

        match c.call("tickets.addTicket", json!({ "values": v })).await {
            Ok(r) => {
                if is_leantime_error(&r) {
                    return error_result(&leantime_error_msg(&r));
                }
                let id = r.as_array().and_then(|x| x.first()).cloned().unwrap_or(r);
                ok_result(&json!({ "id": id }))
            }
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_update_ticket(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let tid = a.get("ticketId").and_then(|v| v.as_str()).unwrap_or("");
        if let Some(editor_id) = a.get("editorId") {
            if !editor_id.is_null() {
                let users = match get_users_simplified(&mut c).await {
                    Ok(u) => u,
                    Err(e) => return error_result(&e),
                };
                if let Err(e) = check_editor_id(editor_id, &users) {
                    return error_result(&e);
                }
            }
        }
        let mut ch = json!({});
        if let Some(v) = a.get("headline") {
            ch["headline"] = v.clone();
        }
        if let Some(d) = a.get("description").and_then(|x| x.as_str()) {
            ch["description"] = json!(markdown_to_html(d));
        }
        for (k, dst) in [
            ("type", "type"),
            ("priority", "priority"),
            ("status", "status"),
            ("editorId", "editorId"),
            ("tags", "tags"),
            ("storypoints", "storypoints"),
            ("dateToFinish", "dateToFinish"),
            ("planHours", "planHours"),
            ("dependingTicketId", "dependingTicketId"),
        ] {
            if let Some(x) = a.get(k) {
                ch[dst] = x.clone();
            }
        }
        if let Some(x) = a.get("milestoneId") {
            ch["milestoneid"] = x.clone();
        }
        if let Some(x) = a.get("sprintId") {
            ch["sprint"] = x.clone();
        }

        if ch.as_object().map(|o| o.is_empty()).unwrap_or(true) {
            if wants_dry_run(&a) {
                return dry_run_result(
                    false,
                    vec!["Nothing to update: provide at least one field to change.".into()],
                    vec![],
                    vec![],
                );
            }
            return error_result("Nothing to update: provide at least one field to change.");
        }
        if wants_dry_run(&a) {
            // One read to resolve from-values (reads are harmless — mutations
            // never happen on a dry run).
            let ticket = match c.call("tickets.getTicket", json!({"id": tid})).await {
                Ok(t) if !t.is_boolean() && !is_leantime_error(&t) => t,
                Ok(_) => {
                    return dry_run_result(
                        false,
                        vec![format!("Ticket {} not found.", tid)],
                        vec![],
                        vec![],
                    )
                }
                Err(e) => return error_result(&e.to_string()),
            };
            let pid = ticket
                .get("projectId")
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_default();
            let sm = c.get_status_map(&pid).await.unwrap_or(json!({}));
            let field_map: &[(&str, &str)] = &[
                ("headline", "headline"),
                ("type", "type"),
                ("priority", "priority"),
                ("status", "status"),
                ("editorId", "editorId"),
                ("tags", "tags"),
                ("storypoints", "storypoints"),
                ("dateToFinish", "dateToFinish"),
                ("planHours", "planHours"),
                ("dependingTicketId", "dependingTicketId"),
                ("milestoneId", "milestoneid"),
                ("sprintId", "sprint"),
            ];
            let (mut changes, warnings) = dry_run_changes(&a, &ticket, field_map, Some(&sm));
            if let Some(d) = a.get("description").and_then(|x| x.as_str()) {
                changes.push(json!({"field": "description", "to": d}));
            }
            return dry_run_result(true, vec![], changes, warnings);
        }
        match c
            .call("tickets.patch", json!({ "id": tid, "params": ch }))
            .await
        {
            Ok(r) => ok_result(&json!({ "ok": r == json!(true), "id": tid })),
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_delete_ticket(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        if let Err(e) = check_destructive(a.get("confirm").and_then(|v| v.as_bool()), "ticket") {
            return error_result(&e);
        }
        let mut c = cl.lock().await;
        let id = a.get("ticketId").and_then(|v| v.as_str()).unwrap_or("");
        match c.call("tickets.delete", json!({ "id": id })).await {
            Ok(r) => {
                if is_leantime_error(&r) {
                    return error_result(&leantime_error_msg(&r));
                }
                ok_result(&json!({ "deleted": true, "ticketId": id }))
            }
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_list_subtasks(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let tid = a.get("ticketId").and_then(|v| v.as_str()).unwrap_or("");
        match c
            .call("tickets.getAllSubtasks", json!({"ticketId": tid}))
            .await
        {
            Ok(r) => ok_result(&json!(r.as_array().cloned().unwrap_or_default())),
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_my_tasks(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        // Param is `project` (not projectId) — must match the service signature.
        let mut p = json!({});
        if let Some(v) = a
            .get("userId")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            p["userId"] = json!(v);
        }
        if let Some(v) = a
            .get("projectId")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            p["project"] = json!(v);
        }
        match c.call("tickets.getAllOpenUserTickets", p).await {
            Ok(r) => ok_result(&json!(r.as_array().cloned().unwrap_or_default())),
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_get_ticket_options(_a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let p = c.call("tickets.getPriorityLabels", json!({})).await;
        let e = c.call("tickets.getEffortLabels", json!({})).await;
        let k = c.call("tickets.getKanbanColumns", json!({})).await;
        let t = c.call("tickets.getTicketTypes", json!({})).await;
        match (p, e, k, t) {
            (Ok(p), Ok(e), Ok(k), Ok(t)) => {
                ok_result(&json!({"priorities": p, "efforts": e, "kanban": k, "types": t}))
            }
            _ => error_result("Failed to fetch options"),
        }
    })
}

fn h_get_ticket_types(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let pid = a.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
        match c
            .call("tickets.getTicketTypes", json!({"projectId": pid}))
            .await
        {
            Ok(r) => ok_result(&r),
            Err(e) => error_result(&e.to_string()),
        }
    })
}

pub(super) fn tools() -> Vec<Tool> {
    let md = MARKDOWN_HINT;
    let assignment = ASSIGNMENT_HINT;
    vec![
        tool("leantime_list_tickets", "List tickets/tasks for a project with optional filters",
            vec![rs("projectId", "The project ID to list tickets for"), os("status", "Filter by status: status label (resolved to its ID), status ID, comma-separated list, or 'done'/'not_done'"), os("milestoneId", "Filter by milestone ID"), os("sprintId", "Filter by sprint ID"), os("userId", "Filter by assigned user ID"), os("type", "Filter by ticket type (task, story, bug, etc.)"), os("search", "Search term for ticket headline/description")],
            vec!["projectId"], Box::new(h_list_tickets)),
        tool("leantime_get_ticket", "Get details of a specific ticket/task",
            vec![rs("projectId", "The project ID"), rs("ticketId", "The ticket ID")], vec!["projectId", "ticketId"], Box::new(h_get_ticket)),
        tool_with_annotations("leantime_create_ticket", format!("Create a new ticket/task in a project. The description is {}. {} Pass the chosen editorId (get candidates with leantime_list_users), or pass unassigned: true ONLY if the user explicitly said to leave it unassigned. Call directly when the user explicitly provided every value. Prefer dryRun: true first when you chose or inferred any value (type, priority, dates…) — show the proposed fields and ask for confirmation before executing.", md, assignment),
            vec![rs("projectId", "The project ID"), rs("headline", "Ticket title/headline"), os("description", format!("Ticket description in {}", md)), os("type", "Ticket type (task, story, bug, etc.)"), on("priority", "Priority (1-5)"), on("status", "Status ID"), os("milestoneId", "Milestone ID to assign to"), os("sprintId", "Sprint ID to assign to"), os("editorId", "Assigned user ID (required unless unassigned: true)"), ob("unassigned", "Set to true ONLY when the user explicitly requested an unassigned ticket"), os("tags", "Comma-separated tags"), os("storypoints", "Story points"), os("dateToFinish", "Due date (YYYY-MM-DD)"), os("dependingTicketId", "Parent ticket ID (for subtasks)"), on("planHours", "Planned hours estimate"), ob("dryRun", DRY_RUN_DESC)],
            vec!["projectId", "headline"], Box::new(h_create_ticket), ToolAnnotations::write()),
        tool_with_annotations("leantime_update_ticket", format!("Update an existing ticket/task. Only the provided fields are changed (Leantime's patch API — only the provided fields change). The description is {} and replaces the previous description entirely. Only set editorId when you intend to change the assignment (validate user IDs with leantime_list_users). Call directly when every value was explicitly given or resolves unambiguously. Prefer dryRun: true first when you interpreted the request or chose values yourself — show the from → to diff and ask for confirmation before executing.", md),
            vec![rs("ticketId", "The ticket ID"), os("headline", "New ticket title"), os("description", format!("New description in {}", md)), os("type", "New ticket type"), on("status", "New status ID"), on("priority", "New priority (1-5)"), os("milestoneId", "New milestone ID"), os("sprintId", "New sprint ID"), os("editorId", "New assigned user ID (validates against leantime_list_users)"), os("tags", "New comma-separated tags"), os("storypoints", "New story points"), os("dateToFinish", "New due date (YYYY-MM-DD)"), os("dependingTicketId", "Parent ticket ID (for subtasks)"), on("planHours", "Planned hours estimate"), ob("dryRun", DRY_RUN_DESC)],
            vec!["ticketId"], Box::new(h_update_ticket), ToolAnnotations::write()),
        tool_with_annotations("leantime_delete_ticket", "Delete a ticket. Destructive: requires explicit user approval (confirm: true) unless LEANTIME_MCP_DESTRUCTIVE_POLICY is set otherwise. Prefer updating the status to a 'done/cancelled' state when possible.",
            vec![rs("ticketId", "The ticket ID"), ob("confirm", "MUST be true to actually delete (ask the user for explicit approval first)")],
            vec!["ticketId"], Box::new(h_delete_ticket), ToolAnnotations::destructive()),
        tool("leantime_list_subtasks", "List the subtasks of a ticket (create subtasks with leantime_create_ticket and dependingTicketId)",
            vec![rs("ticketId", "The parent ticket ID")], vec!["ticketId"], Box::new(h_list_subtasks)),
        tool("leantime_my_tasks", "List the open tickets assigned to a user (defaults to the API key owner) — the 'what's on my plate' view",
            vec![os("userId", "User ID (defaults to the API key owner)"), os("projectId", "Restrict to one project")], vec![], Box::new(h_my_tasks)),
        tool("leantime_get_ticket_options", "Get the pick-list options for tickets of a project: priorities, efforts (story points), kanban columns and ticket types",
            vec![rs("projectId", "The project ID")], vec!["projectId"], Box::new(h_get_ticket_options)),
        tool("leantime_get_statuses", "Get available status labels for a project",
            vec![rs("projectId", "The project ID")], vec!["projectId"],
            rpc("tickets.getStatusLabels", |a: &Value| json!({"projectId": a.get("projectId")}))),
        tool("leantime_get_ticket_types", "Get available ticket types",
            vec![rs("projectId", "The project ID")], vec!["projectId"], Box::new(h_get_ticket_types)),
    ]
}
