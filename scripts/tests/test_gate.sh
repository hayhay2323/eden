#!/usr/bin/env zsh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target/codex-test}"

cd "$ROOT_DIR"

echo "[gate] using target dir: $TARGET_DIR"
pkill cargo >/dev/null 2>&1 || true
pkill rustc >/dev/null 2>&1 || true

echo "[gate] cargo test --lib --no-run"
CARGO_TARGET_DIR="$TARGET_DIR" cargo test --lib --no-run

echo "[gate] cargo test --lib"
CARGO_TARGET_DIR="$TARGET_DIR" cargo test --lib

echo "[gate] cargo test --bins --no-run"
CARGO_TARGET_DIR="$TARGET_DIR" cargo test --bins --no-run

echo "[gate] cargo test --bins"
CARGO_TARGET_DIR="$TARGET_DIR" cargo test --bins

echo "[gate] complete"
