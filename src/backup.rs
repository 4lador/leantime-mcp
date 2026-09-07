//! Project backup — dumps milestones, tickets, sprints (and optionally
//! comments) to a timestamped JSON file. Works as both a CLI command
//! (`leantmcp backup`) and an MCP tool (`leantime_backup_project`).

use serde_json::{json, Value};

use crate::client::{fetch_limit, LeantimeClient};
use crate::config;

/// Result of a backup run — a summary, never the data itself (token-cheap
/// for the MCP tool response).
pub struct BackupResult {
    /// Path of the written JSON file.
    pub path: std::path::PathBuf,
    /// Name of the backed-up project.
    pub project_name: String,
    /// Number of milestones captured.
    pub milestone_count: usize,
    /// Number of tickets captured (including milestones).
    pub ticket_count: usize,
    /// Number of sprints captured.
    pub sprint_count: usize,
    /// Number of comments captured (0 in fast mode).
    pub comment_count: usize,
    /// Size of the backup file in bytes.
    pub file_size: u64,
    /// Non-fatal warnings (e.g. API limit reached — possible truncation).
    pub warnings: Vec<String>,
}

impl BackupResult {
    /// Short human-readable summary (for the MCP tool and CLI output).
    pub fn summary(&self) -> String {
        let comments = if self.comment_count > 0 {
            format!(", {} comments", self.comment_count)
        } else {
            String::new()
        };
        format!(
            "Backup of \"{}\": {} milestones, {} tickets, {} sprints{} → {} ({})",
            self.project_name,
            self.milestone_count,
            self.ticket_count,
            self.sprint_count,
            comments,
            self.path.display(),
            format_size(self.file_size),
        )
    }
}

/// Warning when a fetch returned exactly the requested limit — items beyond
/// it were NOT captured (the API has no offset pagination).
pub(crate) fn limit_warning(what: &str, count: usize, limit: usize) -> Option<String> {
    if count >= limit && limit > 0 {
        Some(format!(
            "{}: fetched exactly {} items (API limit) — items beyond this were NOT captured; raise LEANTIME_MCP_FETCH_LIMIT and re-run for a complete backup",
            what, limit
        ))
    } else {
        None
    }
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    }
}

/// Directory where backups are stored.
fn backup_dir() -> std::path::PathBuf {
    config::secret_dir().join("backups")
}

/// Sanitize a project name for use in a filename.
fn sanitize_name(name: &str) -> String {
    name.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// Run a backup of one project. Fast mode (3 API calls): milestones,
/// tickets, sprints. Full mode adds per-ticket comments (1 call per ticket).
pub async fn backup_project(
    client: &mut LeantimeClient,
    project_id: &str,
    project_name: &str,
    full: bool,
) -> Result<BackupResult, String> {
    // 1. Milestones
    let limit = fetch_limit();
    let milestones = client
        .call(
            "tickets.getAll",
            json!({"searchCriteria": {"currentProject": project_id, "type": "milestone"}, "limit": limit}),
        )
        .await
        .map_err(|e| format!("Could not fetch milestones: {}", e))?;

    // 2. All tickets (including milestones — Leantime stores them together)
    let tickets = client
        .call(
            "tickets.getAll",
            json!({"searchCriteria": {"currentProject": project_id}, "limit": limit}),
        )
        .await
        .map_err(|e| format!("Could not fetch tickets: {}", e))?;

    // 3. Sprints
    let sprints = client
        .call("sprints.getAllSprints", json!({"projectId": project_id}))
        .await
        .map_err(|e| format!("Could not fetch sprints: {}", e))?;

    let milestone_list = milestones.as_array().cloned().unwrap_or_default();
    let ticket_list = tickets.as_array().cloned().unwrap_or_default();
    let sprint_list = sprints.as_array().cloned().unwrap_or_default();

    // Truncation detection: a fetch that returned exactly the limit means
    // items beyond it were silently dropped by the API.
    let mut warnings = Vec::new();
    if let Some(w) = limit_warning("milestones", milestone_list.len(), limit) {
        warnings.push(w);
    }
    if let Some(w) = limit_warning("tickets", ticket_list.len(), limit) {
        warnings.push(w);
    }

    // 4. Comments (optional, expensive: 1 call per ticket)
    let mut comments: Vec<Value> = Vec::new();
    if full {
        for t in &ticket_list {
            let tid = match &t["id"] {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            };
            if let Ok(c) = client
                .call(
                    "comments.getComments",
                    json!({"module": "ticket", "entityId": tid}),
                )
                .await
            {
                if let Some(arr) = c.as_array() {
                    for comment in arr {
                        comments.push(json!({"ticketId": tid, "comment": comment}));
                    }
                }
            }
        }
    }

    // 5. Assemble the backup document
    let now = chrono::Local::now();
    let timestamp = now.format("%Y%m%dT%H%M%S");
    let backup = json!({
        "_meta": {
            "format": "leantime-mcp-backup/1",
            "created": now.format("%Y-%m-%dT%H:%M:%S%:z").to_string(),
            "project": {"id": project_id, "name": project_name},
            "full": full,
            "counts": {
                "milestones": milestone_list.len(),
                "tickets": ticket_list.len(),
                "sprints": sprint_list.len(),
                "comments": comments.len(),
            },
        },
        "milestones": milestone_list,
        "tickets": ticket_list,
        "sprints": sprint_list,
        "comments": comments,
    });

    // 6. Write the file
    let dir = backup_dir();
    std::fs::create_dir_all(&dir)
        .map_err(|e| format!("Could not create backup directory: {}", e))?;

    let filename = format!("{}-{}.json", sanitize_name(project_name), timestamp);
    let path = dir.join(filename);
    let content =
        serde_json::to_string_pretty(&backup).map_err(|e| format!("Serialization error: {}", e))?;

    config::write_private_file(&path, &content)
        .map_err(|e| format!("Could not write backup: {}", e))?;

    let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

    Ok(BackupResult {
        path,
        project_name: project_name.to_string(),
        milestone_count: milestone_list.len(),
        ticket_count: ticket_list.len(),
        sprint_count: sprint_list.len(),
        comment_count: comments.len(),
        file_size,
        warnings,
    })
}

/// List existing backups (newest first).
pub fn list_backups() -> Vec<(std::path::PathBuf, u64)> {
    let dir = backup_dir();
    let mut backups: Vec<(std::path::PathBuf, u64)> = std::fs::read_dir(&dir)
        .map(|entries| {
            entries
                .flatten()
                .filter(|e| e.path().extension().is_some_and(|ext| ext == "json"))
                .map(|e| {
                    let size = e.metadata().ok().map(|m| m.len()).unwrap_or(0);
                    (e.path(), size)
                })
                .collect()
        })
        .unwrap_or_default();
    backups.sort_by(|a, b| b.0.file_name().cmp(&a.0.file_name()));
    backups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn limit_warning_fires_at_exact_limit() {
        let w = limit_warning("tickets", 500, 500).expect("warning expected at exact limit");
        assert!(w.contains("NOT captured"), "{}", w);
        assert!(w.contains("LEANTIME_MCP_FETCH_LIMIT"), "{}", w);
        // Beyond the limit (should not happen normally, but the check is >=)
        assert!(limit_warning("tickets", 600, 500).is_some());
    }

    #[test]
    fn limit_warning_silent_below_limit() {
        assert!(limit_warning("tickets", 279, 500).is_none());
        assert!(limit_warning("tickets", 499, 500).is_none());
        // Degenerate limit 0 — never warn
        assert!(limit_warning("tickets", 0, 0).is_none());
    }
}
