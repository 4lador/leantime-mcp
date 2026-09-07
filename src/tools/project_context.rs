//! `leantime_project_context` — the full picture of a project in one call:
//! project info and progress, health counters (blocked / overdue / unassigned
//! / open), current or upcoming sprint, milestones with their progress,
//! ticket summary by status and type, and recently modified items.
//!
//! Replaces 5-6 agent round-trips with one; the response is capped well
//! below 4 KB so it can be injected into an LLM context wholesale. Drill-down
//! stays available through `leantime_list_tickets` and friends.

use std::future::Future;
use std::pin::Pin;

use serde_json::{json, Map, Value};

use super::shared::{num_coerce, ob, parse_leantime_ts, rs, tool};
use super::{error_result, ok_result, ClientRef, Tool};

/// Milestones are capped at 15 in the output — with 40-char names this
/// guarantees the whole response stays under the 4 KB context budget.
const MAX_MILESTONES: usize = 15;
/// Recently modified items are capped at 5.
const MAX_ACTIVITY: usize = 5;

/// Value → plain id string ("260" whether it arrives as string or number).
fn id_str(v: Option<&Value>) -> String {
    match v {
        Some(Value::String(s)) => s.clone(),
        Some(other) => other.to_string(),
        None => String::new(),
    }
}

/// Parse a Leantime date/datetime field to epoch millis. Handles both wire
/// formats: epoch-millis digit strings and "YYYY-MM-DD[ HH:MM:SS]".
fn ts_of(v: Option<&Value>) -> Option<i64> {
    let s = v?.as_str()?;
    if s.len() >= 10 && s.bytes().all(|b| b.is_ascii_digit()) {
        return s.parse::<i64>().ok();
    }
    parse_leantime_ts(s)
}

/// Epoch millis → "YYYY-MM-DD" (local time, matching the TS semantics).
fn ms_to_date(ms: i64) -> String {
    use chrono::TimeZone;
    chrono::Local
        .timestamp_millis_opt(ms)
        .single()
        .map(|dt| dt.format("%Y-%m-%d").to_string())
        .unwrap_or_default()
}

/// True when the milestone reference field points at an actual milestone
/// (Leantime uses "", "0" and null for "none").
fn has_milestone(raw: &str) -> bool {
    !raw.is_empty() && raw != "0" && raw != "null"
}

fn h_project_context(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let pid = a.get("projectId").and_then(|v| v.as_str()).unwrap_or("");
        let include_milestones = a
            .get("includeMilestones")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);
        let include_activity = a
            .get("includeRecentActivity")
            .and_then(|v| v.as_bool())
            .unwrap_or(true);

        // 1. Project info — the only call that can hard-fail the tool.
        let project = match c.call("projects.getProject", json!({"id": pid})).await {
            Ok(p) => p,
            Err(e) => return error_result(&e.to_string()),
        };
        if project.is_boolean() || super::shared::is_leantime_error(&project) {
            return error_result(&format!("Project {} not found.", pid));
        }

        // 2. Official progress (best effort — computed locally when absent).
        // Leantime stores percentdone as a numeric string, coerce both forms.
        let pct_of = |v: Option<&Value>| -> Option<f64> {
            match v {
                Some(Value::Number(n)) => n.as_f64(),
                Some(Value::String(s)) => s.trim().parse::<f64>().ok(),
                _ => None,
            }
        };
        let official_pct = c
            .call("projects.getProjectProgress", json!({"id": pid}))
            .await
            .ok()
            .and_then(|p| {
                let obj = if p.is_array() {
                    p.get(0).cloned().unwrap_or(json!({}))
                } else {
                    p
                };
                pct_of(obj.get("percentdone")).or_else(|| pct_of(obj.get("percentDone")))
            });

        // 3. All tickets (Leantime stores milestones in the same table —
        //    they arrive here with type "milestone").
        let mut tickets = match c
            .call(
                "tickets.getAll",
                json!({"searchCriteria": {"currentProject": pid}, "limit": 500}),
            )
            .await
        {
            Ok(t) => t.as_array().cloned().unwrap_or_default(),
            Err(e) => return error_result(&e.to_string()),
        };

        // 6. Status labels (cached permanently per project after first call).
        let sm = c.get_status_map(pid).await.unwrap_or(json!({}));
        c.enrich_with_statuses(&mut tickets, &sm);

        let is_done = |t: &Value| t.get("statusType").and_then(|v| v.as_str()) == Some("DONE");
        let is_milestone = |t: &Value| t.get("type").and_then(|v| v.as_str()) == Some("milestone");
        let now_ms = chrono::Local::now().timestamp_millis();

        // --- progress (fallback: computed from the fetched ticket set) ---
        let tickets_total = tickets.len();
        let tickets_done = tickets.iter().filter(|t| is_done(t)).count();
        let percent_done = official_pct.unwrap_or({
            if tickets_total == 0 {
                0.0
            } else {
                (tickets_done as f64 / tickets_total as f64) * 100.0
            }
        });

        // --- health (actionable tickets only — milestones excluded) ---
        let mut blocked = 0u64;
        let mut overdue = 0u64;
        let mut unassigned = 0u64;
        let mut open_total = 0u64;
        for t in tickets.iter().filter(|t| !is_milestone(t)) {
            let done = is_done(t);
            if !done {
                open_total += 1;
                let label = t.get("statusLabel").and_then(|v| v.as_str()).unwrap_or("");
                if t.get("statusType").and_then(|v| v.as_str()) == Some("BLOCKED")
                    || label.to_lowercase().contains("block")
                {
                    blocked += 1;
                }
                if let Some(due) = ts_of(t.get("dateToFinish")) {
                    if due < now_ms {
                        overdue += 1;
                    }
                }
                let editor = id_str(t.get("editorId"));
                if editor.is_empty() || editor == "0" || editor == "null" {
                    unassigned += 1;
                }
            }
        }

        // --- 5. Current / upcoming sprint (computed from dates) ---
        let sprints = c
            .call("sprints.getAllSprints", json!({"projectId": pid}))
            .await
            .ok()
            .map(|s| s.as_array().cloned().unwrap_or_default())
            .unwrap_or_default();
        let sprint_ticket_count = |sid: &str| -> u64 {
            tickets
                .iter()
                .filter(|t| !is_milestone(t) && !is_done(t) && id_str(t.get("sprint")) == sid)
                .count() as u64
        };
        let mut current_sprint = Value::Null;
        let with_window = sprints
            .iter()
            .filter_map(|s| {
                let start = s
                    .get("startDate")
                    .and_then(|v| v.as_str())
                    .and_then(parse_leantime_ts)?;
                let end = s
                    .get("endDate")
                    .and_then(|v| v.as_str())
                    .and_then(parse_leantime_ts)?;
                Some((s.clone(), start, end))
            })
            .collect::<Vec<_>>();
        let sprint_json = |s: &Value, start: i64, end: i64, status: &str| -> Value {
            let sid = id_str(s.get("id"));
            let mut m = Map::new();
            m.insert(
                "id".into(),
                json!(s.get("id").cloned().unwrap_or(Value::Null)),
            );
            m.insert("name".into(), s.get("name").cloned().unwrap_or(Value::Null));
            m.insert("status".into(), json!(status));
            m.insert(
                "startDate".into(),
                s.get("startDate").cloned().unwrap_or(Value::Null),
            );
            m.insert(
                "endDate".into(),
                s.get("endDate").cloned().unwrap_or(Value::Null),
            );
            if status == "current" {
                let days = ((end - now_ms) / 86_400_000).max(0);
                m.insert("daysRemaining".into(), json!(days));
            } else {
                let days = ((start - now_ms) / 86_400_000).max(0);
                m.insert("daysUntilStart".into(), json!(days));
            }
            m.insert("openTickets".into(), json!(sprint_ticket_count(&sid)));
            Value::Object(m)
        };
        if let Some((s, start, end)) = with_window
            .iter()
            .find(|(_, st, en)| *st <= now_ms && now_ms <= *en)
        {
            current_sprint = sprint_json(s, *start, *end, "current");
        } else if let Some((s, start, end)) = with_window
            .iter()
            .filter(|(_, st, _)| *st > now_ms)
            .min_by_key(|(_, st, _)| *st)
        {
            current_sprint = sprint_json(s, *start, *end, "upcoming");
        }

        // --- milestones with in-memory progress (effort × priority, the
        //     same weighted formula as leantime_get_milestone_progress) ---
        let mut milestones_json: Vec<Value> = Vec::new();
        let mut milestones_note: Option<String> = None;
        if include_milestones {
            let mut milestone_list = c
                .call(
                    "tickets.getAll",
                    json!({"searchCriteria": {"currentProject": pid, "type": "milestone"}, "limit": 500}),
                )
                .await
                .ok()
                .map(|m| m.as_array().cloned().unwrap_or_default())
                .unwrap_or_default();
            c.enrich_with_statuses(&mut milestone_list, &sm);

            let mut by_id: Vec<(i64, Value)> = milestone_list
                .into_iter()
                .map(|m| {
                    let n = id_str(m.get("id")).parse::<i64>().unwrap_or(i64::MAX);
                    (n, m)
                })
                .collect();
            by_id.sort_by_key(|(n, _)| *n);
            let total = by_id.len();
            for (_, ms) in by_id.into_iter().take(MAX_MILESTONES) {
                let mid = id_str(ms.get("id"));
                let (done_w, total_w, count) = {
                    let default_effort = 3.0;
                    // Missing, null, "" — or a literal 0 ("not estimated") —
                    // all fall back to the default effort. Otherwise a whole
                    // milestone of unestimated tickets would weigh 0 and
                    // report 0% forever.
                    let empty_or_missing = |v: Option<&Value>| {
                        v.is_none()
                            || matches!(v, Some(Value::Null))
                            || v.and_then(|x| x.as_str()) == Some("")
                            || num_coerce(v, f64::NAN) == 0.0
                    };
                    let mut dw = 0.0;
                    let mut tw = 0.0;
                    let mut n = 0u64;
                    for t in tickets.iter().filter(|t| !is_milestone(t)) {
                        // Both spellings occur across API versions (see restore).
                        let raw = t
                            .get("milestoneid")
                            .or_else(|| t.get("milestoneId"))
                            .map(|v| id_str(Some(v)))
                            .unwrap_or_default();
                        if !has_milestone(&raw) || raw != mid {
                            continue;
                        }
                        n += 1;
                        let effort = if empty_or_missing(t.get("storypoints")) {
                            default_effort
                        } else {
                            num_coerce(t.get("storypoints"), default_effort)
                        };
                        let priority = if empty_or_missing(t.get("priority")) {
                            3.0
                        } else {
                            num_coerce(t.get("priority"), 3.0)
                        };
                        let factor: f64 = match priority as i64 {
                            1 => 2.0,
                            2 => 1.75,
                            3 => 1.5,
                            4 => 1.25,
                            _ => 1.0,
                        };
                        let score = effort * factor;
                        tw += score;
                        if is_done(t) {
                            dw += score;
                        }
                    }
                    (dw, tw, n)
                };
                let pct = if total_w == 0.0 {
                    0.0
                } else {
                    (done_w / total_w * 100.0 * 10.0).round() / 10.0
                };
                let status = match ms.get("statusType").and_then(|v| v.as_str()) {
                    Some("DONE") => "done",
                    Some("IN_PROGRESS") => "in_progress",
                    _ => "new",
                };
                let name: String = ms
                    .get("headline")
                    .or_else(|| ms.get("name"))
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .chars()
                    .take(40)
                    .collect();
                milestones_json.push(json!({
                    "id": ms.get("id").cloned().unwrap_or(Value::Null),
                    "name": name,
                    "status": status,
                    "percentDone": pct,
                    "tickets": count,
                }));
            }
            if total > MAX_MILESTONES {
                milestones_note =
                    Some(format!("{} total, showing first {}", total, MAX_MILESTONES));
            }
        }

        // --- ticket summary (counts only, never lists) ---
        let mut by_status: std::collections::BTreeMap<String, u64> = Default::default();
        let mut by_type: std::collections::BTreeMap<String, u64> = Default::default();
        for t in &tickets {
            let label = t
                .get("statusLabel")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");
            *by_status.entry(label.to_string()).or_default() += 1;
            let ttype = t.get("type").and_then(|v| v.as_str()).unwrap_or("unknown");
            *by_type.entry(ttype.to_string()).or_default() += 1;
        }

        // --- recently modified items (the API has no activity feed —
        //     zp_tickets.date updates on every modification) ---
        let mut recent_activity: Vec<Value> = Vec::new();
        if include_activity {
            let mut dated: Vec<(i64, &Value)> = tickets
                .iter()
                .filter(|t| !is_milestone(t))
                .filter_map(|t| ts_of(t.get("date")).map(|ms| (ms, t)))
                .collect();
            dated.sort_by_key(|(ms, _)| std::cmp::Reverse(*ms));
            for (ms, t) in dated.into_iter().take(MAX_ACTIVITY) {
                let headline = t
                    .get("headline")
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .chars()
                    .take(40)
                    .collect::<String>();
                let tid = id_str(t.get("id"));
                recent_activity.push(json!({
                    "what": format!("Ticket #{} — {}", tid, headline).trim_end_matches(" —").to_string(),
                    "when": ms_to_date(ms),
                }));
            }
        }

        let mut out = Map::new();
        out.insert(
            "generatedAt".into(),
            json!(chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true)),
        );
        out.insert(
            "project".into(),
            json!({
                "id": project.get("id").cloned().unwrap_or(Value::Null),
                "name": project.get("name").cloned().unwrap_or(Value::Null),
                "state": project.get("state").cloned().unwrap_or(json!("active")),
                "progress": {
                    "percentDone": (percent_done * 10.0).round() / 10.0,
                    "ticketsTotal": tickets_total,
                    "ticketsDone": tickets_done,
                },
            }),
        );
        out.insert(
            "health".into(),
            json!({
                "blocked": blocked,
                "overdue": overdue,
                "unassigned": unassigned,
                "openTotal": open_total,
            }),
        );
        out.insert("currentSprint".into(), current_sprint);
        if include_milestones {
            out.insert("milestones".into(), Value::Array(milestones_json));
        }
        if let Some(note) = milestones_note {
            out.insert("milestonesNote".into(), json!(note));
        }
        out.insert(
            "ticketSummary".into(),
            json!({
                "byStatus": by_status,
                "byType": by_type,
            }),
        );
        if include_activity {
            out.insert("recentActivity".into(), Value::Array(recent_activity));
        }
        ok_result(&Value::Object(out))
    })
}

pub(super) fn tools() -> Vec<Tool> {
    vec![tool(
        "leantime_project_context",
        "Get a complete project overview in a single call: project info and progress, health counters (blocked, overdue, unassigned, open tickets), current or upcoming sprint, milestones with their progress, ticket summary by status and type, and recently modified items. Designed to give an agent full context without additional calls — drill down with leantime_list_tickets only where needed.",
        vec![
            rs("projectId", "The project ID"),
            ob("includeMilestones", "Include the milestone list with progress (default true)"),
            ob("includeRecentActivity", "Include recently modified items (default true)"),
        ],
        vec!["projectId"],
        Box::new(h_project_context),
    )]
}
