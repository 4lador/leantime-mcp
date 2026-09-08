//! Project backup — dumps milestones, tickets, sprints (and optionally
//! comments) to a timestamped JSON file. Works as both a CLI command
//! (`leantmcp backup`) and an MCP tool (`leantime_backup_project`).

use serde_json::{json, Value};

use crate::client::LeantimeClient;
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

/// Comment-fetch concurrency for `--full` backups (LEANTIME_MCP_BACKUP_CONCURRENCY).
/// Default 1 = sequential — the safe choice for rate-limited instances where
/// concurrency buys nothing. Higher values (capped at 8) only pay off on
/// instances with generous limits, where round-trip latency — not the rate
/// limit — is the bottleneck.
fn comment_concurrency() -> usize {
    std::env::var("LEANTIME_MCP_BACKUP_CONCURRENCY")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(1)
        .clamp(1, 8)
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
    // 1. Milestones — chunked completeness fetch (fast path: 1 call; falls
    //    back to date-window bisection if the API limit is hit).
    let (milestone_list, mut warnings) = client
        .get_all_tickets_chunked(project_id, json!({"type": "milestone"}))
        .await
        .map_err(|e| format!("Could not fetch milestones: {}", e))?;

    // 2. All tickets (including milestones — Leantime stores them together)
    let (ticket_list, ticket_warnings) = client
        .get_all_tickets_chunked(project_id, json!({}))
        .await
        .map_err(|e| format!("Could not fetch tickets: {}", e))?;
    warnings.extend(ticket_warnings);

    // 3. Sprints
    let sprints = client
        .call("sprints.getAllSprints", json!({"projectId": project_id}))
        .await
        .map_err(|e| format!("Could not fetch sprints: {}", e))?;

    let sprint_list = sprints.as_array().cloned().unwrap_or_default();

    // 4. Comments (optional, expensive: 1 call per ticket). Bounded
    //    concurrency via LEANTIME_MCP_BACKUP_CONCURRENCY (default 1 =
    //    sequential, kind to rate-limited instances); results stay in
    //    ticket order so the backup file is deterministic either way.
    let mut comments: Vec<Value> = Vec::new();
    if full && !ticket_list.is_empty() {
        let tids: Vec<String> = ticket_list
            .iter()
            .map(|t| match &t["id"] {
                Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect();
        let results = client
            .get_comments_for_tickets(&tids, comment_concurrency())
            .await;
        for (tid, res) in tids.iter().zip(results) {
            if let Ok(c) = res {
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
