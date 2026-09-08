# Contributing

## Development setup

```bash
cargo build
cargo test                          # parallel works — env-mutating suites hold locks
```

Credentials resolve from `~/.config/leantime/` (compatible with existing v1.x installs) or environment variables.

## Quality gates (CI-enforced)

```bash
cargo fmt --all --check
cargo clippy --all-targets -- -D warnings
cargo test                          # parallel supported (env-mutating suites serialize via locks)
```

CI runs these on Linux, Windows and macOS (the keyring/config layer is OS-sensitive).

## Tests

```bash
cargo test                          # unit + integration (mocked, no instance needed)
bash tests/e2e/run.sh              # binary-level smoke vs docker Leantime
# Exhaustive e2e (~9 min at the real 10 req/min rate limit):
docker compose up -d
export LEANTIME_URL=http://localhost:8090
export LEANTIME_API_KEY="$(bash scripts/local-instance-bootstrap.sh | tail -1)"
export LEANTIME_E2E=local
cargo test --test e2e_local -- --nocapture --test-threads=1
```

Expectations:

- The exhaustive suite (`tests/e2e_local.rs`) is ported from the legacy TS `local.test.ts` (now on the `frozen-legacy-ts` branch) — this Rust suite is now the source of truth
- Test mocks must mirror real API behavior — a lying mock is worse than no mock
- Anything that touches the live API must clean up strictly by the ids it created
- Read-only assertions on live data never pass vacuously: empty results are loud skips, not silent successes

## Safety expectations

- No destructive MCP tool without the `confirm: true` gate
- No secrets in code, tests, configs, or logs
- Raw HTML in Markdown input is always escaped before reaching Leantime
- Instance names and any value joined into a path are validated (no traversal)

## Conventions

- Conventional commits (`feat:`, `fix:`, `security:`, `test:`, `docs:`…)
- Every user-facing change adds a line to the CHANGELOG under `## Unreleased`
- Public items carry doc comments (`#![warn(missing_docs)]` is on)
- Error messages are actionable: say what failed AND how to fix it

## Reporting issues

Bugs and feature requests go to [GitHub issues](https://github.com/4lador/leantime-mcp/issues).
Security reports follow [SECURITY.md](SECURITY.md) — never a public issue.

## Release process

1. Update `CHANGELOG.md` (move `## Unreleased` under the new version + date)
2. Bump the version in `Cargo.toml` AND in `server.json` (top-level `version` and `packages[0].version` — the CI publish job fails fast if they don't match the tag)
3. Commit, merge to `main`, tag `vX.Y.Z`, push the tag
4. CI does the rest, in order: full test matrix → 5 release artifacts with SHA-256 checksums → GitHub release → **automatic publishing** — the crate to crates.io (keyless trusted publishing via `rust-lang/crates-io-auth-action`, temporary token auto-revoked) and `server.json` to the MCP Registry (OIDC, keyless). A version-consistency guard fails the run before anything is published if versions drift, and an idempotency guard makes tag-workflow re-runs safe.

The README ships inside the crate as it exists at the tag — finalize documentation before tagging. There is no "What's new" section in the README by design; the CHANGELOG is the release history.
