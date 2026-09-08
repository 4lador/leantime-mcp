# leantime-mcp

- MCP Registry name: `mcp-name: io.github.4lador/leantime-mcp`

<p align="center">
  <img src="https://raw.githubusercontent.com/4lador/leantime-mcp/main/docs/hero.png" alt="leantime-mcp" width="600">
</p>

[![CI](https://github.com/4lador/leantime-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/4lador/leantime-mcp/actions/workflows/ci.yml)
[![MCP Registry](https://img.shields.io/badge/MCP_Registry-listed-8A2BE2)](https://registry.modelcontextprotocol.io/v0.1/servers?search=leantime-mcp)
[![crates.io](https://img.shields.io/crates/v/leantime-mcp)](https://crates.io/crates/leantime-mcp)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A [Model Context Protocol](https://modelcontextprotocol.io/) server for [Leantime](https://leantime.io/), enabling LLM-powered tools (opencode, Claude Code, Claude Desktop, Cursor, Codex, or any MCP client) to interact with your Leantime projects.

**~3 MB self-contained binary** (rustls — no system OpenSSL), **~1.2 ms median startup** and **~5 MB idle memory** (VmRSS after handshake — measured on a Linux x86_64 dev machine, n=100 spawns).

**Documentation**: [Key management](#key-management) · [Safety](#safety-destructive-operations) · [Available MCP Tools](#available-mcp-tools) · [Development](#development) · [Migration from v1.x](migration-from-1.x.md) · [CHANGELOG](CHANGELOG.md) · [CONTRIBUTING](CONTRIBUTING.md) · [SECURITY](SECURITY.md) · [LICENSE](LICENSE)

## Features

- Full project-management coverage: projects, clients, tickets, subtasks, milestones, sprints, comments, time tracking and **bulk operations** (42 tools)
- **`leantime_project_context`**: a composite first-call tool that hydrates full project context (progress, health, sprint, milestones, activity) in one round-trip — agents start reasoning instead of paging through lists
- **Dry runs**: every mutation tool accepts `dryRun: true` — same validations, `from → to` diffs on updates, per-item previews on bulk, no write requests. Agents are instructed to dry-run first on conversational-intent updates and inferred creates, and on all bulk batches
- **Backup & recovery**: `leantmcp backup [--full]` snapshots a project (plus `leantmcp restore` to rebuild it into a new project), and `leantime_backup_project` lets agents trigger a cheap backup before bulk modifications
- **Multiple Leantime instances**: named profiles (`instance add`, `instance use`), one server per instance in any harness — the config holds no credentials
- **Automatic 429 retry**: adaptive backoff that discovers the instance's rate limit from response headers — retries are transparent to agents; after 5 attempts the failure surfaces as an explicit error
- Deterministic Markdown → rich HTML descriptions and comments: formatting is applied server-side — the documented subset (see below) converts consistently, so rendering does not depend on the client
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
- **Credentials live in the local keyring** — generated harness configs hold a bare command. The binary resolves credentials at startup: environment variables first (per-run override — they can also be set in a config's env block, though the keyring is the intended path), then the keyring — `~/.config/leantime/instances/<name>/` (`api-key` mode 0600, `instance-url`). One keyring, shared by every harness you use.
- That design is what lets the configs below stay **bare commands with no credentials** in them.

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

Generated configs are **bare commands containing no credentials**: the binary resolves credentials from `~/.config/leantime/instances/` at startup — project-scoped files contain no credentials and can be committed.

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

In a harness config, declare one server per instance — no credentials in the config:

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

Raw HTML in descriptions is escaped before reaching Leantime — it renders as literal text.

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
3. **Bulk: a dry-run first is mandated by the tool descriptions** — 50 writes deserve a per-item preview and an explicit go, even when requested explicitly.

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
| `leantime_update_ticket` | Update a ticket (patch — only provided fields change; `dryRun` supported) |
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
- The key is read from the keyring file or environment — it is not accepted as a command-line argument (shell history) and is not logged; key commands are CLI-only — they are not exposed as MCP tools

## Rate limit handling

The MCP server transparently retries on `429 Too Many Requests` with **adaptive delays**: it discovers the instance's rate limit from the `X-RateLimit-Limit` header on the first 429, then paces requests accordingly (60s ÷ limit). When headers aren't available, it falls back to a conservative 6-second delay (Leantime's default 10 req/min), with up to 5 retries. Reads also retry twice on transient server errors (502/503/504). Mutations retry on rate limits only: a transient 5xx after a mutation surfaces as an explicit `Ambiguous` error — the instance may or may not have applied the change, and a blind retry can duplicate it. On instances with low rate limits, large bulk batches may take several minutes — the tool descriptions inform agents of this.

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

**Large projects**: completeness fetches (backup, restore verification, project_context) handle projects larger than the API's per-call limit (10 000 by default, override with `LEANTIME_MCP_FETCH_LIMIT`) via automatic date-window pagination: the window is bisected until every slice fits under the limit, and results are deduplicated. This is a best-effort snapshot, not an atomic one: tickets modified during the pass move to a later window and reappear as duplicates (deduplicated); tickets deleted during the pass are absent. Windows that cannot be fully resolved produce a `warnings` entry. Projects smaller than the limit pay one request per fetch (the fast path). `leantime_list_tickets` is separately capped at 500 results to protect the agent's context window, and says so in a note when the cap is hit.

The MCP tool `leantime_backup_project` does the same fast backup and returns only a summary (path + counts), so agents can trigger it cheaply — e.g. before bulk modifications.

**Restore** writes the backup into a **newly created project** — it does not merge into, or write to, an existing project. Tickets are created in topological order (parents before subtasks), with cross-references remapped (milestone, sprint, parent ticket). If the backup contains custom statuses that don't exist in the new project, the restore prompts interactively: it asks you to create the statuses in Leantime's UI (showing the project name and ID), then resolves the mapping by re-fetching. The dry-run (default without `--confirm`) shows what would be created and any warnings — it sends no write requests.

## Tool management

Disable tools you don't need — they are omitted from `tools/list` (so they consume no context window), and calling a disabled tool is refused with an actionable error instead of a generic "unknown tool".

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
- **Read-only mode**: the two commands above (`tools disable all` + `tools enable readonly`) suit demonstrations or view-only access

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
| `tests/markdown_test.rs` | Markdown → HTML: headings, lists, task lists, emphasis, multi-backtick code spans, links, escaping, CRLF |
| `tests/markdown_golden_test.rs` | Byte-for-byte stable output pinned by a 73-case golden corpus (committed fixture (regenerate with scripts/generate-golden-corpus.ts)) |
| `tests/config_test.rs` | Keyring round-trip (no newline, 0600), multi-instance isolation, masking, env resolution, instance-name traversal rejection |
| `tests/harness_test.rs` | Config writers: merge preservation, 0600, codex idempotence, `{file:}` pointers, scope/instance/name options, deprecation warning |
| `tests/client_test.rs` | Mocked HTTP (mockito): 429 adaptive retry (seconds + HTTP-date + cap), rate-limit discovery & calibration across calls, read 5xx retries, mutation 5xx ambiguity, chunked pagination, concurrent comments, exhaustion messages, RPC errors |
| `tests/tools_test.rs` | Handlers via mockito: patch semantics, sprint full-field resend, id de-mangling, end-of-day timesheets, computed current sprint, destructive matrix, editorId validation, log_time validation, bulk paths, status label resolution, dry-run, guidance regression, enrichment |
| `tests/key_rotate_test.rs` | Rotation choreography: happy path + relations copy, verification failure leaves keyring untouched, unknown key aborts, relation-copy failure warns, creation refusal aborts |
| `tests/e2e_readonly.rs` | Opt-in (`LEANTIME_URL`+`LEANTIME_API_KEY`): projects non-vacuous, statuses shape, enrichment, milestones — with loud skips |
| `tests/e2e/run.sh` | Binary-level smoke: MCP handshake, 42 tools, live API calls, assignment enforcement, doctor |
| `tests/e2e_local.rs` | **Exhaustive e2e** — opt-in with `LEANTIME_E2E=local`: scratch project, full tool surface, scoping/field-wiping regressions, destructive gating (incl. deny), bulk cycles, capture-only cleanup |

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

Leantime is a product of its respective owners. This project is an independent, unofficial integration and is not affiliated with or endorsed by the Leantime team.
