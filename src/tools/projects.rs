use std::future::Future;
use std::pin::Pin;

use serde_json::{json, Value};

use crate::markdown::markdown_to_html;

use super::shared::*;
use super::{error_result, ok_result, rpc, ClientRef, Tool, ToolAnnotations};

fn h_list_projects(_a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        match c.call("Projects.getAll", json!({})).await {
            Ok(r) => ok_result(&r),
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_get_project(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let id = a.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
        match c.call("projects.getProject", json!({"id": id})).await {
            Ok(r) => ok_result(&r),
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_create_project(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let mut v = json!({"name": a.get("name"), "clientId": a.get("clientId")});
        if let Some(d) = a.get("details").and_then(|x| x.as_str()) {
            v["details"] = json!(markdown_to_html(d));
        }
        if let Some(x) = a.get("hourBudget") {
            v["hourBudget"] = x.clone();
        }
        if let Some(x) = a.get("dollarBudget") {
            v["dollarBudget"] = x.clone();
        }
        match c.call("projects.addProject", json!({"values": v})).await {
            Ok(r) => {
                if is_leantime_error(&r) {
                    return error_result(&leantime_error_msg(&r));
                }
                if r == json!(false) || r.is_null() {
                    return error_result("Project creation failed.");
                }
                let id = r.as_array().and_then(|x| x.first()).cloned().unwrap_or(r);
                ok_result(&json!({ "id": id }))
            }
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_update_project(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let id = a.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
        let mut params = json!({});
        if let Some(x) = a.get("name") {
            params["name"] = x.clone();
        }
        if let Some(d) = a.get("details").and_then(|x| x.as_str()) {
            params["details"] = json!(markdown_to_html(d));
        }
        if let Some(x) = a.get("hourBudget") {
            params["hourBudget"] = x.clone();
        }
        if let Some(x) = a.get("dollarBudget") {
            params["dollarBudget"] = x.clone();
        }
        if params.as_object().map(|o| o.is_empty()).unwrap_or(true) {
            return error_result("Nothing to update: provide at least one field to change.");
        }
        match c
            .call("projects.patch", json!({"id": id, "params": params}))
            .await
        {
            Ok(r) => ok_result(&json!({ "ok": r == json!(true), "id": id })),
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_find_projects(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let term = a.get("term").and_then(|v| v.as_str()).unwrap_or("");
        match c.call("projects.findProject", json!({"term": term})).await {
            Ok(r) => {
                // findProject mangles ids into "id-modified" — normalize back to plain ids.
                let mut items = r.as_array().cloned().unwrap_or_default();
                for p in items.iter_mut() {
                    if let Some(idv) = p.get("id") {
                        let raw = match idv {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        if let Some(obj) = p.as_object_mut() {
                            obj.insert("id".into(), json!(raw.split('-').next().unwrap_or(&raw)));
                        }
                    }
                }
                ok_result(&json!(items))
            }
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_list_project_users(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let pid = a.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
        match c
            .call(
                "projects.getUsersAssignedToProject",
                json!({"projectId": pid}),
            )
            .await
        {
            Ok(r) => {
                let simplified: Vec<Value> = r.as_array().map(|arr| arr.iter().map(|u| {
                    let mut o = json!({
                        "id": u.get("id").map(|v| match v { Value::String(s) => json!(s), other => json!(other.to_string()) }).unwrap_or(json!("")),
                        "name": format!("{} {}",
                            u.get("firstname").and_then(|v| v.as_str()).unwrap_or(""),
                            u.get("lastname").and_then(|v| v.as_str()).unwrap_or("")).trim(),
                    });
                    if let Some(role) = u.get("projectRole") { if !role.is_null() { o["role"] = role.clone(); } }
                    o
                }).collect()).unwrap_or_default();
                ok_result(&json!(simplified))
            }
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_list_clients(_a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        match c.call("Clients.getAll", json!({})).await {
            Ok(r) => {
                let simplified: Vec<Value> = r.as_array().map(|arr| arr.iter().map(|cl| json!({
                    "id": cl.get("id").map(|v| match v { Value::String(s) => json!(s), other => json!(other.to_string()) }).unwrap_or(json!("")),
                    "name": cl.get("name").cloned().unwrap_or(Value::Null),
                })).collect()).unwrap_or_default();
                ok_result(&json!(simplified))
            }
            Err(e) => error_result(&e.to_string()),
        }
    })
}

pub(super) fn tools() -> Vec<Tool> {
    let md = MARKDOWN_HINT;
    vec![
        tool("leantime_list_projects", "List all projects assigned to the current user",
            vec![], vec![], Box::new(h_list_projects)),
        tool("leantime_get_project", "Get details of a specific project",
            vec![rs("projectId", "The project ID")], vec!["projectId"], Box::new(h_get_project)),
        tool("leantime_get_project_progress", "Get progress metrics for a specific project",
            vec![rs("projectId", "The project ID")], vec!["projectId"],
            rpc("projects.getProjectProgress", |a: &Value| json!({"projectId": a.get("projectId")}))),
        tool_with_annotations("leantime_create_project", format!("Create a new project. The details field is {}. Get a valid clientId with leantime_list_clients first.", md),
            vec![rs("name", "Project name"), rs("clientId", "Client ID (see leantime_list_clients)"), os("details", format!("Project details in {}", md)), on("hourBudget", "Hour budget"), on("dollarBudget", "Dollar budget")],
            vec!["name", "clientId"], Box::new(h_create_project), ToolAnnotations::write()),
        tool_with_annotations("leantime_update_project", "Update a project. Only the provided fields are changed (patch API — other fields are never wiped).",
            vec![rs("projectId", "The project ID"), os("name", "New project name"), os("details", format!("New details in {}", md)), on("hourBudget", "New hour budget"), on("dollarBudget", "New dollar budget")],
            vec!["projectId"], Box::new(h_update_project), ToolAnnotations::write()),
        tool("leantime_find_projects", "Search projects by name (fuzzy)",
            vec![rs("term", "Search term")], vec!["term"], Box::new(h_find_projects)),
        tool("leantime_list_project_users", "List the users assigned to a project (id, name) — the valid editorId candidates for tickets and milestones of that project",
            vec![rs("projectId", "The project ID")], vec!["projectId"], Box::new(h_list_project_users)),
        tool("leantime_list_clients", "List all clients (id, name) — clientId is required to create projects",
            vec![], vec![], Box::new(h_list_clients)),
    ]
}
