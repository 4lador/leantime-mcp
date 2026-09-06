use serde_json::json;
use std::path::PathBuf;

use leantmcp::config;
use leantmcp::harness;

fn scratch(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "leantmcp-harness-test-{}-{}",
        tag,
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

/// Serializes the env-mutating tests (parallel `cargo test` support).
static HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn lock() -> std::sync::MutexGuard<'static, ()> {
    HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn set_home(dir: &PathBuf) -> std::sync::MutexGuard<'static, ()> {
    let guard = lock();
    std::env::set_var("HOME", dir);
    // On Windows, dirs::home_dir() reads USERPROFILE — redirect it too.
    #[cfg(windows)]
    if let Some(up) = dir.to_str() {
        std::env::set_var("USERPROFILE", up);
    }
    std::env::remove_var("LEANTIME_URL");
    std::env::remove_var("LEANTIME_API_KEY");
    std::env::remove_var("LEANTIME_INSTANCE");
    guard
}

fn read_json(path: &std::path::Path) -> serde_json::Value {
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

// ---------------------------------------------------------------- mcp json

#[test]
fn write_mcp_json_creates_and_merges() {
    let dir = scratch("mcpjson");
    let _guard = set_home(&dir);
    let path = dir.join("mcp.json");

    // First write: creates the file with the leantime server.
    let r = harness::write_mcp_json(&path, "leantime", "/bin/leantmcp", &[]);
    assert!(r.ok, "{}", r.message);

    let v = read_json(&path);
    assert_eq!(v["mcpServers"]["leantime"]["command"], "/bin/leantmcp");

    // Second write: preserves unrelated servers.
    let mut cfg = read_json(&path);
    cfg["mcpServers"]["other"] = json!({"command": "/bin/other"});
    std::fs::write(&path, serde_json::to_string_pretty(&cfg).unwrap()).unwrap();

    let r = harness::write_mcp_json(&path, "leantime", "/bin/leantmcp-v2", &[]);
    assert!(r.ok);
    let v = read_json(&path);
    assert_eq!(v["mcpServers"]["other"]["command"], "/bin/other");
    assert_eq!(v["mcpServers"]["leantime"]["command"], "/bin/leantmcp-v2");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn write_mcp_json_rejects_non_object() {
    let dir = scratch("mcpjson-bad");
    let _guard = set_home(&dir);
    let path = dir.join("mcp.json");
    std::fs::write(&path, "[1,2,3]").unwrap();

    let r = harness::write_mcp_json(&path, "leantime", "/bin/leantmcp", &[]);
    assert!(!r.ok);
    assert!(r.message.contains("not a JSON object"));

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn write_mcp_json_is_0600() {
    let dir = scratch("mcpjson-mode");
    let _guard = set_home(&dir);
    let path = dir.join("nested/mcp.json");
    let r = harness::write_mcp_json(&path, "leantime", "/bin/leantmcp", &[]);
    assert!(r.ok);
    assert!(path.exists());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600);
    }
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- codex toml

#[test]
fn append_codex_toml_creates_and_is_idempotent_guarded() {
    let dir = scratch("codex");
    let _guard = set_home(&dir);
    let path = dir.join("config.toml");

    let r = harness::append_codex_toml(&path, "leantime", "/bin/leantmcp", &[]);
    assert!(r.ok, "{}", r.message);
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.contains("[mcp_servers.leantime]"));
    assert!(content.contains(r#"command = "/bin/leantmcp""#));

    // Second call: refuses to duplicate the section.
    std::fs::write(&path, content.clone() + "# custom trailing\n").unwrap();
    let r = harness::append_codex_toml(&path, "leantime", "/bin/other", &[]);
    assert!(r.ok);
    assert!(r.message.contains("left untouched"));
    let after = std::fs::read_to_string(&path).unwrap();
    assert_eq!(after, content + "# custom trailing\n");

    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn append_codex_toml_preserves_existing_content() {
    let dir = scratch("codex-existing");
    let _guard = set_home(&dir);
    let path = dir.join("config.toml");
    std::fs::write(&path, "model = \"gpt-5\"\n").unwrap();

    let r = harness::append_codex_toml(&path, "leantime", "/bin/leantmcp", &[]);
    assert!(r.ok);
    let content = std::fs::read_to_string(&path).unwrap();
    assert!(content.starts_with("model = \"gpt-5\"\n"));
    assert!(content.contains("[mcp_servers.leantime]"));

    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- opencode

#[test]
fn setup_opencode_project_with_keyring_pointer() {
    let dir = scratch("oc-project");
    let _guard = set_home(&dir);
    config::write_key("lt_test_key_123").unwrap();
    config::write_url("https://leantime.example/").unwrap();

    // No env vars: everything comes from the keyring → {file:} pointers.
    let cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let r = harness::setup_opencode(false, "leantime", &None);
    assert!(r.ok, "{}", r.message);
    assert!(!r.message.contains("plaintext"), "{}", r.message);

    let path = dir.join("opencode.json");
    let v = read_json(&path);
    let entry = &v["mcp"]["leantime"];
    assert_eq!(entry["type"], "local");
    assert!(entry["command"].is_array());

    let env = &entry["environment"];
    let key_ptr = env["LEANTIME_API_KEY"].as_str().unwrap();
    assert!(key_ptr.starts_with("{file:"), "{key_ptr}");
    // The pointer must resolve to the ACTIVE instance path.
    assert!(key_ptr.contains("instances"), "{key_ptr}");
    // And the pointed file must contain the actual key.
    let key_file = key_ptr.trim_start_matches("{file:").trim_end_matches('}');
    assert_eq!(
        std::fs::read_to_string(key_file).unwrap(),
        "lt_test_key_123"
    );

    let url_ptr = env["LEANTIME_URL"].as_str().unwrap();
    assert!(url_ptr.contains("instances"), "{url_ptr}");
    let url_file = url_ptr.trim_start_matches("{file:").trim_end_matches('}');
    assert_eq!(
        std::fs::read_to_string(url_file).unwrap(),
        "https://leantime.example"
    );

    std::env::set_current_dir(cwd).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn setup_opencode_fresh_key_warns_plaintext() {
    let dir = scratch("oc-fresh");
    let _guard = set_home(&dir);

    std::env::set_var("LEANTIME_URL", "https://fresh.example");
    std::env::set_var("LEANTIME_API_KEY", "lt_fresh_key");

    let cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let r = harness::setup_opencode(false, "leantime", &None);
    assert!(r.ok);
    assert!(r.message.contains("PLAINTEXT"), "{}", r.message);
    assert!(r.message.contains("[DEPRECATED]"), "{}", r.message);
    assert!(r.message.contains("leantmcp key set"), "{}", r.message);

    let v = read_json(&dir.join("opencode.json"));
    assert_eq!(
        v["mcp"]["leantime"]["environment"]["LEANTIME_API_KEY"],
        "lt_fresh_key"
    );

    // The URL was persisted to the keyring and gets a pointer.
    assert_eq!(config::read_url().as_deref(), Some("https://fresh.example"));
    assert!(v["mcp"]["leantime"]["environment"]["LEANTIME_URL"]
        .as_str()
        .unwrap()
        .starts_with("{file:"));

    std::env::set_current_dir(cwd).unwrap();
    std::env::remove_var("LEANTIME_URL");
    std::env::remove_var("LEANTIME_API_KEY");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn setup_opencode_named_instance_pointer() {
    let dir = scratch("oc-named");
    let _guard = set_home(&dir);
    std::env::set_var("LEANTIME_INSTANCE", "prod");
    config::write_key("lt_prod_key").unwrap();
    config::write_url("https://prod.example").unwrap();

    let cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let r = harness::setup_opencode(false, "leantime", &None);
    assert!(r.ok, "{}", r.message);

    let v = read_json(&dir.join("opencode.json"));
    let key_ptr = v["mcp"]["leantime"]["environment"]["LEANTIME_API_KEY"]
        .as_str()
        .unwrap();
    // Path separator differs between platforms (Unix / vs Windows \)
    let sep = std::path::MAIN_SEPARATOR;
    assert!(
        key_ptr.contains(&format!("instances{sep}prod")),
        "{key_ptr}"
    );

    std::env::set_current_dir(cwd).unwrap();
    std::env::remove_var("LEANTIME_INSTANCE");
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- paths

#[test]
fn harness_paths_are_in_home() {
    let dir = scratch("paths");
    let _guard = set_home(&dir);
    assert!(harness::cursor_config_path().starts_with(&dir));
    assert!(harness::codex_config_path().starts_with(&dir));
    assert!(harness::opencode_global_path().starts_with(&dir));
    assert!(harness::claude_desktop_config_path().starts_with(&dir));
    let _ = std::fs::remove_dir_all(&dir);
}

// ---------------------------------------------------------------- scope/instance/name options

#[test]
fn setup_cursor_project_scope_and_instance_pin() {
    let dir = scratch("cursor-proj");
    let _guard = set_home(&dir);
    config::write_url_for("staging", "https://staging.example").unwrap();
    config::write_key_for("staging", "lt_staging_key").unwrap();
    let cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let opts = SetupOptions {
        scope: Some("project"),
        instance: Some("staging"),
        name: Some("leantime-stage"),
    };
    let r = harness::setup_harness("cursor", &opts);
    assert!(r.ok, "{}", r.message);

    let v = read_json(&dir.join(".cursor/mcp.json"));
    assert!(v["mcpServers"]["leantime-stage"]["command"].is_string()); // current_exe = test binary in tests
    assert_eq!(
        v["mcpServers"]["leantime-stage"]["env"]["LEANTIME_INSTANCE"],
        "staging"
    );
    let raw = std::fs::read_to_string(dir.join(".cursor/mcp.json")).unwrap();
    assert!(raw.contains("\"leantime-stage\""), "{raw}");
    assert!(raw.contains("LEANTIME_INSTANCE"), "{raw}");
    assert!(raw.contains("staging"), "{raw}");

    std::env::set_current_dir(cwd).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn setup_codex_project_scope_toml_env_table() {
    let dir = scratch("codex-proj");
    let _guard = set_home(&dir);
    config::write_url_for("staging", "https://staging.example").unwrap();
    config::write_key_for("staging", "lt_staging_key").unwrap();
    let cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let opts = SetupOptions {
        scope: Some("project"),
        instance: Some("staging"),
        name: None,
    };
    let r = harness::setup_harness("codex", &opts);
    assert!(r.ok, "{}", r.message);

    let raw = std::fs::read_to_string(dir.join(".codex/config.toml")).unwrap();
    assert!(raw.contains("[mcp_servers.leantime]"), "{raw}");
    assert!(raw.contains("[mcp_servers.leantime.env]"), "{raw}");
    assert!(raw.contains("LEANTIME_INSTANCE = \"staging\""), "{raw}");

    std::env::set_current_dir(cwd).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn setup_unknown_instance_is_rejected_with_profiles_listed() {
    let dir = scratch("bad-instance");
    let _guard = set_home(&dir);
    config::write_url("https://x.example").unwrap();

    let opts = SetupOptions {
        instance: Some("nope"),
        ..SetupOptions::default()
    };
    let r = harness::setup_harness("cursor", &opts);
    assert!(!r.ok);
    assert!(r.message.contains("not found"), "{}", r.message);
    assert!(r.message.contains("default"), "{}", r.message);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn setup_invalid_server_name_rejected() {
    let opts = SetupOptions {
        name: Some("bad name!"),
        ..SetupOptions::default()
    };
    let r = harness::setup_harness("cursor", &opts);
    assert!(!r.ok);
    assert!(r.message.contains("Invalid server name"), "{}", r.message);
}

#[test]
fn setup_claude_desktop_refuses_project_scope() {
    let opts = SetupOptions {
        scope: Some("project"),
        ..SetupOptions::default()
    };
    let r = harness::setup_harness("claude-desktop", &opts);
    assert!(!r.ok);
    assert!(r.message.contains("no project scope"), "{}", r.message);
}

#[test]
fn setup_claude_code_global_scope_prints_cli_hint_only() {
    let dir = scratch("cc-global");
    let _guard = set_home(&dir);
    let cwd = std::env::current_dir().unwrap();
    std::env::set_current_dir(&dir).unwrap();

    let opts = SetupOptions {
        scope: Some("global"),
        ..SetupOptions::default()
    };
    let r = harness::setup_harness("claude-code", &opts);
    assert!(r.ok, "{}", r.message);
    assert!(r.message.contains("claude mcp add"), "{}", r.message);
    assert!(
        !dir.join(".mcp.json").exists(),
        "global scope must not write .mcp.json"
    );

    std::env::set_current_dir(cwd).unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

use harness::SetupOptions;
