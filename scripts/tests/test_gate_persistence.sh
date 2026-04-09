#!/usr/bin/env zsh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target/codex-test-persistence}"

cd "$ROOT_DIR"

echo "[gate:persistence] using target dir: $TARGET_DIR"
pkill cargo >/dev/null 2>&1 || true
pkill rustc >/dev/null 2>&1 || true

echo "[gate:persistence] cargo test --lib --features persistence --no-run"
CARGO_TARGET_DIR="$TARGET_DIR" cargo test --lib --features persistence --no-run

echo "[gate:persistence] queue pin conflict helper"
CARGO_TARGET_DIR="$TARGET_DIR" cargo test --lib queue_pin_conflict_rule_matches_api_expectation

echo "[gate:persistence] queue pin endpoint"
CARGO_TARGET_DIR="$TARGET_DIR" cargo test --lib --features persistence post_case_queue_pin_sets_and_clears_marker

echo "[gate:persistence] complete"
