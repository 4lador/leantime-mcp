# Contributing

Thanks for considering a contribution!

## Development setup

- [Deno](https://deno.land) 2.x
- No `.env` needed: dev and test flows resolve credentials from the environment first, then from `~/.config/leantime/`:

```bash
leantmcp url set https://your-instance.leantime.io   # or http://localhost:8090 (local)
leantmcp key set                                      # hidden prompt
deno task dev
```

- For end-to-end testing, run a disposable local Leantime with the included `docker-compose.yml` (pinned to the version this MCP is validated against) — see the README's *Local Leantime instance* section.

## Tests

```bash
deno task test:unit   # unit + integration (mocked API — no instance needed)
deno task test        # everything, incl. read-only e2e (live credentials)
```

Expectations:

- CI runs three jobs on every push: unit tests (Linux), smoke on `windows-latest` (tests, native compile, key/URL flows, MCP handshake), and `local-e2e` — the exhaustive suite against a real dockerized Leantime. Keep tests OS-agnostic (guard POSIX-only assertions like file permissions behind `Deno.build.os`, never assume `/tmp` paths)
- Test mocks must mirror the **real API behavior** (response shapes, `searchCriteria` scoping, error forms) — a lying mock is worse than no mock
- **No vacuous tests**: a test that passes on empty data without saying so is a bug — report loud skips (`console.warn`) instead of silent conditional passes
- Anything that touches the live API in tests must be **read-only**, or clean up strictly by the ids it created — the local e2e's `deleteCaptured()` helper refuses anything else, and so should you

To run the exhaustive local e2e yourself:

```bash
docker compose up -d
LEANTIME_E2E=local LEANTIME_URL=http://localhost:8090 \
LEANTIME_API_KEY="$(bash scripts/local-instance-bootstrap.sh | tail -1)" \
deno task test:e2e:local
```

## Safety expectations

- No destructive MCP tool without the `confirm: true` gate and `LEANTIME_MCP_DESTRUCTIVE_POLICY` support (see `src/tools/shared.ts`)
- Never expose credential management as an MCP tool — key/URL commands are CLI-only (`src/keyring.ts`)
- No secrets in code, tests, configs, or logs; configs only ever hold `{file:...}` pointers
- Raw HTML in Markdown input is always escaped before reaching Leantime

## Conventions

- [Conventional commits](https://www.conventionalcommits.org) (`feat:`, `fix:`, `docs:` …)
- Update `CHANGELOG.md` with every user-facing change
- Bump `VERSION` in `src/main.ts` to match the release tag

## Release process

Push a `vX.Y.Z` tag on `main` — CI builds and publishes the binaries (Linux, macOS, Windows) to a GitHub release. The smoke CI (Linux + Windows, including key/URL flows with the compiled binary) must be green first.

## Reporting issues

Bugs and feature requests: [open an issue](https://github.com/4lador/leantime-mcp/issues).
Security reports: see [SECURITY.md](SECURITY.md) — please use private vulnerability reporting, not a public issue.
