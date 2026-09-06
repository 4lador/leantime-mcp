use serde_json::json;

use crate::client::LeantimeClient;
use crate::config;

/// Run the `leantmcp doctor` health check: keyring files, permissions,
/// harness configs and live key validation.
pub async fn run_doctor() {
    let mut results: Vec<(String, String, String)> = Vec::new();

    // Env overrides win over the keyring everywhere below (same semantics as
    // serve). When they fully provide credentials, keyring gaps are surfaced
    // as warnings instead of failures — an env-driven run (CI, e2e) must not
    // fail health checks for a keyring it deliberately doesn't use.
    let env_url = std::env::var("LEANTIME_URL")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let env_key = std::env::var("LEANTIME_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty());
    let env_complete = env_url.is_some() && env_key.is_some();
    let keyring_severity = |fail: &str| -> String {
        if env_complete {
            "warn".into()
        } else {
            fail.into()
        }
    };

    // Default instance (via the validated resolver — never a hand-rolled env read)
    let env_inst: Option<String> = std::env::var("LEANTIME_INSTANCE")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty());
    let active = config::active_instance();
    let default = config::read_default_instance();
    let active_ref = active.as_ref().or(default.as_ref());

    match (&default, &active_ref) {
        (Some(d), Some(a)) => {
            let names = config::instance_names();
            if !names.contains(d) {
                results.push((
                    "default instance".into(),
                    keyring_severity("fail"),
                    format!("\"{}\" is default but doesn't exist", d),
                ));
            } else {
                // The marker tells the truth: only when the *validated* active
                // instance actually came from the env var.
                let via = if active.as_deref() == env_inst.as_deref() {
                    " (via LEANTIME_INSTANCE)"
                } else {
                    ""
                };
                results.push((
                    "default instance".into(),
                    "ok".into(),
                    format!("\"{}\"{}", a, via),
                ));
            }
        }
        (None, _) => {
            results.push((
                "default instance".into(),
                "warn".into(),
                "no default file — first `key set` or `url set` creates one".into(),
            ));
        }
        // (Some, None) is unreachable (active_ref falls back to default) but
        // the compiler cannot prove it.
        _ => {}
    }

    // Per-instance key health
    for name in config::instance_names() {
        let key_path = config::secret_path(Some(&name));
        if key_path.exists() {
            let mode = config::file_mode(&key_path);
            let mode_str = mode.clone().unwrap_or_else(|| "acl".into());
            let ok = mode.map(|m| m == "600").unwrap_or(true);
            results.push((
                format!("instance \"{}\" key", name),
                if ok { "ok".into() } else { "warn".into() },
                mode_str,
            ));
        } else {
            results.push((
                format!("instance \"{}\" key", name),
                keyring_severity("fail"),
                "missing".into(),
            ));
        }
    }

    // Key file (of the active instance)
    let key_path = config::secret_path(active_ref.map(String::as_str));
    if key_path.exists() {
        let mode = config::file_mode(&key_path);
        let mode_str = mode.clone().unwrap_or_else(|| "acl".into());
        let ok = mode.map(|m| m == "600").unwrap_or(true);
        results.push((
            "key file".into(),
            if ok { "ok".into() } else { "warn".into() },
            format!("{} ({})", key_path.display(), mode_str),
        ));
    } else {
        results.push((
            "key file".into(),
            keyring_severity("fail"),
            if env_complete {
                format!("{} missing (env overrides active)", key_path.display())
            } else {
                format!("{} missing — run: leantmcp key set", key_path.display())
            },
        ));
    }

    // Instance URL + live key validation. Env overrides win (same semantics
    // as serve) — `LEANTIME_URL`/`LEANTIME_API_KEY` target a different
    // instance than the keyring's active profile, e.g. in CI and e2e runs.
    let active_url = env_url.clone().or_else(|| {
        active_ref
            .map(String::as_str)
            .and_then(config::read_url_for)
    });
    let active_key = env_key.or_else(|| {
        active_ref
            .map(String::as_str)
            .and_then(config::read_key_for)
    });
    match &active_url {
        Some(url) => {
            let via = if env_url.is_some() { " (env)" } else { "" };
            results.push((
                "instance URL".into(),
                "ok".into(),
                format!("{}{}", url, via),
            ));
        }
        None => results.push(("instance URL".into(), "warn".into(), "not found".into())),
    }

    if let (Some(url), Some(key)) = (&active_url, &active_key) {
        let mut c = LeantimeClient::new(url, key);
        match c.call("users.getAll", json!({})).await {
            Ok(r) => {
                let count = r.as_array().map(|a| a.len()).unwrap_or(0);
                results.push((
                    "key validation".into(),
                    "ok".into(),
                    format!("valid — {} users", count),
                ));
            }
            Err(e) => results.push(("key validation".into(), "fail".into(), e.to_string())),
        }
    }

    // Global opencode config
    let global_config = config::home_dir().join(".opencode").join("opencode.json");
    if global_config.exists() {
        if let Ok(content) = std::fs::read_to_string(&global_config) {
            if let Ok(cfg) = serde_json::from_str::<serde_json::Value>(&content) {
                let env = cfg.pointer("/mcp/leantime/environment");
                let key_is_ptr = env
                    .and_then(|e| e.get("LEANTIME_API_KEY"))
                    .and_then(|k| k.as_str())
                    .map(|s| s.starts_with("{file:"))
                    .unwrap_or(false);
                let url_is_ptr = env
                    .and_then(|e| e.get("LEANTIME_URL"))
                    .and_then(|u| u.as_str())
                    .map(|s| s.starts_with("{file:"))
                    .unwrap_or(false);
                let mode = config::file_mode(&global_config).unwrap_or_else(|| "acl".into());
                let all = key_is_ptr && url_is_ptr;
                results.push((
                    "global opencode config".into(),
                    if all { "ok".into() } else { "warn".into() },
                    format!(
                        "{} (mode {}) — key: {}, url: {}",
                        global_config.display(),
                        mode,
                        if key_is_ptr { "pointer" } else { "plaintext" },
                        if url_is_ptr { "pointer" } else { "plaintext" }
                    ),
                ));
            }
        }
    } else {
        results.push((
            "global opencode config".into(),
            "warn".into(),
            "not found".into(),
        ));
    }

    // Print
    let mut failed = false;
    for (label, status, detail) in &results {
        let icon = match status.as_str() {
            "ok" => "✓",
            "warn" => "!",
            _ => {
                failed = true;
                "✗"
            }
        };
        println!("{} {}: {}", icon, label, detail);
    }
    if failed {
        std::process::exit(1);
    }
}
