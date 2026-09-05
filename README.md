# leantime-mcp

[![GitHub release](https://img.shields.io/github/v/release/4lador/leantime-mcp?logo=github)](https://github.com/4lador/leantime-mcp/releases)
[![CI](https://github.com/4lador/leantime-mcp/actions/workflows/release.yml/badge.svg)](https://github.com/4lador/leantime-mcp/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A [Model Context Protocol](https://modelcontextprotocol.io/) server for [Leantime](https://leantime.io/), enabling LLM-powered tools (opencode, Claude Desktop, Cursor, or any MCP client) to interact with your Leantime projects.

**Documentation**: [Key management](#key-management) · [Safety](#safety-destructive-operations) · [Available MCP Tools](#available-mcp-tools) · [Development](#development) · [CHANGELOG](CHANGELOG.md) · [CONTRIBUTING](CONTRIBUTING.md) · [SECURITY](SECURITY.md) · [LICENSE](LICENSE)

## Features

- Full project-management coverage: projects, clients, tickets, subtasks, milestones, sprints, comments, time tracking and **bulk operations** (40 tools)
- **Multiple Leantime instances**: named profiles (`instance add`, `instance use`), one server per instance in any harness — still zero secrets
- **Automatic 429 retry**: the client retries rate-limited requests with header-aware backoff (up to 3 retries) — agents never need to handle rate limiting
- Deterministic Markdown → rich HTML descriptions and comments: formatting is applied server-side, so everything is always properly rendered in Leantime's editor (no more wall-of-text tickets)
- Mandatory assignment on ticket/milestone creation: the server rejects calls that don't assign a user (or explicitly opt out), so agents always ask who should own the task
- Destructive operations gated behind explicit confirmation (`LEANTIME_MCP_DESTRUCTIVE_POLICY`)
- Automatic status enrichment — every ticket includes `statusLabel`, `statusType`, and `statusColor` so the LLM never misinterprets status values
- Project progress metrics and milestone progress (effort × priority weighted)
- v3.7.x API quirks handled server-side: scoping filters, session-less API keys, id mangling, array-wrapped ids

## Install

**Linux / macOS:**

```bash
curl -fsSL https://raw.githubusercontent.com/4lador/leantime-mcp/main/install.sh | sh
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/4lador/leantime-mcp/main/install.ps1 | iex
```

Binaries are self-contained (~80-110 MB): they embed the Deno/V8 runtime, so nothing else needs to be installed on the target machine. Both installers verify the published SHA-256 checksum before installing and abort on mismatch.

## How it works

- `leantmcp` is a **stdio MCP server**: your harness (opencode, Claude Code, Claude Desktop, Cursor, Codex…) spawns it at session start and stops it at session end. No daemon, no port, nothing runs in the background.
- **Credentials never live in harness configs.** The binary resolves them at startup: environment variables first (per-run override), then the keyring — `~/.config/leantime/api-key` (mode 0600) and `~/.config/leantime/instance-url`. One keyring, shared by every harness you use.
- That fallback is what makes every config below a **bare command with no secrets**: there is nothing sensitive to put in a config file in the first place. `leantmcp doctor` flags any legacy config that still embeds a plaintext key.

## Setup

The universal flow, for every harness:

```bash
leantmcp url set https://your-instance.leantime.io   # once
leantmcp key set                                      # once — hidden prompt
leantmcp setup <your-harness>                         # writes the config (see table)
leantmcp doctor                                       # verify everything end-to-end
```

`setup <harness>` creates the keyring first if it's missing (it prompts), then writes the harness's native config:

| Harness | Command | What gets written | Secrets in the config |
|---|---|---|---|
| opencode | `leantmcp setup global` (or `project`) | `~/.opencode/opencode.json` with `{file:...}` pointers (opencode's native file substitution) | none — pointers |
| Claude Code | `leantmcp setup claude-code` | `./.mcp.json` (project scope, merge) — prints the `claude mcp add --scope user` command for user scope | none — bare command |
| Claude Desktop | `leantmcp setup claude-desktop` | `claude_desktop_config.json` (path per OS), absolute binary path — GUI apps get a limited `PATH` | none — bare command |
| Cursor | `leantmcp setup cursor` | `~/.cursor/mcp.json` (merge, existing servers preserved) | none — bare command |
| Codex | `leantmcp setup codex` | appends `[mcp_servers.leantime]` to `~/.codex/config.toml` (once) | none — bare command |

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

### Any other MCP client

`leantmcp` is a standard stdio MCP server: point your client at the binary, no environment variables required (the keyring provides them). `LEANTIME_URL` / `LEANTIME_API_KEY` environment variables remain available as per-run overrides — e.g. targeting a different instance for a test run.

### Multiple instances

All credentials live in named profiles under `~/.config/leantime/instances/<name>/`. A `default` file names the default instance (used when no override is set).

```bash
leantmcp instance add staging      # prompts for URL + key (hidden)
leantmcp instance list             # profiles + masked keys + default/active markers
leantmcp instance use staging      # set staging as the default instance
leantmcp instance remove staging   # refuses to remove the current default
```

Any command — and any server spawn — targets a profile via `LEANTIME_INSTANCE`:

```bash
LEANTIME_INSTANCE=staging leantmcp key rotate   # rotates staging's key
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

Resolution order: `LEANTIME_URL`/`LEANTIME_API_KEY` env (explicit override) → `LEANTIME_INSTANCE` profile → the `default` file (names the default instance profile).

## Rich text (Markdown)

All rich-text fields — **ticket and milestone descriptions, comments, and project details** — are written in **Markdown** and converted **deterministically server-side** to the rich HTML subset supported by Leantime's editor. Whatever the LLM produces, the rendering in Leantime is always correct — headings, lists, checkboxes, emphasis, code, and links.

Supported syntax:

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

Project hiding/deletion is intentionally not exposed: Leantime's API has no project-delete method that this MCP is willing to drive.

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
| `leantime_create_ticket` | Create a ticket (Markdown description, mandatory assignment, subtasks via dependingTicketId) |
| `leantime_update_ticket` | Update a ticket (patch — other fields are never wiped) |
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
| `leantime_log_time` | Log hours on a ticket (`add` accumulates, `set` is idempotent) |
| `leantime_get_ticket_time` | Total and per-day booked time for a ticket |
| `leantime_list_timesheets` | List time entries between two dates |
| `leantime_delete_timesheet_entry` | Delete a time entry (confirm-gated) |

**Milestones**

| Tool | Description |
|------|-------------|
| `leantime_list_milestones` | List milestones of a project |
| `leantime_get_milestone` | Get milestone details |
| `leantime_create_milestone` | Create a milestone (Markdown description, mandatory assignment) |
| `leantime_update_milestone` | Update a milestone (patch) |
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

## Key management

Configuration lives in named instance profiles — `~/.config/leantime/instances/<name>/` (`api-key`, 0600, and `instance-url`), with a `default` file naming the default. The opencode configs only hold `{file:...}` pointers, so they contain no secrets and are safe to commit as-is.

```bash
leantmcp url set https://your-instance.leantime.io   # instance URL (argument OK — not a secret)
leantmcp url show                                     # resolved URL + where it comes from
leantmcp key set      # hidden prompt (or LEANTIME_API_KEY env var) → instances/<name>/api-key (0600)
leantmcp setup global # config then only holds "{file:...}" pointers
leantmcp key show     # masked display (lt_h13…Fc3O)
leantmcp key test     # live validation against the instance
leantmcp key rotate   # mint a new key (same role), verify it live, replace the stored one
leantmcp doctor       # health check: key file, permissions, config, live key
```

- One secret in one place: the key file has mode 0600 (POSIX) or is protected by the user-profile ACLs (Windows); the URL lives in a sibling file — changing instances (`leantmcp url set`) updates every pointer-based config automatically
- `setup` writes native `{file:...}` pointers (opencode substitutes file contents) whenever the stored key matches — no plaintext in `opencode.json`
- Rotating: `leantmcp key rotate` (same role, live-verified before replacing anything), then delete the old key in the Leantime UI
- Environment variables (`LEANTIME_URL`, `LEANTIME_API_KEY`) remain the per-run override mechanism — e.g. targeting the local docker instance for e2e tests
- The key is never accepted as a command-line argument (shell history), never logged, and key commands are CLI-only — they are not exposed as MCP tools

**Bulk operations**

| Tool | Description |
|------|-------------|
| `leantime_bulk_create_tickets` | Create up to 50 tickets — validated upfront, Markdown converted, per-item results |
| `leantime_bulk_update_tickets` | Update up to 50 tickets via safe patch — per-item results |
| `leantime_bulk_schedule_tickets` | Schedule up to 50 tickets (sprint, dates) via patch |

## Rate limit handling

The MCP server transparently retries on `429 Too Many Requests` with **adaptive delays**: it discovers the instance's rate limit from the `X-RateLimit-Limit` header on the first 429, then paces requests accordingly (60s ÷ limit). When headers aren't available, it falls back to a conservative 6-second delay (Leantime's default 10 req/min). Up to 5 retries for rate limits, 2 for transient network errors (502/503/504). On instances with low rate limits, large bulk batches may take several minutes — the tool descriptions inform agents of this.

## Getting your Leantime API key

1. Go to your Leantime instance
2. Navigate to **My Account** → **API Keys**
3. Generate a new key

## Development

See [CONTRIBUTING.md](CONTRIBUTING.md) for the full guide (setup, tests, conventions, safety expectations, release process). The short version:

No `.env` needed — dev and test flows resolve credentials from the environment first, then from `~/.config/leantime/` (see [Key management](#key-management)):

```bash
deno task dev
# Override per run (e.g. against the local docker instance):
LEANTIME_URL=http://localhost:8090 LEANTIME_API_KEY=lt_local... deno task dev
```

### Local Leantime instance

A `docker-compose.yml` is included to run a disposable local Leantime (pinned to the version this MCP is validated against) for development and testing:

```bash
docker compose up -d
bash scripts/local-instance-bootstrap.sh   # waits for health, runs the first-run wizard, creates an API key
```

The bootstrap script prints the API key on its last line — capture it and point the MCP at the local instance:

```bash
LEANTIME_URL=http://localhost:8090 LEANTIME_API_KEY="$(bash scripts/local-instance-bootstrap.sh | tail -1)" deno task dev
```

The local instance raises the API rate limit to 120 req/min (Leantime's default of 10 req/min is too low for automated testing).

### Tests

```bash
deno task test:unit        # unit + integration (mocked API, no instance needed)
deno task test             # all tests incl. read-only e2e (credentials resolve from env or the keyring)
LEANTIME_E2E=local LEANTIME_URL=http://localhost:8090 LEANTIME_API_KEY=lt_local... deno task test:e2e:local   # exhaustive e2e (see below)
```

The exhaustive local e2e (`tests/e2e/local.test.ts`) is what the CI `local-e2e` job runs on every push: it creates a scratch project on a real instance, exercises the full tool surface (including the destructive cycles: rejection without `confirm: true`, execution with it, and the `deny` policy), then cleans up **only the ids it created** — the helper refuses to delete anything else — and asserts the scratch is left empty. Read-only assertions on live data never pass vacuously: empty results are reported as loud skips, not silent successes.

## Build

```bash
deno task build
```

Produces a standalone `leantmcp` binary.

## License

[MIT](LICENSE)
