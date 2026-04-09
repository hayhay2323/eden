#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT_DIR"

BIND_ADDR="${1:-127.0.0.1:8787}"
TIMEOUT_SECS="${2:-30}"
LOG_PATH="${3:-.run/eden-api.log}"
DB_PATH="${EDEN_API_DB_PATH:-data/eden.db}"

mkdir -p .run

if lsof -nP -iTCP:"${BIND_ADDR##*:}" -sTCP:LISTEN >/dev/null 2>&1; then
  echo "port already in use: ${BIND_ADDR}" >&2
  exit 1
fi

echo "starting eden-api bind=${BIND_ADDR} db=${DB_PATH}"
EDEN_API_DB_PATH="$DB_PATH" nohup ./target/debug/eden-api serve --bind "$BIND_ADDR" >"$LOG_PATH" 2>&1 &
PID=$!
echo "$PID" > .run/eden-api.pid

if ! scripts/ops/wait_for_api.sh "http://${BIND_ADDR}" "$TIMEOUT_SECS"; then
  echo "eden-api failed readiness check; tailing log:" >&2
  tail -n 80 "$LOG_PATH" >&2 || true
  kill "$PID" 2>/dev/null || true
  exit 1
fi

echo "eden-api pid=${PID} ready at http://${BIND_ADDR} db=${DB_PATH}"
