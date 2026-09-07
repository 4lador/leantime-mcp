//! `leantime_backup_project` — dump a project to a local JSON file.
//! The response is a summary only (path + counts), never the data itself.

use std::future::Future;
use std::pin::Pin;

use serde_json::{json, Value};

use super::shared::tool;
use super::{error_result, ok_result, ClientRef, Tool};
use crate::backup;

fn h_backup_project(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let pid = a.get("projectId").and_then(|v| v.as_str()).unwrap_or("");

        // Fetch the project name for the summary and filename
        let project = match c.call("projects.getProject", json!({"id": pid})).await {
            Ok(p) => p,
            Err(e) => return error_result(&e.to_string()),
        };
        let project_name = project
            .get("name")
            .and_then(|n| n.as_str())
            .unwrap_or("project")
            .to_string();

        match backup::backup_project(&mut c, pid, &project_name, false).await {
            Ok(r) => ok_result(&json!({
                "ok": true,
                "backupPath": r.path.display().to_string(),
                "project": r.project_name,
                "milestones": r.milestone_count,
                "tickets": r.ticket_count,
                "sprints": r.sprint_count,
                "fileSize": format!("{} bytes", r.file_size),
                "warnings": r.warnings,
                "note": "Comments excluded (fast mode) — run 'leantmcp backup --full' from the CLI for a complete backup including comments."
            })),
            Err(e) => error_result(&e),
        }
    })
}

pub(super) fn tools() -> Vec<Tool> {
    vec![tool(
        "leantime_backup_project",
        "Backup a project to a local timestamped JSON file (milestones, tickets, sprints — comments excluded for speed). The backup lands in ~/.config/leantime/backups/. Use this before bulk modifications or destructive operations to ensure data can be recovered. For a full backup including comments, run 'leantmcp backup --full' from the CLI.",
        vec![super::shared::rs("projectId", "The project ID")],
        vec!["projectId"],
        Box::new(h_backup_project),
    )]
}
