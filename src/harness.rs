//! Harness setup — writes MCP server configs for the common harnesses.
//!
//! Two families:
//! - opencode (global/project): `{file:...}` pointers when the key already
//!   lives in the keyring — no plaintext secret in the config.
//! - claude-code / claude-desktop / cursor / codex: a BARE COMMAND with no
//!   environment block. The binary resolves credentials from the keyring
//!   (~/.config/leantime/) at startup, so no secret ever lands in a config.

use std::io::{BufRead, Write};
use std::path::PathBuf;

use serde_json::{json, Value};

use crate::config;

/// Outcome of a `setup <harness>` command.
pub struct HarnessResult {
    /// Whether the operation succeeded.
    pub ok: bool,
    /// Human-readable outcome (paths written, or the error).
    pub message: String,
}

impl HarnessResult {
    fn ok(message: impl Into<String>) -> Self {
        Self {
            ok: true,
            message: message.into(),
        }
    }
    fn err(message: impl Into<String>) -> Self {
        Self {
            ok: false,
            message: message.into(),
        }
    }
}

/// Absolute path of the leantmcp executable.
pub fn server_command() -> String {
    std::env::current_exe()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|_| "leantmcp".to_string())
}

// ---------------------------------------------------------------- input

fn prompt(line: &str) -> String {
    print!("{line}");
    let _ = std::io::stdout().flush();
    let mut buf = String::new();
    let _ = std::io::stdin().lock().read_line(&mut buf);
    buf.trim().to_string()
}

fn prompt_hidden(line: &str) -> String {
    eprint!("{line}");
    let _ = std::io::stderr().flush();
    rpassword::read_password()
        .unwrap_or_default()
        .trim()
        .to_string()
}

/// Make sure the keyring exists, prompting (or reading env) for what's missing.
/// Write failures are surfaced, not swallowed — a keyring that "succeeded"
/// without persisting would only explode later, at server startup.
pub fn ensure_keyring() -> HarnessResult {
    let mut problems: Vec<String> = Vec::new();
    if config::read_url().is_none() {
        let url = std::env::var("LEANTIME_URL")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| prompt("Leantime instance URL: "));
        if url.is_empty() {
            problems.push("LEANTIME_URL".into());
        } else if let Err(e) = config::write_url(&url) {
            problems.push(format!("could not store the URL ({})", e));
        }
    }
    if config::read_key().is_none() {
        let key = std::env::var("LEANTIME_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| prompt_hidden("API key (input hidden): "));
        if key.is_empty() {
            problems.push("LEANTIME_API_KEY".into());
        } else if let Err(e) = config::write_key(&key) {
            problems.push(format!("could not store the key ({})", e));
        }
    }
    if !problems.is_empty() {
        return HarnessResult::err(format!(
            "Keyring setup failed: {} — run 'leantmcp url set' / 'leantmcp key set' to diagnose.",
            problems.join(" and ")
        ));
    }
    HarnessResult::ok(String::new())
}

// ---------------------------------------------------------------- paths

/// OS-specific `claude_desktop_config.json` path.
pub fn claude_desktop_config_path() -> PathBuf {
    if cfg!(target_os = "macos") {
        config::home_dir().join("Library/Application Support/Claude/claude_desktop_config.json")
    } else if cfg!(target_os = "windows") {
        config::home_dir().join("AppData/Roaming/Claude/claude_desktop_config.json")
    } else {
        config::home_dir().join(".config/Claude/claude_desktop_config.json")
    }
}

/// `~/.cursor/mcp.json`.
pub fn cursor_config_path() -> PathBuf {
    config::home_dir().join(".cursor/mcp.json")
}

/// `~/.codex/config.toml`.
pub fn codex_config_path() -> PathBuf {
    config::home_dir().join(".codex/config.toml")
}

/// `~/.opencode/opencode.json`.
pub fn opencode_global_path() -> PathBuf {
    config::home_dir().join(".opencode/opencode.json")
}

/// `./opencode.json` (current directory).
pub fn opencode_project_path() -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("opencode.json")
}

// ---------------------------------------------------------------- writers

fn read_json(path: &std::path::Path) -> Value {
    std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_else(|| json!({}))
}

/// Write a file with mode 0600 from the first byte (no 0644 window — the
/// opencode fallback may hold a plaintext API key). Belt-and-braces chmod
/// enforces 0600 even on pre-existing files. (Shared implementation in config.)
fn write_private_file(path: &std::path::Path, content: &str) -> std::io::Result<()> {
    config::write_private_file(path, content)
}

fn write_json_0600(path: &std::path::Path, v: &Value) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    write_private_file(path, &(serde_json::to_string_pretty(v).unwrap() + "\n"))
}

/// Merge `mcpServers.<name> = { command, env? }` into a JSON config (bare
/// command — the binary resolves credentials from the keyring at startup).
pub fn write_mcp_json(
    path: &std::path::Path,
    name: &str,
    command: &str,
    env: &[(&str, &str)],
) -> HarnessResult {
    let mut config = read_json(path);
    if !config.is_object() {
        return HarnessResult::err(format!(
            "{} exists but is not a JSON object — inspect it manually.",
            path.display()
        ));
    }
    let servers = config
        .as_object_mut()
        .unwrap()
        .entry("mcpServers")
        .or_insert_with(|| json!({}));
    if !servers.is_object() {
        return HarnessResult::err(format!(
            "{} has a non-object 'mcpServers' — inspect it manually.",
            path.display()
        ));
    }
    let mut entry = json!({ "command": command });
    if !env.is_empty() {
        let env_map: serde_json::Map<String, Value> =
            env.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
        entry["env"] = Value::Object(env_map);
    }
    servers.as_object_mut().unwrap().insert(name.into(), entry);
    if write_json_0600(path, &config).is_err() {
        return HarnessResult::err(format!("Could not write {}", path.display()));
    }
    HarnessResult::ok(format!(
        "Written to {} — bare command, no secrets (the binary reads ~/.config/leantime/ itself).",
        path.display()
    ))
}

/// Append `[mcp_servers.<name>]` to a TOML config (idempotent-guarded).
pub fn append_codex_toml(
    path: &std::path::Path,
    name: &str,
    command: &str,
    env: &[(&str, &str)],
) -> HarnessResult {
    let existing = std::fs::read_to_string(path).unwrap_or_default();
    let section = format!("[mcp_servers.{name}]");
    if existing.contains(&section) {
        return HarnessResult::ok(format!(
            "{} already has a {section} section — left untouched. Remove it first to regenerate.",
            path.display()
        ));
    }
    if let Some(dir) = path.parent() {
        if std::fs::create_dir_all(dir).is_err() {
            return HarnessResult::err(format!("Could not create {}", dir.display()));
        }
    }
    let mut out = existing.clone();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&format!(
        "\n{section}\ncommand = {}\n",
        serde_json::to_string(command).unwrap() // JSON string escaping is valid TOML
    ));
    if !env.is_empty() {
        out.push_str(&format!("\n[mcp_servers.{name}.env]\n"));
        for (k, v) in env {
            out.push_str(&format!("{k} = {}\n", serde_json::to_string(v).unwrap()));
        }
    }
    if write_private_file(path, &out).is_err() {
        return HarnessResult::err(format!("Could not write {}", path.display()));
    }
    HarnessResult::ok(format!(
        "Appended {section} to {} — bare command, no secrets.",
        path.display()
    ))
}

// ---------------------------------------------------------------- opencode

/// Setup for opencode (global or project scope). Without `--instance`, uses
/// {file:...} pointers when the key already lives in the keyring — no
/// plaintext secret. With `--instance PROFILE`, emits a bare command +
/// `LEANTIME_INSTANCE` env: the binary resolves the pinned keyring profile
/// at startup.
pub fn setup_opencode(global: bool, server_name: &str, pinned: &Option<String>) -> HarnessResult {
    if let Some(profile) = pinned {
        // Pinned-instance form: bare command + env (no key material involved).
        let path = if global {
            opencode_global_path()
        } else {
            opencode_project_path()
        };
        let mut existing = read_json(&path);
        if !existing.is_object() {
            return HarnessResult::err(format!(
                "{} exists but is not a JSON object — inspect it manually.",
                path.display()
            ));
        }
        let mcp = existing
            .as_object_mut()
            .unwrap()
            .entry("mcp")
            .or_insert_with(|| json!({}));
        if !mcp.is_object() {
            return HarnessResult::err(format!(
                "{} has a non-object 'mcp' — inspect it manually.",
                path.display()
            ));
        }
        mcp.as_object_mut().unwrap().insert(
            server_name.into(),
            json!({
                "type": "local",
                "command": [server_command()],
                "environment": { "LEANTIME_INSTANCE": profile }
            }),
        );
        if write_json_0600(&path, &existing).is_err() {
            return HarnessResult::err(format!("Could not write {}", path.display()));
        }
        return HarnessResult::ok(format!(
            "Written to {} — server {server_name:?} pinned to keyring instance {profile:?}, no secrets.",
            path.display()
        ));
    }

    // Gather URL and key (env > prompt > existing keyring).
    let mut url = std::env::var("LEANTIME_URL")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_default();
    let mut api_key = std::env::var("LEANTIME_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_default();

    if url.is_empty() {
        if let Some(stored) = config::read_url() {
            url = stored;
        } else {
            url = prompt("Leantime instance URL: ");
        }
    }
    if api_key.is_empty() && config::read_key().is_none() {
        api_key = prompt_hidden("API Key (input hidden): ");
    }
    if url.is_empty() || (api_key.is_empty() && config::read_key().is_none()) {
        return HarnessResult::err("Leantime URL and API key are required.");
    }
    let url = url.trim_end_matches('/').to_string();

    // The URL becomes the single source of truth in the keyring;
    // the config only gets a {file:...} pointer.
    if config::write_url(&url).is_err() {
        return HarnessResult::err("Could not write the instance URL to the keyring.");
    }

    // When the key already lives in the secret file, store only a {file:...}
    // pointer in the config — opencode substitutes file contents natively.
    // Pointers resolve through the ACTIVE instance (created above by write_url).
    let inst = config::active_instance();
    let stored_key = config::read_key();
    let key_value = match (&stored_key, api_key.is_empty()) {
        (Some(_), true) => format!(
            "{{file:{}}}",
            config::secret_path(inst.as_deref()).display()
        ),
        (Some(stored), false) if stored == &api_key => {
            format!(
                "{{file:{}}}",
                config::secret_path(inst.as_deref()).display()
            )
        }
        _ => api_key.clone(),
    };
    let url_value = format!(
        "{{file:{}}}",
        config::instance_url_path(inst.as_deref()).display()
    );

    let path = if global {
        opencode_global_path()
    } else {
        opencode_project_path()
    };
    let mut existing = read_json(&path);
    if !existing.is_object() {
        return HarnessResult::err(format!(
            "{} exists but is not a JSON object — inspect it manually.",
            path.display()
        ));
    }
    let mcp = existing
        .as_object_mut()
        .unwrap()
        .entry("mcp")
        .or_insert_with(|| json!({}));
    if !mcp.is_object() {
        return HarnessResult::err(format!(
            "{} has a non-object 'mcp' — inspect it manually.",
            path.display()
        ));
    }
    mcp.as_object_mut().unwrap().insert(
        server_name.into(),
        json!({
            "type": "local",
            "command": [server_command()],
            "environment": {
                "LEANTIME_URL": url_value,
                "LEANTIME_API_KEY": key_value,
            }
        }),
    );
    if write_json_0600(&path, &existing).is_err() {
        return HarnessResult::err(format!("Could not write {}", path.display()));
    }

    let mut message = format!("Written to {}", path.display());
    if !key_value.starts_with("{file:") {
        message.push_str(&format!(
            "\n! [DEPRECATED] The API key was stored in PLAINTEXT in {}. This fallback exists for\n  bootstrapping only and will be removed in a future version.\n  Migrate now: run 'leantmcp key set' (stores the key in the 0600 keyring),\n  then re-run 'leantmcp setup {}' — the config then only holds a {{file:}} pointer, no secret.",
            path.display(),
            if global { "global" } else { "project" }
        ));
    }
    HarnessResult::ok(message)
}

// ---------------------------------------------------------------- entry

/// Options accepted by every `setup <harness>` invocation.
#[derive(Default)]
pub struct SetupOptions<'a> {
    /// `--scope global|project` (None = harness default).
    pub scope: Option<&'a str>,
    /// `--instance PROFILE`: pin a keyring profile via `LEANTIME_INSTANCE`.
    pub instance: Option<&'a str>,
    /// `--name SERVER`: config key for the server (default: leantime).
    pub name: Option<&'a str>,
}

/// Server names become config keys — keep them conservative identifiers.
fn validate_server_name(name: &str) -> Result<(), String> {
    let n = name.chars().count();
    if n == 0 || n > 64 {
        return Err(format!(
            "Invalid server name {name:?}: must be 1-64 characters"
        ));
    }
    let ok = name
        .chars()
        .next()
        .is_some_and(|c| c.is_ascii_alphanumeric())
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if ok {
        Ok(())
    } else {
        Err(format!(
            "Invalid server name {name:?}: allowed characters are letters, digits, '-' and '_'"
        ))
    }
}

/// Resolve `--instance` into a validated existing keyring profile.
fn resolve_instance(instance: Option<&str>) -> Result<Option<String>, String> {
    match instance {
        None => Ok(None),
        Some(name) => {
            config::validate_instance_name(name)?;
            let names = config::instance_names();
            if !names.contains(&name.to_string()) {
                return Err(format!(
                    "Instance profile {name:?} not found. Available: {}",
                    names.join(", ")
                ));
            }
            Ok(Some(name.to_string()))
        }
    }
}

/// Dispatch a `setup <harness>` command.
pub fn setup_harness(harness: &str, opts: &SetupOptions<'_>) -> HarnessResult {
    // Common validation and derived values.
    let server_name = match opts.name {
        None => "leantime".to_string(),
        Some(n) => match validate_server_name(n) {
            Ok(()) => n.to_string(),
            Err(e) => return HarnessResult::err(e),
        },
    };
    let pinned = match resolve_instance(opts.instance) {
        Ok(p) => p,
        Err(e) => return HarnessResult::err(e),
    };
    // With --instance, every harness gets a bare command + LEANTIME_INSTANCE
    // env block — the binary resolves the keyring profile at startup.
    let env: Vec<(String, String)> = pinned
        .as_ref()
        .map(|p| vec![("LEANTIME_INSTANCE".to_string(), p.clone())])
        .unwrap_or_default();
    let env_ref: Vec<(&str, &str)> = env.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();

    match harness {
        "opencode" => {
            let scope = opts.scope.unwrap_or("global");
            setup_opencode(scope == "global", &server_name, &pinned)
        }
        "claude-code" => {
            // Project scope by default (.mcp.json in cwd) — the committed, shared form.
            if opts.scope == Some("global") {
                return HarnessResult::ok(format!(
                    "User-scoped Claude Code servers are managed by the claude CLI itself. Run:\n  claude mcp add {name} --scope user -- {cmd}",
                    name = server_name,
                    cmd = server_command()
                ));
            }
            let path = std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(".mcp.json");
            let r = write_mcp_json(&path, &server_name, &server_command(), &env_ref);
            HarnessResult {
                ok: r.ok,
                message: format!(
                    "{}\nFor a user-scoped server instead, run:\n  claude mcp add {name} --scope user -- {cmd}",
                    r.message,
                    name = server_name,
                    cmd = server_command()
                ),
            }
        }
        "claude-desktop" => {
            if opts.scope == Some("project") {
                return HarnessResult::err(
                    "Claude Desktop has no project scope — it is a GUI app with a single global config.",
                );
            }
            if pinned.is_none() {
                let keyring = ensure_keyring();
                if !keyring.ok {
                    return keyring;
                }
            }
            write_mcp_json(
                &claude_desktop_config_path(),
                &server_name,
                &server_command(),
                &env_ref,
            )
        }
        "cursor" => {
            let path = match opts.scope.unwrap_or("global") {
                "project" => std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(".cursor")
                    .join("mcp.json"),
                _ => cursor_config_path(),
            };
            if pinned.is_none() {
                let keyring = ensure_keyring();
                if !keyring.ok {
                    return keyring;
                }
            }
            write_mcp_json(&path, &server_name, &server_command(), &env_ref)
        }
        "codex" => {
            let path = match opts.scope.unwrap_or("global") {
                "project" => std::env::current_dir()
                    .unwrap_or_else(|_| PathBuf::from("."))
                    .join(".codex")
                    .join("config.toml"),
                _ => codex_config_path(),
            };
            if pinned.is_none() {
                let keyring = ensure_keyring();
                if !keyring.ok {
                    return keyring;
                }
            }
            append_codex_toml(&path, &server_name, &server_command(), &env_ref)
        }
        other => HarnessResult::err(format!("Unknown harness: {other}")),
    }
}
