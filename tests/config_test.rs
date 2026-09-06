use std::env;
use std::fs;
use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use leantmcp::config;
use leantmcp::config::{active_instance, validate_instance_name};

static HOME_LOCK: Mutex<()> = Mutex::new(());

fn lock() -> MutexGuard<'static, ()> {
    HOME_LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn temp_home() -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .subsec_nanos();
    let dir = std::env::temp_dir().join(format!("leantmcp-test-{}-{}", std::process::id(), nanos));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

/// (home, orig HOME, orig USERPROFILE, lock-guard). Keep `_guard` alive in
/// the test body — it serializes the env-mutating tests under parallel runs.
#[allow(clippy::type_complexity)]
fn setup() -> (
    PathBuf,
    Option<String>,
    Option<String>,
    MutexGuard<'static, ()>,
) {
    let guard = lock();
    let home = temp_home();
    let orig = env::var("HOME").ok();
    let orig_up = env::var("USERPROFILE").ok();
    env::set_var("HOME", &home);
    // On Windows, dirs::home_dir() reads USERPROFILE — redirect it too so the
    // tests isolate the keyring on every OS.
    #[cfg(windows)]
    if let Some(up) = home.to_str() {
        env::set_var("USERPROFILE", up);
    }
    env::remove_var("LEANTIME_INSTANCE");
    env::remove_var("LEANTIME_URL");
    env::remove_var("LEANTIME_API_KEY");
    (home, orig, orig_up, guard)
}

fn teardown(home: PathBuf, orig: Option<String>, orig_up: Option<String>) {
    match orig {
        Some(h) => env::set_var("HOME", h),
        None => env::remove_var("HOME"),
    }
    #[cfg(windows)]
    match orig_up {
        Some(up) => env::set_var("USERPROFILE", up),
        None => env::remove_var("USERPROFILE"),
    }
    #[cfg(not(windows))]
    let _ = orig_up;

    env::remove_var("LEANTIME_INSTANCE");
    env::remove_var("LEANTIME_URL");
    env::remove_var("LEANTIME_API_KEY");
    let _ = fs::remove_dir_all(&home);
}

#[test]
fn secret_path_default_instance() {
    let (home, orig, orig_up, _guard) = setup();
    config::write_default_instance("prod").unwrap();
    let path = config::secret_path(config::active_instance().as_deref());
    assert!(path.ends_with("instances/prod/api-key"), "got: {:?}", path);
    teardown(home, orig, orig_up);
}

#[test]
fn secret_path_env_instance() {
    let (home, orig, orig_up, _guard) = setup();
    env::set_var("LEANTIME_INSTANCE", "staging");
    let path = config::secret_path(config::active_instance().as_deref());
    assert!(
        path.ends_with("instances/staging/api-key"),
        "got: {:?}",
        path
    );
    teardown(home, orig, orig_up);
}

#[test]
fn key_round_trip_no_trailing_newline() {
    let (home, orig, orig_up, _guard) = setup();
    let key = "lt_test_key_123456789012345678901234567890";
    let path = config::write_key(key).unwrap();
    let raw = fs::read_to_string(&path).unwrap();
    assert_eq!(raw, key, "trailing newline detected");
    let read = config::read_key().unwrap();
    assert_eq!(read, key);
    teardown(home, orig, orig_up);
}

#[cfg(unix)]
#[test]
fn key_file_permissions_0600() {
    use std::os::unix::fs::PermissionsExt;
    let (home, orig, orig_up, _guard) = setup();
    let path = config::write_key("lt_test").unwrap();
    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600, "expected 0600, got {:o}", mode);
    teardown(home, orig, orig_up);
}

#[test]
fn url_round_trip_strips_trailing_slash() {
    let (home, orig, orig_up, _guard) = setup();
    config::write_url("https://leantime.test/").unwrap();
    let url = config::read_url().unwrap();
    assert_eq!(url, "https://leantime.test");
    teardown(home, orig, orig_up);
}

#[test]
fn instance_isolation() {
    let (home, orig, orig_up, _guard) = setup();
    config::write_default_instance("default").unwrap();
    config::write_url("https://default.test").unwrap();
    config::write_key("lt_default_key").unwrap();
    env::set_var("LEANTIME_INSTANCE", "prod");
    config::write_url("https://prod.test").unwrap();
    config::write_key("lt_prod_key").unwrap();
    assert_eq!(config::read_url().unwrap(), "https://prod.test");
    assert_eq!(config::read_key().unwrap(), "lt_prod_key");
    env::remove_var("LEANTIME_INSTANCE");
    assert_eq!(config::read_url().unwrap(), "https://default.test");
    assert_eq!(config::read_key().unwrap(), "lt_default_key");
    teardown(home, orig, orig_up);
}

#[test]
fn instance_names_lists_profiles() {
    let (home, orig, orig_up, _guard) = setup();
    for name in ["prod", "staging", "local"] {
        env::set_var("LEANTIME_INSTANCE", name);
        config::write_url(&format!("https://{}.test", name)).unwrap();
        config::write_key("lt_key").unwrap();
    }
    env::remove_var("LEANTIME_INSTANCE");
    let names = config::instance_names();
    assert_eq!(names, vec!["local", "prod", "staging"]);
    teardown(home, orig, orig_up);
}

#[test]
fn mask_key_formats_correctly() {
    let _guard = lock();
    assert_eq!(config::mask_key("lt_h13dyVu1uWRw5K3EZT79ze"), "lt_h13…79ze");
    assert_eq!(config::mask_key("short"), "sho…");
}

#[test]
fn resolve_server_env_from_keyring() {
    let (home, orig, orig_up, _guard) = setup();
    config::write_url("https://leantime.test").unwrap();
    config::write_key("lt_test_key").unwrap();
    let env = config::resolve_server_env().unwrap();
    assert_eq!(env.url, "https://leantime.test");
    assert_eq!(env.api_key, "lt_test_key");
    teardown(home, orig, orig_up);
}

#[test]
fn resolve_server_env_env_overrides_keyring() {
    let (home, orig, orig_up, _guard) = setup();
    config::write_url("https://keyring.test").unwrap();
    config::write_key("lt_keyring").unwrap();
    env::set_var("LEANTIME_URL", "https://env-override.test");
    env::set_var("LEANTIME_API_KEY", "lt_env_override");
    let env = config::resolve_server_env().unwrap();
    assert_eq!(env.url, "https://env-override.test");
    assert_eq!(env.api_key, "lt_env_override");
    teardown(home, orig, orig_up);
}

// ---------------------------------------------------------------- instance-name validation

#[test]
fn instance_name_rejects_traversal() {
    for bad in ["../../evil", "..", "a/../b", "/abs", "./x", "a/b", "a\\b"] {
        assert!(
            validate_instance_name(bad).is_err(),
            "{:?} must be rejected",
            bad
        );
    }
}

#[test]
fn instance_name_rejects_bad_start_and_charset() {
    for bad in ["", "-lead", ".lead", "_lead", "sp ace", "é"] {
        assert!(
            validate_instance_name(bad).is_err(),
            "{:?} must be rejected",
            bad
        );
    }
}

#[test]
fn instance_name_accepts_conservative_names() {
    for good in ["default", "prod", "local2", "staging-eu", "team_b", "v1.2"] {
        assert!(
            validate_instance_name(good).is_ok(),
            "{:?} must be accepted",
            good
        );
    }
    assert!(validate_instance_name(&"x".repeat(64)).is_ok());
    assert!(validate_instance_name(&"x".repeat(65)).is_err());
}

#[test]
fn active_instance_neutralizes_malicious_env() {
    let (home, orig, orig_up, _guard) = setup();
    env::set_var("LEANTIME_INSTANCE", "../../evil");
    // Must NOT resolve to a traversable instance. With no default file → None.
    assert!(active_instance().is_none());
    // With a default file → genuine fallback to the default (never the
    // malicious name), which is the loud-warning fallback semantics.
    fs::create_dir_all(config::secret_dir()).unwrap();
    fs::write(config::default_instance_file(), "prod").unwrap();
    assert_eq!(active_instance().as_deref(), Some("prod"));
    env::set_var("LEANTIME_INSTANCE", "ok-name");
    assert_eq!(active_instance().as_deref(), Some("ok-name"));
    teardown(home, orig, orig_up);
}
