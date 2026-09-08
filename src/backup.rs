//! Project backup — dumps milestones, tickets, sprints (and optionally
//! comments) to a timestamped JSON file. Works as both a CLI command
//! (`leantmcp backup`) and an MCP tool (`leantime_backup_project`).

use serde_json::{json, Value};
use std::path::Path;

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

/// Human-readable size for reports ("4.2 MB").
pub fn format_size(bytes: u64) -> String {
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
    let dir = backup_dir();
    backup_project_to(client, project_id, project_name, full, &dir).await
}

/// Backup with an explicit output directory (CLI `--output`; the MCP tool
/// and the default CLI path use the standard keyring dir).
pub async fn backup_project_to(
    client: &mut LeantimeClient,
    project_id: &str,
    project_name: &str,
    full: bool,
    dir: &Path,
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

    // 6. Write the file (atomic: temp + rename — a partial file is never
    //    visible under the final name)
    std::fs::create_dir_all(dir)
        .map_err(|e| format!("Could not create backup directory: {}", e))?;

    let filename = format!("{}-{}.json", sanitize_name(project_name), timestamp);
    let path = dir.join(&filename);
    let content =
        serde_json::to_string_pretty(&backup).map_err(|e| format!("Serialization error: {}", e))?;

    let tmp = dir.join(format!(".{}.tmp", filename));
    config::write_private_file(&tmp, &content)
        .map_err(|e| format!("Could not write backup: {}", e))?;
    std::fs::rename(&tmp, &path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("Could not finalize backup: {}", e)
    })?;

    let file_size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);

    // 7. Retention chain (opt-in): validate the fresh backup, then purge
    //    same-project backups older than the window. Warnings only.
    if let Some(msg) = retention_after_backup(dir, &path, &sanitize_name(project_name)) {
        warnings.push(msg);
    }

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

// ---------------------------------------------------------------------------
// Retention & purge
// ---------------------------------------------------------------------------

/// Retention window in days — `LEANTIME_MCP_BACKUP_RETENTION_DAYS`
/// (default 0 = keep everything, opt-in only: an upgrade must not silently
/// delete existing backups).
pub fn retention_days() -> i64 {
    std::env::var("LEANTIME_MCP_BACKUP_RETENTION_DAYS")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(0)
        .max(0)
}

/// Validate a freshly written backup before any purge touches older ones:
/// re-read the file from disk and parse it (structure `_meta`, sections,
/// counts present). A kill mid-write or a full disk leaves a partial file —
/// the validate-then-purge guard keeps the old generation intact.
pub fn validate_backup_file(path: &Path) -> Result<(), String> {
    let raw = std::fs::read_to_string(path).map_err(|e| format!("backup re-read failed: {}", e))?;
    let parsed: Value =
        serde_json::from_str(&raw).map_err(|e| format!("backup re-parse failed: {}", e))?;
    let meta = parsed
        .get("_meta")
        .ok_or_else(|| "backup _meta section missing".to_string())?;
    if meta.get("format").and_then(|f| f.as_str()).is_none() {
        return Err("backup _meta.format missing".into());
    }
    if meta.get("counts").and_then(|c| c.as_object()).is_none() {
        return Err("backup _meta.counts missing".into());
    }
    for section in ["tickets", "sprints"] {
        if parsed.get(section).and_then(|s| s.as_array()).is_none() {
            return Err(format!(
                "backup section '{}' missing or not an array",
                section
            ));
        }
    }
    Ok(())
}

/// Purge backups of ONE project (filename prefix) older than the retention
/// window. Never touches `fresh` (the just-written backup). Returns
/// (count purged, bytes freed). Purge issues are warnings, not errors —
/// the backup itself already succeeded.
pub fn purge_old_backups(
    dir: &Path,
    fresh: &Path,
    project_prefix: &str,
    days: i64,
) -> Result<(usize, u64), String> {
    if days <= 0 {
        return Ok((0, 0));
    }
    let cutoff = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_secs()
        .saturating_sub((days as u64).saturating_mul(86_400));
    let mut purged = 0usize;
    let mut freed = 0u64;
    let entries = std::fs::read_dir(dir).map_err(|e| format!("purge: {}", e))?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        // Scope: same-project backups only (filename prefix), .json only,
        // and never the file just written.
        if !name.starts_with(project_prefix) || !name.ends_with(".json") {
            continue;
        }
        if path == fresh {
            continue;
        }
        let mtime = match entry.metadata().ok().and_then(|m| m.modified().ok()) {
            Some(t) => t
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            None => continue,
        };
        if mtime >= cutoff {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if std::fs::remove_file(&path).is_ok() {
            purged += 1;
            freed += size;
        }
    }
    Ok((purged, freed))
}

/// Run the full post-backup retention chain: validate the fresh backup,
/// then purge same-project backups older than the retention window.
/// Returns a human-readable line, or None when retention is disabled (0).
pub fn retention_after_backup(dir: &Path, fresh: &Path, project_prefix: &str) -> Option<String> {
    let days = retention_days();
    if days <= 0 {
        return None;
    }
    if let Err(e) = validate_backup_file(fresh) {
        return Some(format!(
            "retention skipped — fresh backup failed validation ({}); nothing was purged",
            e
        ));
    }
    match purge_old_backups(dir, fresh, project_prefix, days) {
        Ok((0, _)) => None,
        Ok((n, bytes)) => Some(format!(
            "pruned {} backup(s) older than {}d (freed {})",
            n,
            days,
            format_size(bytes)
        )),
        Err(e) => Some(format!("retention purge warning: {}", e)),
    }
}

#[cfg(test)]
mod retention_tests {
    use super::*;
    use std::path::{Path, PathBuf};

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("leantmcp-ret-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(d.join("backups")).unwrap();
        d
    }

    fn touch(dir: &Path, name: &str, age_secs: u64, content: &str) -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, content).unwrap();
        let past = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs()
            .saturating_sub(age_secs);
        let t = std::time::SystemTime::UNIX_EPOCH
            .checked_add(std::time::Duration::from_secs(past))
            .unwrap();
        std::fs::File::options()
            .write(true)
            .open(&p)
            .unwrap()
            .set_modified(t)
            .unwrap();
        p
    }

    #[test]
    fn purge_never_touches_fresh_or_other_projects() {
        let d = tmpdir("guard");
        let dir = d.join("backups");
        let fresh = touch(&dir, "Vision-20260908T120000.json", 0, "{}");
        let old_same = touch(&dir, "Vision-20260101T000000.json", 86_400 * 90, "{}");
        let old_other = touch(&dir, "Other-20260101T000000.json", 86_400 * 90, "{}");
        let (n, _) = purge_old_backups(&dir, &fresh, "Vision", 30).unwrap();
        assert_eq!(n, 1, "only the old same-project file goes");
        assert!(fresh.exists(), "fresh backup survives");
        assert!(old_other.exists(), "other-project backups survive");
        assert!(!old_same.exists(), "old same-project backup purged");
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn purge_respects_the_retention_window() {
        let d = tmpdir("window");
        let dir = d.join("backups");
        let fresh = touch(&dir, "P-now.json", 0, "{}");
        let within = touch(&dir, "P-recent.json", 86_400 * 10, "{}");
        let beyond = touch(&dir, "P-old.json", 86_400 * 40, "{}");
        let (n, _) = purge_old_backups(&dir, &fresh, "P", 30).unwrap();
        assert_eq!(n, 1);
        assert!(within.exists());
        assert!(!beyond.exists());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn zero_days_disables_purge() {
        let d = tmpdir("zero");
        let dir = d.join("backups");
        let fresh = touch(&dir, "P-now.json", 0, "{}");
        touch(&dir, "P-old.json", 86_400 * 400, "{}");
        let (n, _) = purge_old_backups(&dir, &fresh, "P", 0).unwrap();
        assert_eq!(n, 0);
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn validate_rejects_partial_files() {
        let d = tmpdir("validate");
        let good = d.join("good.json");
        std::fs::write(
            &good,
            r#"{"_meta": {"format": "leantime-mcp-backup/1", "counts": {"tickets": 1}}, "tickets": [], "sprints": []}"#,
        )
        .unwrap();
        assert!(validate_backup_file(&good).is_ok());

        let truncated = d.join("truncated.json");
        std::fs::write(&truncated, r#"{"_meta": {"format""#).unwrap();
        assert!(validate_backup_file(&truncated).is_err());

        let no_meta = d.join("nometa.json");
        std::fs::write(&no_meta, r#"{"tickets": []}"#).unwrap();
        assert!(validate_backup_file(&no_meta).is_err());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn retention_skips_purge_when_validation_fails() {
        let d = tmpdir("chain");
        let dir = d.join("backups");
        let fresh = touch(&dir, "P-bad.json", 0, "{partial");
        touch(&dir, "P-old.json", 86_400 * 90, "{}");
        std::env::set_var("LEANTIME_MCP_BACKUP_RETENTION_DAYS", "30");
        let msg = retention_after_backup(&dir, &fresh, "P").expect("active");
        std::env::remove_var("LEANTIME_MCP_BACKUP_RETENTION_DAYS");
        assert!(msg.contains("validation"), "{}", msg);
        assert!(
            dir.join("P-old.json").exists(),
            "old generation kept when fresh is invalid"
        );
        let _ = std::fs::remove_dir_all(&d);
    }
}
