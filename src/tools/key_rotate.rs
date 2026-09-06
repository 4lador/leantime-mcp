//! Key rotation (CLI-only, not exposed as an MCP tool).

use serde_json::{json, Value};

use crate::client::LeantimeClient;
use crate::config;

use super::shared::{is_leantime_error, leantime_error_msg};

/// Rotate the stored API key: identify the current key entry, mint a new key
/// (same role), copy project relations, live-verify, then replace the stored
/// key. The previous key is left untouched on any failure.
pub async fn key_rotate(
    c: &mut LeantimeClient,
    current_key: &str,
    url: &str,
    name: &str,
) -> Result<String, String> {
    let keys = c
        .call("Api.getAPIKeys", json!({}))
        .await
        .map_err(|e| e.to_string())?;
    let user_segment = current_key
        .strip_prefix("lt_")
        .unwrap_or(current_key)
        .split('_')
        .next()
        .unwrap_or("")
        .to_string();
    let entry = keys
        .as_array()
        .and_then(|arr| {
            arr.iter().find(|k| {
                k.get("username")
                    .and_then(|u| u.as_str())
                    .map(|u| {
                        let prefix: String = u.chars().take(5).collect();
                        user_segment.starts_with(&prefix)
                    })
                    .unwrap_or(false)
            })
        })
        .ok_or("Could not identify current key")?;

    let role = entry
        .get("role")
        .map(|r| match r {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_else(|| "20".into());
    let entry_id = entry
        .get("id")
        .map(|v| match v {
            Value::String(s) => s.clone(),
            other => other.to_string(),
        })
        .unwrap_or_default();

    let created = c
        .call(
            "Api.createAPIKey",
            json!({"values": {"firstname": name, "role": role, "source": "api"}}),
        )
        .await
        .map_err(|e| format!("Key creation failed: {}", e))?;
    if is_leantime_error(&created) {
        return Err(format!(
            "Key creation failed: {}",
            leantime_error_msg(&created)
        ));
    }
    if created == Value::Bool(false) || created.is_null() {
        return Err(
            "Key creation failed: the instance refused (check the key name/role).".to_string(),
        );
    }

    let new_user = created
        .get("user")
        .and_then(|u| u.as_str())
        .ok_or("Missing user")?;
    let new_pass = created
        .get("passwordClean")
        .or_else(|| created.get("password"))
        .and_then(|p| p.as_str())
        .ok_or("Missing password")?;
    let new_key = format!("lt_{}_{}", new_user, new_pass);

    // Copy project relations from the old key entry.
    let mut relations_warning = String::new();
    if let Ok(projects) = c
        .call(
            "Projects.getProjectsAssignedToUser",
            json!({"userId": entry_id}),
        )
        .await
    {
        if let Some(arr) = projects.as_array() {
            if !arr.is_empty() {
                if let Some(new_id) = created.get("id").map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                }) {
                    let ids: Vec<Value> = arr
                        .iter()
                        .map(|p| p.get("id").cloned().unwrap_or(Value::Null))
                        .collect();
                    match c.call("Projects.editUserProjectRelations", json!({"id": new_id, "projects": ids})).await {
                        Ok(_) => relations_warning = format!(" Project relations copied ({}).", arr.len()),
                        Err(e) => relations_warning = format!("\n! WARNING: could not copy project relations ({}). Assign the key to its projects in the Leantime UI.", e),
                    }
                }
            }
        }
    }

    // Live-verify before replacing anything.
    let mut tc = LeantimeClient::new(url, &new_key);
    tc.call("users.getAll", json!({}))
        .await
        .map_err(|e| format!("Verification failed ({}) — previous key untouched.", e))?;

    let path = config::write_key(&new_key).map_err(|e| e.to_string())?;
    Ok(format!("Key rotated: {} (role {}). Stored at {}.{} Delete the old key {} in the Leantime UI (My Account → API Keys).",
        config::mask_key(&new_key), role, path.display(), relations_warning, config::mask_key(current_key)))
}
