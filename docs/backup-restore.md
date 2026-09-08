# Backup & recovery

Snapshot any Leantime project to a local JSON file, and rebuild it into a
new project. The agent-facing entry point is the `leantime_backup_project`
MCP tool; the CLI adds `--full` mode and `restore`.

## Backup

```bash
leantmcp backup                      # backup the first project on the active instance
leantmcp backup --project 3         # specific project
leantmcp backup --project 3 --full  # include per-ticket comments (slower)
leantmcp backup --list              # show existing backups
```

Backups land in `~/.config/leantime/backups/<project-name>-<timestamp>.json`
(mode 0600). Fast mode captures milestones, tickets and sprints in 3 API
calls. `--full` adds per-ticket comments at 1 call per ticket — on a
rate-limited instance (~10 req/min), expect roughly 1 minute per 10 tickets.
On instances with generous limits, `LEANTIME_MCP_BACKUP_CONCURRENCY=N`
(1-8, default 1) fetches comments concurrently — results stay in ticket
order so the backup file is identical either way.

The MCP tool `leantime_backup_project` does the same fast backup and returns
only a summary (path + counts), so agents can trigger it cheaply — e.g.
before bulk modifications.

## Large projects (date-window pagination)

Completeness fetches (backup, restore verification, project_context) handle
projects larger than the API's per-call limit (10 000 by default, override
with `LEANTIME_MCP_FETCH_LIMIT`) via automatic date-window pagination: the
window is bisected until every slice fits under the limit, and results are
deduplicated.

This is a best-effort snapshot, not an atomic one: tickets modified during
the pass move to a later window and reappear as duplicates (deduplicated);
tickets deleted during the pass are absent. Windows that cannot be fully
resolved produce a `warnings` entry. Projects smaller than the limit pay one
request per fetch (the fast path).

`leantime_list_tickets` is separately capped at 500 results to protect the
agent's context window, and says so in a note when the cap is hit.

## Restore

```bash
leantmcp restore backup.json             # dry-run: shows what would be restored
leantmcp restore backup.json --confirm   # execute the restore
```

**Restore** writes the backup into a **newly created project** — it does not
merge into, or write to, an existing project. Tickets are created in
topological order (parents before subtasks), with cross-references remapped
(milestone, sprint, parent ticket). If the backup contains custom statuses
that don't exist in the new project, the restore prompts interactively: it
asks you to create the statuses in Leantime's UI (showing the project name
and ID), then resolves the mapping by re-fetching. The dry-run (default
without `--confirm`) shows what would be created and any warnings — it sends
no write requests.

## Rate-limit handling (how backups behave on slow instances)

The MCP server transparently retries on `429 Too Many Requests` with
adaptive delays: it discovers the instance's rate limit from the
`X-RateLimit-Limit` header on the first 429, then paces requests accordingly
(60s ÷ limit). When headers aren't available, it falls back to a
conservative 6-second delay (Leantime's default 10 req/min), with up to 5
retries. Reads also retry twice on transient server errors (502/503/504).
Mutations retry on rate limits only: a transient 5xx after a mutation
surfaces as an explicit `Ambiguous` error — the instance may or may not have
applied the change, and a blind retry can duplicate it. On instances with
low rate limits, large bulk batches may take several minutes — the tool
descriptions inform agents of this.
