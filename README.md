# leantime-mcp

[![GitHub release](https://img.shields.io/github/v/release/4lador/leantime-mcp?logo=github)](https://github.com/4lador/leantime-mcp/releases)
[![CI](https://github.com/4lador/leantime-mcp/actions/workflows/release.yml/badge.svg)](https://github.com/4lador/leantime-mcp/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A [Model Context Protocol](https://modelcontextprotocol.io/) server for [Leantime](https://leantime.io/), enabling LLM-powered tools (opencode, Claude Desktop, Cursor, or any MCP client) to interact with your Leantime projects.

## Features

- Full project-management coverage: projects, clients, tickets, subtasks, milestones, sprints, comments and time tracking (37 tools)
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

## Setup

The `leantmcp setup` commands write configuration for **opencode** specifically. If you use another client, skip to the relevant section below.

### opencode

**Global (all projects):**

```bash
leantmcp setup global
```

Prompts for your Leantime URL and API key, writes config to `~/.opencode/opencode.json`.

**Current project only:**

```bash
cd your-project
leantmcp setup project
```

Writes config to `./opencode.json` in the current directory.

Both commands accept `LEANTIME_URL` and `LEANTIME_API_KEY` env vars to skip prompts.

### Claude Desktop

Add to your `claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "leantime": {
      "command": "/path/to/leantmcp",
      "env": {
        "LEANTIME_URL": "https://your-instance.leantime.io",
        "LEANTIME_API_KEY": "lt_xxx..."
      }
    }
  }
}
```

### Cursor

Add to your `.cursor/mcp.json`:

```json
{
  "mcpServers": {
    "leantime": {
      "command": "/path/to/leantmcp",
      "env": {
        "LEANTIME_URL": "https://your-instance.leantime.io",
        "LEANTIME_API_KEY": "lt_xxx..."
      }
    }
  }
}
```

### Any other MCP client

`leantmcp` is a standard stdio MCP server. Configure your client to launch the binary with the two environment variables below:

| Variable | Description |
|----------|-------------|
| `LEANTIME_URL` | Base URL of your Leantime instance |
| `LEANTIME_API_KEY` | Leantime API key (see below) |

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

The API key never needs to live in plaintext config files:

```bash
leantmcp key set      # hidden prompt (or LEANTIME_API_KEY env var) → ~/.config/leantime/api-key (0600)
leantmcp setup global # config then only holds "{file:~/.config/leantime/api-key}"
leantmcp key show     # masked display (lt_h13…Fc3O)
leantmcp key test     # live validation against the instance
leantmcp key rotate   # mint a new key (same role), verify it live, replace the stored one
leantmcp doctor       # health check: key file, permissions, config, live key
```

- One secret in one place: the key file has mode 0600 (POSIX) or is protected by the user-profile ACLs (Windows)
- `setup` writes a native `{file:...}` pointer (opencode substitutes file contents) whenever the stored key matches — no plaintext in `opencode.json`
- Rotating: `leantmcp key rotate [--name X]` mints a new key with the same role via the API, verifies it live before replacing the stored one, then instructs you to delete the old key in the Leantime UI (the API has no key-deletion method). On any failure the previous key is left untouched.
- The key is never accepted as a command-line argument (shell history), never logged, and key commands are CLI-only — they are not exposed as MCP tools

## Getting your Leantime API key

1. Go to your Leantime instance
2. Navigate to **My Account** → **API Keys**
3. Generate a new key

## Development

```bash
cp .env.example .env
# Edit .env with your Leantime URL and API key
deno task dev
```

### Local Leantime instance

A `docker-compose.yml` is included to run a disposable local Leantime (pinned to the version this MCP is validated against) for development and testing:

```bash
docker compose up -d
```

1. Open http://localhost:8090 and complete the first-run setup wizard (~2 min)
2. Create an API key in **Company Settings → API Keys**
3. Point the MCP at it:

```bash
LEANTIME_URL=http://localhost:8090 LEANTIME_API_KEY=lt_xxx... deno task dev
```

The local instance raises the API rate limit to 120 req/min (Leantime's default of 10 req/min is too low for automated testing).

### Tests

```bash
deno task test:unit        # unit + integration (mocked API, no instance needed)
deno task test             # all tests incl. e2e (read-only, needs .env credentials)
```

End-to-end validation runs against the local instance: create a scratch project, exercise every tool (including the destructive cycles: rejection without `confirm: true`, execution with it), then clean up **only the ids created during the run** — never a sweep.

## Build

```bash
deno task build
```

Produces a standalone `leantmcp` binary.

## License

[MIT](LICENSE)
