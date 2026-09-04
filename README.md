# leantime-mcp

[![GitHub release](https://img.shields.io/github/v/release/4lador/leantime-mcp?logo=github)](https://github.com/4lador/leantime-mcp/releases)
[![CI](https://github.com/4lador/leantime-mcp/actions/workflows/release.yml/badge.svg)](https://github.com/4lador/leantime-mcp/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A [Model Context Protocol](https://modelcontextprotocol.io/) server for [Leantime](https://leantime.io/), enabling LLM-powered tools (opencode, Claude Desktop, Cursor, or any MCP client) to interact with your Leantime projects.

## Features

- List, get, create, and update tickets/tasks
- Deterministic Markdown → rich HTML descriptions: ticket formatting is applied server-side, so descriptions are always properly rendered in Leantime's editor (no more wall-of-text tickets)
- Mandatory assignment on ticket creation: the server rejects calls that don't assign a user (or explicitly opt out), so agents always ask who should own the task
- List projects, milestones, sprints, and users
- Automatic status enrichment — every ticket includes `statusLabel`, `statusType`, and `statusColor` so the LLM never misinterprets status values
- Project progress metrics

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

## Ticket descriptions (Markdown)

Ticket descriptions are written in **Markdown** and converted **deterministically server-side** to the rich HTML subset supported by Leantime's editor. Whatever the LLM produces, the rendering in Leantime is always correct — headings, lists, checkboxes, emphasis, code, and links.

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

Creating a ticket requires an explicit assignment decision:

- either `editorId` — the user the ticket is assigned to (validated against the real user list; call `leantime_list_users` to get candidates),
- or `unassigned: true` — only when the user explicitly asked to leave the ticket unassigned.

If neither is provided, the server rejects the call with an error instructing the agent to ask the user first. Updates only validate `editorId` when you actually change the assignment.

## Available MCP Tools

| Tool | Description |
|------|-------------|
| `leantime_list_projects` | List all projects |
| `leantime_get_project` | Get project details |
| `leantime_get_project_progress` | Get project progress metrics |
| `leantime_list_tickets` | List tickets with filters (status, milestone, sprint, user, type, search) |
| `leantime_get_ticket` | Get ticket details |
| `leantime_create_ticket` | Create a new ticket (Markdown description, mandatory assignment) |
| `leantime_update_ticket` | Update an existing ticket (Markdown description) |
| `leantime_get_statuses` | Get status labels for a project |
| `leantime_get_ticket_types` | Get ticket types for a project |
| `leantime_list_milestones` | List milestones |
| `leantime_get_milestone` | Get milestone details |
| `leantime_list_sprints` | List sprints |
| `leantime_list_users` | List users (id, name) — for ticket assignment |

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

## Build

```bash
deno task build
```

Produces a standalone `leantmcp` binary.

## License

[MIT](LICENSE)
