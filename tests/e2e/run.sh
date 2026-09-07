#!/bin/bash
# E2E test: spawns the Rust binary, speaks MCP over stdin/stdout,
# and validates responses against a real Leantime instance.
set -euo pipefail

BIN="${1:-./target/release/leantmcp}"
URL="${LEANTIME_URL:-http://localhost:8090}"
KEY="${LEANTIME_API_KEY:-}"   # :- so `set -u` lets the keyring fallback below run

if [ -z "$KEY" ]; then
    # Try the keyring
    KEY_FILE="$HOME/.config/leantime/instances/local/api-key"
    if [ -f "$KEY_FILE" ]; then
        KEY=$(cat "$KEY_FILE")
    else
        echo "No API key. Set LEANTIME_API_KEY or have instance 'local' configured." >&2
        exit 1
    fi
fi

PASS=0; FAIL=0

run_mcp() {
    local input="$1"
    echo "$input" | env LEANTIME_URL="$URL" LEANTIME_API_KEY="$KEY" timeout 10 "$BIN" serve 2>/dev/null || true
}

expect_contains() {
    local label="$1"; local haystack="$2"; local needle="$3"
    if printf "%s" "$haystack" | grep -qF -- "$needle"; then
        echo "  [OK] $label"
        PASS=$((PASS+1))
    else
        echo "  [FAIL] $label — expected '$needle' in: $(echo "$haystack" | head -c 120)"
        FAIL=$((FAIL+1))
    fi
}

echo "=== E2E: $BIN against $URL ==="

# 1. Initialize handshake
R=$(run_mcp '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"1"}}}')
expect_contains "handshake" "$R" '"name":"leantime-mcp"'
expect_contains "version" "$R" 'serverInfo'

# 2. tools/list → 42 tools
R=$(printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"1"}}}\n{"jsonrpc":"2.0","method":"notifications/initialized"}\n{"jsonrpc":"2.0","id":2,"method":"tools/list"}\n' | env LEANTIME_URL="$URL" LEANTIME_API_KEY="$KEY" timeout 10 "$BIN" serve 2>/dev/null | tail -1 || true)
TOOL_COUNT=$(echo "$R" | python3 -c "import json,sys; d=json.load(sys.stdin); print(len(d.get('result',{}).get('tools',[])))" 2>/dev/null || echo 0)
if [ "$TOOL_COUNT" -ge 42 ]; then
    echo "  [OK] tools/list has $TOOL_COUNT tools"
    PASS=$((PASS+1))
else
    echo "  [FAIL] tools/list expected ≥42 tools, got $TOOL_COUNT"
    FAIL=$((FAIL+1))
fi

# 3. list_projects
R=$(printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"1"}}}\n{"jsonrpc":"2.0","method":"notifications/initialized"}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"leantime_list_projects","arguments":{}}}\n' | env LEANTIME_URL="$URL" LEANTIME_API_KEY="$KEY" timeout 10 "$BIN" serve 2>/dev/null | tail -1 || true)
expect_contains "list_projects" "$R" 'clientId'

# 4. create_ticket without assignment → rejected
R=$(printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"1"}}}\n{"jsonrpc":"2.0","method":"notifications/initialized"}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"leantime_create_ticket","arguments":{"projectId":"1","headline":"e2e-no-assignment"}}}\n' | env LEANTIME_URL="$URL" LEANTIME_API_KEY="$KEY" timeout 10 "$BIN" serve 2>/dev/null | tail -1 || true)
expect_contains "assignment enforcement" "$R" 'Assignment required'

# 5. list_users — needle on the actual PAYLOAD: '"id"' alone matches the
# JSON-RPC envelope ("id":2) of error responses too (vacuous pass).
R=$(printf '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"e2e","version":"1"}}}\n{"jsonrpc":"2.0","method":"notifications/initialized"}\n{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"leantime_list_users","arguments":{}}}\n' | env LEANTIME_URL="$URL" LEANTIME_API_KEY="$KEY" timeout 10 "$BIN" serve 2>/dev/null | tail -1 || true)
expect_contains "list_users" "$R" '\"name\"'   # escaped: the tool payload is JSON-in-JSON

# 6. doctor — assert on the EXIT CODE (captured past || true), not on '✓':
# doctor always prints ✓ for local checks even when it exits 1 with
# ✗ key validation, so a grep on '✓' could never fail.
R=$(env LEANTIME_URL="$URL" LEANTIME_API_KEY="$KEY" timeout 10 "$BIN" doctor 2>&1 || echo "__DOCTOR_FAILED__")
if ! echo "$R" | grep -q "__DOCTOR_FAILED__" && echo "$R" | grep -q '✓'; then
    echo "  [OK] doctor"
    PASS=$((PASS+1))
else
    echo "  [FAIL] doctor — $(echo "$R" | grep '✗' | head -1)"
    FAIL=$((FAIL+1))
fi

echo ""
echo "=== Results: $PASS passed, $FAIL failed ==="
[ "$FAIL" -eq 0 ]
