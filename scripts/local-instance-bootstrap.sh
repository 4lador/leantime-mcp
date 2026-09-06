#!/usr/bin/env bash
# Bootstrap a local Leantime instance (docker-compose.yml) for e2e testing:
# waits for health, completes the first-run wizard if needed, logs in and
# creates a dedicated API key. Prints the key on stdout (last line).
#
# Env overrides:
#   LEANTIME_URL             (default http://localhost:8090)
#   LEANTIME_ADMIN_EMAIL     (default admin@leantime.local)
#   LEANTIME_ADMIN_PASSWORD  (default DevAdmin1-Pass — fresh-instance default)
set -euo pipefail

URL="${LEANTIME_URL:-http://localhost:8090}"
URL="${URL%/}"
ADMIN_EMAIL="${LEANTIME_ADMIN_EMAIL:-admin@leantime.local}"
ADMIN_PASS="${LEANTIME_ADMIN_PASSWORD:-DevAdmin1-Pass}"
JAR="$(mktemp)"
trap 'rm -f "$JAR"' EXIT

log()  { echo "→ $*" >&2; }
die()  { echo "✗ $*" >&2; exit 1; }

# --- 1. Wait for the instance to answer
log "waiting for ${URL} ..."
for i in $(seq 1 60); do
  CODE="$(curl -s -o /dev/null -w '%{http_code}' -m 5 "$URL" || true)"
  [ "$CODE" != "000" ] && [ -n "$CODE" ] && break
  sleep 5
done
[ "${CODE:-000}" = "000" ] && die "instance unreachable at ${URL}"

# --- 2. First-run wizard if needed
LOCATION="$(curl -s -o /dev/null -w '%{redirect_url}' "$URL")"
if echo "$LOCATION" | grep -q "/install"; then
  log "fresh instance — running install wizard"
  curl -s -m 60 -c "$JAR" -b "$JAR" -X POST "$URL/install" \
    --data-urlencode "email=$ADMIN_EMAIL" \
    --data-urlencode "firstname=Dev" \
    --data-urlencode "lastname=Admin" \
    --data-urlencode "company=leantime-mcp-e2e" \
    --data-urlencode "install=Install" \
    --data-urlencode "installAction=Install" \
    -o /dev/null

  TOKEN="$(docker compose exec -T leantime-db mysql -uroot -proot-dev leantime -N -s \
    -e "SELECT pwReset FROM zp_user WHERE id=1" 2>/dev/null | tr -d '[:space:]')"
  [ -n "$TOKEN" ] || die "could not read the invite token from the database"

  log "completing user invite (5 steps)"
  INV="$URL/auth/userInvite/$TOKEN"
  curl -s -m 30 -c "$JAR" -b "$JAR" -X POST "$INV" \
    --data-urlencode "step=1" --data-urlencode "name=Dev Admin" \
    --data-urlencode "jobTitle=Dev" --data-urlencode "password=$ADMIN_PASS" \
    --data-urlencode "saveAccount=1" --data-urlencode "createAccount=Next" -o /dev/null
  curl -s -m 30 -c "$JAR" -b "$JAR" -X POST "$INV" \
    --data-urlencode "step=2" --data-urlencode "theme=default" \
    --data-urlencode "themeFont=Inter" -o /dev/null
  curl -s -m 30 -c "$JAR" -b "$JAR" -X POST "$INV" \
    --data-urlencode "step=3" --data-urlencode "colormode=light" \
    --data-urlencode "colorscheme=themeDefault" -o /dev/null
  curl -s -m 30 -c "$JAR" -b "$JAR" -X POST "$INV" \
    --data-urlencode "step=4" --data-urlencode "daySchedule-workStart=09:00" \
    --data-urlencode "daySchedule-lunch=12:00" \
    --data-urlencode "daySchedule-workEnd=18:00" -o /dev/null
  curl -s -m 30 -c "$JAR" -b "$JAR" -X POST "$INV" \
    --data-urlencode "step=5" -o /dev/null
fi

# --- 3. Login (fresh wizard ends logged in; existing instances need it)
curl -s -m 30 -c "$JAR" -b "$JAR" -X POST "$URL/auth/login" \
  --data-urlencode "username=$ADMIN_EMAIL" \
  --data-urlencode "password=$ADMIN_PASS" \
  -o /dev/null -w '%{http_code}' >&2 || true
DASH="$(curl -s -o /dev/null -w '%{http_code}' -m 15 -b "$JAR" "$URL/dashboard/home")"
[ "$DASH" = "200" ] || die "login failed (dashboard: $DASH) — set LEANTIME_ADMIN_EMAIL/LEANTIME_ADMIN_PASSWORD"
log "logged in as $ADMIN_EMAIL"

# --- 4. Create a dedicated e2e API key
log "creating e2e API key"
KEYPAGE="$(curl -s -m 30 -c "$JAR" -b "$JAR" -X POST "$URL/api/newApiKey" \
  --data-urlencode "save=1" \
  --data-urlencode "firstname=e2e-$(date +%Y%m%d-%H%M%S)" \
  --data-urlencode "role=50" \
  --data-urlencode "projects[]=0")"
KEY="$(echo "$KEYPAGE" | grep -oE 'lt_[A-Za-z0-9_-]{20,}' | head -1 || true)"
[ -n "$KEY" ] || die "could not extract the API key from the response"

log "API key created"
echo "$KEY"
