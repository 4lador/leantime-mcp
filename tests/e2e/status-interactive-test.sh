#!/bin/bash
# Interactive status resolution test — validates the restore's ability to
# detect missing custom statuses, prompt the user, and resolve via re-fetch.
#
# Usage: tests/e2e/status-interactive-test.sh [1|5|all]
#   1   = single missing status
#   5   = five missing statuses with partial retry
#   all = both scenarios (default)
#
# SAFETY: This test ONLY targets the local docker Leantime instance
# (localhost:8090). It never touches any other instance.

set -euo pipefail

SCENARIO="${1:-all}"
BIN="${BIN:-$HOME/.local/bin/leantmcp}"
TS=$(date +%s)
LOG_DIR=$(mktemp -d)
DB_CMD="docker exec leantime-mcp-leantime-db-1 sh -c"
DB_SQL="mysql -uleantime -pleantime-dev leantime -N -s -e"
PASS=0; FAIL=0

# --- Safety: refuse to run if not targeting localhost ---
if ! curl -s -o /dev/null -w "%{http_code}" --max-time 3 http://localhost:8090 2>/dev/null | grep -q "^[23]"; then
    echo "✗ Local Leantime not reachable at localhost:8090 — aborting."
    exit 1
fi

log()  { echo "→ $*"; }
ok()   { echo "  ✓ $*"; PASS=$((PASS+1)); }
fail() { echo "  ✗ $*"; FAIL=$((FAIL+1)); }

# --- Helpers ---

wait_for_pattern() {
    local pattern="$1" logfile="$2" timeout="${3:-120}"
    local elapsed=0
    while [ $elapsed -lt $timeout ]; do
        if grep -q "$pattern" "$logfile" 2>/dev/null; then
            return 0
        fi
        sleep 1
        elapsed=$((elapsed+1))
    done
    return 1
}

create_project() {
    local name="$1"
    local result
    result=$(LEANTIME_INSTANCE=local timeout 30 "$BIN" serve <<EOF 2>/dev/null | tail -1
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"leantime_create_project","arguments":{"name":"$name","clientId":"1"}}}
EOF
    )
    echo "$result" | python3 -c "import json,sys; r=json.load(sys.stdin); d=json.loads(r['result']['content'][0]['text']); print(d.get('id',''))" 2>/dev/null
}

insert_status() {
    local project_id="$1" label="$2"
    $DB_CMD "$DB_SQL \"INSERT INTO zp_status_labels (project, label, statusType, class, sortKey) VALUES ($project_id, '$label', 'BLOCKED', 'label-danger', 10);\"" 2>/dev/null
}

create_ticket_with_status() {
    local project_id="$1" headline="$2" status_id="$3"
    # Create ticket first
    local result
    result=$(LEANTIME_INSTANCE=local timeout 30 "$BIN" serve <<EOF 2>/dev/null | tail -1
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"leantime_create_ticket","arguments":{"projectId":"$project_id","headline":"$headline","editorId":"1"}}}
EOF
    )
    local tid=$(echo "$result" | python3 -c "import json,sys; r=json.load(sys.stdin); d=json.loads(r['result']['content'][0]['text']); print(d.get('id',''))" 2>/dev/null)

    # Then update its status via SQL (faster than MCP for test setup)
    $DB_CMD "$DB_SQL \"UPDATE zp_tickets SET status=$status_id WHERE id=$tid;\"" 2>/dev/null
    echo "$tid"
}

get_new_project_id() {
    local logfile="$1"
    grep -o 'project id=[0-9]*' "$logfile" | head -1 | grep -o '[0-9]*'
}

verify_ticket_status() {
    local ticket_id="$1" expected_label="$2"
    sleep 7  # rate limit
    local result
    result=$(LEANTIME_INSTANCE=local timeout 30 "$BIN" serve <<EOF 2>/dev/null | tail -1
{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"test","version":"0"}}}
{"jsonrpc":"2.0","method":"notifications/initialized"}
{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"leantime_get_ticket","arguments":{"projectId":"0","ticketId":"$ticket_id"}}}
EOF
    )
    echo "$result" | python3 -c "
import json,sys
r = json.load(sys.stdin)
text = r['result']['content'][0]['text']
d = json.loads(text)
print(d.get('statusLabel','?'))
" 2>/dev/null
}

cleanup_projects() {
    local -a pids=("$@")
    for pid in "${pids[@]}"; do
        [ -z "$pid" ] && continue
        $DB_CMD "$DB_SQL \"DELETE FROM zp_tickets WHERE projectId=$pid; DELETE FROM zp_projects WHERE id=$pid; DELETE FROM zp_status_labels WHERE project=$pid;\"" 2>/dev/null || true
    done
}

# --- Interactive restore with FIFO ---
run_interactive_restore() {
    local backup_file="$1" logfile="$2"
    shift 2
    local -a first_batch=("${@:1:$#/2}")  # not reliable, use explicit params
    # We'll pass the batches differently
}

# --- Scénario A : 1 missing status ---
test_single_gap() {
    echo ""
    echo "═══ Scénario A: 1 missing status ═══"
    SETUP_PID="" RESTORED_PID=""

    trap 'cleanup_projects "$SETUP_PID" "$RESTORED_PID"; trap - EXIT' EXIT

    log "Creating test project with 1 custom status..."
    SETUP_PID=$(create_project "test-1gap-$TS")
    if [ -z "$SETUP_PID" ]; then fail "Could not create project"; return; fi
    ok "Project created: id=$SETUP_PID"

    log "Inserting custom status 'Blocked'..."
    insert_status "$SETUP_PID" "Blocked"

    # Get the status ID
    local blocked_id=$($DB_CMD "$DB_SQL \"SELECT id FROM zp_status_labels WHERE project=$SETUP_PID AND label='Blocked';\"" 2>/dev/null | tr -d '[:space:]')
    if [ -z "$blocked_id" ]; then fail "Could not insert status"; return; fi
    ok "Status 'Blocked' inserted (id=$blocked_id)"

    log "Creating ticket with 'Blocked' status..."
    local tid=$(create_ticket_with_status "$SETUP_PID" "blocked-ticket" "$blocked_id")
    if [ -z "$tid" ]; then fail "Could not create ticket"; return; fi
    ok "Ticket created: id=$tid"

    log "Backing up..."
    sleep 7
    local backup_file
    backup_file=$(LEANTIME_INSTANCE=local timeout 120 "$BIN" backup --project "$SETUP_PID" 2>&1 | grep -o '/[^ ]*\.json' | head -1)
    if [ -z "$backup_file" ]; then fail "Backup failed"; return; fi
    ok "Backup: $backup_file"

    log "Hiding original project..."
    $DB_CMD "$DB_SQL \"UPDATE zp_projects SET state=-1 WHERE id=$SETUP_PID;\"" 2>/dev/null

    # --- Interactive restore ---
    log "Starting interactive restore..."
    local logfile="$LOG_DIR/restore-1gap.log"
    local fifo="$LOG_DIR/stdin-1gap"
    mkfifo "$fifo"

    # Start restore reading from FIFO
    LEANTIME_INSTANCE=local timeout 300 "$BIN" restore "$backup_file" --confirm < "$fifo" > "$logfile" 2>&1 &
    local restore_pid=$!
    exec 3>"$fifo"

    # Wait for the prompt
    if ! wait_for_pattern "Press Enter when done" "$logfile" 30; then
        fail "Interactive prompt did not appear"
        kill $restore_pid 2>/dev/null; exec 3>&-; rm -f "$fifo"
        return
    fi
    ok "Prompt appeared with gap detection"

    # Verify the gap is "Blocked"
    if grep -q "Blocked" "$logfile"; then
        ok "Gap correctly identified as 'Blocked'"
    else
        fail "Gap not identified as 'Blocked'"
    fi

    # Create the status on the new project
    RESTORED_PID=$(get_new_project_id "$logfile")
    if [ -z "$RESTORED_PID" ]; then
        fail "Could not find restored project ID"
        kill $restore_pid 2>/dev/null; exec 3>&-; rm -f "$fifo"
        return
    fi
    log "Restored project: id=$RESTORED_PID"

    log "Creating 'Blocked' status on restored project..."
    insert_status "$RESTORED_PID" "Blocked"

    # Send Enter
    echo "" >&3
    ok "Enter sent — waiting for resolution..."

    # Wait for completion
    wait $restore_pid || true
    exec 3>&-
    rm -f "$fifo"

    # Check the log for success
    if grep -q "Blocked.*found" "$logfile" || grep -q "All statuses" "$logfile"; then
        ok "Status resolved after Enter"
    else
        fail "Status not resolved after Enter"
        cat "$logfile" | tail -5
    fi

    # Verify the ticket has "Blocked" status
    # Find the restored ticket
    local restored_tid=$($DB_CMD "$DB_SQL \"SELECT id FROM zp_tickets WHERE projectId=$RESTORED_PID LIMIT 1;\"" 2>/dev/null | tr -d '[:space:]')
    if [ -n "$restored_tid" ]; then
        local status_label=$(verify_ticket_status "$restored_tid" "Blocked")
        if [ "$status_label" = "Blocked" ]; then
            ok "Ticket has status 'Blocked' ✓"
        else
            fail "Ticket status is '$status_label' (expected 'Blocked')"
        fi
    fi

    # Cleanup
    cleanup_projects "$SETUP_PID" "$RESTORED_PID"
    trap - EXIT
}

# --- Scénario B : 5 missing statuses with partial retry ---
test_five_gaps() {
    echo ""
    echo "═══ Scénario B: 5 missing statuses (partial retry) ═══"
    SETUP_PID="" RESTORED_PID=""
    local -a statuses=("Blocked" "In Review" "Deployed" "On Hold" "Escalated")
    local -a tids=()

    trap 'cleanup_projects "$SETUP_PID" "$RESTORED_PID"; trap - EXIT' EXIT

    log "Creating test project with 5 custom statuses..."
    SETUP_PID=$(create_project "test-5gaps-$TS")
    if [ -z "$SETUP_PID" ]; then fail "Could not create project"; return; fi
    ok "Project created: id=$SETUP_PID"

    for i in "${!statuses[@]}"; do
        local label="${statuses[$i]}"
        insert_status "$SETUP_PID" "$label"
        local sid=$($DB_CMD "$DB_SQL \"SELECT id FROM zp_status_labels WHERE project=$SETUP_PID AND label='$label';\"" 2>/dev/null | tr -d '[:space:]')
        if [ -n "$sid" ]; then
            local tid=$(create_ticket_with_status "$SETUP_PID" "test-ticket-$((i+1))" "$sid")
            tids+=("$tid")
            ok "Status '$label' (id=$sid) + ticket $tid"
        else
            fail "Could not insert status '$label'"
            return
        fi
        sleep 7
    done

    log "Backing up..."
    sleep 7
    local backup_file
    backup_file=$(LEANTIME_INSTANCE=local timeout 120 "$BIN" backup --project "$SETUP_PID" 2>&1 | grep -o '/[^ ]*\.json' | head -1)
    if [ -z "$backup_file" ]; then fail "Backup failed"; return; fi
    ok "Backup: $backup_file"

    log "Hiding original project..."
    $DB_CMD "$DB_SQL \"UPDATE zp_projects SET state=-1 WHERE id=$SETUP_PID;\"" 2>/dev/null

    # --- Interactive restore with partial resolution ---
    log "Starting interactive restore (5 gaps, partial retry)..."
    local logfile="$LOG_DIR/restore-5gaps.log"
    local fifo="$LOG_DIR/stdin-5gaps"
    mkfifo "$fifo"

    LEANTIME_INSTANCE=local timeout 600 "$BIN" restore "$backup_file" --confirm < "$fifo" > "$logfile" 2>&1 &
    local restore_pid=$!
    exec 3>"$fifo"

    # Wait for the prompt
    if ! wait_for_pattern "Press Enter when done" "$logfile" 30; then
        fail "Interactive prompt did not appear"
        kill $restore_pid 2>/dev/null; exec 3>&-; rm -f "$fifo"
        return
    fi
    ok "Prompt appeared"

    # Verify all 5 gaps are shown
    local gap_count=$(grep -c "•" "$logfile" || true)
    if [ "$gap_count" -ge 5 ]; then
        ok "All 5 gaps shown at once ($gap_count found)"
    else
        fail "Expected 5 gaps, found $gap_count"
    fi

    RESTORED_PID=$(get_new_project_id "$logfile")
    if [ -z "$RESTORED_PID" ]; then
        fail "Could not find restored project ID"
        kill $restore_pid 2>/dev/null; exec 3>&-; rm -f "$fifo"
        return
    fi
    log "Restored project: id=$RESTORED_PID"

    # ROUND 1: Create only 3 of 5 statuses
    log "Round 1: creating 3 of 5 statuses..."
    for label in "Blocked" "In Review" "Deployed"; do
        insert_status "$RESTORED_PID" "$label"
    done

    echo "" >&3
    ok "Enter sent (round 1)"

    # Wait for "Still missing" (retry prompt)
    if wait_for_pattern "Still missing\|not found\|missing" "$logfile" 30; then
        ok "Retry prompt appeared for remaining statuses"

        # ROUND 2: Create the remaining 2
        log "Round 2: creating remaining 2 statuses..."
        for label in "On Hold" "Escalated"; do
            insert_status "$RESTORED_PID" "$label"
        done

        echo "" >&3
        ok "Enter sent (round 2)"
    else
        log "No retry prompt — all statuses may have been resolved in round 1"
    fi

    # Wait for restore to complete
    wait $restore_pid || true
    exec 3>&-
    rm -f "$fifo"

    # Check the log
    if grep -q "Restore complete" "$logfile"; then
        ok "Restore completed successfully"
    else
        fail "Restore did not complete"
        tail -5 "$logfile"
    fi

    # Verify all 5 tickets have correct statuses
    log "Verifying ticket statuses..."
    local restored_tids=$($DB_CMD "$DB_SQL \"SELECT id FROM zp_tickets WHERE projectId=$RESTORED_PID ORDER BY id;\"" 2>/dev/null | tr '\n' ' ')
    local tid_array=($restored_tids)

    for i in "${!statuses[@]}"; do
        local expected="${statuses[$i]}"
        local tid="${tid_array[$i]:-}"
        [ -z "$tid" ] && continue

        local actual=$(verify_ticket_status "$tid" "$expected")
        if [ "$actual" = "$expected" ]; then
            ok "Ticket $tid: '$expected' ✓"
        else
            fail "Ticket $tid: got '$actual', expected '$expected'"
        fi
    done

    # Cleanup
    cleanup_projects "$SETUP_PID" "$RESTORED_PID"
    trap - EXIT
}

# --- Main ---
echo "=== Interactive Status Resolution Test ==="
echo "Target: localhost:8099 (local docker ONLY)"
echo ""

case "$SCENARIO" in
    1)   test_single_gap ;;
    5)   test_five_gaps ;;
    all) test_single_gap && test_five_gaps ;;
    *)   echo "Usage: $0 [1|5|all]"; exit 1 ;;
esac

echo ""
echo "═══ Results: $PASS passed, $FAIL failed ═══"
rm -rf "$LOG_DIR"
[ "$FAIL" -eq 0 ]
