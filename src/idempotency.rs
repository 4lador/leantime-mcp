//! Idempotency journal for mutation retries — lets an agent replay a
//! mutation without duplicating it when the first response was lost on
//! the agent's side (harness timeout, session restart, retry loop).
//!
//! Scope (deliberately narrow, documented in the spec): the journal
//! captures **successful** results only, keyed by an explicit
//! `idempotencyKey` scoped to (instance, tool, key). It does NOT cover
//! `Ambiguous` outcomes (a lost server response leaves nothing to
//! journal — the error still says "verify before retrying").
//!
//! Storage: one JSON file per instance profile (or a fallback file when
//! the server runs on pure env credentials), written atomically
//! (temp + rename, 0600). Read-modify-write per operation: concurrent
//! sessions on the same instance can race (last writer wins) — losing a
//! journal entry only means a rare duplicate on retry, which is the
//! status quo without keys.

use serde_json::Value;
use std::path::{Path, PathBuf};

/// Maximum live entries before recording refuses (DoS guard).
const MAX_ENTRIES: usize = 10_000;
/// Default retention, in days. `0` = keep forever.
const DEFAULT_TTL_DAYS: i64 = 7;

/// Outcome of a journal lookup for a key.
#[derive(Debug)]
pub enum Lookup {
    /// The key succeeded before on the same tool — the cached result.
    Hit(Value),
    /// The key was used by a DIFFERENT tool — never silently reuse it.
    Mismatch(String),
    /// Unknown (or expired) key.
    Miss,
}

/// Where the journal lives: the instance profile dir when the server
/// runs on a keyring instance, a fallback beside the keyring otherwise.
fn journal_path(dir: Option<&Path>) -> PathBuf {
    match dir {
        Some(d) => d.join("idempotency.json"),
        None => crate::config::secret_dir().join("idempotency.json"),
    }
}

/// Validate a user-provided key: 1..=128 characters.
pub fn validate_key(key: &str) -> Result<(), String> {
    if key.is_empty() || key.chars().count() > 128 {
        return Err(
            "idempotencyKey must be 1 to 128 characters — generate a fresh key for each new logical operation"
                .to_string(),
        );
    }
    Ok(())
}

fn ttl_days() -> i64 {
    std::env::var("LEANTIME_MCP_IDEMPOTENCY_TTL_DAYS")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .unwrap_or(DEFAULT_TTL_DAYS)
        .max(0)
}

fn is_expired(recorded_at: &str, ttl: i64) -> bool {
    if ttl == 0 {
        return false;
    }
    chrono::DateTime::parse_from_rfc3339(recorded_at)
        .map(|t| (chrono::Utc::now() - t.with_timezone(&chrono::Utc)).num_days() >= ttl)
        .unwrap_or(true) // unparseable timestamp → treat as expired
}

/// Load the journal, purging expired entries. A missing or corrupt file
/// yields an empty journal — a broken journal must not break mutations.
fn load_purged(path: &Path) -> serde_json::Map<String, Value> {
    let Ok(raw) = std::fs::read_to_string(path) else {
        return serde_json::Map::new();
    };
    let parsed: Value = match serde_json::from_str(&raw) {
        Ok(v) => v,
        Err(_) => return serde_json::Map::new(),
    };
    let Some(entries) = parsed.get("entries").and_then(|e| e.as_object()) else {
        return serde_json::Map::new();
    };
    let ttl = ttl_days();
    entries
        .iter()
        .filter(|(_, entry)| {
            entry
                .get("recordedAt")
                .and_then(|t| t.as_str())
                .map(|t| !is_expired(t, ttl))
                .unwrap_or(false)
        })
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

fn persist(path: &Path, entries: &serde_json::Map<String, Value>) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("idempotency journal: {}", e))?;
    }
    let body = serde_json::json!({ "entries": entries });
    let content =
        serde_json::to_string_pretty(&body).map_err(|e| format!("idempotency journal: {}", e))?;
    crate::config::write_private_file(path, &content)
        .map_err(|e| format!("idempotency journal: {}", e))
}

/// Look up a key for a tool. Loads (and TTL-purges in memory) the
/// journal — expired entries simply read as `Miss`.
pub fn lookup(dir: Option<&Path>, key: &str, tool: &str) -> Lookup {
    let entries = load_purged(&journal_path(dir));
    match entries.get(key) {
        Some(entry) => {
            let recorded_tool = entry.get("tool").and_then(|t| t.as_str()).unwrap_or("");
            if recorded_tool != tool {
                return Lookup::Mismatch(recorded_tool.to_string());
            }
            match entry.get("result") {
                Some(result) => Lookup::Hit(result.clone()),
                None => Lookup::Miss,
            }
        }
        None => Lookup::Miss,
    }
}

/// Record a successful result under a key. Fails (actionable message)
/// when the live-entry cap is reached; a TTL purge runs first so expired
/// entries free their slots.
pub fn record(dir: Option<&Path>, key: &str, tool: &str, result: &Value) -> Result<(), String> {
    validate_key(key)?;
    let path = journal_path(dir);
    let mut entries = load_purged(&path);
    // Purge-driven rewrite even without a cap hit keeps the file bounded.
    if entries.len() >= MAX_ENTRIES {
        return Err(format!(
            "idempotency journal is full ({} live entries, cap {}) — keys older than the TTL are purged automatically; raise LEANTIME_MCP_IDEMPOTENCY_TTL_DAYS? no: lower it, or use fewer keys",
            entries.len(),
            MAX_ENTRIES
        ));
    }
    entries.insert(
        key.to_string(),
        serde_json::json!({
            "tool": tool,
            "result": result,
            "recordedAt": chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Secs, true),
        }),
    );
    persist(&path, &entries)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn tmpdir(tag: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("leantmcp-idem-{}-{}", tag, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn key_validation_bounds() {
        assert!(validate_key("abc").is_ok());
        assert!(validate_key("").is_err());
        assert!(validate_key(&"x".repeat(128)).is_ok());
        assert!(validate_key(&"x".repeat(129)).is_err());
    }

    #[test]
    fn round_trip_hit_miss_mismatch() {
        let dir = tmpdir("round");
        assert!(matches!(
            lookup(Some(&dir), "k1", "leantime_create_ticket"),
            Lookup::Miss
        ));
        record(
            Some(&dir),
            "k1",
            "leantime_create_ticket",
            &json!({"id": 777}),
        )
        .unwrap();
        match lookup(Some(&dir), "k1", "leantime_create_ticket") {
            Lookup::Hit(v) => assert_eq!(v, json!({"id": 777})),
            other => panic!("expected hit, got {:?}", other),
        }
        match lookup(Some(&dir), "k1", "leantime_create_milestone") {
            Lookup::Mismatch(t) => assert_eq!(t, "leantime_create_ticket"),
            other => panic!("expected mismatch, got {:?}", other),
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ttl_zero_keeps_entries() {
        let dir = tmpdir("ttl0");
        // Default TTL expires old entries; write one with an old timestamp
        // by manipulating the file directly.
        record(
            Some(&dir),
            "old",
            "leantime_create_ticket",
            &json!({"id": 1}),
        )
        .unwrap();
        let path = dir.join("idempotency.json");
        let raw = std::fs::read_to_string(&path).unwrap();
        let ancient = chrono::Utc::now() - chrono::Duration::days(3650);
        let raw = raw.replace(
            &chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            &ancient.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        );
        std::fs::write(&path, raw).unwrap();
        std::env::set_var("LEANTIME_MCP_IDEMPOTENCY_TTL_DAYS", "0");
        assert!(matches!(
            lookup(Some(&dir), "old", "leantime_create_ticket"),
            Lookup::Hit(_)
        ));
        std::env::set_var("LEANTIME_MCP_IDEMPOTENCY_TTL_DAYS", "7");
        assert!(matches!(
            lookup(Some(&dir), "old", "leantime_create_ticket"),
            Lookup::Miss
        ));
        std::env::remove_var("LEANTIME_MCP_IDEMPOTENCY_TTL_DAYS");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn corrupt_journal_reads_as_miss() {
        let dir = tmpdir("corrupt");
        std::fs::write(dir.join("idempotency.json"), "{not json").unwrap();
        assert!(matches!(
            lookup(Some(&dir), "anything", "leantime_create_ticket"),
            Lookup::Miss
        ));
        // Recording over the corrupt file heals it
        record(
            Some(&dir),
            "fresh",
            "leantime_create_ticket",
            &json!({"id": 2}),
        )
        .unwrap();
        assert!(matches!(
            lookup(Some(&dir), "fresh", "leantime_create_ticket"),
            Lookup::Hit(_)
        ));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn record_is_parseable_json_on_disk() {
        let dir = tmpdir("atomic");
        record(Some(&dir), "k", "leantime_log_time", &json!({"ok": true})).unwrap();
        let raw = std::fs::read_to_string(dir.join("idempotency.json")).unwrap();
        let v: Value = serde_json::from_str(&raw).unwrap();
        assert!(v["entries"]["k"]["tool"].is_string());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
