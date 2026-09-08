# Migrating from v1.x

v1.x was TypeScript/Deno; v2.0.0+ is a Rust rewrite. The keyring,
credentials and harness configs are **fully compatible** — the v2 binary
is a drop-in replacement:

- **Your keyring works as-is**: `~/.config/leantime/instances/<name>/` is unchanged since v1.7.0. Both profiles and the `default` file resolve identically.
- **Your install URL still works**: `curl -fsSL https://raw.githubusercontent.com/4lador/leantime-mcp/main/install.sh | sh` delivers the v2 binary at the same location (`~/.local/bin/leantmcp`). The installer verifies the SHA-256 checksum as before.
- **Your harness configs need no change**: they point to `~/.local/bin/leantmcp` (bare command) — replacing the binary replaces the server. Restart your MCP session to pick up the new version.
- **The v1.x source code is preserved** on the [`frozen-legacy-ts`](https://github.com/4lador/leantime-mcp/tree/frozen-legacy-ts) branch. It will not receive updates or security fixes (see [SECURITY.md](SECURITY.md)).

Why the rewrite? The v1.x binary embedded the Deno/V8 runtime:

| | v1.x (Deno/TypeScript) | v2 (Rust) |
|---|---|---|
| Binary size | ~100 MB | **~3 MB** |
| Startup | ~200 ms | **1.2 ms** (median, n=100) |
| Memory (idle) | ~50 MB | **~5 MB** (VmRSS) |
| Runtime deps | Deno/V8 embedded | **none (rustls)** |

Release history: [CHANGELOG.md](CHANGELOG.md) · Back to the [README](README.md).
