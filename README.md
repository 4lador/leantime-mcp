# leantime-mcp

- MCP Registry name: `mcp-name: io.github.4lador/leantime-mcp`

<p align="center">
  <img src="https://raw.githubusercontent.com/4lador/leantime-mcp/main/docs/hero.png" alt="leantime-mcp" width="600">
</p>

[![CI](https://github.com/4lador/leantime-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/4lador/leantime-mcp/actions/workflows/ci.yml)
[![MCP Registry](https://img.shields.io/badge/MCP_Registry-listed-8A2BE2)](https://registry.modelcontextprotocol.io/v0.1/servers?search=leantime-mcp)
[![crates.io](https://img.shields.io/crates/v/leantime-mcp)](https://crates.io/crates/leantime-mcp)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)

An unofficial [Model Context Protocol](https://modelcontextprotocol.io/) server for [Leantime](https://leantime.io/), designed agent-first: it lets LLM tools (opencode, Claude Code, Claude Desktop, Cursor, Codex, or any MCP client) work on your Leantime projects through 42 tools with dry-run validation, bulk operations, backup/restore and one-call project context.

It is an independent, open-source integration — stdio only, running locally with your own API key and your own instance. It does not aim to replace Leantime's official offerings; if you want vendor-supported integrations, use those.

**~3 MB self-contained binary** (rustls — no system OpenSSL), **~1.2 ms median startup** and **~5 MB idle memory** (VmRSS after handshake — measured on a Linux x86_64 dev machine, n=100 spawns).

**Documentation**: [Quick start](#quick-start) · [Configuration](#configuration) · [Safety](#safety) · [Tools](#tools) · [Troubleshooting](#troubleshooting) · [Harness setup](docs/harness-setup.md) · [Backup & recovery](docs/backup-restore.md) · [Migration from v1.x](migration-from-1.x.md) · [CHANGELOG](CHANGELOG.md) · [CONTRIBUTING](CONTRIBUTING.md) · [SECURITY](SECURITY.md)

## Demo

An example session (abridged):

> **You:** What's the status of the Acme launch project?
>
> **Agent** calls `leantime_project_context` — one call returning progress, health, sprint and milestones:
>
> ```json
> { "project": { "name": "Acme launch", "progress": { "percentDone": 64.5, "ticketsTotal": 279, "ticketsDone": 180 } },
>   "health": { "blocked": 0, "overdue": 7, "unassigned": 3, "openTotal": 99 },
>   "currentSprint": { "name": "Sprint 4", "status": "current", "daysRemaining": 6, "openTickets": 8 } }
> ```
>
> **Agent:** Acme launch is at 64.5% (180 of 279 tickets done). 99 tickets are open — 7 overdue, 3 unassigned, none blocked. Sprint 4 ends in 6 days with 8 open tickets; the milestone in progress (PHASE 8) sits at 47.8%.

## Features

- 42 tools: projects, tickets, subtasks, milestones, sprints, comments, time tracking, bulk operations
- `leantime_project_context` — the whole project picture in one call (progress, health, sprint, milestones), under 4 KB
- `dryRun: true` on every mutation — same validations, `from → to` diffs on updates, no write requests
- Bulk operations up to 50 items, validated upfront (all-or-nothing on creates)
- Backup & restore — snapshot a project to local JSON, rebuild it into a new project
- Adaptive rate-limit retries, transparent to agents
- Credentials in a local keyring (mode 0600) — harness configs hold a bare command
- Deletes confirm-gated; `LEANTIME_MCP_DESTRUCTIVE_POLICY=deny` as an emergency stop
- Per-instance tool management — disable what you don't use, read-only mode in two commands
- Markdown → Leantime rich HTML, converted deterministically server-side

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

## Quick start

```bash
leantmcp url set https://your-instance.leantime.io   # once
leantmcp key set                                      # once — hidden prompt
leantmcp setup <your-harness>                         # writes the config
leantmcp doctor                                       # verify everything end-to-end
```

Your Leantime API key: **My Account → API Keys → Generate** on your instance.

| Harness | Default scope | Config file |
|---|---|---|
| opencode | global | `~/.opencode/opencode.json` |
| Claude Code | **project** | `./.mcp.json` |
| Claude Desktop | global | `claude_desktop_config.json` (path per OS) |
| Cursor | global | `~/.cursor/mcp.json` |
| Codex | global | `~/.codex/config.toml` |

Every generated config is a **bare command with no credentials** — the binary resolves them from the keyring at startup. Project-scoped setups, `--scope`/`--instance`/`--name` options and multiple instances: see [Harness setup](docs/harness-setup.md).

## Configuration

Credentials live in named instance profiles — `~/.config/leantime/instances/<name>/` (`api-key`, mode 0600, and `instance-url`), with a `default` file naming the default. The key is read from the keyring file or environment — it is not accepted as a command-line argument (shell history) and is not logged; key commands are CLI-only and are not exposed as MCP tools.

```bash
leantmcp url show          # resolved URL + where it comes from
leantmcp key show          # masked display (lt_h13…Fc3O)
leantmcp key test          # live validation against the instance
leantmcp key rotate        # mint a new key (same role), verify it live, replace the stored one
leantmcp instance add …    # multiple Leantime instances (see Harness setup)
```

`LEANTIME_URL` / `LEANTIME_API_KEY` environment variables remain available as per-run overrides; `LEANTIME_INSTANCE` selects a profile. Resolution order: env (explicit override) → `LEANTIME_INSTANCE` profile → the `default` file.

## Safety

This software is provided without warranty (MIT). It drives Leantime with your API key on your behalf — **back up your Leantime data** before letting agents operate on it.

- **Deletes are confirm-gated** (`confirm: true` required by default; `LEANTIME_MCP_DESTRUCTIVE_POLICY=deny` refuses them outright, `allow` skips the gate for CI). Project hiding/deletion is intentionally not exposed.
- **Dry runs**: every mutation tool accepts `dryRun: true` — validations run, updates resolve `from → to` values (with status labels), bulk tools return per-item previews, and no write request is sent. Agent guidance is built into the tool descriptions on a three-tier policy: direct execution for explicit values, dry-run-then-confirm when the agent chose the values itself, dry-run mandated for bulk.
- **Assignment is mandatory** on ticket/milestone creation (`editorId` validated against the real user list, or an explicit `unassigned: true`).
- **Tool management**: disabled tools are omitted from `tools/list` (no context-window cost) and calling one is refused with an actionable error. Read-only mode: `leantmcp tools disable all && leantmcp tools enable readonly`. State is stored per instance profile.
- **Backup-first**: `leantime_backup_project` (MCP) or `leantmcp backup` (CLI) snapshot a project cheaply — agents are instructed to use it before bulk modifications. See [Backup & recovery](docs/backup-restore.md).

## Tools

42 tools across 9 domains: projects & clients (8), tickets (10), comments (4), time tracking (4), milestones (6), sprints (4), users (1), bulk operations (3), backup & context (2). Updates use the patch API — only provided fields change. Descriptions follow a Markdown subset converted server-side; raw HTML in input is escaped.

<details>
<summary><b>Supported Markdown subset</b> (for descriptions and comments)</summary>

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

</details>

<details>
<summary><b>All 42 tools</b> (click to expand)</summary>

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
| `leantime_bulk_update_tickets` | Update up to 50 tickets via patch — per-item results (`dryRun` supported) |
| `leantime_bulk_schedule_tickets` | Schedule up to 50 tickets (sprint, dates) via patch |

**Backup & context**

| Tool | Description |
|------|-------------|
| `leantime_backup_project` | Dump a project to a timestamped local JSON file (milestones, tickets, sprints — the response is a summary only, not the data) |
| `leantime_project_context` | Full project overview in one call (progress, health, sprint, milestones, ticket summary, recent activity) — under 4 KB, the agent's natural first call |

</details>

## Backup & recovery

```bash
leantmcp backup --project 3 --full   # snapshot incl. comments (~/.config/leantime/backups/, 0600)
leantmcp restore backup.json         # dry-run, then --confirm to execute
```

Restore writes the backup into a newly created project — it does not merge into, or write to, an existing project. Large projects are handled via date-window pagination (best-effort snapshot, not atomic). Details, env vars and rate-limit behavior: [Backup & recovery](docs/backup-restore.md).

## Troubleshooting

- **Start with `leantmcp doctor`** — it checks the key file, permissions, config and the key against the live instance.
- **Bulk operations are slow** — the instance's rate limit governs throughput (Leantime defaults to 10 req/min); retries are transparent but a 50-item batch can take minutes. Instances with generous limits are correspondingly faster.
- **"Ambiguous outcome" error after a mutation** — the instance returned a transient 5xx after the request was sent; the change may or may not have been applied. Verify the result (re-read the entity) before retrying — a blind retry can duplicate it.
- **Key rejected** — `leantmcp key test` validates live; `leantmcp key rotate` mints a replacement and swaps it in.
- **Windows** — the PowerShell installer puts the binary in `%USERPROFILE%\.local\bin`; make sure it is on `PATH`.
- Still stuck? [Open an issue](https://github.com/4lador/leantime-mcp/issues).

## Why this isn't for you

- You want **vendor-supported, official integrations** — use [Leantime's official offerings](https://leantime.io); this project is independent and unofficial.
- You need a **hosted/remote (HTTP) MCP server** — leantmcp is stdio-only and runs locally next to your MCP client.
- You need **project deletion or hiding** — intentionally not exposed by this server.

## Development

```bash
cargo build --release
cargo test                          # parallel works — env-mutating suites hold locks
cargo fmt --all --check && cargo clippy --all-targets -- -D warnings
```

A pinned docker Leantime + MySQL instance ships with the repo (`docker compose up -d`, then `scripts/local-instance-bootstrap.sh`). The full test-suite guide lives in [CONTRIBUTING.md](CONTRIBUTING.md); CI runs unit tests on Linux, Windows and macOS plus an exhaustive e2e suite against the docker instance.

## License

[MIT](LICENSE)

Leantime is a product of its respective owners. This project is an independent, unofficial integration and is not affiliated with or endorsed by the Leantime team.
