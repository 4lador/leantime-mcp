# Changelog

## v1.5.0 — Honest docs, exhaustive shipped e2e, harness support

- **Exhaustive local e2e shipped in the repo** (`tests/e2e/local.test.ts`, opt-in via `LEANTIME_E2E=local`): scratch project, full tool surface, assignment enforcement, markdown conversions, scoping regression (a project's tickets can never leak into another), destructive cycles (reject without `confirm`, execute with it, `deny` policy), then cleanup strictly by captured ids — the `deleteCaptured()` helper refuses anything it did not create — and a final emptiness assertion. CI runs it on every push (`local-e2e` job) against the dockerized Leantime
- **`scripts/local-instance-bootstrap.sh`**: fully automated first-run wizard (install → invite → login → API key) for the local docker instance — no manual clicking
- **No more vacuous tests**: the read-only e2e now fails on an empty project list and reports loud, explicit skips instead of silently passing on empty data (the failure mode that once masked a total data loss behind a green "e2e 4/4")
- **Harness support**: `setup claude-code` (`./.mcp.json` + user-scope hint), `setup claude-desktop` (per-OS config path), `setup cursor` (`~/.cursor/mcp.json`), `setup codex` (`~/.codex/config.toml`) — each ensures the keyring exists (prompting if needed) then writes a **bare command with no secrets**; the binary resolves credentials from `~/.config/leantime/` at startup
- **`doctor`** now detects plaintext API keys in known harness configs and suggests re-running the matching setup
- README rewritten around transparency: "How it works" (stdio server spawned by your harness — no daemon; env > keyring resolution; one keyring shared by all harnesses), a per-harness compatibility table, the universal setup flow — and zero plaintext key examples anywhere
- New unit tests for harness writers (per-OS paths, merge/idempotence, no-secret guarantees) and doctor detection — 151 total

## v1.4.3 — Documentation

- CONTRIBUTING.md (dev setup, tests, safety expectations, release process)
- README: Documentation section referencing all docs; Tests section no longer mentions `.env` (credentials resolve from env or the keyring)
- SECURITY.md: key-transit wording updated for the keyring architecture (`~/.config/leantime/api-key` → env → header)

## v1.4.2 — Unified configuration, no plaintext anywhere

- The instance URL now lives beside the key in `~/.config/leantime/instance-url` — a single source of truth for both credentials; `leantmcp setup` writes TWO `{file:...}` pointers (URL + key) into the configs, which therefore contain no secrets at all and are safe to commit as-is
- New commands: `leantmcp url set <url>` (argument allowed — not a secret; live-verifies the stored key against the new instance, warns about legacy plaintext configs) and `leantmcp url show` (resolved URL + source). Changing instances updates every pointer-based config automatically
- `serve()` and the e2e suite resolve credentials as env > dotenv (.env legacy/override) > keyring files — a `.env` is no longer needed for development
- `key rotate` now preserves the old key's PROJECT ASSIGNMENTS (a freshly minted key is assigned to no project and sees nothing — the same service-vs-controller gap as `source: 'api'`); a copy failure warns and points to the UI
- `doctor` upgrades: URL check with source, expects both pointers in the config, and detects key copies in a cwd `.env` (stale → strong warning; duplicate → removal hint)
- CI windows-latest gains a URL-flow step next to the key-flow (regression guard with the compiled binary)
- 10 new unit tests (URL store, resolution order incl. pointer-ignoring config fallback, server env resolution, url set/show, doctor drift detection via chdir)

## v1.4.1 — `key rotate` command

- `leantmcp key rotate [--name X]`: mints a new API key with the same role as the current one (identified through the instance's key list), verifies it live, and only then replaces the stored key — on any failure the previous key is left untouched
- Hardcodes `source: "api"` in the creation payload — Leantime's `Api.createAPIKey` service does not set it (only the web UI controller does), and without it the minted key is rejected with 401
- The new secret exists only in process memory; masked reporting only; instructs to delete the old key in the UI (the API has no deletion method)
- SECURITY.md: credential creation is CLI-only, never exposed as an MCP tool
- 5 new unit tests (happy path incl. call shapes and ordering, live-failure keeps keyring intact, unknown key aborts, no stored key, custom name)

## v1.4.0 — Secure key management

- `leantmcp key set|show|test` and `leantmcp doctor`: the API key now lives in a single dedicated file (`~/.config/leantime/api-key`, 0600) instead of plaintext configs
- `setup global/project` writes a native `{file:...}` pointer (opencode file substitution) whenever the stored key matches — no plaintext secret in `opencode.json` — and sets mode 600 on the written config (POSIX)
- Hidden prompt for `key set` (raw-mode, no echo); key accepted via env var, never via argv
- Masked display everywhere (`lt_h13…Fc3O`); live key validation via the API
- Windows: POSIX `chmod`/`stat.mode` are unimplemented in Deno — guarded behind `Deno.build.os`; privacy relies on user-profile ACLs; CI runs a real key-flow (`key set` + `key show`) on a windows-latest runner to catch regressions
- 13 new unit tests (round-trip without trailing newline, permissions, masking, pointer/fallback, doctor, OS-aware skips)

## v1.3.1 — Windows support verified

- **Fix**: `setup global` used `HOME`, which is not defined on Windows — falls back to `USERPROFILE`
- **Fix**: `install.ps1` now downloads to a temp file and moves it into place (Windows locks a running executable — direct overwrite failed while the MCP server was running)
- **CI**: new workflow (`ci.yml`) — unit tests on Linux, plus a `windows-latest` job that runs the test suite on Windows, compiles a native binary and performs an MCP handshake smoke test (`scripts/smoke.ts`, tool list checked against the registry)
- `SECURITY.md` (private vulnerability reporting, scope, security-relevant design) and a no-warranty/backup notice in the README
- Docs: CHANGELOG created, rich-text section covers comments and project details, test workflow documented

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
