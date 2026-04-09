#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
cd "$ROOT_DIR"

MARKET="${1:-hk}"
API_BIND="${2:-127.0.0.1:8787}"
DB_PATH="${3:-$ROOT_DIR/.run/stack-${MARKET}.db}"
API_TIMEOUT="${4:-30}"

mkdir -p .run

RUNTIME_LOG=".run/eden-${MARKET}.log"
API_LOG=".run/eden-api-${MARKET}.log"

if [[ "$MARKET" != "hk" && "$MARKET" != "us" ]]; then
  echo "usage: start_stack.sh [hk|us] [bind_addr] [db_path] [api_timeout_secs]" >&2
  exit 1
fi

echo "starting ${MARKET} runtime db=${DB_PATH}"
if [[ "$MARKET" == "hk" ]]; then
  EDEN_DB_PATH="$DB_PATH" nohup ./target/debug/eden >"$RUNTIME_LOG" 2>&1 &
else
  EDEN_DB_PATH="$DB_PATH" nohup ./target/debug/eden us >"$RUNTIME_LOG" 2>&1 &
fi
RUNTIME_PID=$!
echo "$RUNTIME_PID" > ".run/eden-${MARKET}.pid"

EDEN_API_DB_PATH="$DB_PATH" scripts/ops/start_api.sh "$API_BIND" "$API_TIMEOUT" "$API_LOG"

echo "${MARKET} runtime pid=${RUNTIME_PID} log=${RUNTIME_LOG}"
echo "api log=${API_LOG}"
