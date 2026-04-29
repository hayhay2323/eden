# Eden Repo Tree Convergence

This document defines the target repository tree after the current ontology/frontend refactor.

The goal is not to create a perfect taxonomy.
The goal is to make the repo readable, navigable, and stable enough that product work stops competing with tree hygiene.

## 1. Current Problem

The repo has three different kinds of content mixed together:

- product/runtime code
- frontend application code
- local execution artifacts and operational tooling

The core issue is not that there are "too many folders".
The issue is that local runtime outputs, internal scripts, logs, and product modules all compete for attention at the top level.

## 2. Top-Level Contract

The intended top-level tree should be read like this:

- `src/`
  Rust product/runtime code
- `frontend/`
  React application
- `docs/`
  architecture and domain docs
- `scripts/`
  operational helper scripts and local tooling
- `config/`
  small static configuration and schemas
- `crates/`
  auxiliary Rust crates
- `.run/`
  local transient run artifacts only
- `logs/`
  local logs only
- `data/`
  local runtime/state/output only

Everything else should either be:

- tiny repo metadata
- build system files
- or removed

## 3. What Is Canonical

### Canonical product code

- `src/`
- `frontend/src/`

### Canonical documentation

- `docs/architecture/`
- `docs/domain/`

### Canonical tooling

- `scripts/`

### Non-canonical local artifacts

- `.run/`
- `logs/`
- `data/`
- `frontend/dist/`
- `frontend/*.tsbuildinfo`
- `target/`

These must not shape architectural decisions.

## 4. Rust Tree Target

The Rust tree is already moving in the right direction.
The target mental model should be:

- `src/api/`
  external surfaces
- `src/ontology/`
  object contracts, navigation, graph identity
- `src/core/`
  runtime coordination
- `src/agent/`
  agent-facing views/builders/tools
- `src/cases/`
  workflow/case domain
- `src/runtime_tasks/`
  runtime task lifecycle and shared execution state
- `src/persistence/`
  storage
- `src/pipeline/`
  analysis/derivation
- `src/hk/`, `src/us/`
  market-specific runtime adapters

The following should be treated as supporting or legacy-heavy areas:

- `src/graph/`
- `src/temporal/`
- `src/external/`
- `src/bridges/`
- `src/logic/`
- `src/trading/`

The goal is not necessarily to delete those directories now.
The goal is to stop letting them define the repo's primary mental model.

`src/runtime_tasks/` now exists because runtime lifecycle is a first-class product concern, not just a shell/process detail.

## 5. Frontend Tree Target

The frontend should remain converged around:

- `frontend/src/routes/`
  page composition only
- `frontend/src/features/desk/`
  shared desk/workbench panels
- `frontend/src/features/desk/object-inspector/`
  shared object drill-down system
- `frontend/src/features/workspace/`
  workspace-specific boards and view-models
- `frontend/src/features/signals/`
  signals-specific boards and view-models
- `frontend/src/features/workbench/`
  small cross-page UI helpers
- `frontend/src/lib/api/`
  API contracts/client
- `frontend/src/lib/query/`
  query/loading policy
- `frontend/src/state/`
  global app state
- `frontend/src/shell/`
  shell layout and route error handling
- `frontend/src/styles/`
  shared styles only

Do not introduce:

- page-specific inspectors
- duplicate board implementations inside `routes/`
- a generic component system larger than the current product actually needs

## 6. Tooling Tree Target

`scripts/` should stay small and operational, with lightweight grouping:

- `scripts/analysis/`
- `scripts/data/`
- `scripts/ops/`
- `scripts/tests/`

If a script becomes:

- product-critical
- imported by runtime
- or part of canonical operator workflow

it should be reconsidered as product code, not left as a loose script forever.

## 7. What We Already Did

This convergence work is already in motion:

- frontend routes are now composition-heavy instead of monolithic
- frontend object inspector has been split into submodules
- frontend workbench state is object-first
- backend ontology/query/feed surfaces are split
- local transient `.run/` artifacts are now explicitly ignored
- frontend build artifacts and tsbuildinfo are now explicitly ignored

## 8. Immediate Rules

From this point forward:

1. Do not add new tracked artifacts under `.run/`, `logs/`, `data/`, or `frontend/dist/`.
2. Do not put product rendering logic into `frontend/src/routes/`.
3. Do not add new top-level directories unless they hold a genuinely new subsystem.
4. Prefer extending `features/<domain>/` over creating new cross-cutting folders.
5. Treat `docs/` as the place to freeze architecture intent once a structure is good enough.

## 9. Next File-Tree Moves

If further convergence is needed later, the next safe moves are:

- split `src/scripts`-worthy operational concerns more clearly from product runtime
- reduce the conceptual weight of `src/graph/` and `src/temporal/` in favor of `ontology/`, `cases/`, and `core/`
- keep frontend additions inside `workspace/`, `signals/`, or `desk/` instead of inventing new feature roots

These are secondary.
The repo is already converged enough to prioritize product execution over further tree cleanup.

## 10. Stop Line

Do not keep reorganizing the tree unless one of these is true:

- a directory is blocking feature work
- ownership is genuinely ambiguous
- repeated code is landing in the wrong layer

Otherwise, tree work should stop and product work should continue.
