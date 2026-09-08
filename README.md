# leantime-mcp

- MCP Registry name: `mcp-name: io.github.4lador/leantime-mcp`

<p align="center">
  <img src="https://raw.githubusercontent.com/4lador/leantime-mcp/main/docs/hero.png" alt="leantime-mcp" width="600">
</p>

[![CI](https://github.com/4lador/leantime-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/4lador/leantime-mcp/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A [Model Context Protocol](https://modelcontextprotocol.io/) server for [Leantime](https://leantime.io/), enabling LLM-powered tools (opencode, Claude Code, Claude Desktop, Cursor, Codex, or any MCP client) to interact with your Leantime projects.

**~3 MB self-contained binary** (rustls — no system OpenSSL), **1.2 ms median startup** (measured over 100 spawns), **~5 MB idle memory** (VmRSS after handshake).

**Documentation**: [Migrating from v1.x](#migrating-from-v1x) · [Key management](#key-management) · [Safety](#safety-destructive-operations) · [Available MCP Tools](#available-mcp-tools) · [Development](#development) · [CHANGELOG](CHANGELOG.md) · [CONTRIBUTING](CONTRIBUTING.md) · [SECURITY](SECURITY.md) · [LICENSE](LICENSE)

## What's new in v2.3.1

- **Dry-run by default (agent guidance, three-tier policy)**: mutation tool descriptions now instruct agents to execute directly when values are explicit or resolve unambiguously, to dry-run and confirm when they interpreted or chose values themselves, and to always dry-run bulk batches. Agents validate their own inferences without turning every trivial change into a permission loop.

## What's new in v2.3.0

- **`dryRun: true`** on all mutation tools — validate without executing: same checks, `from → to` diffs on updates, per-item previews on bulk, zero API writes. See [Dry runs](#dry-runs).

## What's new in v2.2.0

- **`leantime_project_context`** — the full picture of a project in one call: progress, health counters (blocked / overdue / unassigned / open), current-or-upcoming sprint, milestone progress, ticket summary and recently modified items. Replaces 5-6 agent round-trips with a single response capped under 4 KB, timestamped `generatedAt`.

## What's new in v2.1.0

- **`leantmcp restore`** — rebuilds a backup into a NEW project (never merges with existing data): topological ordering (parents before subtasks), full ID remapping (milestones, sprints, tickets, comments), interactive status resolution and post-restore verification. See [Backup & recovery](#backup--recovery).

## What's new in v2.0.0

v2.0.0 is a complete rewrite in Rust (v1.x was TypeScript/Deno — see [Migrating from v1.x](#migrating-from-v1x)). Beyond the language change:

- **Backup & recovery**: `leantmcp backup` and the `leantime_backup_project` MCP tool dump a project to a timestamped JSON file — agents can trigger a cheap backup before bulk modifications
- **Tool management**: `leantmcp tools enable|disable` lets you hide tools from agents entirely (zero context-window cost), per instance profile, with groups (`destructive`, `readonly`, `write`, `all`)
- **MCP tool annotations**: all 42 tools carry `readOnlyHint`/`destructiveHint`/`idempotentHint`/`openWorldHint` so clients can group, gate and cache them intelligently
- **`--instance` flag**: `leantmcp backup --instance prod` — target any keyring profile on any command, no env prefix needed
- **24h cap on `leantime_log_time`**: entries over 24 hours are rejected (a timesheet line targets ONE date — beyond 24h is impossible data, typically a hallucinated value)
- **Protocol negotiation**: supports MCP revisions `2024-11-05` and `2025-06-18`, echoes the client's version when known
- **Universal harness setup**: `setup <harness> [--scope global|project] [--instance PROFILE] [--name SERVER]` for opencode, Claude Code, Claude Desktop, Cursor and Codex — project-scoped configs are bare-command, git-committable, zero secrets
- **Security hardening**: path-traversal guard on instance names, DoS caps on all server-controlled inputs (Retry-After ≤60s, responses ≤64MB, markdown ≤1MB, stdin ≤10MB), files created 0600 from the first byte, UTF-8-safe string handling throughout

## Migrating from v1.x

The keyring, credentials and harness configs are **fully compatible** — the v2 binary is a drop-in replacement:

- **Your keyring works as-is**: `~/.config/leantime/instances/<name>/` is unchanged since v1.7.0. Both profiles and the `default` file resolve identically.
- **Your install URL still works**: `curl -fsSL https://raw.githubusercontent.com/4lador/leantime-mcp/main/install.sh | sh` delivers the v2 binary at the same location (`~/.local/bin/leantmcp`). The installer verifies the SHA-256 checksum as before.
- **Your harness configs need no change**: they point to `~/.local/bin/leantmcp` (bare command) — replacing the binary replaces the server. Restart your MCP session to pick up the new version.
- **The v1.x source code is preserved** on the [`frozen-legacy-ts`](https://github.com/4lador/leantime-mcp/tree/frozen-legacy-ts) branch. It will not receive updates or security fixes.

Why the rewrite? The v1.x binary embedded the Deno/V8 runtime:

| | v1.x (Deno/TypeScript) | v2.0.0 (Rust) |
|---|---|---|
| Binary size | ~100 MB | **~3 MB** |
| Startup | ~200 ms | **1.2 ms** (median, n=100) |
| Memory (idle) | ~50 MB | **~5 MB** (VmRSS) |
| Runtime deps | Deno/V8 embedded | **none (rustls)** |

## Features

- Full project-management coverage: projects, clients, tickets, subtasks, milestones, sprints, comments, time tracking and **bulk operations** (42 tools)
- **`leantime_project_context`**: a composite first-call tool that hydrates full project context (progress, health, sprint, milestones, activity) in one round-trip — agents start reasoning instead of paging through lists
- **Dry runs**: every mutation tool accepts `dryRun: true` — same validations, `from → to` diffs on updates, per-item previews on bulk, zero API writes. Agents are instructed to dry-run first on conversational-intent updates and inferred creates, and always on bulk batches
- **Backup & recovery**: `leantmcp backup [--full]` snapshots a project (plus `leantmcp restore` to rebuild it into a new project), and `leantime_backup_project` lets agents trigger a cheap backup before bulk modifications
- **Multiple Leantime instances**: named profiles (`instance add`, `instance use`), one server per instance in any harness — still zero secrets
- **Automatic 429 retry**: adaptive backoff that discovers the instance's rate limit from response headers — agents never handle rate limiting
- Deterministic Markdown → rich HTML descriptions and comments: formatting is applied server-side, so everything is always properly rendered in Leantime's editor
- Mandatory assignment on ticket/milestone creation: the server rejects calls that don't assign a user (or explicitly opt out)
- Destructive operations gated behind explicit confirmation (`LEANTIME_MCP_DESTRUCTIVE_POLICY`)
- v3.7.x API quirks handled server-side: scoping filters, session-less API keys, id mangling, array-wrapped ids

## Install

**Linux / macOS (x86_64, aarch64):**

```bash
curl -fsSL https://raw.githubusercontent.com/4lador/leantime-mcp/main/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/4lador/leantime-mcp/main/install.ps1 | iex
```

Both installers verify the published SHA-256 checksum before installing and abort on mismatch. Release binaries cover **all 5 targets** — Linux (x86_64, aarch64), Windows x86_64 and macOS (Intel, ARM) — on the [releases page](https://github.com/4lador/leantime-mcp/releases).

**Via cargo** (requires the Rust toolchain):

```bash
cargo install leantime-mcp   # installs the leantmcp binary to ~/.cargo/bin
```

**From source:**

```bash
cargo build --release   # → target/release/leantmcp
```

## How it works

- `leantmcp` is a **stdio MCP server**: your harness (opencode, Claude Code, Claude Desktop, Cursor, Codex…) spawns it at session start and stops it at session end. No daemon, no port, nothing runs in the background.
- **Credentials never live in harness configs.** The binary resolves them at startup: environment variables first (per-run override), then the keyring — `~/.config/leantime/instances/<name>/` (`api-key` mode 0600, `instance-url`). One keyring, shared by every harness you use — .
- That fallback is what makes every config below a **bare command with no secrets**: there is nothing sensitive to put in a config file in the first place.

## Setup

The universal flow, for every harness:

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

| Harness | Default scope | Project file | Global file |
|---|---|---|---|
| opencode | global | `./opencode.json` (merges with global; `{file:}` pointers, git-safe) | `~/.opencode/opencode.json` |
| Claude Code | **project** (its committable form) | `./.mcp.json` | user scope via the `claude` CLI (printed for you) |
| Claude Desktop | global (GUI app — no project concept) | — | `claude_desktop_config.json` (path per OS) |
| Cursor | global | `./.cursor/mcp.json` (merge) | `~/.cursor/mcp.json` (merge) |
| Codex | global | `./.codex/config.toml` (trusted projects only) | `~/.codex/config.toml` |

All configs are **bare commands with no secrets**: the binary resolves credentials from `~/.config/leantime/instances/` at startup — there is nothing sensitive to put in a config file, which is what makes project-scoped files safe to commit.

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

### Per-project setup

```bash
cd my-repo
leantmcp setup cursor --scope project --instance staging --name leantime-staging
git add .cursor/mcp.json && git commit
```

Everyone on the team who clones the repo gets the server declaration for free; each member's own keyring provides their credentials. No secrets in the file.

### Any other MCP client

`leantmcp` is a standard stdio MCP server: point your client at the binary, no environment variables required (the keyring provides them). `LEANTIME_URL` / `LEANTIME_API_KEY` environment variables remain available as per-run overrides. Protocol revisions `2024-11-05` and `2025-06-18` are supported and negotiated at handshake (the client's version is echoed when known). All 42 tools carry MCP annotations (`readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint`) so clients can group, gate and cache them intelligently.

### Multiple instances

All credentials live in named profiles under `~/.config/leantime/instances/<name>/`. A `default` file names the default instance (used when no override is set).

```bash
leantmcp instance add staging      # prompts for URL + key (hidden)
leantmcp instance list             # profiles + masked keys + default/active markers
leantmcp instance use staging      # set staging as the default instance
leantmcp instance remove staging   # refuses to remove the current default
```

Any command targets a profile via `--instance` (recommended) or `LEANTIME_INSTANCE`:

```bash
leantmcp key rotate --instance staging   # rotates staging's key
# or equivalently:
LEANTIME_INSTANCE=staging leantmcp key rotate
```

In a harness config, declare one server per instance — still zero secrets:

```json
{
  "mcpServers": {
    "leantime":       { "command": "/path/to/leantmcp" },
    "leantime-stage": { "command": "/path/to/leantmcp", "env": { "LEANTIME_INSTANCE": "staging" } }
  }
}
```

Resolution order: `LEANTIME_URL`/`LEANTIME_API_KEY` env (explicit override) → `LEANTIME_INSTANCE` profile → the `default` file.

## Rich text (Markdown)

All rich-text fields — **ticket and milestone descriptions, comments, and project details** — are written in **Markdown** and converted **deterministically server-side** to the rich HTML subset supported by Leantime's editor.

| Markdown | Result |
|----------|--------|
| `#` … `####` | Headings (levels 5+ clamp to h4) |
| blank-line separated text | Paragraphs (single newlines become line breaks) |
| `- item` / `1. item` | Unordered / ordered lists (nesting supported) |
| `- [ ] x` / `- [x] x` | Interactive checklists |
| `**bold**`, `*italic*`, `~~strike~~` | Emphasis |
| `` `code` `` and fenced ` ``` ` blocks | Inline and block code |
| `[label](https://…)` | Links (http/https/mailto only) |
| `> quote` | Blockquote |
| `---` | Horizontal rule |

Raw HTML in descriptions is always escaped — it renders as literal text, never as markup.

## Assignment policy

Creating a ticket or milestone requires an explicit assignment decision:

- either `editorId` — the user the ticket is assigned to (validated against the real user list; call `leantime_list_users` or `leantime_list_project_users` to get candidates),
- or `unassigned: true` — only when the user explicitly asked to leave it unassigned.

If neither is provided, the server rejects the call with an error instructing the agent to ask the user first. Updates only validate `editorId` when you actually change the assignment.

## Safety: destructive operations

This software is provided without warranty (MIT). It drives Leantime with your API key on your behalf — **back up your Leantime data** before letting agents operate on it.

The delete tools (`leantime_delete_ticket`, `leantime_delete_milestone`, `leantime_delete_comment`, `leantime_delete_timesheet_entry`) are gated behind an explicit confirmation:

- by default (`ask`), they refuse to run unless called with `confirm: true` — the tool error instructs the agent to obtain the user's explicit approval first and to retry;
- `LEANTIME_MCP_DESTRUCTIVE_POLICY=deny` refuses deletions outright, even with `confirm: true` (emergency stop);
- `LEANTIME_MCP_DESTRUCTIVE_POLICY=allow` skips the confirmation (CI/scripting).

Project hiding/deletion is intentionally not exposed.

### Dry runs

The mutation tools (`leantime_create_ticket`, `leantime_update_ticket`, `leantime_create_milestone`, `leantime_update_milestone`, `leantime_bulk_create_tickets`, `leantime_bulk_update_tickets`, `leantime_log_time`) accept `dryRun: true`: every validation runs (assignment, editorId existence, value constraints), update tools resolve `from → to` values against the current entity (with status labels and a warning when a field already holds the target value), bulk tools return per-item previews — and **no mutation is ever sent to Leantime**. A failed validation is returned as `valid: false` with the errors, not as a tool error. Natural chain: `leantime_backup_project` → `dryRun: true` → execute.

**Agent guidance is built into the tool descriptions** (since v2.3.1), on a three-tier policy:

1. **Direct execution** when every value was explicitly given or resolves unambiguously — "passe le ticket #535 en Terminé" maps to one status, the agent writes and reports. No permission-asking loop for mechanical changes.
2. **Dry-run then confirm** when the agent interpreted the request or chose values itself — "configure ce projet pour du dev agile" involves many agent-decided values, so it shows the diff/proposal first.
3. **Bulk always dry-runs** — 50 writes deserve a per-item preview and an explicit go, even when requested explicitly.

## Available MCP Tools

**Projects & clients**

| Tool | Description |
|------|-------------|
| `leantime_list_projects` | List all projects |
| `leantime_get_project` | Get project details |
| `leantime_get_project_progress` | Get project progress metrics |
| `leantime_create_project` | Create a project (Markdown details, clientId required) |
| `leantime_update_project` | Update a project (patch — only provided fields change) |
| `leantime_find_projects` | Search projects by name |
| `leantime_list_project_users` | List users assigned to a project (valid editorId candidates) |
| `leantime_list_clients` | List clients (clientId needed to create projects) |

**Tickets**

| Tool | Description |
|------|-------------|
| `leantime_list_tickets` | List tickets with filters (status, milestone, sprint, user, type, search) |
| `leantime_get_ticket` | Get ticket details |
| `leantime_create_ticket` | Create a ticket (Markdown description, mandatory assignment, subtasks via dependingTicketId; `dryRun` supported) |
| `leantime_update_ticket` | Update a ticket (patch — other fields are never wiped; `dryRun` supported) |
| `leantime_delete_ticket` | Delete a ticket (confirm-gated) |
| `leantime_list_subtasks` | List a ticket's subtasks |
| `leantime_my_tasks` | Open tickets assigned to a user (default: the API key owner) |
| `leantime_get_ticket_options` | Priorities, efforts, kanban columns and ticket types |
| `leantime_get_statuses` | Get status labels for a project |
| `leantime_get_ticket_types` | Get ticket types for a project |

**Comments**

| Tool | Description |
|------|-------------|
| `leantime_list_comments` | List a ticket's discussion |
| `leantime_add_comment` | Comment on a ticket (Markdown converted to rich HTML) |
| `leantime_update_comment` | Edit a comment (Markdown) |
| `leantime_delete_comment` | Delete a comment (confirm-gated) |

**Time tracking**

| Tool | Description |
|------|-------------|
| `leantime_log_time` | Log hours on a ticket (`add` accumulates, `set` is idempotent; `dryRun` supported) |
| `leantime_get_ticket_time` | Total and per-day booked time for a ticket |
| `leantime_list_timesheets` | List time entries between two dates |
| `leantime_delete_timesheet_entry` | Delete a time entry (confirm-gated) |

**Milestones**

| Tool | Description |
|------|-------------|
| `leantime_list_milestones` | List milestones of a project |
| `leantime_get_milestone` | Get milestone details |
| `leantime_create_milestone` | Create a milestone (Markdown description, mandatory assignment; `dryRun` supported) |
| `leantime_update_milestone` | Update a milestone (patch; `dryRun` supported) |
| `leantime_get_milestone_progress` | Completion % (effort × priority weighted, Leantime's formula) |
| `leantime_delete_milestone` | Delete a milestone (confirm-gated; its tickets are kept) |

**Sprints**

| Tool | Description |
|------|-------------|
| `leantime_list_sprints` | List sprints of a project |
| `leantime_create_sprint` | Create a sprint |
| `leantime_update_sprint` | Update a sprint (name/dates) |
| `leantime_get_current_sprint` | Sprint in progress (or next upcoming), computed from dates |

**Users**

| Tool | Description |
|------|-------------|
| `leantime_list_users` | List all users (id, name) — for assignment |

**Bulk operations**

| Tool | Description |
|------|-------------|
| `leantime_bulk_create_tickets` | Create up to 50 tickets — validated upfront, Markdown converted, per-item results (`dryRun` supported) |
| `leantime_bulk_update_tickets` | Update up to 50 tickets via safe patch — per-item results (`dryRun` supported) |
| `leantime_bulk_schedule_tickets` | Schedule up to 50 tickets (sprint, dates) via patch |

**Backup & recovery**

| Tool | Description |
|------|-------------|
| `leantime_backup_project` | Dump a project to a timestamped local JSON file (milestones, tickets, sprints — the response is a summary only, not the data) |
| `leantime_project_context` | Full project overview in one call (progress, health, sprint, milestones, ticket summary, recent activity) — under 4 KB, the agent's natural first call |

## Key management

Configuration lives in named instance profiles — `~/.config/leantime/instances/<name>/` (`api-key`, 0600, and `instance-url`), with a `default` file naming the default.

```bash
leantmcp url set https://your-instance.leantime.io   # instance URL (argument OK — not a secret)
leantmcp url show                                     # resolved URL + where it comes from
leantmcp key set      # hidden prompt (or LEANTIME_API_KEY env var) → instances/<name>/api-key (0600)
leantmcp setup opencode # config then only holds "{file:...}" pointers
leantmcp key show     # masked display (lt_h13…Fc3O)
leantmcp key test     # live validation against the instance
leantmcp key rotate   # mint a new key (same role), verify it live, replace the stored one
leantmcp doctor       # health check: key file, permissions, config, live key
```

- One secret in one place: the key file has mode 0600; the URL lives in a sibling file — changing instances updates every pointer-based config automatically
- The key is never accepted as a command-line argument (shell history), never logged, and key commands are CLI-only — they are not exposed as MCP tools

## Rate limit handling

The MCP server transparently retries on `429 Too Many Requests` with **adaptive delays**: it discovers the instance's rate limit from the `X-RateLimit-Limit` header on the first 429, then paces requests accordingly (60s ÷ limit). When headers aren't available, it falls back to a conservative 6-second delay (Leantime's default 10 req/min). Up to 5 retries for rate limits, 2 for transient network errors (502/503/504). On instances with low rate limits, large bulk batches may take several minutes — the tool descriptions inform agents of this.

## Backup & recovery

```bash
leantmcp backup                      # backup the first project on the active instance
leantmcp backup --project 3         # specific project
leantmcp backup --project 3 --full  # include per-ticket comments (slower)
leantmcp backup --list              # show existing backups
leantmcp restore backup.json        # dry-run: shows what would be restored
leantmcp restore backup.json --confirm  # execute the restore
```

Backups land in `~/.config/leantime/backups/<project-name>-<timestamp>.json` (mode 0600). Fast mode captures milestones, tickets and sprints in 3 API calls. `--full` adds per-ticket comments at 1 call per ticket — on a rate-limited instance (~10 req/min), expect roughly 1 minute per 10 tickets. On instances with generous limits, `LEANTIME_MCP_BACKUP_CONCURRENCY=N` (1-8, default 1) fetches comments concurrently — results stay in ticket order so the backup file is identical either way.

**Large projects**: completeness fetches (backup, restore verification, project_context) are immune to the API's per-call limit — a project larger than the limit (10 000 by default, override with `LEANTIME_MCP_FETCH_LIMIT`) is fetched completely via automatic date-window pagination: the window is bisected until every slice fits under the limit, results are deduplicated, and concurrent modifications can only produce duplicates, never losses. Normal projects pay exactly one request per fetch (the fast path). Only pathological cases (more tickets than the limit sharing one exact timestamp) still produce a `warnings` entry. `leantime_list_tickets` is separately capped at 500 results to protect the agent's context window, and says so in a note when the cap is hit.

The MCP tool `leantime_backup_project` does the same fast backup and returns only a summary (path + counts), so agents can trigger it cheaply — e.g. before bulk modifications.

**Restore** rebuilds a backup into a **NEW project** (never merges with existing data — zero risk of overwriting). Tickets are created in topological order (parents before subtasks), with all cross-references remapped (milestone, sprint, parent ticket). If the backup contains custom statuses that don't exist in the new project, the restore prompts interactively: it asks you to create the statuses in Leantime's UI (showing the exact project name and ID), then resolves the mapping by re-fetching. The dry-run (default without `--confirm`) shows exactly what would be created and any warnings — no API writes.

## Tool management

Disable tools you don't need — they disappear from `tools/list` entirely (zero context-window cost), and calling a disabled tool returns an actionable error instead of a generic "unknown tool".

```bash
leantmcp tools list                                    # all 42 tools with their status
leantmcp tools disable leantime_delete_ticket         # one tool
leantmcp tools disable destructive --instance local   # group on a specific profile
leantmcp tools enable readonly                        # re-enable a group
leantmcp tools enable all --instance prod             # reset on prod
```

The `--instance` flag (global, works on any subcommand) targets a specific keyring profile — same as `LEANTIME_INSTANCE=prod leantmcp …` but more discoverable.

**Read-only mode** (two clear commands):
```bash
leantmcp tools disable all
leantmcp tools enable readonly
```

Tool state is stored **per instance profile** (`~/.config/leantime/instances/<name>/tools.json`, mode 0600) — the same profile your harness config pins via `LEANTIME_INSTANCE`, so each instance can have its own tool set. Changes take effect on the next MCP session restart.

**Why disable tools?**
- **Reduce context cost**: 42 tool descriptions ≈ 4K tokens; trimming to what you use saves tokens per conversation
- **Safety**: disable destructive tools entirely — the agent can't even see they exist
- **Simplicity**: fewer tools = faster agent decisions, less confusion
- **Read-only mode**: `--preset readonly` is ideal for demonstrations or giving someone view-only access

## Getting your Leantime API key

1. Go to your Leantime instance
2. Navigate to **My Account** → **API Keys**
3. Generate a new key

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full guide. The short version:

```bash
cargo build --release
cargo test                          # parallel works — env-mutating suites hold locks
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
```

### Local Leantime instance

The repo ships a `docker-compose.yml` (pinned Leantime + MySQL, API rate limit at the real default of 10 req/min) and a bootstrap script:

```bash
docker compose up -d
export LEANTIME_URL=http://localhost:8090
export LEANTIME_API_KEY="$(bash scripts/local-instance-bootstrap.sh | tail -1)"
```

### Tests

| Suite | What it covers |
|---|---|
| `tests/markdown_test.rs` (24) | Markdown → HTML: headings, lists, task lists, emphasis, multi-backtick code spans, links, escaping, CRLF |
| `tests/markdown_golden_test.rs` (1) | Byte-for-byte stable output pinned by a 73-case golden corpus (committed fixture (regenerate with scripts/generate-golden-corpus.ts)) |
| `tests/config_test.rs` (14) | Keyring round-trip (no newline, 0600), multi-instance isolation, masking, env resolution, instance-name traversal rejection |
| `tests/harness_test.rs` (15) | Config writers: merge preservation, 0600, codex idempotence, `{file:}` pointers, scope/instance/name options, deprecation warning |
| `tests/client_test.rs` (14) | Mocked HTTP (mockito): 429 adaptive retry (seconds + HTTP-date + cap), rate-limit discovery & calibration across calls, 503×2 retries, 502 exhaustion, exhaustion messages byte-parity, RPC errors + `data` separator |
| `tests/tools_test.rs` (38) | Handlers via mockito: patch semantics, sprint full-field resend, id de-mangling, end-of-day timesheets, computed current sprint, destructive matrix (ask/deny/allow/invalid + 4-tool sweep), editorId validation, log_time validation (24h daily cap), bulk caps + happy/partial paths, comment crash recovery, enrichment |
| `tests/key_rotate_test.rs` (6) | Rotation choreography: happy path + relations copy, verification failure leaves keyring untouched, unknown key aborts, relation-copy failure warns, creation refusal aborts |
| `tests/e2e_readonly.rs` (1) | Opt-in (`LEANTIME_URL`+`LEANTIME_API_KEY`): projects non-vacuous, statuses shape, enrichment, milestones — with loud skips |
| `tests/e2e/run.sh` (7 checks) | Binary-level smoke: MCP handshake, 42 tools, live API calls, assignment enforcement, doctor |
| `tests/e2e_local.rs` (1, 8 sections) | **Exhaustive e2e** — opt-in with `LEANTIME_E2E=local`: scratch project, full tool surface, scoping/field-wiping regressions, destructive gating (incl. deny), bulk cycles, capture-only cleanup |

```bash
# exhaustive e2e against the local docker instance (~9 min at 10 req/min):
LEANTIME_E2E=local cargo test --test e2e_local -- --nocapture --test-threads=1
```

CI runs unit tests on Linux, Windows and macOS, plus the full exhaustive e2e on Linux against the docker instance on every push.

## Build

```bash
cargo build --release
# → target/release/leantmcp (~3 MB, opt-level=z + LTO + strip, rustls)
```

## License

[MIT](LICENSE)
