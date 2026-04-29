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

echo "[ci-baseline] persistence feature check (catches compile errors in #[cfg(feature = \"persistence\")] blocks)"
CARGO_TARGET_DIR="$TARGET_DIR" cargo check --locked --features persistence --lib

# 2026-04-19 — feature-gated test safety net.
# Default `cargo test` does not exercise #[cfg(feature = "...")] test
# bodies. Run the feature-combined lib suite so both compile drift and
# feature-specific behavioural regressions fail CI.
echo "[ci-baseline] persistence+coordinator lib tests"
CARGO_TARGET_DIR="$TARGET_DIR" cargo test --locked --features persistence,coordinator --lib

echo "[ci-baseline] complete"
