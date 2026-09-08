//! Project restore — rebuilds a backup `leantime-mcp-backup/1` into a NEW
//! project on a Leantime instance — writes into a new project, does not
//! merge into existing data.
//!
//! Pipeline: validate → preflight → dry-run (default) → confirm →
//! sequential restore (topological order) → journal → verify.

use serde_json::{json, Value};

use crate::client::LeantimeClient;

/// Result of a dry-run (plan without executing).
pub struct RestorePlan {
    /// Name of the project in the backup.
    pub project_name: String,
    /// Number of milestones to restore.
    pub milestone_count: usize,
    /// Number of sprints to restore.
    pub sprint_count: usize,
    /// Total tickets to restore.
    pub ticket_count: usize,
    /// Tickets that are subtasks (have a parent).
    pub subtask_count: usize,
    /// Comments to restore (0 in fast backups).
    pub comment_count: usize,
    /// Estimated total API calls needed.
    pub estimated_api_calls: usize,
    /// Non-blocking warnings discovered during planning.
    pub warnings: Vec<String>,
}

impl RestorePlan {
    /// Human-readable summary of the restore plan (for dry-run display).
    pub fn summary(&self) -> String {
        let mut s = format!(
            "Would create:\n  1 project  → \"{} (restored)\"\n  {} milestones\n  {} sprints\n  {} tickets (incl. {} subtasks)\n  {} comments",
            self.project_name,
            self.milestone_count,
            self.sprint_count,
            self.ticket_count,
            self.subtask_count,
            self.comment_count,
        );
        s.push_str(&format!(
            "\n\nEstimated API calls: ~{}\nWarnings: {}",
            self.estimated_api_calls,
            if self.warnings.is_empty() {
                "none".into()
            } else {
                self.warnings.join("; ")
            }
        ));
        s
    }
}

/// Old→new id mappings accumulated during a restore (for the manifest).
#[derive(Default)]
pub struct IdMaps {
    /// Milestone ids: old → new.
    pub milestones: std::collections::HashMap<String, String>,
    /// Sprint ids: old → new.
    pub sprints: std::collections::HashMap<String, String>,
    /// Ticket ids: old → new.
    pub tickets: std::collections::HashMap<String, String>,
}

/// Write the restore manifest next to the backup file: source, target and
/// the full old→new id mapping — restores become auditable after the fact.
pub fn write_restore_manifest(
    backup_path: &std::path::Path,
    result: &RestoreResult,
) -> Result<std::path::PathBuf, String> {
    let manifest_path = backup_path.with_extension("restore-manifest.json");
    let to_map = |m: &std::collections::HashMap<String, String>| -> Value {
        let mut out = serde_json::Map::new();
        let mut keys: Vec<_> = m.keys().collect();
        keys.sort();
        for k in keys {
            out.insert(k.clone(), json!(m[k]));
        }
        Value::Object(out)
    };
    let manifest = json!({
        "source": { "backup": backup_path.display().to_string() },
        "target": {
            "projectId": result.new_project_id,
            "projectName": result.new_project_name,
        },
        "restoredAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        "counts": {
            "milestones": result.milestones_created,
            "sprints": result.sprints_created,
            "tickets": result.tickets_created,
            "comments": result.comments_created,
        },
        "mapping": {
            "milestones": to_map(&result.id_maps.milestones),
            "sprints": to_map(&result.id_maps.sprints),
            "tickets": to_map(&result.id_maps.tickets),
        },
    });
    let content = serde_json::to_string_pretty(&manifest)
        .map_err(|e| format!("manifest serialization: {}", e))?;
    std::fs::write(&manifest_path, content).map_err(|e| format!("manifest write: {}", e))?;
    Ok(manifest_path)
}

/// Result of an executed restore.
pub struct RestoreResult {
    /// ID of the newly created project.
    pub new_project_id: String,
    /// Old→new id mappings (for the restore manifest).
    pub id_maps: IdMaps,
    /// Path of the source backup file.
    pub source_path: String,
    /// Name of the newly created project.
    pub new_project_name: String,
    /// Milestones successfully created.
    pub milestones_created: usize,
    /// Sprints successfully created.
    pub sprints_created: usize,
    /// Tickets successfully created.
    pub tickets_created: usize,
    /// Comments successfully created.
    pub comments_created: usize,
    /// Errors encountered during restore (per-object).
    pub failures: Vec<String>,
    /// Non-blocking warnings.
    pub warnings: Vec<String>,
}

impl RestoreResult {
    /// Human-readable summary of the executed restore.
    pub fn summary(&self) -> String {
        format!(
            "Restore complete: project id={} name=\"{}\"\n  {} milestones, {} sprints, {} tickets, {} comments\n  {} failures{}",
            self.new_project_id,
            self.new_project_name,
            self.milestones_created,
            self.sprints_created,
            self.tickets_created,
            self.comments_created,
            self.failures.len(),
            if self.failures.is_empty() {
                String::new()
            } else {
                format!(":\n    {}", self.failures.join("\n    "))
            },
        )
    }
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate the backup file structure. Returns the parsed JSON or an error.
pub fn validate_backup(path: &std::path::Path) -> Result<Value, String> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("Could not read {}: {}", path.display(), e))?;
    let backup: Value = serde_json::from_str(&content)
        .map_err(|e| format!("Invalid JSON in {}: {}", path.display(), e))?;

    let format = backup
        .pointer("/_meta/format")
        .and_then(|v| v.as_str())
        .unwrap_or("");
    if format != "leantime-mcp-backup/1" {
        return Err(format!(
            "Unsupported backup format: {:?} (expected \"leantime-mcp-backup/1\")",
            format
        ));
    }

    for section in ["milestones", "tickets", "sprints"] {
        if backup.get(section).is_none() {
            return Err(format!("Missing required section: {}", section));
        }
    }

    Ok(backup)
}

// ---------------------------------------------------------------------------
// Topological sort of tickets (parents before children)
// ---------------------------------------------------------------------------

/// Sort tickets so that parents (no dependingTicketId) come first,
/// then children ordered by their parent's creation order.
/// Orphan subtasks (parent not in backup) become top-level with a warning.
/// Handles both string and integer dependingTicketId values.
pub fn topological_sort(tickets: &[Value]) -> (Vec<Value>, Vec<String>) {
    let mut warnings = Vec::new();
    let id_str = |v: &Value| -> String {
        match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        }
    };

    let all_ids: Vec<String> = tickets.iter().map(|t| id_str(&t["id"])).collect();

    // Phase 1: parents (no dependingTicketId or dependingTicketId == 0)
    let mut sorted: Vec<Value> = Vec::new();
    let mut remaining: Vec<Value> = Vec::new();

    for t in tickets {
        let dep = t.get("dependingTicketId").map(&id_str).unwrap_or_default();
        let dep_clean = dep.trim().to_string();
        if dep_clean.is_empty() || dep_clean == "0" || dep_clean == "null" {
            sorted.push(t.clone());
        } else {
            remaining.push(t.clone());
        }
    }

    // Track which IDs are now created (in sorted)
    let mut created: std::collections::HashSet<String> =
        sorted.iter().map(|t| id_str(&t["id"])).collect();

    // Phase 2: children — iterate until stable
    let mut progressed = true;
    while !remaining.is_empty() && progressed {
        progressed = false;
        let mut still_remaining: Vec<Value> = Vec::new();
        for t in remaining.drain(..) {
            let dep = id_str(&t["dependingTicketId"]);
            if created.contains(&dep) {
                created.insert(id_str(&t["id"]));
                sorted.push(t);
                progressed = true;
            } else {
                still_remaining.push(t);
            }
        }
        remaining = still_remaining;
    }

    // Phase 3: orphans (parent not in backup)
    for t in remaining.drain(..) {
        let dep = id_str(&t["dependingTicketId"]);
        let tid = id_str(&t["id"]);
        if !all_ids.contains(&dep) {
            warnings.push(format!(
                "orphan subtask: ticket {} references parent {} not in backup — creating as top-level",
                tid, dep
            ));
        } else {
            warnings.push(format!(
                "circular dependency detected: ticket {} → parent {} — creating as top-level",
                tid, dep
            ));
        }
        sorted.push(t);
    }

    (sorted, warnings)
}

// ---------------------------------------------------------------------------
// Status management (interactive resolution)
// ---------------------------------------------------------------------------

/// A unique status from the backup: (status_id, label, status_type, ticket_count)
pub type BackupStatus = (i64, String, String, usize);

/// Extract unique statuses from backup tickets (scans the enriched data).
/// Handles missing/empty/placeholder labels ("?", "") by falling back to
/// statusType for matching, then to the status ID as last resort.
pub fn extract_backup_statuses(backup: &Value) -> Vec<BackupStatus> {
    let tickets = backup["tickets"].as_array().cloned().unwrap_or_default();
    let mut map: std::collections::HashMap<i64, BackupStatus> = std::collections::HashMap::new();

    for t in &tickets {
        let status_id = t.get("status").and_then(|v| v.as_i64()).unwrap_or(0);
        let stype = t
            .get("statusType")
            .and_then(|v| v.as_str())
            .unwrap_or("NEW")
            .to_string();

        // Normalize the label: "?", "", null, or missing → use statusType
        // as a semantic label for matching (better than "Unknown")
        let raw_label = t
            .get("statusLabel")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let label = if raw_label.is_empty() || raw_label == "?" || raw_label == "null" {
            // No meaningful label — use statusType as the matching key
            stype.clone()
        } else {
            raw_label
        };

        let entry = map.entry(status_id).or_insert((status_id, label, stype, 0));
        entry.3 += 1;
    }

    let mut statuses: Vec<BackupStatus> = map.into_values().collect();
    statuses.sort_by_key(|s| std::cmp::Reverse(s.3)); // Most used first
    statuses
}

/// Detect which backup statuses have no match in the target project.
/// Matching cascade: label (case-insensitive) → statusType → gap.
/// Only true gaps (neither label nor type matches) trigger interactive resolution.
pub fn detect_status_gaps(
    backup_statuses: &[BackupStatus],
    project_statuses: &Value,
) -> Vec<BackupStatus> {
    let normalize = |s: &str| s.trim().to_lowercase();

    backup_statuses
        .iter()
        .filter(|(_, label, stype, _)| {
            let label_match = project_statuses
                .as_object()
                .map(|obj| {
                    obj.values().any(|v| {
                        v.get("name")
                            .and_then(|n| n.as_str())
                            .map(|n| normalize(n) == normalize(label))
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false);

            let type_match = project_statuses
                .as_object()
                .map(|obj| {
                    obj.values().any(|v| {
                        v.get("statusType")
                            .and_then(|t| t.as_str())
                            .map(|t| t.eq_ignore_ascii_case(stype))
                            .unwrap_or(false)
                    })
                })
                .unwrap_or(false);

            // It's a gap only if NEITHER label NOR statusType matches
            !label_match && !type_match
        })
        .cloned()
        .collect()
}

/// Build the status mapping: backup label → new project status ID.
/// Matching cascade:
/// 1. Exact label match (case-insensitive)
/// 2. statusType match (e.g., backup DONE → any project status with type DONE)
/// 3. No match → gap (needs interactive resolution)
pub fn build_status_mapping(
    backup_statuses: &[BackupStatus],
    project_statuses: &Value,
) -> std::collections::HashMap<String, i64> {
    let normalize = |s: &str| s.trim().to_lowercase();
    let mut mapping = std::collections::HashMap::new();

    for (_, label, stype, _) in backup_statuses {
        // Pass 1: exact label match
        if let Some(obj) = project_statuses.as_object() {
            for (id_str, info) in obj {
                if let (Ok(id), Some(name)) = (
                    id_str.parse::<i64>(),
                    info.get("name").and_then(|n| n.as_str()),
                ) {
                    if normalize(name) == normalize(label) {
                        mapping.insert(label.clone(), id);
                        break;
                    }
                }
            }
        }

        // Pass 2: if no label match, try statusType
        if !mapping.contains_key(label.as_str()) {
            if let Some(obj) = project_statuses.as_object() {
                for (id_str, info) in obj {
                    if let (Ok(id), Some(ptype)) = (
                        id_str.parse::<i64>(),
                        info.get("statusType").and_then(|t| t.as_str()),
                    ) {
                        if ptype.eq_ignore_ascii_case(stype) {
                            mapping.insert(label.clone(), id);
                            break;
                        }
                    }
                }
            }
        }
    }

    mapping
}

/// Interactive CLI prompt to resolve missing statuses.
/// Asks the user to create the statuses in Leantime UI with the same names,
/// then re-fetches and matches by label.
pub async fn resolve_status_gaps_interactive(
    client: &mut LeantimeClient,
    project_id: &str,
    gaps: &[BackupStatus],
    backup_statuses: &[BackupStatus],
) -> Result<std::collections::HashMap<String, i64>, String> {
    use std::io::Write as _;

    if gaps.is_empty() {
        return Ok(build_status_mapping(backup_statuses, &json!({})));
    }

    // Fetch the project name so the user knows exactly where to go
    let project_name = client
        .call("projects.getProject", json!({"id": project_id}))
        .await
        .ok()
        .and_then(|p| {
            p.get("name")
                .and_then(|n| n.as_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| format!("id={}", project_id));

    println!("\n⚠ Status mapping needed:");
    for (_, label, stype, count) in gaps {
        println!(
            "  \"{}\" (type: {}, {} tickets) — not in this project",
            label, stype, count
        );
    }
    println!("\nThe Leantime API cannot create custom statuses.");
    println!(
        "Open Leantime → project \"{}\" (id={}) → Settings → Statuses",
        project_name, project_id
    );
    println!("and create these with the same names:\n");
    for (_, label, _, _) in gaps {
        println!("  • {}", label);
    }

    let mut skipped: Vec<String> = Vec::new();

    loop {
        print!("\nPress Enter when done (or 's' to skip all, 'c' to cancel): ");
        let _ = std::io::stdout().flush();
        let mut input = String::new();
        std::io::stdin()
            .read_line(&mut input)
            .map_err(|e| format!("stdin error: {}", e))?;
        let input = input.trim();

        if input.eq_ignore_ascii_case("c") {
            return Err("Cancelled by user.".into());
        }
        if input.eq_ignore_ascii_case("s") {
            for (_, label, _, _) in gaps {
                skipped.push(label.clone());
            }
            break;
        }

        // Re-fetch project statuses
        let project_statuses = client
            .call("tickets.getStatusLabels", json!({"projectId": project_id}))
            .await
            .map_err(|e| format!("Could not fetch statuses: {}", e))?;

        // Check which gaps are now resolved
        let remaining: Vec<&BackupStatus> = gaps
            .iter()
            .filter(|(_, label, _, _)| {
                !project_statuses
                    .as_object()
                    .map(|obj| {
                        obj.values().any(|v| {
                            v.get("name")
                                .and_then(|n| n.as_str())
                                .map(|n| n.trim().eq_ignore_ascii_case(label.trim()))
                                .unwrap_or(false)
                        })
                    })
                    .unwrap_or(false)
            })
            .collect();

        if remaining.is_empty() {
            println!("  ✓ All statuses found!");
            break;
        }

        println!("\nStill missing:");
        for (_, label, _, count) in remaining {
            println!("  ⚠ \"{}\" ({} tickets)", label, count);
        }
        println!("\nCreate them and press Enter again, or 's' to skip remaining:");
    }

    // Build final mapping
    let project_statuses = client
        .call("tickets.getStatusLabels", json!({"projectId": project_id}))
        .await
        .map_err(|e| format!("Could not fetch statuses: {}", e))?;

    let mut mapping = build_status_mapping(backup_statuses, &project_statuses);

    // For skipped statuses, map to the project's NEW-type status (not hardcoded 0)
    if !skipped.is_empty() {
        // Find the first status with statusType "NEW" (or fallback to 0)
        let new_status_id = project_statuses
            .as_object()
            .and_then(|obj| {
                obj.iter()
                    .filter(|(_, v)| {
                        v.get("statusType")
                            .and_then(|t| t.as_str())
                            .is_some_and(|t| t.eq_ignore_ascii_case("NEW"))
                    })
                    .filter_map(|(k, _)| k.parse::<i64>().ok())
                    .min()
            })
            .unwrap_or(0);

        for label in &skipped {
            mapping.insert(label.clone(), new_status_id);
            println!(
                "  ⚠ \"{}\" will be restored as status {} (user skipped)",
                label, new_status_id
            );
        }
    }

    Ok(mapping)
}

/// Analyze the backup and produce a restore plan. Read-only, no mutations.
pub fn plan_restore(backup: &Value) -> RestorePlan {
    let project_name = backup
        .pointer("/_meta/project/name")
        .and_then(|v| v.as_str())
        .unwrap_or("project")
        .to_string();

    let milestones = backup["milestones"].as_array().cloned().unwrap_or_default();
    let sprints = backup["sprints"].as_array().cloned().unwrap_or_default();
    let tickets = backup["tickets"].as_array().cloned().unwrap_or_default();
    let comments = backup["comments"].as_array().cloned().unwrap_or_default();

    let id_of = |v: &Value| -> String {
        match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        }
    };

    let subtask_count = tickets
        .iter()
        .filter(|t| {
            let dep = t.get("dependingTicketId").map(&id_of).unwrap_or_default();
            !dep.trim().is_empty() && dep.trim() != "0" && dep.trim() != "null"
        })
        .count();

    let mut warnings = Vec::new();
    if comments.is_empty() {
        warnings.push("fast backup — comments not restorable".into());
    }

    let estimated = 1 + milestones.len() + sprints.len() + tickets.len() + comments.len();

    RestorePlan {
        project_name,
        milestone_count: milestones.len(),
        sprint_count: sprints.len(),
        ticket_count: tickets.len(),
        subtask_count,
        comment_count: comments.len(),
        estimated_api_calls: estimated,
        warnings,
    }
}

// ---------------------------------------------------------------------------
// Restore (sequential, topological)
// ---------------------------------------------------------------------------

/// Execute the restore: create all objects in a NEW project.
/// Uses the rate-limited client; on failure, logs and continues.
pub async fn execute_restore(
    client: &mut LeantimeClient,
    backup: &Value,
    source_display: &str,
) -> Result<RestoreResult, String> {
    let original_name = backup
        .pointer("/_meta/project/name")
        .and_then(|v| v.as_str())
        .unwrap_or("project");

    let now = chrono::Local::now().format("%Y-%m-%d %H:%M");
    let new_name = format!("{} (restored {})", original_name, now);

    let milestones = backup["milestones"].as_array().cloned().unwrap_or_default();
    let sprints = backup["sprints"].as_array().cloned().unwrap_or_default();
    let tickets_raw = backup["tickets"].as_array().cloned().unwrap_or_default();
    let comments = backup["comments"].as_array().cloned().unwrap_or_default();

    let mut failures: Vec<String> = Vec::new();
    let mut warnings: Vec<String> = Vec::new();
    let mut status_mapping: std::collections::HashMap<String, i64> =
        std::collections::HashMap::new();

    // Maps: old_id → new_id
    let mut ms_map: std::collections::HashMap<String, String> = std::collections::HashMap::new();
    let mut sprint_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();
    let mut ticket_map: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    let id_str = |v: &Value| -> String {
        match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        }
    };

    // 1. Create the project
    let project_result = client
        .call(
            "projects.addProject",
            json!({"values": {"name": new_name, "clientId": "1"}}),
        )
        .await
        .map_err(|e| format!("Project creation failed: {}", e))?;

    let new_project_id = project_result
        .as_array()
        .and_then(|a| a.first())
        .map(id_str)
        .unwrap_or_else(|| project_result.to_string());

    // 1b. Status analysis and interactive resolution
    let backup_statuses = extract_backup_statuses(backup);
    let project_statuses = client
        .call(
            "tickets.getStatusLabels",
            json!({"projectId": &new_project_id}),
        )
        .await
        .map_err(|e| format!("Could not fetch new project statuses: {}", e))?;

    let gaps = detect_status_gaps(&backup_statuses, &project_statuses);
    if !gaps.is_empty() {
        let mapping =
            resolve_status_gaps_interactive(client, &new_project_id, &gaps, &backup_statuses)
                .await?;

        // Display the final mapping
        println!("\nFinal status mapping:");
        for (old_id, label, _, count) in &backup_statuses {
            if let Some(new_id) = mapping.get(label) {
                // Look up the new label for display
                let new_label = project_statuses
                    .as_object()
                    .and_then(|obj| {
                        obj.get(&new_id.to_string())
                            .and_then(|v| v.get("name"))
                            .and_then(|n| n.as_str())
                            .map(|s| s.to_string())
                    })
                    .unwrap_or_else(|| format!("status {}", new_id));

                if *new_id != *old_id {
                    println!(
                        "  ✓ {} → {} = {} ({} tickets)",
                        label, new_id, new_label, count
                    );
                } else {
                    println!("  ✓ {} ({} tickets)", label, count);
                }
            }
        }
        // Store the mapping for use during ticket creation
        status_mapping = mapping;
    } else {
        println!("  ✓ All statuses map cleanly");
    }

    // 2. Create milestones
    let mut ms_created = 0;
    for ms in &milestones {
        let old_id = id_str(&ms["id"]);
        let mut values = json!({
            "headline": ms.get("headline").cloned().unwrap_or(json!("")),
            "type": "milestone",
            "projectId": json!(&new_project_id),
        });
        // Restore description if present
        if let Some(desc) = ms.get("description").and_then(|d| d.as_str()) {
            if !desc.is_empty() {
                values["description"] = json!(desc);
            }
        }
        match client
            .call("tickets.addTicket", json!({"values": values}))
            .await
        {
            Ok(r) => {
                let new_id = r
                    .as_array()
                    .and_then(|a| a.first())
                    .map(&id_str)
                    .unwrap_or_default();
                ms_map.insert(old_id, new_id);
                ms_created += 1;
            }
            Err(e) => {
                failures.push(format!("milestone {}: {}", old_id, e));
            }
        }
    }

    // 3. Create sprints
    let mut sprint_created = 0;
    for sp in &sprints {
        let old_id = id_str(&sp["id"]);
        let values = json!({
            "name": sp.get("name").cloned().unwrap_or(json!("Sprint")),
            "startDate": sp.get("startDate").cloned().unwrap_or(json!("")),
            "endDate": sp.get("endDate").cloned().unwrap_or(json!("")),
            "projectId": json!(&new_project_id),
        });
        match client
            .call("sprints.addSprint", json!({"params": values}))
            .await
        {
            Ok(r) => {
                let new_id = r
                    .as_array()
                    .and_then(|a| a.first())
                    .map(&id_str)
                    .unwrap_or_default();
                sprint_map.insert(old_id, new_id);
                sprint_created += 1;
            }
            Err(e) => {
                failures.push(format!("sprint {}: {}", old_id, e));
            }
        }
    }

    // 4. Create tickets (topological order)
    let (sorted_tickets, sort_warnings) = topological_sort(&tickets_raw);
    warnings.extend(sort_warnings);

    let mut tickets_created = 0;
    for t in &sorted_tickets {
        let old_id = id_str(&t["id"]);
        let mut values = json!({
            "headline": t.get("headline").cloned().unwrap_or(json!("")),
            "projectId": json!(&new_project_id),
        });

        // Apply status mapping (label-based, resolved interactively if needed).
        // Normalize the backup label the SAME way as extract_backup_statuses
        // so the mapping keys match.
        let backup_stype = t
            .get("statusType")
            .and_then(|v| v.as_str())
            .unwrap_or("NEW")
            .to_string();
        let raw_status_label = t
            .get("statusLabel")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();
        let backup_status_label =
            if raw_status_label.is_empty() || raw_status_label == "?" || raw_status_label == "null"
            {
                backup_stype.clone()
            } else {
                raw_status_label
            };

        if let Some(new_status) = status_mapping.get(&backup_status_label) {
            values["status"] = json!(new_status);
        } else if let Some(status) = t.get("status").filter(|s| s.is_number()) {
            // No mapping found — use original ID (should be rare after
            // the statusType fallback in build_status_mapping)
            values["status"] = status.clone();
        }

        // Restore editorId (string, may reference a user not on this instance)
        if let Some(editor) = t.get("editorId") {
            if let Some(e) = editor.as_str() {
                if !e.is_empty() {
                    values["editorId"] = json!(e);
                }
            } else if let Some(e) = editor.as_i64() {
                if e > 0 {
                    values["editorId"] = json!(e.to_string());
                }
            }
        }

        // Remap milestone reference (handles both string and integer IDs)
        let old_ms = t.get("milestoneid").map(&id_str).unwrap_or_default();
        if !old_ms.is_empty() && old_ms != "0" {
            if let Some(new_ms) = ms_map.get(&old_ms) {
                values["milestoneid"] = json!(new_ms);
            }
        }
        // Also check "milestoneId" (camelCase from some API versions)
        let old_ms2 = t.get("milestoneId").map(&id_str).unwrap_or_default();
        if !old_ms2.is_empty() && old_ms2 != "0" {
            if let Some(new_ms) = ms_map.get(&old_ms2) {
                values["milestoneid"] = json!(new_ms);
            }
        }

        // Remap sprint reference (handles both string and integer IDs)
        let old_sp = t.get("sprint").map(&id_str).unwrap_or_default();
        if !old_sp.is_empty() && old_sp != "0" {
            if let Some(new_sp) = sprint_map.get(&old_sp) {
                values["sprint"] = json!(new_sp);
            }
        }

        // Remap parent ticket (dependingTicketId, handles both types)
        let old_dep = t.get("dependingTicketId").map(&id_str).unwrap_or_default();
        if !old_dep.is_empty() && old_dep != "0" {
            if let Some(new_dep) = ticket_map.get(&old_dep) {
                values["dependingTicketId"] = json!(new_dep);
            }
            // If parent not in map (orphan), we already warned — create top-level
        }

        // Restore description (already HTML in backup — pass through)
        if let Some(desc) = t.get("description").and_then(|d| d.as_str()) {
            if !desc.is_empty() {
                values["description"] = json!(desc);
            }
        }

        // SKIP milestones in the tickets section — already created from the
        // milestones section. Map their old ticket-id to the new milestone-id
        // so subtask references resolve correctly.
        if t.get("type").and_then(|v| v.as_str()) == Some("milestone") {
            let old_tid = id_str(&t["id"]);
            if let Some(new_ms_id) = ms_map.get(&old_tid) {
                ticket_map.insert(old_tid, new_ms_id.clone());
                continue; // NE PAS créer — déjà fait depuis la section milestones
            }
            // Milestone not in ms_map (inconsistent backup) — create as ticket + warning
            warnings.push(format!(
                "milestone ticket {} not found in milestones section — creating as regular ticket",
                old_tid
            ));
        }

        // Restore type for non-milestone tickets
        if let Some(ttype) = t.get("type").and_then(|v| v.as_str()) {
            if ttype != "milestone" {
                values["type"] = json!(ttype);
            }
        }

        match client
            .call("tickets.addTicket", json!({"values": values}))
            .await
        {
            Ok(r) => {
                let new_id = r
                    .as_array()
                    .and_then(|a| a.first())
                    .map(&id_str)
                    .unwrap_or_default();
                ticket_map.insert(old_id, new_id);
                tickets_created += 1;
            }
            Err(e) => {
                failures.push(format!("ticket {}: {}", old_id, e));
            }
        }
    }

    // 5. Create comments (if present)
    let mut comments_created = 0;
    for c in &comments {
        let old_ticket_id = c.get("ticketId").and_then(|v| v.as_str()).unwrap_or("");
        let new_ticket_id = match ticket_map.get(old_ticket_id) {
            Some(id) => id.clone(),
            None => continue, // Skip comments for tickets that failed to restore
        };

        let text = c
            .pointer("/comment/text")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if text.is_empty() {
            continue;
        }

        // The entity object needs the ticket's actual type and headline —
        // fetch the restored ticket (the backup's hardcodes won't work).
        let ticket_info = match client
            .call("tickets.getTicket", json!({"id": &new_ticket_id}))
            .await
        {
            Ok(t) => json!({
                "id": &new_ticket_id,
                "type": t.get("type").cloned().unwrap_or(json!("task")),
                "headline": t.get("headline").cloned().unwrap_or(json!("")),
            }),
            Err(_) => json!({
                "id": &new_ticket_id,
                "type": "task",
                "headline": ""
            }),
        };

        let add_result = client
            .call(
                "comments.addComment",
                json!({
                    "values": {"text": text, "father": 0},
                    "module": "ticket",
                    "entityId": &new_ticket_id,
                    "entity": ticket_info
                }),
            )
            .await;

        // v3.7.3 bug: the comment row is inserted, then the notification build
        // crashes. Verify the comment actually landed before surfacing an error.
        if add_result.is_err() {
            let landed = client
                .call(
                    "comments.getComments",
                    json!({"module": "ticket", "entityId": &new_ticket_id}),
                )
                .await
                .map(|r| {
                    r.as_array()
                        .map(|arr| {
                            arr.iter()
                                .any(|cm| cm.get("text").and_then(|t| t.as_str()) == Some(text))
                        })
                        .unwrap_or(false)
                })
                .unwrap_or(false);
            if landed {
                comments_created += 1;
                continue;
            }
            failures.push(format!(
                "comment on ticket {}: {}",
                old_ticket_id,
                add_result.err().unwrap()
            ));
        } else {
            comments_created += 1;
        }
    }

    // 6. Verify — chunked completeness fetch (immune to the API limit).
    // Verification stays best-effort: a fetch failure was skipped before
    // chunking and still is.
    if let Ok((verify_items, verify_warnings)) = client
        .get_all_tickets_chunked(&new_project_id, json!({}))
        .await
    {
        let actual = verify_items.len();
        let expected = tickets_raw.len();
        warnings.extend(verify_warnings);
        if actual < expected {
            warnings.push(format!(
                "verification: expected {} tickets, found {} — some creations may have failed silently",
                expected, actual
            ));
        }
    }

    Ok(RestoreResult {
        new_project_id,
        new_project_name: new_name,
        id_maps: IdMaps {
            milestones: ms_map,
            sprints: sprint_map,
            tickets: ticket_map,
        },
        source_path: source_display.to_string(),
        milestones_created: ms_created,
        sprints_created: sprint_created,
        tickets_created,
        comments_created,
        failures,
        warnings,
    })
}
