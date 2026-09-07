//! Project restore — rebuilds a backup `leantime-mcp-backup/1` into a NEW
//! project on a Leantime instance. Never merges with existing data.
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

/// Result of an executed restore.
pub struct RestoreResult {
    /// ID of the newly created project.
    pub new_project_id: String,
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
pub fn topological_sort(tickets: &[Value]) -> (Vec<Value>, Vec<String>) {
    let mut warnings = Vec::new();
    let id_str = |v: &Value| -> String {
        match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        }
    };

    let all_ids: Vec<String> = tickets.iter().map(|t| id_str(&t["id"])).collect();

    // Phase 1: parents (no dependingTicketId)
    let mut sorted: Vec<Value> = Vec::new();
    let mut remaining: Vec<Value> = Vec::new();

    for t in tickets {
        let dep = t
            .get("dependingTicketId")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let dep_clean = dep.trim();
        if dep_clean.is_empty() || dep_clean == "0" {
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
// Dry-run (plan without executing)
// ---------------------------------------------------------------------------

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

    let subtask_count = tickets
        .iter()
        .filter(|t| {
            let dep = t
                .get("dependingTicketId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            !dep.trim().is_empty() && dep.trim() != "0"
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

        // Restore status if it's a valid number
        if let Some(status) = t.get("status").filter(|s| s.is_number()) {
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

        // Remap milestone reference
        if let Some(old_ms) = t.get("milestoneid").and_then(|v| v.as_str()) {
            if let Some(new_ms) = ms_map.get(old_ms) {
                values["milestoneid"] = json!(new_ms);
            }
        }
        // Also check "milestoneId" (camelCase from some API versions)
        if let Some(old_ms) = t.get("milestoneId").and_then(|v| v.as_str()) {
            if let Some(new_ms) = ms_map.get(old_ms) {
                values["milestoneid"] = json!(new_ms);
            }
        }

        // Remap sprint reference
        if let Some(old_sp) = t.get("sprint").and_then(|v| v.as_str()) {
            if let Some(new_sp) = sprint_map.get(old_sp) {
                values["sprint"] = json!(new_sp);
            }
        }

        // Remap parent ticket (dependingTicketId)
        let old_dep = t
            .get("dependingTicketId")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !old_dep.is_empty() && old_dep != "0" {
            if let Some(new_dep) = ticket_map.get(old_dep) {
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

        // Restore type
        if let Some(ttype) = t.get("type").and_then(|v| v.as_str()) {
            if ttype != "milestone" {
                // Don't duplicate milestones (they're in the milestone section)
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

        match client
            .call(
                "comments.addComment",
                json!({
                    "values": {"text": text, "father": 0},
                    "module": "ticket",
                    "entityId": new_ticket_id,
                    "entity": {"id": new_ticket_id, "type": "task", "headline": ""}
                }),
            )
            .await
        {
            Ok(_) => comments_created += 1,
            Err(e) => {
                failures.push(format!("comment on ticket {}: {}", old_ticket_id, e));
            }
        }
    }

    // 6. Verify
    let verify = client
        .call(
            "tickets.getAll",
            json!({"searchCriteria": {"currentProject": &new_project_id}, "limit": 500}),
        )
        .await;
    if let Ok(verify_result) = verify {
        let actual = verify_result.as_array().map(|a| a.len()).unwrap_or(0);
        let expected = tickets_raw.len();
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
        milestones_created: ms_created,
        sprints_created: sprint_created,
        tickets_created,
        comments_created,
        failures,
        warnings,
    })
}
