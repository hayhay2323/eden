#!/usr/bin/env bash
set -euo pipefail

API_BASE="${1:-http://127.0.0.1:8787}"
TIMEOUT_SECS="${2:-30}"

deadline=$((SECONDS + TIMEOUT_SECS))

while [ "$SECONDS" -lt "$deadline" ]; do
  if curl -sf "${API_BASE}/health/report" >/dev/null 2>&1; then
    echo "ready ${API_BASE}"
    exit 0
  fi
  sleep 1
done

echo "timeout waiting for ${API_BASE}/health/report" >&2
exit 1
