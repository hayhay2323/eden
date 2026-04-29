#!/usr/bin/env bash
# Generate index.jsonl for a decisions/YYYY/MM/DD directory.
#
# Usage:
#   ./scripts/ops/generate_decision_index.sh decisions/2026/04/15
#
# For each *.json file in the directory (excluding index.jsonl and
# session-recap.json), emit one line in index.jsonl with a compact
# summary: decision_id, timestamp, action, market, symbol, file.
#
# Output is sorted lexicographically, which is equivalent to chronological
# order since decision_id begins with ISO8601 timestamp.
#
# Exit codes:
#   0 on success
#   1 on bad usage
#   2 on invalid input directory
#   3 if any decision file fails to parse

set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "Usage: $0 <day_dir>" >&2
  echo "Example: $0 decisions/2026/04/15" >&2
  exit 1
fi

DAY_DIR="$1"

if [[ ! -d "$DAY_DIR" ]]; then
  echo "error: not a directory: $DAY_DIR" >&2
  exit 2
fi

INDEX_PATH="$DAY_DIR/index.jsonl"
TMP_PATH="$(mktemp)"
trap 'rm -f "$TMP_PATH"' EXIT

count=0
shopt -s nullglob
for f in "$DAY_DIR"/*.json; do
  base="$(basename "$f")"

  # Skip the index itself and session-recap (different schema)
  case "$base" in
    index.jsonl) continue ;;
    session-recap.json) continue ;;
  esac

  if ! jq -c --arg fname "$base" '{
    decision_id,
    timestamp,
    action,
    market,
    symbol,
    file: $fname
  }' "$f" >> "$TMP_PATH" 2>/dev/null; then
    echo "error: failed to parse $f" >&2
    exit 3
  fi

  count=$((count + 1))
done
shopt -u nullglob

# Sort lexicographically — decision_id starts with ISO8601 timestamp,
# so this equals chronological order. Ties break on symbol alphabetically.
sort "$TMP_PATH" > "$INDEX_PATH"

echo "wrote $count decisions → $INDEX_PATH"
