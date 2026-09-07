use std::future::Future;
use std::pin::Pin;

use serde_json::{json, Value};

use super::shared::*;
use super::{error_result, ok_result, ClientRef, Tool, ToolAnnotations};

fn h_list_sprints(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let pid = a.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
        match c
            .call("sprints.getAllSprints", json!({"projectId": pid}))
            .await
        {
            Ok(r) => ok_result(&json!(r.as_array().cloned().unwrap_or_default())),
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_create_sprint(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let pid = a.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
        // addSprint defaults projectId to the session's current project, which is
        // NOT set for API keys — always pass it explicitly.
        match c.call("sprints.addSprint", json!({"params": {
            "name": a.get("name"), "startDate": a.get("startDate"), "endDate": a.get("endDate"), "projectId": pid
        }})).await {
            Ok(r) => {
                if is_leantime_error(&r) { return error_result(&leantime_error_msg(&r)); }
                if r == json!(false) || r.is_null() { return error_result(&format!("Sprint creation failed for project {}.", pid)); }
                let id = r.as_array().and_then(|x| x.first()).cloned().unwrap_or(r);
                ok_result(&json!({ "id": id, "projectId": pid }))
            }
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_update_sprint(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let sid = a.get("sprintId").and_then(|v| v.as_str()).unwrap_or("");
        if a.get("name").is_none() && a.get("startDate").is_none() && a.get("endDate").is_none() {
            return error_result("Nothing to update: provide name, startDate and/or endDate.");
        }
        // editSprint overwrites projectId from the session (unset for API keys):
        // fetch the sprint first and always resend its full field set.
        let sprint = match c.call("sprints.getSprint", json!({"id": sid})).await {
            Ok(s) => s,
            Err(e) => return error_result(&e.to_string()),
        };
        if sprint.is_boolean() || is_leantime_error(&sprint) {
            return error_result(&format!("Sprint {} not found.", sid));
        }
        let get_field = |name: &str| sprint.get(name).cloned().unwrap_or(Value::Null);
        let new_name = a.get("name").cloned().unwrap_or_else(|| get_field("name"));
        let new_start = a
            .get("startDate")
            .cloned()
            .unwrap_or_else(|| get_field("startDate"));
        let new_end = a
            .get("endDate")
            .cloned()
            .unwrap_or_else(|| get_field("endDate"));

        match c.call("sprints.editSprint", json!({"params": {
            "id": sid, "projectId": get_field("projectId"), "name": new_name, "startDate": new_start, "endDate": new_end
        }})).await {
            Ok(r) => ok_result(&json!({ "ok": !is_leantime_error(&r), "sprintId": sid })),
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_get_current_sprint(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let pid = a.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
        // Computed from sprint dates — Leantime's session-based currentSprint
        // is unavailable to API keys.
        let sprints = match c
            .call("sprints.getAllSprints", json!({"projectId": pid}))
            .await
        {
            Ok(s) => s,
            Err(e) => return error_result(&e.to_string()),
        };
        let now = chrono::Local::now().timestamp_millis();
        let mut with_time: Vec<Value> = Vec::new();
        for s in sprints.as_array().cloned().unwrap_or_default().into_iter() {
            let start = s
                .get("startDate")
                .and_then(|v| v.as_str())
                .and_then(parse_leantime_ts);
            let end = s
                .get("endDate")
                .and_then(|v| v.as_str())
                .and_then(parse_leantime_ts);
            let mut item = s;
            item["start"] = start.map(|ms| json!(ms)).unwrap_or(json!(null));
            item["end"] = end.map(|ms| json!(ms)).unwrap_or(json!(null));
            with_time.push(item);
        }
        let current = with_time
            .iter()
            .find(|s| {
                s.get("start")
                    .and_then(|v| v.as_i64())
                    .map(|st| st <= now)
                    .unwrap_or(false)
                    && s.get("end")
                        .and_then(|v| v.as_i64())
                        .map(|e| now <= e)
                        .unwrap_or(false)
            })
            .cloned();
        let upcoming = with_time
            .iter()
            .filter(|s| {
                s.get("start")
                    .and_then(|v| v.as_i64())
                    .map(|st| st > now)
                    .unwrap_or(false)
            })
            .min_by_key(|s| s.get("start").and_then(|v| v.as_i64()).unwrap_or(i64::MAX))
            .cloned();
        ok_result(
            &json!({ "current": current, "upcoming": if current.is_some() { json!(null) } else { upcoming.unwrap_or(json!(null)) } }),
        )
    })
}

pub(super) fn tools() -> Vec<Tool> {
    vec![
        tool("leantime_list_sprints", "List all sprints for a project",
            vec![rs("projectId", "The project ID")], vec!["projectId"], Box::new(h_list_sprints)),
        tool_with_annotations("leantime_create_sprint", "Create a sprint in a project",
            vec![rs("projectId", "The project ID"), rs("name", "Sprint name"), rs("startDate", "Start date, YYYY-MM-DD"), rs("endDate", "End date, YYYY-MM-DD")],
            vec!["projectId", "name", "startDate", "endDate"], Box::new(h_create_sprint), ToolAnnotations::write()),
        tool_with_annotations("leantime_update_sprint", "Update a sprint (name and/or dates)",
            vec![rs("sprintId", "The sprint ID"), os("name", "New sprint name"), os("startDate", "New start date, YYYY-MM-DD"), os("endDate", "New end date, YYYY-MM-DD")],
            vec!["sprintId"], Box::new(h_update_sprint), ToolAnnotations::write()),
        tool("leantime_get_current_sprint", "Get the sprint currently in progress for a project (falls back to the next upcoming one). Computed from sprint dates — Leantime's session-based currentSprint is unavailable to API keys.",
            vec![rs("projectId", "The project ID")], vec!["projectId"], Box::new(h_get_current_sprint)),
    ]
}
