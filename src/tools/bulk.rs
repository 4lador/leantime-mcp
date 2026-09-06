use std::future::Future;
use std::pin::Pin;

use serde_json::{json, Value};

use crate::markdown::markdown_to_html;

use super::shared::*;
use super::{error_result, ok_result, ClientRef, Tool, ToolAnnotations};

fn bulk_summary(results: Vec<Value>) -> Value {
    let total = results.len();
    let created = results
        .iter()
        .filter(|r| r.get("ok") == Some(&json!(true)))
        .count();
    json!({ "summary": { "total": total, "created": created, "failed": total - created }, "results": results })
}

fn check_batch_len(len: usize) -> Result<(), String> {
    if len == 0 {
        return Err("Batch must contain at least 1 item.".into());
    }
    if len > MAX_BATCH {
        return Err(format!("Batch too large: max {} items.", MAX_BATCH));
    }
    Ok(())
}

fn h_bulk_create(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let pid = a.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
        let tickets = a
            .get("tickets")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if let Err(e) = check_batch_len(tickets.len()) {
            return error_result(&e);
        }
        let users = match get_users_simplified(&mut c).await {
            Ok(u) => u,
            Err(e) => return error_result(&e),
        };

        // ---- Phase 1: Upfront validation (zero API writes) ----
        let mut errors = Vec::new();
        for (i, t) in tickets.iter().enumerate() {
            let idx = i + 1;
            let headline = t.get("headline").and_then(|v| v.as_str()).unwrap_or("");
            let has_editor = t.get("editorId").map(|v| !v.is_null()).unwrap_or(false);
            let unassigned = t
                .get("unassigned")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            if !has_editor && !unassigned {
                errors.push(format!(
                    "Item {} (\"{}\"): assignment required — pass editorId or unassigned: true",
                    idx, headline
                ));
            }
            if let Some(eid) = t.get("editorId") {
                let eid_str = match eid {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                if !users.iter().any(|(uid, _)| *uid == eid_str) {
                    errors.push(format!(
                        "Item {}: editorId \"{}\" does not exist. Available: {}",
                        idx,
                        eid_str,
                        users_list(&users)
                    ));
                }
            }
        }
        if !errors.is_empty() {
            return error_result(&format!(
                "Validation failed — NOTHING was created (all-or-nothing):\n{}",
                errors.join("\n")
            ));
        }

        // ---- Phase 2: Sequential creation ----
        // Leantime's addTicket expects an int projectId in the bulk path
        // (isUserAssignedToProject receives null on a string) — coerce to number.
        let pid_num: Value = pid.parse::<i64>().map(|n| json!(n)).unwrap_or(json!(pid));
        let mut results = Vec::new();
        for (i, t) in tickets.iter().enumerate() {
            let mut v = json!({ "headline": t.get("headline"), "projectId": pid_num });
            if let Some(d) = t.get("description").and_then(|x| x.as_str()) {
                v["description"] = json!(markdown_to_html(d));
            }
            for (k, dst) in [
                ("type", "type"),
                ("priority", "priority"),
                ("editorId", "editorId"),
                ("tags", "tags"),
                ("dateToFinish", "dateToFinish"),
                ("planHours", "planHours"),
                ("dependingTicketId", "dependingTicketId"),
            ] {
                if let Some(x) = t.get(k) {
                    v[dst] = x.clone();
                }
            }
            if let Some(x) = t.get("milestoneId") {
                v["milestoneid"] = x.clone();
            }
            if let Some(x) = t.get("sprintId") {
                v["sprint"] = x.clone();
            }

            match c.call("tickets.addTicket", json!({ "values": v })).await {
                Ok(r) => {
                    if let Some(id) = r.as_array().and_then(|x| x.first()) {
                        let id_str = match id {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        results.push(json!({ "index": i + 1, "ok": true, "id": id_str }));
                    } else {
                        results.push(json!({ "index": i + 1, "ok": false, "error": "unexpected API response" }));
                    }
                }
                Err(e) => {
                    results.push(json!({ "index": i + 1, "ok": false, "error": e.to_string() }))
                }
            }
        }
        ok_result(&bulk_summary(results))
    })
}

fn h_bulk_update(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let updates = a
            .get("updates")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if let Err(e) = check_batch_len(updates.len()) {
            return error_result(&e);
        }
        // Upfront: validate any editorId changes
        let editor_ids: Vec<&Value> = updates
            .iter()
            .filter_map(|u| u.get("editorId"))
            .filter(|v| !v.is_null())
            .collect();
        if !editor_ids.is_empty() {
            let users = match get_users_simplified(&mut c).await {
                Ok(u) => u,
                Err(e) => return error_result(&e),
            };
            for id in &editor_ids {
                let id_str = match id {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                if !users.iter().any(|(uid, _)| *uid == id_str) {
                    // Message byte-parity with the TS edition (bulk.ts).
                    return error_result(&format!(
                        "editorId \"{}\" does not exist — NOTHING was updated. Available: {}",
                        id_str,
                        users_list(&users)
                    ));
                }
            }
        }

        let mut results = Vec::new();
        for (i, u) in updates.iter().enumerate() {
            let tid = match u.get("ticketId") {
                Some(Value::String(s)) => s.clone(),
                Some(other) => other.to_string(),
                None => String::new(),
            };
            let mut ch = json!({});
            if let Some(v) = u.get("headline") {
                ch["headline"] = v.clone();
            }
            if let Some(d) = u.get("description").and_then(|x| x.as_str()) {
                ch["description"] = json!(markdown_to_html(d));
            }
            for (k, dst) in [
                ("type", "type"),
                ("status", "status"),
                ("priority", "priority"),
                ("editorId", "editorId"),
                ("tags", "tags"),
                ("storypoints", "storypoints"),
                ("dateToFinish", "dateToFinish"),
                ("planHours", "planHours"),
            ] {
                if let Some(x) = u.get(k) {
                    ch[dst] = x.clone();
                }
            }
            if let Some(x) = u.get("milestoneId") {
                ch["milestoneid"] = x.clone();
            }
            if let Some(x) = u.get("sprintId") {
                ch["sprint"] = x.clone();
            }

            if ch.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                results.push(json!({ "index": i + 1, "ok": false, "id": tid, "error": "no fields to update" }));
                continue;
            }
            match c
                .call("tickets.patch", json!({ "id": tid, "params": ch }))
                .await
            {
                Ok(r) => {
                    let ok = r == json!(true) || (r.is_array() && r[0] == json!(true));
                    results.push(json!({ "index": i + 1, "ok": ok, "id": tid }));
                }
                Err(e) => results.push(
                    json!({ "index": i + 1, "ok": false, "id": tid, "error": e.to_string() }),
                ),
            }
        }
        ok_result(&bulk_summary(results))
    })
}

fn h_bulk_schedule(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let schedules = a
            .get("schedules")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();
        if let Err(e) = check_batch_len(schedules.len()) {
            return error_result(&e);
        }
        let mut results = Vec::new();
        for (i, s) in schedules.iter().enumerate() {
            let tid = match s.get("ticketId") {
                Some(Value::String(t)) => t.clone(),
                Some(other) => other.to_string(),
                None => String::new(),
            };
            let mut ch = json!({});
            if let Some(v) = s.get("sprintId") {
                ch["sprint"] = v.clone();
            }
            if let Some(v) = s.get("editFrom") {
                ch["editFrom"] = v.clone();
            }
            if let Some(v) = s.get("editTo") {
                ch["editTo"] = v.clone();
            }

            if ch.as_object().map(|o| o.is_empty()).unwrap_or(true) {
                results.push(json!({ "index": i + 1, "ok": false, "id": tid, "error": "nothing to schedule" }));
                continue;
            }
            match c
                .call("tickets.patch", json!({ "id": tid, "params": ch }))
                .await
            {
                Ok(r) => {
                    let ok = r == json!(true) || (r.is_array() && r[0] == json!(true));
                    results.push(json!({ "index": i + 1, "ok": ok, "id": tid }));
                }
                Err(e) => results.push(
                    json!({ "index": i + 1, "ok": false, "id": tid, "error": e.to_string() }),
                ),
            }
        }
        ok_result(&bulk_summary(results))
    })
}

// ---------------------------------------------------------------------------
// Registry — all 40 tools
// ---------------------------------------------------------------------------

pub(super) fn tools() -> Vec<Tool> {
    let rate = RATE_LIMIT_NOTE;
    vec![
        tool_with_annotations("leantime_bulk_create_tickets", format!("Create multiple tickets in one call (max {}). ALL items are validated BEFORE anything is created — if any item fails validation (missing assignment, unknown editorId), nothing is created. Descriptions are Markdown, converted to rich HTML per ticket. Each item requires editorId or unassigned: true. {}", MAX_BATCH, rate),
            vec![rs("projectId", "The project ID"), ("tickets".to_string(), json!({"type": "array", "description": format!("Array of ticket specifications (max {})", MAX_BATCH), "minItems": 1, "maxItems": MAX_BATCH}))],
            vec!["projectId", "tickets"], Box::new(h_bulk_create), ToolAnnotations::write()),
        tool_with_annotations("leantime_bulk_update_tickets", format!("Update multiple tickets in one call (max {}). Uses the safe patch API — only provided fields change, others are never wiped. Results are per-item: some may succeed while others fail. {}", MAX_BATCH, rate),
            vec![rs("projectId", "The project ID"), ("updates".to_string(), json!({"type": "array", "description": format!("Array of ticket updates (max {})", MAX_BATCH), "minItems": 1, "maxItems": MAX_BATCH}))],
            vec!["projectId", "updates"], Box::new(h_bulk_update), ToolAnnotations::write()),
        tool_with_annotations("leantime_bulk_schedule_tickets", format!("Schedule multiple tickets at once (max {}): assign to a sprint and/or set editFrom/editTo dates. Uses the safe patch API. {}", MAX_BATCH, rate),
            vec![rs("projectId", "The project ID"), ("schedules".to_string(), json!({"type": "array", "description": format!("Array of ticket schedules (max {})", MAX_BATCH), "minItems": 1, "maxItems": MAX_BATCH}))],
            vec!["projectId", "schedules"], Box::new(h_bulk_schedule), ToolAnnotations::write()),
    ]
}
