set shell := ["zsh", "-lc"]

default:
  @just --list

check:
  cargo check --lib

test:
  cargo test --lib

ci-baseline:
  ./scripts/tests/test_ci_baseline.sh

gate:
  ./scripts/tests/test_gate.sh

gate-persist:
  ./scripts/tests/test_gate_persistence.sh

api:
  cargo run --bin eden-api -- serve

api-persist:
  cargo run --features persistence --bin eden-api -- serve

hk:
  cargo run

hk-persist:
  cargo run --features persistence

us:
  cargo run -- us

us-persist:
  cargo run --features persistence -- us

mint-key label="frontend" ttl="24" scope="frontend:readonly":
  cargo run --bin eden-api -- mint-key --label {{label}} --ttl-hours {{ttl}} --scope {{scope}}

frontend-install:
  cd frontend && npm install

frontend-dev:
  cd frontend && npm run dev

frontend-build:
  cd frontend && npm run build

health-report:
  node scripts/ops/health_report.mjs
