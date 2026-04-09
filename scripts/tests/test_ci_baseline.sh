#!/usr/bin/env zsh
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "$0")/../.." && pwd)"
TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT_DIR/target/ci-baseline}"

cd "$ROOT_DIR"

echo "[ci-baseline] using target dir: $TARGET_DIR"

echo "[ci-baseline] runtime loop contracts"
CARGO_TARGET_DIR="$TARGET_DIR" cargo test --locked --lib runtime_loop::tests

echo "[ci-baseline] knowledge lifecycle invariants"
CARGO_TARGET_DIR="$TARGET_DIR" cargo test --locked --lib ontology::store::knowledge::tests
CARGO_TARGET_DIR="$TARGET_DIR" cargo test --locked --lib ontology::store::init::tests

echo "[ci-baseline] replay fingerprint regression"
CARGO_TARGET_DIR="$TARGET_DIR" cargo test --locked --bin replay

echo "[ci-baseline] complete"
