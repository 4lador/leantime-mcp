# Harness setup

How to declare the `leantmcp` stdio server in every supported harness —
plus per-project setups and multiple instances. See the
[README](../README.md#quick-start) for the short version.

## The universal flow

```bash
leantmcp url set https://your-instance.leantime.io   # once
leantmcp key set                                      # once — hidden prompt
leantmcp setup <your-harness>                         # writes the config (see table)
                                                       # bare `leantmcp setup` = `setup opencode`
leantmcp doctor                                       # verify everything end-to-end
```

```bash
leantmcp setup <harness> [--scope global|project] [--instance PROFILE] [--name SERVER]
```

Options:
- `--scope global|project` — where the config lives: machine-wide or committed to the repo root (default per harness, see table). Project-scoped files are designed to be committed to Git.
- `--instance PROFILE` — pin a keyring instance profile: the config gets a bare command + `LEANTIME_INSTANCE` env block, and the binary resolves that profile's credentials from the keyring at startup. Without it, the active/default instance is used.
- `--name SERVER` — the server key in the config (default: `leantime`) — for multiple instances side by side (e.g. `leantime` + `leantime-staging`).

## Harness reference

| Harness | Default scope | Project file | Global file |
|---|---|---|---|
| opencode | global | `./opencode.json` (merges with global; `{file:}` pointers, git-safe) | `~/.opencode/opencode.json` |
| Claude Code | **project** (its committable form) | `./.mcp.json` | user scope via the `claude` CLI (printed for you) |
| Claude Desktop | global (GUI app — no project concept) | — | `claude_desktop_config.json` (path per OS) |
| Cursor | global | `./.cursor/mcp.json` (merge) | `~/.cursor/mcp.json` (merge) |
| Codex | global | `./.codex/config.toml` (trusted projects only) | `~/.codex/config.toml` |

Generated configs are **bare commands containing no credentials**: the binary
resolves credentials from `~/.config/leantime/instances/` at startup —
project-scoped files contain no credentials and can be committed.

The config a harness ends up with is simply:

```json
{
  "mcpServers": {
    "leantime": {
      "command": "/absolute/path/to/leantmcp"
    }
  }
}
```

## Per-project setup

```bash
cd my-repo
leantmcp setup cursor --scope project --instance staging --name leantime-staging
git add .cursor/mcp.json && git commit
```

Everyone on the team who clones the repo gets the server declaration for
free; each member's own keyring provides their credentials. No secrets in
the file.

## Any other MCP client

`leantmcp` is a standard stdio MCP server: point your client at the binary,
no environment variables required (the keyring provides them).
`LEANTIME_URL` / `LEANTIME_API_KEY` environment variables remain available
as per-run overrides. Protocol revisions `2024-11-05` and `2025-06-18` are
supported and negotiated at handshake (the client's version is echoed when
known). All 42 tools carry MCP annotations (`readOnlyHint`,
`destructiveHint`, `idempotentHint`, `openWorldHint`) so clients can group,
gate and cache them intelligently.

## Multiple instances

All credentials live in named profiles under `~/.config/leantime/instances/<name>/`.
A `default` file names the default instance (used when no override is set).

```bash
leantmcp instance add staging      # prompts for URL + key (hidden)
leantmcp instance list             # profiles + masked keys + default/active markers
leantmcp instance use staging      # set staging as the default instance
leantmcp instance remove staging   # refuses to remove the current default
```

Any command targets a profile via `--instance` (recommended) or
`LEANTIME_INSTANCE`:

```bash
leantmcp key rotate --instance staging   # rotates staging's key
# or equivalently:
LEANTIME_INSTANCE=staging leantmcp key rotate
```

In a harness config, declare one server per instance — no credentials in
the config:

```json
{
  "mcpServers": {
    "leantime":       { "command": "/path/to/leantmcp" },
    "leantime-stage": { "command": "/path/to/leantmcp", "env": { "LEANTIME_INSTANCE": "staging" } }
  }
}
```

Resolution order: `LEANTIME_URL`/`LEANTIME_API_KEY` env (explicit override)
→ `LEANTIME_INSTANCE` profile → the `default` file.
