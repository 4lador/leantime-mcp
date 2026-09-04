# Security Policy

## Reporting a vulnerability

Please use GitHub's [private vulnerability reporting](https://github.com/4lador/leantime-mcp/security/advisories/new).
Do **not** open a public issue for security reports.

## Supported versions

Only the latest release receives security fixes.

## Scope

**In scope** — the leantime-mcp server itself, for example:

- injection through the Markdown → HTML conversion
- exposure or logging of the Leantime API key
- privilege issues in tool handling

**Out of scope:**

- vulnerabilities in Leantime itself — report them upstream: <https://github.com/Leantime/leantime>
- the security of your own Leantime instance (API key scopes, network exposure, backups)
- actions performed by an agent/LLM you configured with your own API key — see the destructive-operations policy in the README

## Security-relevant design

- Raw HTML in Markdown input is always escaped — it cannot inject arbitrary markup into Leantime's rich-text fields.
- The API key only transits from the environment to the `x-api-key` request header; it is never logged and never included in tool responses.
- Destructive tools require an explicit `confirm: true` and respect `LEANTIME_MCP_DESTRUCTIVE_POLICY` (`ask`/`deny`/`allow`); project deletion is not exposed at all.
- Credential creation (`key rotate` mints a new API key) is available only through the local CLI — never as an MCP tool an agent could invoke.

## Expectations

Reports are handled on a best-effort basis; we aim to acknowledge within 7 days. No SLA is guaranteed.
