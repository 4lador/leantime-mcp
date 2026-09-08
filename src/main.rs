use leantmcp::{client, config, doctor, harness, tools};

use clap::{Arg, Command};
use serde_json::json;

#[tokio::main]
async fn main() {
    let matches = Command::new("leantmcp")
        .version(env!("CARGO_PKG_VERSION"))
        .about("Leantime MCP Server — Rust edition")
        .arg(
            Arg::new("instance")
                .long("instance")
                .short('i')
                .global(true)
                .value_name("PROFILE")
                .help("Target a specific keyring instance profile (e.g. --instance local)"),
        )
        .subcommand(Command::new("serve").about("Start the MCP server (default)"))
        .subcommand(
            Command::new("key")
                .about("API key management")
                .subcommand(Command::new("set").about("Store key (hidden prompt)"))
                .subcommand(Command::new("show").about("Show key (masked)"))
                .subcommand(Command::new("test").about("Validate key"))
                .subcommand(
                    Command::new("rotate")
                        .about("Mint new key")
                        .arg(Arg::new("name").long("name").help("New key name")),
                ),
        )
        .subcommand(
            Command::new("url")
                .about("Instance URL")
                .subcommand(
                    Command::new("set")
                        .about("Set URL")
                        .arg(Arg::new("url").index(1).help("The URL")),
                )
                .subcommand(Command::new("show").about("Show URL")),
        )
        .subcommand(
            Command::new("instance")
                .about("Instance profiles")
                .subcommand(
                    Command::new("add")
                        .about("Add profile")
                        .arg(Arg::new("name").index(1).required(true)),
                )
                .subcommand(Command::new("list").about("List profiles"))
                .subcommand(
                    Command::new("use")
                        .about("Set default")
                        .arg(Arg::new("name").index(1).required(true)),
                )
                .subcommand(
                    Command::new("remove")
                        .about("Remove profile")
                        .arg(Arg::new("name").index(1).required(true)),
                ),
        )
        .subcommand(
            Command::new("setup")
                .about("Configure harness")
                .subcommand(
                    Command::new("opencode")
                        .about("opencode (~/.opencode/opencode.json or ./opencode.json)")
                        .arg(Arg::new("scope").long("scope").value_parser(["global", "project"])
                            .help("Config scope (default: global)"))
                        .arg(Arg::new("instance").long("instance").value_name("PROFILE")
                            .help("Pin a keyring instance profile (bare command + LEANTIME_INSTANCE env)"))
                        .arg(Arg::new("name").long("name").value_name("SERVER")
                            .help("Server name in the config (default: leantime)")),
                )
                .subcommand(
                    Command::new("claude-code")
                        .about("Claude Code (./.mcp.json by default — its shared, committable form)")
                        .arg(Arg::new("scope").long("scope").value_parser(["global", "project"])
                            .help("Config scope (default: project)"))
                        .arg(Arg::new("instance").long("instance").value_name("PROFILE")
                            .help("Pin a keyring instance profile (bare command + LEANTIME_INSTANCE env)"))
                        .arg(Arg::new("name").long("name").value_name("SERVER")
                            .help("Server name in the config (default: leantime)")),
                )
                .subcommand(
                    Command::new("claude-desktop")
                        .about("Claude Desktop (global only — GUI app, no project concept)")
                        .arg(Arg::new("instance").long("instance").value_name("PROFILE")
                            .help("Pin a keyring instance profile (bare command + LEANTIME_INSTANCE env)"))
                        .arg(Arg::new("name").long("name").value_name("SERVER")
                            .help("Server name in the config (default: leantime)")),
                )
                .subcommand(
                    Command::new("cursor")
                        .about("Cursor (~/.cursor/mcp.json or ./.cursor/mcp.json)")
                        .arg(Arg::new("scope").long("scope").value_parser(["global", "project"])
                            .help("Config scope (default: global)"))
                        .arg(Arg::new("instance").long("instance").value_name("PROFILE")
                            .help("Pin a keyring instance profile (bare command + LEANTIME_INSTANCE env)"))
                        .arg(Arg::new("name").long("name").value_name("SERVER")
                            .help("Server name in the config (default: leantime)")),
                )
                .subcommand(
                    Command::new("codex")
                        .about("Codex (~/.codex/config.toml or ./.codex/config.toml)")
                        .arg(Arg::new("scope").long("scope").value_parser(["global", "project"])
                            .help("Config scope (default: global; project files load in trusted projects only)"))
                        .arg(Arg::new("instance").long("instance").value_name("PROFILE")
                            .help("Pin a keyring instance profile (bare command + LEANTIME_INSTANCE env)"))
                        .arg(Arg::new("name").long("name").value_name("SERVER")
                            .help("Server name in the config (default: leantime)")),
                ),
        )
        .subcommand(
            Command::new("backup")
                .about("Backup a project to a timestamped JSON file")
                .arg(Arg::new("project").long("project").short('p').value_name("ID")
                    .help("Project ID (default: the first project on the instance)"))
                .arg(Arg::new("full").long("full").action(clap::ArgAction::SetTrue)
                    .help("Include per-ticket comments (slower: 1 API call per ticket)"))
                .arg(Arg::new("list").long("list").action(clap::ArgAction::SetTrue)
                    .help("List existing backups")),
        )
        .subcommand(
            Command::new("restore")
                .about("Restore a backup file into a new project (does not merge into existing data)")
                .arg(Arg::new("file").index(1).required(true).value_name("FILE")
                    .help("Path to the backup JSON file"))
                .arg(Arg::new("confirm").long("confirm").action(clap::ArgAction::SetTrue)
                    .help("Execute the restore (without this flag, only shows a dry-run plan)")),
        )
        .subcommand(
            Command::new("tools")
                .about("Enable/disable MCP tools on this server")
                .subcommand(Command::new("list").about("List all tools with their status"))
                .subcommand(
                    Command::new("enable")
                        .about("Enable tools — by name, group (all/destructive/readonly/write)")
                        .arg(Arg::new("target").index(1).required(true).value_name("TOOL|GROUP")
                            .help("Tool name (e.g. leantime_delete_ticket) or group: all, destructive, readonly, write")),
                )
                .subcommand(
                    Command::new("disable")
                        .about("Disable tools — by name, group (all/destructive/readonly/write)")
                        .arg(Arg::new("target").index(1).required(true).value_name("TOOL|GROUP")
                            .help("Tool name (e.g. leantime_delete_ticket) or group: all, destructive, readonly, write")),
                ),
        )
        .subcommand(
            Command::new("doctor").about("Health check"),
        )
        .get_matches();

    // --instance PROFILE → set the env var once, before any async code runs.
    // All instance-aware code paths (keyring, tools, backup, doctor) read
    // LEANTIME_INSTANCE; this is the CLI-equivalent of the env prefix.
    if let Some(inst) = matches.get_one::<String>("instance") {
        std::env::set_var("LEANTIME_INSTANCE", inst);
    }

    match matches.subcommand() {
        Some(("serve", _)) | None => {
            serve().await;
        }
        Some(("key", sub)) => handle_key(sub).await,
        Some(("url", sub)) => handle_url(sub),
        Some(("instance", sub)) => handle_instance(sub),
        Some(("setup", sub)) => handle_setup(sub),
        Some(("backup", args)) => handle_backup(args).await,
        Some(("restore", args)) => handle_restore(args).await,
        Some(("tools", sub)) => handle_tools(sub),
        Some(("doctor", _)) => {
            doctor::run_doctor().await;
        }
        // Unreachable in practice (clap rejects unknown subcommands at parse
        // time) but required for match exhaustiveness.
        _ => {
            eprintln!("Unknown command.");
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// serve — MCP stdio server
// ---------------------------------------------------------------------------

async fn serve() {
    let env = match config::resolve_server_env() {
        Ok(e) => e,
        Err(msg) => {
            eprintln!("Error: {}", msg);
            std::process::exit(1);
        }
    };
    if let Some(w) = config::http_warning(&env.url) {
        eprintln!("{}", w);
    }

    // Idempotency journal base: the resolved instance profile dir, or the
    // keyring root when the server runs on pure env credentials.
    let idem_base = config::active_instance()
        .map(|n| config::instance_dir(&n))
        .unwrap_or_else(config::secret_dir);
    let client = std::sync::Arc::new(tokio::sync::Mutex::new(
        client::LeantimeClient::new(&env.url, &env.api_key).with_idempotency_dir(idem_base),
    ));
    let registry = tools::create_registry();

    // Load disabled tools and partition the registry. Disabled tools are
    // hidden from tools/list (zero tokens) but calling one returns an
    // actionable error instead of "unknown tool" (the agent might know the
    // name from context, documentation or pattern inference).
    let disabled: Vec<String> = config::read_disabled_tools().unwrap_or_default();
    let active_registry: Vec<&tools::Tool> = registry
        .iter()
        .filter(|t| !disabled.contains(&t.name.to_string()))
        .collect();
    if !disabled.is_empty() {
        eprintln!(
            "! {} tool(s) disabled on this server ({} enabled). Manage with: leantmcp tools list",
            disabled.len(),
            active_registry.len()
        );
    }

    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    let mut stdout = tokio::io::stdout();
    let mut reader = BufReader::new(tokio::io::stdin());
    const MAX_LINE: usize = 10 * 1024 * 1024; // 10 MB — reject pathological inputs
    let mut line = String::new();

    loop {
        line.clear();
        match reader.read_line(&mut line).await {
            Ok(0) => break, // EOF — harness closed the pipe
            Ok(_) => {}
            Err(e) => {
                eprintln!("stdin read error: {}", e);
                std::process::exit(1);
            }
        }
        // A blank line is a keep-alive, not EOF — ignore it.
        if line.trim().is_empty() {
            continue;
        }
        if line.len() > MAX_LINE {
            let resp = json!({ "jsonrpc": "2.0", "id": null,
                "error": { "code": -32600, "message": "Request too large (10 MB limit)" } });
            let _ = stdout.write_all(format!("{}\n", resp).as_bytes()).await;
            let _ = stdout.flush().await;
            continue;
        }

        let msg: serde_json::Value = match serde_json::from_str(line.trim_end()) {
            Ok(v) => v,
            Err(_) => {
                let resp = json!({ "jsonrpc": "2.0", "id": null,
                    "error": { "code": -32700, "message": "Parse error" } });
                let _ = stdout.write_all(format!("{}\n", resp).as_bytes()).await;
                let _ = stdout.flush().await;
                continue;
            }
        };

        let method = msg.get("method").and_then(|m| m.as_str()).unwrap_or("");
        let id = msg.get("id").cloned();
        let params = msg.get("params").cloned().unwrap_or(json!({}));

        let response = match method {
            "initialize" => {
                // MCP versions its protocol by revision DATE (2024-10-07 →
                // 2024-11-05 → 2025-03-26 → 2025-06-18). The revisions between
                // ours are additive/optional for a tools-only stdio server, so
                // we can safely claim both. Per spec: echo the client's version
                // when we support it, otherwise answer with the oldest we do.
                const SUPPORTED_PROTOCOL_VERSIONS: [&str; 2] =
                    ["2024-11-05", "2025-06-18"];
                let requested = params
                    .get("protocolVersion")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let negotiated = if SUPPORTED_PROTOCOL_VERSIONS
                    .contains(&requested)
                {
                    requested
                } else {
                    SUPPORTED_PROTOCOL_VERSIONS[0]
                };
                Some(json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": {
                        "protocolVersion": negotiated,
                        "capabilities": { "tools": { "listChanged": true } },
                        "serverInfo": { "name": "leantime-mcp", "version": env!("CARGO_PKG_VERSION") }
                    }
                }))
            }
            "notifications/initialized" => None,
            "tools/list" => {
                let tl: Vec<_> = active_registry
                    .iter()
                    .map(|t| {
                        json!({
                            "name": t.name, "description": t.description,
                            "inputSchema": t.schema, "annotations": t.annotations.to_json()
                        })
                    })
                    .collect();
                Some(json!({ "jsonrpc": "2.0", "id": id, "result": { "tools": tl } }))
            }
            "tools/call" => {
                let name = params.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                let result = match active_registry.iter().find(|t| t.name == name) {
                    Some(t) => (t.handler)(args, client.clone()).await,
                    None => {
                        // Option C: a disabled tool gets an actionable error,
                        // not a generic "unknown" (the agent might know the name).
                        if disabled.iter().any(|d| d == name) {
                            tools::error_result(&format!(
                                "Tool '{}' is disabled on this server. Enable it with: leantmcp tools enable {}",
                                name, name
                            ))
                        } else {
                            tools::error_result(&format!("Unknown tool: {}", name))
                        }
                    }
                };
                Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
            }
            _ => {
                id.as_ref().map(|rid| json!({ "jsonrpc": "2.0", "id": rid,
                        "error": { "code": -32601, "message": format!("Method not found: {}", method) } }))
            }
        };

        if let Some(resp) = response {
            let mut out = serde_json::to_string(&resp).unwrap_or_default();
            out.push('\n');
            let _ = stdout.write_all(out.as_bytes()).await;
            let _ = stdout.flush().await;
        }
    }
}

// ---------------------------------------------------------------------------
// CLI handlers
// ---------------------------------------------------------------------------

async fn handle_key(sub: &clap::ArgMatches) {
    match sub.subcommand() {
        Some(("set", _)) => {
            let key = std::env::var("LEANTIME_API_KEY")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| {
                    print!("API key (input hidden): ");
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                    rpassword::read_password().unwrap_or_default()
                });
            if key.trim().is_empty() {
                eprintln!("✗ No key.");
                std::process::exit(1);
            }
            match config::write_key(key.trim()) {
                Ok(p) => println!(
                    "✓ Key stored ({}) at {}",
                    config::mask_key(key.trim()),
                    p.display()
                ),
                Err(e) => {
                    eprintln!("✗ {}", e);
                    std::process::exit(1);
                }
            }
        }
        Some(("show", _)) => match config::read_key() {
            Some(k) => println!("key: {}", config::mask_key(&k)),
            None => {
                eprintln!("No key. Run: leantmcp key set");
                std::process::exit(1);
            }
        },
        Some(("test", _)) => {
            let key = config::read_key().unwrap_or_default();
            let url = config::read_url().unwrap_or_default();
            let mut c = client::LeantimeClient::new(&url, &key);
            match c.call("users.getAll", json!({})).await {
                Ok(r) => println!(
                    "✓ {} — {} users",
                    url,
                    r.as_array().map(|a| a.len()).unwrap_or(0)
                ),
                Err(e) => {
                    eprintln!("✗ {}", e);
                    std::process::exit(1);
                }
            }
        }
        Some(("rotate", args)) => {
            let name = args.get_one::<String>("name").cloned().unwrap_or_else(|| {
                format!("MCP-rotated-{}", chrono::Local::now().format("%Y-%m-%d"))
            });
            let key = config::read_key().unwrap_or_default();
            let url = config::read_url().unwrap_or_default();
            let mut c = client::LeantimeClient::new(&url, &key);
            match tools::key_rotate(&mut c, &key, &url, &name).await {
                Ok(msg) => println!("✓ {}", msg),
                Err(e) => {
                    eprintln!("✗ {}", e);
                    std::process::exit(1);
                }
            }
        }
        _ => {
            eprintln!("Usage: leantmcp key set|show|test|rotate");
            std::process::exit(1);
        }
    }
}

fn handle_url(sub: &clap::ArgMatches) {
    let _ = sub;
    match sub.subcommand() {
        Some(("set", args)) => {
            let url = args
                .get_one::<String>("url")
                .cloned()
                .or_else(|| {
                    std::env::var("LEANTIME_URL")
                        .ok()
                        .filter(|s| !s.trim().is_empty())
                })
                .unwrap_or_else(|| {
                    print!("Leantime instance URL: ");
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                    let mut s = String::new();
                    let _ = std::io::stdin().read_line(&mut s);
                    s.trim().to_string()
                });
            if url.is_empty() {
                eprintln!("✗ No URL.");
                std::process::exit(1);
            }
            if let Some(w) = config::http_warning(&url) {
                eprintln!("{}", w);
            }
            match config::write_url(&url) {
                Ok(p) => println!("✓ URL stored at {}", p.display()),
                Err(e) => {
                    eprintln!("✗ {}", e);
                    std::process::exit(1);
                }
            }
        }
        Some(("show", _)) => match config::read_url() {
            Some(u) => println!("{}", u),
            None => {
                eprintln!("No URL.");
                std::process::exit(1);
            }
        },
        _ => {
            eprintln!("Usage: leantmcp url set <url>|show");
            std::process::exit(1);
        }
    }
}

fn handle_instance(sub: &clap::ArgMatches) {
    let _ = sub;
    match sub.subcommand() {
        Some(("add", args)) => {
            let name = args
                .get_one::<String>("name")
                .map(|s| s.as_str())
                .unwrap_or("");
            if let Err(e) = config::validate_instance_name(name) {
                eprintln!("✗ {}", e);
                std::process::exit(1);
            }
            let url = std::env::var("LEANTIME_URL")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| {
                    print!("[{}] URL: ", name);
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                    let mut s = String::new();
                    let _ = std::io::stdin().read_line(&mut s);
                    s.trim().to_string()
                });
            let key = std::env::var("LEANTIME_API_KEY")
                .ok()
                .filter(|s| !s.trim().is_empty())
                .unwrap_or_else(|| {
                    print!("[{}] API key (hidden): ", name);
                    use std::io::Write;
                    let _ = std::io::stdout().flush();
                    rpassword::read_password().unwrap_or_default()
                });
            let url_write = config::write_url_for(name, &url);
            let key_write = config::write_key_for(name, &key);
            if let Err(e) = url_write {
                eprintln!("✗ {}", e);
                std::process::exit(1);
            }
            if let Err(e) = key_write {
                eprintln!("✗ {}", e);
                std::process::exit(1);
            }
            println!(
                "✓ Instance \"{}\" stored ({})",
                name,
                config::mask_key(&key)
            );
        }
        Some(("list", _)) => {
            let names = config::instance_names();
            let default = config::read_default_instance();
            let active = std::env::var("LEANTIME_INSTANCE")
                .ok()
                .or_else(|| default.clone());
            for name in &names {
                let masked = config::read_key_for(name)
                    .map(|k| config::mask_key(&k))
                    .unwrap_or("(no key)".into());
                let markers = {
                    let mut m = Vec::new();
                    if Some(name) == default.as_ref() {
                        m.push("← default");
                    }
                    if Some(name) == active.as_ref() {
                        m.push("← active");
                    }
                    m.join(", ")
                };
                println!("{:<14} {:<18} {}", name, masked, markers);
            }
        }
        Some(("use", args)) => {
            let name = args
                .get_one::<String>("name")
                .map(|s| s.as_str())
                .unwrap_or("");
            let names = config::instance_names();
            if !names.contains(&name.to_string()) {
                eprintln!(
                    "Instance \"{}\" not found. Available: {}",
                    name,
                    names.join(", ")
                );
                std::process::exit(1);
            }
            let _ = config::write_default_instance(name);
            println!("✓ Default is now \"{}\"", name);
        }
        Some(("remove", args)) => {
            let name = args
                .get_one::<String>("name")
                .map(|s| s.as_str())
                .unwrap_or("");
            let names = config::instance_names();
            if !names.contains(&name.to_string()) {
                eprintln!("Not found.");
                std::process::exit(1);
            }
            if config::read_default_instance().as_deref() == Some(name) {
                eprintln!("\"{}\" is default — switch first.", name);
                std::process::exit(1);
            }
            let _ = std::fs::remove_dir_all(config::instance_dir(name));
            println!("✓ Removed \"{}\"", name);
        }
        _ => {
            eprintln!("Usage: leantmcp instance add|list|use|remove");
            std::process::exit(1);
        }
    }
}

fn handle_setup(sub: &clap::ArgMatches) {
    let (harness_name, args) = sub.subcommand().unwrap_or(("opencode", sub)); // bare `leantmcp setup` = opencode global
    let opts = harness::SetupOptions {
        scope: args.get_one::<String>("scope").map(|s| s.as_str()),
        instance: args.get_one::<String>("instance").map(|s| s.as_str()),
        name: args.get_one::<String>("name").map(|s| s.as_str()),
    };
    match harness::setup_harness(harness_name, &opts) {
        harness::HarnessResult { ok: true, message } => {
            for line in message.lines() {
                println!("{line}");
            }
        }
        harness::HarnessResult { ok: false, message } => {
            eprintln!("{message}");
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// backup — dump a project to a timestamped JSON file
// ---------------------------------------------------------------------------

async fn handle_backup(args: &clap::ArgMatches) {
    if args.get_flag("list") {
        let backups = leantmcp::backup::list_backups();
        if backups.is_empty() {
            println!(
                "No backups found in {}",
                leantmcp::config::secret_dir().join("backups").display()
            );
            return;
        }
        println!("{:<50} {:>10}", "Backup file", "Size");
        for (path, size) in &backups {
            println!(
                "{:<50} {:>10}",
                path.file_name().unwrap_or_default().to_string_lossy(),
                format_size(*size)
            );
        }
        return;
    }

    let env = match config::resolve_server_env() {
        Ok(e) => e,
        Err(msg) => {
            eprintln!("Error: {}", msg);
            std::process::exit(1);
        }
    };
    let mut c = client::LeantimeClient::new(&env.url, &env.api_key);

    // Resolve project ID and name
    let (pid, pname) = match args.get_one::<String>("project") {
        Some(pid) => match c.call("projects.getProject", json!({"id": pid})).await {
            Ok(p) => (
                pid.clone(),
                p.get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("project")
                    .to_string(),
            ),
            Err(e) => {
                eprintln!("✗ Could not find project {}: {}", pid, e);
                std::process::exit(1);
            }
        },
        None => match c.call("Projects.getAll", json!({})).await {
            Ok(projects) => {
                let arr = projects.as_array().cloned().unwrap_or_default();
                if arr.is_empty() {
                    eprintln!("✗ No projects found on this instance.");
                    std::process::exit(1);
                }
                let first = &arr[0];
                let id = match &first["id"] {
                    serde_json::Value::String(s) => s.clone(),
                    o => o.to_string(),
                };
                let name = first
                    .get("name")
                    .and_then(|n| n.as_str())
                    .unwrap_or("project")
                    .to_string();
                (id, name)
            }
            Err(e) => {
                eprintln!("✗ {}", e);
                std::process::exit(1);
            }
        },
    };

    let full = args.get_flag("full");
    if full {
        eprintln!("→ Full backup (includes comments — this may take a while at the instance's rate limit)...");
    }

    match leantmcp::backup::backup_project(&mut c, &pid, &pname, full).await {
        Ok(r) => {
            println!("✓ {}", r.summary());
            if full {
                println!("  (full backup — comments included)");
            }
            for w in &r.warnings {
                println!("⚠ {}", w);
            }
        }
        Err(e) => {
            eprintln!("✗ {}", e);
            std::process::exit(1);
        }
    }
}

fn format_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    }
}

// ---------------------------------------------------------------------------
// tools — enable/disable MCP tools on this server
// ---------------------------------------------------------------------------

/// Resolve a user-provided target (tool name or group name) into tool names.
/// Groups: all (42), destructive (delete_* + bulk_*), readonly (list/get/find/
/// my_tasks/backup/project_context), write (create/update/add/log —
/// everything that mutates).
fn resolve_target(target: &str, registry: &[tools::Tool]) -> Result<Vec<String>, String> {
    let is_group = |t: &tools::Tool, group: &str| -> bool {
        match group {
            "destructive" => {
                t.name.starts_with("leantime_delete") || t.name.starts_with("leantime_bulk")
            }
            "readonly" => {
                t.name.starts_with("leantime_list")
                    || t.name.starts_with("leantime_get")
                    || t.name.starts_with("leantime_find")
                    || t.name == "leantime_my_tasks"
                    || t.name == "leantime_backup_project"
                    || t.name == "leantime_project_context"
            }
            "write" => {
                t.name.starts_with("leantime_create")
                    || t.name.starts_with("leantime_update")
                    || t.name.starts_with("leantime_add")
                    || t.name.starts_with("leantime_log")
            }
            "all" => true,
            _ => false,
        }
    };

    let names: Vec<String> = match target {
        "all" | "destructive" | "readonly" | "write" => registry
            .iter()
            .filter(|t| is_group(t, target))
            .map(|t| t.name.to_string())
            .collect(),
        // Individual tool name
        tool => {
            if registry.iter().any(|t| t.name == tool) {
                vec![tool.to_string()]
            } else {
                return Err(format!(
                    "Unknown tool or group: '{}'. Valid groups: all, destructive, readonly, write. Run 'leantmcp tools list' for individual tool names.",
                    tool
                ));
            }
        }
    };
    Ok(names)
}

fn handle_tools(sub: &clap::ArgMatches) {
    match sub.subcommand() {
        Some(("list", _)) => {
            let disabled = config::read_disabled_tools().unwrap_or_default();
            let registry = tools::create_registry();
            println!("{:<45} Status", "Tool");
            println!("{}", "-".repeat(55));
            for t in &registry {
                let status = if disabled.contains(&t.name.to_string()) {
                    "disabled"
                } else {
                    "enabled"
                };
                println!("{:<45} {}", t.name, status);
            }
            let enabled = registry.len() - disabled.len();
            println!("{}", "-".repeat(55));
            println!(
                "{} tools: {} enabled, {} disabled",
                registry.len(),
                enabled,
                disabled.len()
            );
        }
        Some(("enable", args)) | Some(("disable", args)) => {
            let is_enable = sub.subcommand_name() == Some("enable");
            let registry = tools::create_registry();
            let target = args
                .get_one::<String>("target")
                .map(|s| s.as_str())
                .unwrap_or("");

            let targets = match resolve_target(target, &registry) {
                Ok(t) => t,
                Err(e) => {
                    eprintln!("✗ {}", e);
                    std::process::exit(1);
                }
            };

            // Read-modify-write. "enable all" clears the entire list —
            // stale entries (renamed tools, manual edits) are pruned too.
            let mut disabled = config::read_disabled_tools().unwrap_or_default();
            if is_enable && target == "all" {
                disabled.clear();
            } else {
                for t in &targets {
                    if is_enable {
                        disabled.retain(|d| d != t);
                    } else if !disabled.contains(t) {
                        disabled.push(t.clone());
                    }
                }
            }
            disabled.sort();

            match config::write_disabled_tools(&disabled) {
                Ok(_) => {
                    let verb = if is_enable { "Enabled" } else { "Disabled" };
                    if targets.len() == 1 {
                        println!("✓ {} {}", verb, targets[0]);
                    } else {
                        println!("✓ {} {} tools", verb, targets.len());
                    }
                    let remaining = registry.len() - disabled.len();
                    println!(
                        "  {} of {} tools enabled. Restart your MCP session to apply.",
                        remaining,
                        registry.len()
                    );
                }
                Err(e) => {
                    eprintln!("✗ Could not persist: {}", e);
                    std::process::exit(1);
                }
            }
        }
        _ => {
            eprintln!("Usage: leantmcp tools list|enable|disable <tool|group>");
            std::process::exit(1);
        }
    }
}

// ---------------------------------------------------------------------------
// restore — rebuild a backup into a NEW project
// ---------------------------------------------------------------------------

async fn handle_restore(args: &clap::ArgMatches) {
    let file = args
        .get_one::<String>("file")
        .map(|s| s.as_str())
        .unwrap_or("");
    let confirm = args.get_flag("confirm");

    // Validate the backup structure
    let backup = match leantmcp::restore::validate_backup(std::path::Path::new(file)) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("✗ {}", e);
            std::process::exit(1);
        }
    };

    // Dry-run plan
    let plan = leantmcp::restore::plan_restore(&backup);
    println!("=== Dry-run ===");
    println!("{}", plan.summary());

    if !confirm {
        println!("\nRun with --confirm to execute.");
        return;
    }

    // Preflight: check instance is reachable
    let env = match config::resolve_server_env() {
        Ok(e) => e,
        Err(msg) => {
            eprintln!("Error: {}", msg);
            std::process::exit(1);
        }
    };
    let mut c = client::LeantimeClient::new(&env.url, &env.api_key);
    if let Err(e) = c.call("users.getAll", json!({})).await {
        eprintln!("✗ Instance unreachable: {}", e);
        std::process::exit(1);
    }

    println!("\n=== Restoring... ===");
    match leantmcp::restore::execute_restore(&mut c, &backup).await {
        Ok(r) => {
            println!("✓ {}", r.summary());
            for w in &r.warnings {
                println!("  ⚠ {}", w);
            }
        }
        Err(e) => {
            eprintln!("✗ {}", e);
            std::process::exit(1);
        }
    }
}
