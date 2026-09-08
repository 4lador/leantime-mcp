use std::future::Future;
use std::pin::Pin;

use serde_json::{json, Value};

use super::shared::*;
use super::{error_result, ok_result, ClientRef, Tool, ToolAnnotations};

fn h_log_time(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let tid = a.get("ticketId").and_then(|v| v.as_str()).unwrap_or("");
        let hours = a.get("hours").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let kind = a
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("GENERAL_BILLABLE");
        let mode = a.get("mode").and_then(|v| v.as_str()).unwrap_or("add");

        // Dry run: all local checks accumulated (never a single API call).
        if wants_dry_run(&a) {
            let mut errors = Vec::new();
            if hours <= 0.0 {
                errors.push("Hours must be a positive number.".to_string());
            }
            const MAX_HOURS_PER_DAY: f64 = 24.0;
            if hours > MAX_HOURS_PER_DAY {
                errors.push(format!(
                    "Hours must be at most {} for a single day's entry (got {}). Split the entry across days or fix the value.",
                    MAX_HOURS_PER_DAY, hours
                ));
            }
            if !HOUR_KINDS.contains(&kind) {
                errors.push(format!(
                    "Invalid kind \"{}\". Valid kinds: {}",
                    kind,
                    HOUR_KINDS.join(", ")
                ));
            }
            if mode != "add" && mode != "set" {
                errors.push("Invalid mode: use \"add\" or \"set\".".to_string());
            }
            if !errors.is_empty() {
                return dry_run_result(false, errors, vec![], vec![]);
            }
            let date = a
                .get("date")
                .and_then(|v| v.as_str())
                .unwrap_or(&chrono::Utc::now().format("%Y-%m-%d").to_string())
                .to_string();
            let changes = vec![json!({
                "field": "timesheet entry",
                "to": { "ticketId": tid, "hours": hours, "kind": kind, "mode": mode, "date": date }
            })];
            return dry_run_result(true, vec![], changes, vec![]);
        }

        if hours <= 0.0 {
            return error_result("Hours must be a positive number.");
        }
        // One timesheet line targets ONE date (mode "set" sets that day/kind
        // total) — anything over 24h is impossible data, not atypical input.
        // The TS edition has no cap; this is a deliberate divergence.
        const MAX_HOURS_PER_DAY: f64 = 24.0;
        if hours > MAX_HOURS_PER_DAY {
            return error_result(&format!(
                "Hours must be at most {} for a single day's entry (got {}). Split the entry across days or fix the value.",
                MAX_HOURS_PER_DAY, hours
            ));
        }
        let kind = a
            .get("kind")
            .and_then(|v| v.as_str())
            .unwrap_or("GENERAL_BILLABLE");
        if !HOUR_KINDS.contains(&kind) {
            return error_result(&format!(
                "Invalid kind \"{}\". Valid kinds: {}",
                kind,
                HOUR_KINDS.join(", ")
            ));
        }
        let mode = a.get("mode").and_then(|v| v.as_str()).unwrap_or("add");
        if mode != "add" && mode != "set" {
            return error_result("Invalid mode: use \"add\" or \"set\".");
        }
        let date = a
            .get("date")
            .and_then(|v| v.as_str())
            .unwrap_or(&chrono::Utc::now().format("%Y-%m-%d").to_string())
            .to_string();
        let method = if mode == "set" {
            "timesheets.upsertTime"
        } else {
            "timesheets.logTime"
        };
        let mut params = json!({ "kind": kind, "hours": hours, "date": date });
        if let Some(d) = a.get("description").and_then(|v| v.as_str()) {
            params["description"] = json!(d);
        }

        let idem_key = a.get("idempotencyKey").and_then(|v| v.as_str());
        match idempotency_check(&c, idem_key, "leantime_log_time") {
            Ok(Some(replay)) => return ok_result(&replay),
            Ok(None) => {}
            Err(e) => return error_result(&e),
        }

        match c
            .call(method, json!({"ticketId": tid, "params": params}))
            .await
        {
            Ok(r) => {
                if is_leantime_error(&r) {
                    return error_result(&leantime_error_msg(&r));
                }
                let mut echo = json!({ "ok": true, "ticketId": tid, "mode": mode });
                for (k, v) in params.as_object().unwrap() {
                    echo[k] = v.clone();
                }
                if let Some(w) = idempotency_note(&c, idem_key, "leantime_log_time", &echo) {
                    echo["idempotencyWarning"] = json!(w);
                }
                ok_result(&echo)
            }
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_get_ticket_time(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let tid = a.get("ticketId").and_then(|v| v.as_str()).unwrap_or("");
        let total = match c
            .call(
                "timesheets.getSumLoggedHoursForTicket",
                json!({"ticketId": tid}),
            )
            .await
        {
            Ok(t) => t,
            Err(e) => return error_result(&e.to_string()),
        };
        // The API wraps the sum in an array — unwrap.
        let total_hours = total
            .as_array()
            .and_then(|x| x.first())
            .cloned()
            .unwrap_or(total);
        let by_date = match c
            .call(
                "timesheets.getLoggedHoursForTicketByDate",
                json!({"ticketId": tid}),
            )
            .await
        {
            Ok(b) => b,
            Err(e) => return error_result(&e.to_string()),
        };
        ok_result(
            &json!({ "totalHours": total_hours, "byDate": by_date.as_array().cloned().unwrap_or_else(std::vec::Vec::new) }),
        )
    })
}

fn h_list_timesheets(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        let mut c = cl.lock().await;
        let date_from = a.get("dateFrom").and_then(|v| v.as_str()).unwrap_or("");
        let date_to = a.get("dateTo").and_then(|v| v.as_str()).unwrap_or("");
        // A bare YYYY-MM-DD dateTo means "end of that day" — Leantime's
        // whereBetween would otherwise exclude everything after 00:00:00.
        let date_to_end = if date_to.len() == 10 {
            format!("{} 23:59:59", date_to)
        } else {
            date_to.to_string()
        };
        let mut p = json!({ "dateFrom": date_from, "dateTo": date_to_end });
        if let Some(pid) = a
            .get("projectId")
            .and_then(|v| v.as_str())
            .filter(|s| !s.is_empty())
        {
            p["projectId"] = json!(pid);
        }
        match c.call("timesheets.getAll", p).await {
            Ok(r) => ok_result(&json!(r.as_array().cloned().unwrap_or_default())),
            Err(e) => error_result(&e.to_string()),
        }
    })
}

fn h_delete_timesheet(a: Value, cl: ClientRef) -> Pin<Box<dyn Future<Output = Value> + Send>> {
    Box::pin(async move {
        if let Err(e) = check_destructive(
            a.get("confirm").and_then(|v| v.as_bool()),
            "timesheet entry",
        ) {
            return error_result(&e);
        }
        let mut c = cl.lock().await;
        let id = a.get("entryId").and_then(|v| v.as_str()).unwrap_or("");
        match c.call("timesheets.deleteTime", json!({ "id": id })).await {
            Ok(_) => ok_result(&json!({ "deleted": true, "entryId": id })),
            Err(e) => error_result(&e.to_string()),
        }
    })
}

// ---------------------------------------------------------------------------
// Bulk
// ---------------------------------------------------------------------------

pub(super) fn tools() -> Vec<Tool> {
    vec![
        tool_with_annotations("leantime_log_time", "Log time on a ticket. mode \"add\" accumulates hours (default); mode \"set\" is idempotent (sets the total for that day/kind).",
            vec![rs("ticketId", "The ticket ID"), rn("hours", "Hours to log (duration in decimal hours, max 24 — one entry covers a single day; split across days beyond that)"), os("kind", "Hour type (GENERAL_BILLABLE, GENERAL_NOT_BILLABLE, PROJECTMANAGEMENT, DEVELOPMENT, BUGFIXING_NOT_BILLABLE, TESTING; default GENERAL_BILLABLE)"), os("date", "Work date, YYYY-MM-DD (default today)"), os("description", "What was done"), os("mode", "add = accumulate (logTime), set = idempotent total (upsertTime). Default \"add\""), ob("dryRun", DRY_RUN_DESC), os("idempotencyKey", IDEMPOTENCY_HINT)],
            vec!["ticketId", "hours"], Box::new(h_log_time), ToolAnnotations::write()),
        tool("leantime_get_ticket_time", "Get time booked on a ticket: total hours and per-day breakdown",
            vec![rs("ticketId", "The ticket ID")], vec!["ticketId"], Box::new(h_get_ticket_time)),
        tool("leantime_list_timesheets", "List booked time entries between two dates (all projects or one project)",
            vec![rs("dateFrom", "Start date, YYYY-MM-DD"), rs("dateTo", "End date, YYYY-MM-DD"), os("projectId", "Restrict to one project")],
            vec!["dateFrom", "dateTo"], Box::new(h_list_timesheets)),
        tool_with_annotations("leantime_delete_timesheet_entry", "Delete a booked time entry. Destructive: requires explicit user approval (confirm: true) unless LEANTIME_MCP_DESTRUCTIVE_POLICY is set otherwise.",
            vec![rs("entryId", "The timesheet entry ID"), ob("confirm", "MUST be true to actually delete (ask the user for explicit approval first)")],
            vec!["entryId"], Box::new(h_delete_timesheet), ToolAnnotations::destructive()),
    ]
}
