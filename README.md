# leantime-mcp

[![GitHub release](https://img.shields.io/github/v/release/4lador/leantime-mcp?logo=github)](https://github.com/4lador/leantime-mcp/releases)
[![CI](https://github.com/4lador/leantime-mcp/actions/workflows/release.yml/badge.svg)](https://github.com/4lador/leantime-mcp/actions/workflows/release.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

A [Model Context Protocol](https://modelcontextprotocol.io/) server for [Leantime](https://leantime.io/), enabling LLM-powered tools (opencode, Claude Desktop, Cursor, etc.) to interact with your Leantime projects.

## Features

- List, get, create, and update tickets/tasks
- List projects and milestones
- Automatic status enrichment — every ticket includes `statusLabel`, `statusType`, and `statusColor` so the LLM never misinterprets status values
- List sprints and ticket types
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

### Global (all projects)

```bash
leantmcp setup global
```

Prompts for your Leantime URL and API key, writes config to `~/.opencode/opencode.json`.

### Per-project

```bash
cd your-project
leantmcp setup project
```

Writes config to `./opencode.json` in the current directory.

### Environment variables

Both commands accept `LEANTIME_URL` and `LEANTIME_API_KEY` env vars to skip prompts:

```bash
LEANTIME_URL=https://your-instance.leantime.io \
LEANTIME_API_KEY=lt_xxx... \
leantmcp setup global
```

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

## Available MCP Tools

| Tool | Description |
|------|-------------|
| `leantime_list_projects` | List all projects |
| `leantime_get_project` | Get project details |
| `leantime_get_project_progress` | Get project progress metrics |
| `leantime_list_tickets` | List tickets with filters (status, milestone, sprint, user, type, search) |
| `leantime_get_ticket` | Get ticket details |
| `leantime_create_ticket` | Create a new ticket |
| `leantime_update_ticket` | Update an existing ticket |
| `leantime_get_statuses` | Get status labels for a project |
| `leantime_get_ticket_types` | Get ticket types for a project |
| `leantime_list_milestones` | List milestones |
| `leantime_get_milestone` | Get milestone details |
| `leantime_list_sprints` | List sprints |

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

## Build

```bash
deno task build
```

Produces a standalone `leantmcp` binary.

## License

[MIT](LICENSE)
