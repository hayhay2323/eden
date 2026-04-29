#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$repo_root"

report_sizes() {
  du -sh target/debug target/debug/deps target/debug/build target/debug/.fingerprint 2>/dev/null || true
}

echo "== target/debug before =="
report_sizes

# Keep the cleanup narrow: trim the workspace crate plus the SurrealDB /
# RocksDB chain that dominates debug artifact size when `persistence` is
# enabled, without wiping the whole target dir.
cargo clean --quiet \
  -p eden \
  -p surrealdb \
  -p surrealdb-core \
  -p surrealdb-librocksdb-sys

# Finder / shell retries have occasionally left zero-byte duplicate top-level
# binaries (`eden 2`, `eden 3`, ...). They do not help Cargo and just add
# directory noise.
find target/debug -maxdepth 1 -type f \
  \( -name 'eden [0-9]*' \
  -o -name 'eden-api [0-9]*' \
  -o -name 'analyze [0-9]*' \
  -o -name 'dbpeek [0-9]*' \
  -o -name 'replay [0-9]*' \
  -o -name 'test_* [0-9]*' \) \
  -delete 2>/dev/null || true

echo
echo "== target/debug after =="
report_sizes
