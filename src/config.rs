use std::io::Write as _;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Paths
// ---------------------------------------------------------------------------

/// Home directory of the current user.
pub fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

/// Root of the keyring: `~/.config/leantime/`.
pub fn secret_dir() -> PathBuf {
    home_dir().join(".config").join("leantime")
}

/// Directory holding the named instance profiles.
pub fn instances_dir() -> PathBuf {
    secret_dir().join("instances")
}

/// File naming the default instance profile.
pub fn default_instance_file() -> PathBuf {
    secret_dir().join("default")
}

/// Directory of one instance profile. The name MUST be validated first
/// (`validate_instance_name`) — it is joined into a filesystem path.
pub fn instance_dir(name: &str) -> PathBuf {
    instances_dir().join(name)
}

/// Path of the API key file for an instance (None = legacy flat layout).
pub fn secret_path(instance: Option<&str>) -> PathBuf {
    match instance {
        Some(n) => instance_dir(n).join("api-key"),
        None => secret_dir().join("api-key"),
    }
}

/// Path of the instance URL file for an instance (None = legacy flat layout).
pub fn instance_url_path(instance: Option<&str>) -> PathBuf {
    match instance {
        Some(n) => instance_dir(n).join("instance-url"),
        None => secret_dir().join("instance-url"),
    }
}

// ---------------------------------------------------------------------------
// Instance name validation (path-traversal guard)
// ---------------------------------------------------------------------------

/// Instance profile names become directory names under ~/.config/leantime/instances/.
/// This rejects path traversal (`..`), separators, absolute names and anything
/// that is not a conservative filename: `^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$`.
pub fn validate_instance_name(name: &str) -> Result<(), String> {
    let n = name.chars().count();
    if n == 0 || n > 64 {
        return Err(format!(
            "Invalid instance name {:?}: must be 1-64 characters",
            name
        ));
    }
    let mut chars = name.chars();
    let first = chars.next().unwrap();
    if !first.is_ascii_alphanumeric() {
        return Err(format!(
            "Invalid instance name {:?}: must start with a letter or digit",
            name
        ));
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
    {
        return Err(format!(
            "Invalid instance name {:?}: allowed characters are letters, digits, '.', '_' and '-'",
            name
        ));
    }
    if name.contains("..") {
        return Err(format!(
            "Invalid instance name {:?}: '..' is not allowed",
            name
        ));
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Active instance resolution
// ---------------------------------------------------------------------------

/// LEANTIME_INSTANCE env → default file → None. An invalid env name is
/// rejected loudly (a typo'd profile silently operating on another
/// instance's credentials is exactly the surprise this must not allow) and
/// resolution genuinely falls back to the default file. The warning prints
/// once per process even though resolution may be queried several times.
pub fn active_instance() -> Option<String> {
    if let Ok(name) = std::env::var("LEANTIME_INSTANCE") {
        let trimmed = name.trim().to_string();
        if !trimmed.is_empty() {
            return match validate_instance_name(&trimmed) {
                Ok(()) => Some(trimmed),
                Err(e) => {
                    warn_invalid_instance_once(&format!(
                        "LEANTIME_INSTANCE rejected ({}) — using the default instance instead.",
                        e
                    ));
                    read_default_instance()
                }
            };
        }
    }
    read_default_instance()
}

/// Print the invalid-instance warning at most once per process (serve
/// resolves credentials once, but several helpers query the active instance
/// during startup — the user doesn't need the same line twice).
fn warn_invalid_instance_once(msg: &str) {
    static WARNED: std::sync::OnceLock<()> = std::sync::OnceLock::new();
    if WARNED.set(()).is_ok() {
        eprintln!("! WARNING: {}", msg);
    }
}

/// The instance named in the `default` file (validated).
pub fn read_default_instance() -> Option<String> {
    let name = std::fs::read_to_string(default_instance_file())
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())?;
    validate_instance_name(&name).ok()?;
    Some(name)
}

/// Write the `default` file (mode 0600).
pub fn write_default_instance(name: &str) -> std::io::Result<()> {
    validate_instance_name(name)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    ensure_private_dirs(&secret_dir())?;
    write_private_file(&default_instance_file(), name)
}

/// Resolve the instance for write ops, auto-creating "default" on first use.
fn active_instance_for_write() -> Result<String, std::io::Error> {
    match active_instance() {
        Some(n) => Ok(n),
        None => {
            write_default_instance("default")?;
            Ok("default".to_string())
        }
    }
}

// ---------------------------------------------------------------------------
// Private file helpers (create-with-mode: no 0644 window, ever)
// ---------------------------------------------------------------------------

/// Write a file with mode 0600 from the very first byte (no write-then-chmod
/// race). On non-Unix, plain write (Windows ACLs apply). Shared with the
/// harness writers (their configs can hold a plaintext key fallback).
pub(crate) fn write_private_file(path: &Path, content: &str) -> std::io::Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        use std::os::unix::fs::PermissionsExt;
        let mut f = std::fs::OpenOptions::new()
            .write(true)
            .create(true)
            .truncate(true)
            .mode(0o600)
            .open(path)?;
        f.write_all(content.as_bytes())?;
        // Belt and braces: enforce 0600 even if the file pre-existed with wider perms.
        let _ = std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600));
    }
    #[cfg(not(unix))]
    {
        std::fs::write(path, content)?;
    }
    Ok(())
}

/// Create the keyring directory chain and tighten it to 0700 from
/// `~/.config/leantime` down (never touches `~/.config` itself).
fn ensure_private_dirs(dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let base = secret_dir();
        let mut cur = Some(dir);
        while let Some(d) = cur {
            if d == base {
                let _ = std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700));
                break;
            }
            if d.starts_with(&base) {
                let _ = std::fs::set_permissions(d, std::fs::Permissions::from_mode(0o700));
            }
            cur = d.parent();
        }
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Read/Write credentials
// ---------------------------------------------------------------------------

/// Read the API key of a specific instance (trimmed, never empty).
/// The name must be valid (see [`validate_instance_name`]).
pub fn read_key_for(instance: &str) -> Option<String> {
    validate_instance_name(instance).ok()?;
    std::fs::read_to_string(secret_path(Some(instance)))
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Store the API key of a specific instance (mode 0600, dirs 0700).
pub fn write_key_for(instance: &str, key: &str) -> std::io::Result<PathBuf> {
    validate_instance_name(instance)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let path = secret_path(Some(instance));
    ensure_private_dirs(path.parent().unwrap_or(&instances_dir()))?;
    write_private_file(&path, key.trim())?;
    Ok(path)
}

/// Read the URL of a specific instance (trailing slashes stripped).
pub fn read_url_for(instance: &str) -> Option<String> {
    validate_instance_name(instance).ok()?;
    std::fs::read_to_string(instance_url_path(Some(instance)))
        .ok()
        .map(|s| s.trim().trim_end_matches('/').to_string())
        .filter(|s| !s.is_empty())
}

/// Store the URL of a specific instance (mode 0600 — it identifies your server).
pub fn write_url_for(instance: &str, url: &str) -> std::io::Result<PathBuf> {
    validate_instance_name(instance)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
    let path = instance_url_path(Some(instance));
    ensure_private_dirs(path.parent().unwrap_or(&instances_dir()))?;
    write_private_file(&path, url.trim().trim_end_matches('/'))?;
    Ok(path)
}

/// Read the API key of the active instance (env override → default file).
pub fn read_key() -> Option<String> {
    read_key_for(&active_instance()?)
}

/// Store the API key of the active instance (mode 0600, dirs 0700).
pub fn write_key(key: &str) -> std::io::Result<PathBuf> {
    write_key_for(&active_instance_for_write()?, key)
}

/// Read the URL of the active instance (trailing slashes stripped).
pub fn read_url() -> Option<String> {
    read_url_for(&active_instance()?)
}

/// Store the URL of the active instance (mode 0600 — it identifies your server).
pub fn write_url(url: &str) -> std::io::Result<PathBuf> {
    write_url_for(&active_instance_for_write()?, url)
}

// ---------------------------------------------------------------------------
// Instance profiles
// ---------------------------------------------------------------------------

/// Sorted names of the existing instance profiles.
pub fn instance_names() -> Vec<String> {
    let mut names = Vec::new();
    if let Ok(entries) = std::fs::read_dir(instances_dir()) {
        for entry in entries.flatten() {
            if entry.file_type().map(|t| t.is_dir()).unwrap_or(false) {
                if let Some(name) = entry.file_name().to_str() {
                    names.push(name.to_string());
                }
            }
        }
    }
    names.sort();
    names
}

/// Masked display of an API key. Char-based: safe on any UTF-8 input.
/// Parity with the TS edition (first 3 chars of short keys are always the
/// non-secret `lt_` prefix of Leantime keys).
pub fn mask_key(key: &str) -> String {
    let n = key.chars().count();
    if n <= 10 {
        let head: String = key.chars().take(3).collect();
        format!("{}…", head)
    } else {
        let head: String = key.chars().take(6).collect();
        let tail: String = key.chars().skip(n - 4).collect();
        format!("{}…{}", head, tail)
    }
}

// ---------------------------------------------------------------------------
// HTTP scheme warning
// ---------------------------------------------------------------------------

/// Returns a warning when the URL would send the API key in cleartext
/// (plain http to a non-local host). Advisory only — the user decides.
pub fn http_warning(url: &str) -> Option<String> {
    let lower = url.to_ascii_lowercase();
    let rest = lower.strip_prefix("http://")?;
    if rest.is_empty() {
        return None;
    }
    let host = rest.split(['/', ':']).next().unwrap_or("");
    let local = matches!(host, "localhost" | "127.0.0.1" | "::1" | "[::1]");
    if local {
        None
    } else {
        Some(format!(
            "! WARNING: {} is plain HTTP — the API key travels unencrypted. Use HTTPS unless this is a trusted local network.",
            url
        ))
    }
}

// ---------------------------------------------------------------------------
// Tool enable/disable state (per instance profile)
// ---------------------------------------------------------------------------

/// Path of the disabled-tools list for an instance.
fn tools_config_path(instance: &str) -> PathBuf {
    instance_dir(instance).join("tools.json")
}

/// Read the set of disabled tool names for the active instance.
pub fn read_disabled_tools() -> std::io::Result<Vec<String>> {
    let inst = active_instance_for_write()?;
    let path = tools_config_path(&inst);
    let content = std::fs::read_to_string(path).unwrap_or_default();
    serde_json::from_str::<Vec<String>>(&content)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))
}

/// Write the set of disabled tool names for the active instance (mode 0600).
pub fn write_disabled_tools(tools: &[String]) -> std::io::Result<()> {
    let inst = active_instance_for_write()?;
    let path = tools_config_path(&inst);
    ensure_private_dirs(path.parent().unwrap_or(&instances_dir()))?;
    let content = serde_json::to_string_pretty(tools).unwrap_or_default();
    write_private_file(&path, &content)
}

// ---------------------------------------------------------------------------
// File mode (for doctor)
// ---------------------------------------------------------------------------

/// Octal mode string (unix) or None (Windows uses ACLs).
pub fn file_mode(path: &Path) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        std::fs::metadata(path)
            .ok()
            .map(|m| format!("{:03o}", m.mode() & 0o777))
    }
    #[cfg(not(unix))]
    {
        None // Windows uses ACLs
    }
}

// ---------------------------------------------------------------------------
// Server env resolution (for serve)
// ---------------------------------------------------------------------------

/// Credentials resolved for a server spawn (env override → keyring).
pub struct ServerEnv {
    /// Instance URL (scheme + host, no trailing slash).
    pub url: String,
    /// The Leantime API key.
    pub api_key: String,
}

/// Resolution order: environment (explicit override) → active instance keyring.
/// The active instance is resolved ONCE and shared for both credentials (an
/// invalid-LEANTIME_INSTANCE warning also prints once per process).
pub fn resolve_server_env() -> Result<ServerEnv, String> {
    let env_url = std::env::var("LEANTIME_URL")
        .ok()
        .filter(|s| !s.trim().is_empty());

    let env_key = std::env::var("LEANTIME_API_KEY")
        .ok()
        .filter(|s| !s.trim().is_empty());

    let inst = active_instance();
    let url = env_url.or_else(|| inst.as_deref().and_then(read_url_for));
    let api_key = env_key.or_else(|| inst.as_deref().and_then(read_key_for));

    match (url, api_key) {
        (Some(url), Some(api_key)) => Ok(ServerEnv { url, api_key }),
        _ => Err(
            "Missing LEANTIME_URL and/or LEANTIME_API_KEY — run: leantmcp key set && leantmcp url set <url>"
                .to_string(),
        ),
    }
}
