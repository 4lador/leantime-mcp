# Changelog

## v2.3.3 — 2026-09-08

### Fixed

- **`leantime_list_tickets` status label filter returned the wrong tickets**: the label was passed raw to the API, which `intval()`s it — `"New"` became `0` and silently filtered on status ID 0 (on the test project, asking for "New" returned every Done ticket). Labels are now resolved to their IDs server-side against the project's cached status map (case-insensitive), comma-separated lists resolve token by token, and an unknown label returns an actionable error listing the project's valid statuses with their IDs. The server's magic values `done` / `not_done` (resolved by statusType) pass through untouched, as do numeric IDs.

## v2.3.2 — 2026-09-07

### Fixed

- **Silent truncation at 500 items on completeness fetches**: backups of projects larger than 500 tickets were silently truncated by the API's per-call limit — and the restore verification would compare 500 expected vs 500 found and report success while tickets had never been captured. All completeness paths (backup, restore verification, milestone list and progress, project_context) now request 10 000 items per call, overridable via `LEANTIME_MCP_FETCH_LIMIT` (no numeric clamp — admin-controlled like `LEANTIME_URL`; the 64 MB streaming cap is the backstop). A fetch returning exactly the limit produces an explicit warning in the backup result (CLI + MCP `warnings` field) and at restore verification. `leantime_list_tickets` stays capped at 500 — its result goes into the LLM context, the cap protects the context window — and now returns `{tickets, note}` with a "refine filters" note when the cap is hit instead of a silently-truncated array.

## v2.3.1 — 2026-09-07

### Changed

- **Dry-run agent guidance in tool descriptions (three-tier policy)**: mutation tools now instruct agents on when to validate first — execute directly when every value was explicitly given or resolves unambiguously ("passe #535 en Terminé" → one status, write and report); dry-run and confirm when the agent interpreted the request or chose values itself ("configure ce projet pour du dev agile" → show the diff/proposal first); always dry-run bulk batches (per-item preview, explicit approval). `leantime_create_ticket`, `leantime_update_ticket`, `leantime_create_milestone`, `leantime_update_milestone`, `leantime_bulk_create_tickets`, `leantime_bulk_update_tickets`; `leantime_log_time` stays guidance-free by design. A regression test pins the guidance strings.

## v2.3.0 — 2026-09-07

### Added

- **`dryRun: true` on mutation tools**: `leantime_create_ticket`, `leantime_update_ticket`, `leantime_create_milestone`, `leantime_update_milestone`, `leantime_bulk_create_tickets`, `leantime_bulk_update_tickets` and `leantime_log_time` accept `dryRun: true` — every validation runs (assignment, editorId existence, value constraints) and a verdict is returned instead of mutating. Updates resolve `from → to` values against the current entity (one read), with status labels and a warning when a field already holds the target value; bulk tools return per-item previews (payload builders extracted so the dry-run and write paths can never drift); `log_time` dry-runs 100% locally and accumulates all errors. A failed validation is `valid: false` with the errors — not an MCP error. Delete tools are unchanged: their `confirm` gate already is a dry-run.

## v2.2.0 — 2026-09-07

### Added

- **`leantime_project_context`** (42nd tool): the full picture of a project in a single call — project info and progress, health counters (blocked / overdue / unassigned / open), current-or-upcoming sprint (explicit `status` discriminator, `daysRemaining` vs `daysUntilStart`), milestones with weighted progress computed in memory, ticket summary by status and type, and recently modified items. Response capped under 4 KB (`generatedAt` up front, milestones max 15, names truncated at 40 chars) — designed as the agent's first call, replacing 5-6 round-trips. Milestone grouping reads both `milestoneid` and `milestoneId` spellings (see v2.1.0 restore).

### Fixed

- **Milestone progress with unestimated tickets**: storypoints of `0` now fall back to the default effort of 3.0 — a milestone whose tickets carry no estimates no longer reports 0% forever (`leantime_get_milestone_progress` and the new composite tool).
- **`leantime_get_current_sprint` annotations**: `idempotentHint` is now `true`. Per the MCP spec the hint describes side effects, not response stability — a read-only call is idempotent even when its result changes over time. The internal `readonly_volatile` preset is retired.

## v2.1.0 — 2026-09-07

### Added

- **Restore**: `leantmcp restore <file> [--confirm]` — rebuilds a backup into a NEW project (never merges with existing data). Topological sort (parents before subtasks), ID remapping (milestones, sprints, tickets, comments), interactive status resolution (label match → statusType fallback → prompt with project name/ID), v3.7.3 comment crash recovery, post-restore verification. 13 unit tests + live-tested with 279-ticket Vision backup (208 subtasks, 156 milestone refs, 5 sprints — all cross-references remapped correctly, 0 failures).

## v2.0.0 — 2026-09-07

Complete rewrite in Rust (v1.x was TypeScript/Deno, now on the `frozen-legacy-ts` branch). The keyring, credentials and harness configs are fully compatible — v2.0.0 is a drop-in replacement. See [Migrating from v1.x](README.md#migrating-from-v1x).

### Added

- **Backup & recovery**: `leantmcp backup [--project ID] [--full] [--list]` CLI command and the `leantime_backup_project` MCP tool (41st tool) — dump a project to a timestamped JSON file (milestones, tickets, sprints; `--full` adds per-ticket comments). Files stored in `~/.config/leantime/backups/` (mode 0600). The MCP tool response is a summary only (path + counts), not the data — agents can trigger it cheaply before bulk modifications.

- **Tool management**: `leantmcp tools list|enable|disable <tool|group>` — hide tools from agents entirely (zero context-window cost), per instance profile. Groups are first-class: `all` (41), `destructive` (delete_*/bulk_*, 7 tools), `readonly` (list/get/find/backup/my_tasks, 23 tools), `write` (create/update/add/log, 11 tools). Disabled tools are hidden from `tools/list` but calling one returns an actionable error with the enable instruction. State stored in `~/.config/leantime/instances/<name>/tools.json` (mode 0600).

- **MCP tool annotations**: all 41 tools carry MCP spec annotations (`readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint`) in their `tools/list` entries. Read-only tools → readOnly+idempotent; destructive → destructive+idempotent; write → neither; `get_current_sprint` → readOnly without idempotent (result changes with time). False values omitted from the wire (absent = false per spec).

- **Global `--instance` flag**: `leantmcp <command> --instance PROFILE` on any subcommand — the CLI-native way to target a keyring profile, replacing the `LEANTIME_INSTANCE=…` env prefix for interactive use.

- **Universal harness setup**: `leantmcp setup <opencode|claude-code|claude-desktop|cursor|codex> [--scope global|project] [--instance PROFILE] [--name SERVER]`. Project-scoped configs (`./opencode.json`, `./.mcp.json`, `./.cursor/mcp.json`, `.codex/config.toml`) are bare-command with no secrets — designed to be committed to Git. `--instance PROFILE` pins a keyring profile via `LEANTIME_INSTANCE` in the config's env block. `--name SERVER` (default `leantime`) enables multiple instances side by side.

- **MCP protocol negotiation**: supports revisions `2024-11-05` and `2025-06-18`, echoes the client's requested version when known (per spec), falls back to the oldest supported otherwise.

- **41 MCP tools** covering projects, clients, tickets, subtasks, milestones, sprints, comments, time tracking, bulk operations and backup.

### Changed (intentional divergences from v1.x)

- `leantime_log_time` rejects entries over 24 hours (a timesheet line targets ONE date — beyond 24h is impossible data, typically a hallucinated value; v1.x accepted any positive number)
- The destructive-operation confirmation prompt normalizes a double-space template artifact from the v1.x TypeScript (cosmetic, no behavioral difference)
- `leantime_get_ticket_types` now requires `projectId` (aligns with the Leantime API)
- New `setup` syntax replaces `setup global`/`setup project` (see Universal harness setup above)

### Fixed

- Server crash (panic) on Unicode whitespace in nested-list indentation — the child-line slicer assumed ASCII bytes and could cut inside a multi-byte character. Indentation is now counted and sliced in characters; pinned by 3 golden-corpus cases (NBSP, mixed, ideographic space).
- `leantmcp doctor` now honors `LEANTIME_URL`/`LEANTIME_API_KEY` env overrides for its live validation (previously validated the keyring's active instance only, breaking e2e flows targeting another instance). Keyring gaps are downgraded to warnings when env credentials fully provide the run.
- `leantmcp tools enable all` now clears stale entries (renamed tools, manual edits) from the disabled list.
- Nested tokio runtime panic on `key test`/`key rotate` — handlers are now async, single runtime.
- Blank line on stdin no longer terminates the MCP server; malformed JSON gets `-32700`; requests over 10 MB rejected with `-32600`.
- RPC error `data` field now carries its ` — ` separator (was glued to the message).
- Rate-limit exhaustion error includes `(waited ~Ns per retry)` clause.
- `find_projects` de-mangles ids (`id-modified` → plain id).
- `list_timesheets` extends `dateTo` to `23:59:59` (bare dates excluded same-day entries).
- `get_current_sprint` computed from sprint dates (was delegating to the session-based API, unavailable to API keys).
- 64 MB response cap enforced while streaming (chunk loop), including chunked bodies without content-length.
- `mask_key` and `key rotate` username prefix are UTF-8 safe (char-based, not byte-index slicing).

### Security

- Instance names validated (`^[A-Za-z0-9][A-Za-z0-9._-]{0,63}$`, no `..`) at every resolution point — kills the path-traversal class via `instance add` or `LEANTIME_INSTANCE`
- All keyring and harness files created directly with mode 0600 via `OpenOptions` (no 0644 write-then-chmod window — including the opencode fallback that can hold a plaintext key); keyring directory chain tightened to 0700
- DoS caps on server-controlled inputs: Retry-After ≤ 60s, HTTP responses ≤ 64 MB (streaming), markdown input ≤ 1 MB, list nesting ≤ 32, stdin lines ≤ 10 MB (rejection policy)
- Plain `http://` to a non-local host warns at `url set` and server startup
- Plaintext API key in `opencode.json` is a `[DEPRECATED]` fallback with the exact migration path documented
- `Cargo.lock` committed (reproducible builds, auditable deps); `reqwest` uses rustls (no system OpenSSL); `unsafe_code = "forbid"`; MSRV 1.88 declared and verified by a dedicated CI job

### Tests

114 tests across 12 suites (parallel-safe — env-mutating tests hold locks), plus:
- 73-case golden corpus for the Markdown converter (byte-for-byte stable output, regenerated via `scripts/generate-golden-corpus.ts`)
- Exhaustive e2e suite (`tests/e2e_local.rs`, 8 ordered sections against a real dockerized Leantime at 10 req/min)
- Read-only e2e (`tests/e2e_readonly.rs`, opt-in via env)
- Binary-level smoke (`tests/e2e/run.sh`, 7 checks)

CI runs on Linux, Windows and macOS with `fmt --check`, `clippy -D warnings`, and `--locked` builds throughout.

---

## v1.x (TypeScript/Deno)

History for v1.x releases (v1.4.0 through v1.9.1) is preserved on the [`frozen-legacy-ts`](https://github.com/4lador/leantime-mcp/tree/frozen-legacy-ts) branch. That branch is end-of-life and will not receive updates or security fixes.
