# Security Policy

## Reporting a vulnerability

Please use GitHub's private vulnerability reporting. Do **not** open a public issue for security reports.

## Supported versions

Only the latest release receives security fixes.

**v2.0.0+** (Rust, `main` branch) — actively maintained.

**v1.x** (TypeScript/Deno, [`frozen-legacy-ts`](https://github.com/4lador/leantime-mcp/tree/frozen-legacy-ts) branch) — **end-of-life**. No security fixes will be backported. All v1.x users should migrate to v2.0.0 (drop-in replacement — same keyring, same configs, same install URL).

## Scope

In scope:

- The `leantmcp` binary (CLI, MCP stdio server, keyring, harness config writers)
- The installers (`install.sh`, `install.ps1`)
- The CI/release pipeline of this repository

Out of scope:

- Leantime itself (report upstream: https://github.com/Leantime/leantime — including its API/auth behavior)
- Vulnerabilities requiring prior compromise of the user's account or machine
- Automated scanner noise without a demonstrated impact

Also see the [RustSec advisories database](https://rustsec.org/advisories/) — dependencies are pinned via a committed `Cargo.lock`.

## Security-relevant design

- Raw HTML in Markdown input is always escaped — it cannot inject arbitrary markup into Leantime's rich-text fields. Link URLs are allowlisted (http/https/mailto).
- The API key lives in a single local keyring file (created mode 0600 on POSIX — no plaintext window; user-profile ACLs on Windows). It is never logged, never echoed in full, and never included in tool responses.
- Credentials never appear in harness configs: opencode configs use `{file:...}` pointers (a plaintext fallback exists for bootstrapping but is deprecated and warns).
- Destructive tools require an explicit `confirm: true` and respect `LEANTIME_MCP_DESTRUCTIVE_POLICY` (`ask`/`deny`/`allow`).
- Instance names are validated before being joined into filesystem paths; file writes are created private-by-design and the keyring directory chain is 0700.
- Server-controlled inputs are bounded: Retry-After capped at 60s, HTTP responses at 64 MB (enforced while streaming, including chunked bodies), markdown input at 1 MB, list nesting at depth 32. Stdin lines over 10 MB are rejected — a policy against protocol confusion (the line is buffered before rejection; the peer is the local, trusted harness).
- Plain `http://` instance URLs warn at `url set` and server startup (the key would travel unencrypted); localhost is exempt.

## Expectations

Reports are handled on a best-effort basis; we aim to acknowledge within 7 days. No SLA is guaranteed.
