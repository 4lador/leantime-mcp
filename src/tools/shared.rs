//! Cross-domain helpers shared by the tool modules.

use serde_json::Value;

use crate::client::LeantimeClient;

use super::{Handler, Tool, ToolAnnotations};

/// The three reusable description fragments used across tool schemas.
pub(super) const MARKDOWN_HINT: &str = "Markdown (## headings, lists, - [ ] checklists, **bold**, `code`, links) — converted to rich HTML for Leantime's editor";
/// Assignment rule shown to agents on create tools.
pub(super) const ASSIGNMENT_HINT: &str =
    "You MUST ask the user who to assign to. Pass editorId or unassigned: true.";
/// Rate-limit note shown on bulk tools.
pub(super) const RATE_LIMIT_NOTE: &str = "The server transparently retries on 429 rate limits. On low-limit instances (10 req/min default), large batches may take several minutes.";

/// The rule quoted verbatim in assignment-enforcement errors.
pub(super) const ASSIGNMENT_RULE: &str =
    "You MUST ask the user who the ticket should be assigned to before calling this tool.";

/// Valid hour kinds for `timesheets.logTime`.
pub(super) const HOUR_KINDS: [&str; 6] = [
    "GENERAL_BILLABLE",
    "GENERAL_NOT_BILLABLE",
    "PROJECTMANAGEMENT",
    "DEVELOPMENT",
    "BUGFIXING_NOT_BILLABLE",
    "TESTING",
];

/// Maximum items per bulk batch.
pub(super) const MAX_BATCH: usize = 50;

/// Leantime services report errors as objects like `{msg, type: "error"}` —
/// surface them as tool errors.
pub(super) fn is_leantime_error(v: &Value) -> bool {
    v.get("type").and_then(|t| t.as_str()) == Some("error") && v.get("msg").is_some()
}

/// Extract the `msg` field of a Leantime soft error.
pub(super) fn leantime_error_msg(v: &Value) -> String {
    v.get("msg")
        .and_then(|m| m.as_str())
        .unwrap_or("Leantime error")
        .to_string()
}

/// Fetch the user list (cached 5 min) as simplified `(id, name)` pairs.
pub(super) async fn get_users_simplified(
    c: &mut LeantimeClient,
) -> Result<Vec<(String, String)>, String> {
    let users = c.get_users().await.map_err(|e| e.to_string())?;
    Ok(users
        .iter()
        .map(|u| {
            let id = u
                .get("id")
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_default();
            let name = format!(
                "{} {}",
                u.get("firstname").and_then(|v| v.as_str()).unwrap_or(""),
                u.get("lastname").and_then(|v| v.as_str()).unwrap_or("")
            );
            (id, name.trim().to_string())
        })
        .collect())
}

/// "1 (Ada Lovelace), 2 (Bob)" — used in error messages listing candidates.
pub(super) fn users_list(users: &[(String, String)]) -> String {
    users
        .iter()
        .map(|(id, name)| format!("{} ({})", id, name))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Assignment enforcement on create: `editorId` (validated) or explicit
/// `unassigned: true`. Error messages match the TS edition (the confirm
/// prompt normalizes its double-space template artifact — TS renders
/// "delete this  ticket", we emit "delete this ticket").
pub(super) fn check_assignment(args: &Value, users: &[(String, String)]) -> Result<(), String> {
    let editor_id = args.get("editorId");
    let unassigned = args
        .get("unassigned")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if editor_id.map(|v| v.is_null()).unwrap_or(true) && !unassigned {
        return Err(format!(
            "Assignment required: {} Then pass either editorId or unassigned: true (ONLY if the user explicitly opted out of assignment). Available users: {}",
            ASSIGNMENT_RULE,
            users_list(users)
        ));
    }
    if let Some(id) = editor_id {
        let id_str = match id {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        if !users.iter().any(|(uid, _)| *uid == id_str) {
            return Err(format!(
                "editorId \"{}\" does not exist. Available users: {}",
                id_str,
                users_list(users)
            ));
        }
    }
    Ok(())
}

/// editorId validation on update — only when the assignment actually changes.
pub(super) fn check_editor_id(editor_id: &Value, users: &[(String, String)]) -> Result<(), String> {
    let id_str = match editor_id {
        Value::String(s) => s.clone(),
        other => other.to_string(),
    };
    if !users.iter().any(|(uid, _)| *uid == id_str) {
        return Err(format!(
            "editorId \"{}\" does not exist. Available users: {}",
            id_str,
            users_list(users)
        ));
    }
    Ok(())
}

/// LEANTIME_MCP_DESTRUCTIVE_POLICY: ask (default) | deny | allow.
/// Messages match the TS edition, except the confirm prompt normalizes its
/// double-space template artifact ("delete this ticket", not "delete this  ticket").
pub(super) fn check_destructive(confirm: Option<bool>, what: &str) -> Result<(), String> {
    let policy = std::env::var("LEANTIME_MCP_DESTRUCTIVE_POLICY")
        .unwrap_or_default()
        .trim()
        .to_lowercase();
    match policy.as_str() {
        "deny" => Err(format!(
            "Destructive operations are disabled on this server (LEANTIME_MCP_DESTRUCTIVE_POLICY=deny). Deleting this {} is refused. Ask the server administrator to change the policy if this deletion is really needed.",
            what
        )),
        "allow" => Ok(()),
        _ => {
            if confirm != Some(true) {
                Err(format!(
                    "Confirmation required: ask the user for EXPLICIT approval to delete this {}, then retry with confirm: true. NEVER pass confirm: true without the user's explicit consent.",
                    what
                ))
            } else {
                Ok(())
            }
        }
    }
}

/// Numeric coercion mirroring TS `Number()`: numbers pass through, numeric
/// strings parse, anything else falls back to the default.
pub(super) fn num_coerce(v: Option<&Value>, default: f64) -> f64 {
    match v {
        Some(Value::Number(n)) => n.as_f64().unwrap_or(default),
        Some(Value::String(s)) => s.trim().parse::<f64>().unwrap_or(default),
        _ => default,
    }
}

/// Leantime dates come as "YYYY-MM-DD" or "YYYY-MM-DD HH:MM:SS" — epoch
/// millis, local time (matches the TS `new Date(...)` semantics).
pub(super) fn parse_leantime_ts(s: &str) -> Option<i64> {
    use chrono::TimeZone;
    let normalized = if s.len() == 10 {
        format!("{}T00:00:00", s)
    } else {
        s.replace(' ', "T")
    };
    chrono::NaiveDateTime::parse_from_str(&normalized, "%Y-%m-%dT%H:%M:%S")
        .ok()
        .and_then(|dt| chrono::Local.from_local_datetime(&dt).single())
        .map(|dt| dt.timestamp_millis())
}

// ---------------------------------------------------------------------------
// JSON-schema property helpers
// ---------------------------------------------------------------------------

fn schema(props: Vec<(String, Value)>, required: Vec<&str>) -> Value {
    let mut properties = serde_json::Map::new();
    for (k, v) in props {
        properties.insert(k, v);
    }
    serde_json::json!({ "type": "object", "properties": properties, "required": required })
}

/// Build a Tool from its parts (schema assembled from the property list).
pub(super) fn tool(
    name: &'static str,
    description: impl Into<String>,
    props: Vec<(String, Value)>,
    required: Vec<&str>,
    handler: Handler,
) -> Tool {
    tool_with_annotations(
        name,
        description,
        props,
        required,
        handler,
        ToolAnnotations::readonly(),
    )
}

/// Build a Tool with explicit MCP annotations.
pub(super) fn tool_with_annotations(
    name: &'static str,
    description: impl Into<String>,
    props: Vec<(String, Value)>,
    required: Vec<&str>,
    handler: Handler,
    annotations: ToolAnnotations,
) -> Tool {
    Tool {
        name,
        description: description.into(),
        schema: schema(props, required),
        annotations,
        handler,
    }
}

/// Required string property.
pub(super) fn rs(name: &str, desc: impl Into<String>) -> (String, Value) {
    (
        name.to_string(),
        serde_json::json!({ "type": "string", "description": desc.into() }),
    )
}

/// Optional string property.
pub(super) fn os(name: &str, desc: impl Into<String>) -> (String, Value) {
    (
        name.to_string(),
        serde_json::json!({ "type": "string", "description": desc.into(), "optional": true }),
    )
}

/// Optional number property.
pub(super) fn on(name: &str, desc: impl Into<String>) -> (String, Value) {
    (
        name.to_string(),
        serde_json::json!({ "type": "number", "description": desc.into(), "optional": true }),
    )
}

/// Required number property.
pub(super) fn rn(name: &str, desc: impl Into<String>) -> (String, Value) {
    (
        name.to_string(),
        serde_json::json!({ "type": "number", "description": desc.into() }),
    )
}

/// Optional boolean property.
pub(super) fn ob(name: &str, desc: impl Into<String>) -> (String, Value) {
    (
        name.to_string(),
        serde_json::json!({ "type": "boolean", "description": desc.into(), "optional": true }),
    )
}
