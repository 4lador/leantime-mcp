# Changelog

## v1.3.0 — Feature-complete Core PM (37 tools)

- **Comments**: list / add / update / delete on tickets (Markdown converted to rich HTML; `add` recovers from Leantime v3.7.3's post-insert notification crash over JSON-RPC)
- **Time tracking**: `log_time` (additive or idempotent set), `get_ticket_time`, `list_timesheets` (inclusive end-of-day range), `delete_timesheet_entry`
- **Milestones**: create (mandatory assignment, richer than `quickAddMilestone`), update via `tickets.patch` (`quickUpdateMilestone` depends on session state, unusable with API keys), progress (client-side reimplementation of Leantime's effort × priority formula — the RPC endpoint takes a union-typed parameter their binder cannot cast), delete
- **Sprints**: create / update (full field resend with explicit projectId), `get_current_sprint` computed from dates (the session-based API is unavailable to API keys)
- **Projects & clients**: create (Markdown details), update (patch), `find_projects` (normalizes Leantime's mangled `id-modified` ids), `list_project_users`, `list_clients`
- **Tickets extras**: `list_subtasks`, `my_tasks`, `get_ticket_options`, `delete_ticket`
- **Safety**: destructive tools gated behind `confirm: true`; `LEANTIME_MCP_DESTRUCTIVE_POLICY=ask|deny|allow` (default `ask`)
- v3.7.3 quirks normalized server-side: array-wrapped ids, `entityId` (not `moduleId`) for comments, explicit `projectId` everywhere (no session fallback)

## v1.2.0 — Deterministic formatting, mandatory assignment, critical RPC fixes

- Descriptions are Markdown, converted server-side to Leantime's TipTap HTML subset (zero-dependency converter, raw HTML always escaped)
- Ticket creation requires an explicit assignment decision (`editorId` validated against real users, or `unassigned: true`)
- New `leantime_list_users` tool; `users.getAll` cached (TTL 5 min — Leantime's default API rate limit is 10 req/min)
- **Critical fix**: `list_tickets`/`list_milestones` sent filters as flat params that Leantime silently ignored (returned every ticket instance-wide); filters now go through `searchCriteria` with the correct keys
- **Critical fix**: partial updates wiped unspecified fields (`editorId`, status, priority, tags…) — switched to `tickets.patch`; removed the phantom `percentDone` param
- `install.sh`: atomic replace (temp file + `mv`) so updates work while the server binary is running
- `docker-compose.yml`: disposable local Leantime 3.7.3 + MySQL 8.4 for development and testing
- Test mocks made honest: they apply `searchCriteria` filters and mirror real API response shapes

## v1.1.0

- Use `editorId` for assignment, add `dependingTicketId` / `planHours` support, fix the `values` wrapper

## v1.0.1

- Use `environment` instead of `env` in the opencode config

## v1.0.0

- Initial release: tickets, projects, milestones, sprints, status enrichment, setup CLI, install scripts, CI
