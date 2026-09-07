use std::future::Future;
use std::pin::Pin;

use serde_json::{json, Value};

use crate::markdown::markdown_to_html;

use super::shared::*;
use super::{error_result, ok_result, ClientRef, Tool, ToolAnnotations};

fn h_list_milestones(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let pid = a.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
        match c.call("tickets.getAll", json!({"searchCriteria": {"currentProject": pid, "type": "milestone"}, "limit": 200})).await {
            Ok(r) => {
                let sm = c.get_status_map(pid).await.unwrap_or(json!({}));
                let mut items = r.as_array().cloned().unwrap_or_default();
                c.enrich_with_statuses(&mut items, &sm);
                ok_result(&json!(items))
            }
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_get_milestone(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let pid = a.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
        let mid = a.get("milestoneId").and_then(|v| v.as_str()).unwrap_or("");
        match c.call("tickets.getTicket", json!({"id": mid})).await {
            Ok(mut r) => {
                c.enrich_single_with_statuses(&mut r, pid).await;
                ok_result(&r)
            }
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_create_milestone(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
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

        // Milestones are zp_tickets rows: addTicket with type "milestone" gives the
        // richest field support (quickAddMilestone drops the description entirely).
        let mut v = json!({"headline": a.get("headline"), "type": "milestone", "projectId": a.get("projectId")});
        if let Some(d) = a.get("description").and_then(|x| x.as_str()) {
            v["description"] = json!(markdown_to_html(d));
        }
        if let Some(x) = a.get("editorId") {
            v["editorId"] = x.clone();
        }
        if let Some(x) = a.get("dateToFinish") {
            v["dateToFinish"] = x.clone();
        }
        if let Some(x) = a.get("dependentMilestone") {
            v["milestoneid"] = x.clone();
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

        match c.call("tickets.addTicket", json!({"values": v})).await {
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

fn h_update_milestone(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let mid = a.get("milestoneId").and_then(|v| v.as_str()).unwrap_or("");
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
        if let Some(x) = a.get("editorId") {
            ch["editorId"] = x.clone();
        }
        if let Some(x) = a.get("status") {
            ch["status"] = x.clone();
        }
        if let Some(x) = a.get("dateToFinish") {
            ch["dateToFinish"] = x.clone();
        }
        if let Some(x) = a.get("dependentMilestone") {
            ch["milestoneid"] = x.clone();
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
            let ms = match c.call("tickets.getTicket", json!({"id": mid})).await {
                Ok(m) if !m.is_boolean() && !is_leantime_error(&m) => m,
                Ok(_) => {
                    return dry_run_result(
                        false,
                        vec![format!("Milestone {} not found.", mid)],
                        vec![],
                        vec![],
                    )
                }
                Err(e) => return error_result(&e.to_string()),
            };
            let pid = ms
                .get("projectId")
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_default();
            let sm = c.get_status_map(&pid).await.unwrap_or(json!({}));
            let field_map: &[(&str, &str)] = &[
                ("headline", "headline"),
                ("status", "status"),
                ("editorId", "editorId"),
                ("dateToFinish", "dateToFinish"),
                ("dependentMilestone", "milestoneid"),
            ];
            let (mut changes, warnings) = dry_run_changes(&a, &ms, field_map, Some(&sm));
            if let Some(d) = a.get("description").and_then(|x| x.as_str()) {
                changes.push(json!({"field": "description", "to": d}));
            }
            return dry_run_result(true, vec![], changes, warnings);
        }
        // quickUpdateMilestone reads projectId from the session (unset for API
        // keys) — use the safe generic ticket patch instead.
        match c
            .call("tickets.patch", json!({ "id": mid, "params": ch }))
            .await
        {
            Ok(r) => ok_result(&json!({ "ok": r == json!(true), "id": mid })),
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_get_milestone_progress(
    a: Value,
    cl: ClientRef,
) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let mid = a.get("milestoneId").and_then(|v| v.as_str()).unwrap_or("");
        let ms = match c.call("tickets.getTicket", json!({"id": mid})).await {
            Ok(m) => m,
            Err(e) => return error_result(&e.to_string()),
        };
        if ms.is_boolean() || is_leantime_error(&ms) {
            return error_result(&format!("Milestone {} not found.", mid));
        }
        let pid = ms
            .get("projectId")
            .map(|v| match v {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .unwrap_or_default();
        let tickets = match c
            .call(
                "tickets.getAll",
                json!({"searchCriteria": {"milestone": mid, "currentProject": pid}, "limit": 500}),
            )
            .await
        {
            Ok(t) => t,
            Err(e) => return error_result(&e.to_string()),
        };
        let sm = c.get_status_map(&pid).await.unwrap_or(json!({}));

        let default_effort = 3.0;
        let default_priority = 3.0;
        let mut total: f64 = 0.0;
        let mut done: f64 = 0.0;
        // Missing, null, "" — or a literal 0 ("not estimated") — all fall
        // back to the defaults. Otherwise a milestone of unestimated tickets
        // would weigh 0 and report 0% forever.
        let empty_or_missing = |v: Option<&Value>, s: &str| {
            v.is_none()
                || matches!(v, Some(Value::Null))
                || v.and_then(|x| x.as_str()) == Some(s)
                || num_coerce(v, f64::NAN) == 0.0
        };
        if let Some(arr) = tickets.as_array() {
            for t in arr {
                let effort = if empty_or_missing(t.get("storypoints"), "") {
                    default_effort
                } else {
                    num_coerce(t.get("storypoints"), default_effort)
                };
                let priority = if empty_or_missing(t.get("priority"), "") {
                    default_priority
                } else {
                    num_coerce(t.get("priority"), default_priority)
                };
                let factor: f64 = match priority as i64 {
                    1 => 2.0,
                    2 => 1.75,
                    3 => 1.5,
                    4 => 1.25,
                    _ => 1.0,
                };
                let score = effort * factor;
                total += score;
                let sk = t
                    .get("status")
                    .map(|v| match v {
                        Value::String(s) => s.clone(),
                        other => other.to_string(),
                    })
                    .unwrap_or_default();
                if let Some(l) = sm.get(&sk) {
                    if l.get("statusType").and_then(|v| v.as_str()) == Some("DONE") {
                        done += score;
                    }
                }
            }
        }
        let pct: f64 = if total == 0.0 {
            0.0
        } else {
            done / total * 100.0
        };
        ok_result(
            &json!({ "milestoneId": mid, "percentDone": (pct * 10.0).round() / 10.0, "tickets": tickets.as_array().map(|a| a.len()).unwrap_or(0) }),
        )
    })
}

fn h_delete_milestone(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        if let Err(e) = check_destructive(a.get("confirm").and_then(|v| v.as_bool()), "milestone") {
            return error_result(&e);
        }
        let mut c = cl.lock().await;
        let id = a.get("milestoneId").and_then(|v| v.as_str()).unwrap_or("");
        match c.call("tickets.deleteMilestone", json!({ "id": id })).await {
            Ok(r) => {
                if is_leantime_error(&r) {
                    return error_result(&leantime_error_msg(&r));
                }
                ok_result(&json!({ "deleted": true, "milestoneId": id }))
            }
            Err(e) => error_result(&e.to_string()),
        }
    })
}

pub(super) fn tools() -> Vec<Tool> {
    let md = MARKDOWN_HINT;
    let assignment = ASSIGNMENT_HINT;
    vec![
        tool("leantime_list_milestones", "List all milestones for a project",
            vec![rs("projectId", "The project ID")], vec!["projectId"], Box::new(h_list_milestones)),
        tool("leantime_get_milestone", "Get details of a specific milestone",
            vec![rs("projectId", "The project ID"), rs("milestoneId", "The milestone ID")], vec!["projectId", "milestoneId"], Box::new(h_get_milestone)),
        tool_with_annotations("leantime_create_milestone", format!("Create a milestone in a project. The description is {}. {} Pass the chosen editorId (get candidates with leantime_list_users), or pass unassigned: true ONLY if the user explicitly said to leave it unassigned.", md, assignment),
            vec![rs("projectId", "The project ID"), rs("headline", "Milestone title"), os("description", format!("Milestone description in {}", md)), os("editorId", "Assigned user ID (required unless unassigned: true)"), ob("unassigned", "Set to true ONLY when the user explicitly requested an unassigned milestone"), os("dateToFinish", "Due date (YYYY-MM-DD)"), os("dependentMilestone", "Parent milestone ID"), ob("dryRun", DRY_RUN_DESC)],
            vec!["projectId", "headline"], Box::new(h_create_milestone), ToolAnnotations::write()),
        tool_with_annotations("leantime_update_milestone", format!("Update an existing milestone. Only the provided fields are changed (Leantime's patch API — other fields are never wiped). The description is {}.", md),
            vec![rs("milestoneId", "The milestone ID"), os("headline", "New milestone title"), os("description", format!("New description in {}", md)), os("editorId", "New assigned user ID (validates against leantime_list_users)"), on("status", "New status ID"), os("dateToFinish", "New due date (YYYY-MM-DD)"), os("dependentMilestone", "New parent milestone ID"), ob("dryRun", DRY_RUN_DESC)],
            vec!["milestoneId"], Box::new(h_update_milestone), ToolAnnotations::write()),
        tool("leantime_get_milestone_progress", "Get the completion percentage of a milestone (weighted by effort and priority of its tickets, mirroring Leantime's own formula)",
            vec![rs("milestoneId", "The milestone ID")], vec!["milestoneId"], Box::new(h_get_milestone_progress)),
        tool_with_annotations("leantime_delete_milestone", "Delete a milestone (its tickets are kept). Destructive: requires explicit user approval (confirm: true) unless LEANTIME_MCP_DESTRUCTIVE_POLICY is set otherwise.",
            vec![rs("milestoneId", "The milestone ID"), ob("confirm", "MUST be true to actually delete (ask the user for explicit approval first)")],
            vec!["milestoneId"], Box::new(h_delete_milestone), ToolAnnotations::destructive()),
    ]
}
