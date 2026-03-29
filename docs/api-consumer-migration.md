# Eden API Consumer Migration Checklist

This checklist tracks repo-internal consumers that still rely on compatibility-shaped `agent/*`
 surfaces versus the primary `ontology/*` and `feed/*` surfaces.

Use this as the migration plan for future cleanup work.

## Status Legend

- `compat-intentional`
  The consumer is expected to remain on `agent/*` because it is the compatibility layer itself.

- `aligned`
  The consumer now prefers `ontology/*` and/or `feed/*`.

- `partial`
  The consumer has moved where payload shape stayed stable, but still uses `agent/*` for derived
  analyst views.

- `blocked-by-shape`
  The consumer still depends on `agent/*` because the replacement would require changing payload
  shape or downstream assumptions.

## 1. Compatibility Layer Modules

These are not migration targets. They define or preserve the compatibility surface.

| Component | Status | Notes |
| --- | --- | --- |
| [src/api/agent_api.rs](/Users/hayhay2323/Desktop/eden/src/api/agent_api.rs) | `compat-intentional` | Legacy `/agent/*` HTTP handlers. Some already proxy to feed/object helpers. |
| [src/api/agent_surface.rs](/Users/hayhay2323/Desktop/eden/src/api/agent_surface.rs) | `compat-intentional` | Legacy `/stream/agent/*` SSE handlers. |
| [src/api/agent_graph.rs](/Users/hayhay2323/Desktop/eden/src/api/agent_graph.rs) | `compat-intentional` | Thin wrappers over `ontology_graph_api`. |
| [src/api/core/router.rs](/Users/hayhay2323/Desktop/eden/src/api/core/router.rs) | `compat-intentional` | Still exposes legacy routes by design. |

## 2. Internal Consumers Already Aligned

| Component | Status | Notes |
| --- | --- | --- |
| [src/agent_llm/protocol.rs](/Users/hayhay2323/Desktop/eden/src/agent_llm/protocol.rs) | `aligned` | Tool policy now frames `watchlist`/`recommendations` as derived views and prioritizes feed/object drill-down. |
| [docs/api-surfaces.md](/Users/hayhay2323/Desktop/eden/docs/api-surfaces.md) | `aligned` | Documents `agent/*` as compatibility only. |

## 3. Partial Migrations

| Component | Status | Notes |
| --- | --- | --- |
| [src/agent/tools.rs](/Users/hayhay2323/Desktop/eden/src/agent/tools.rs) | `partial` | `notices`, `transitions_since`, `world_state`, and `backward_investigation` now point to primary surfaces. |

### `src/agent/tools.rs` route breakdown

#### Already on primary surfaces

- `transitions_since` -> `/api/feed/:market/transitions`
- `notices` -> `/api/feed/:market/notices`
- `world_state` -> `/api/ontology/:market/world`
- `backward_investigation` -> `/api/ontology/:market/backward/:symbol`

#### Still on `agent/*` because they are derived analyst views

- `wake`
- `session`
- `watchlist`
- `recommendations`
- `alert_scoreboard`
- `eod_review`
- `threads`
- `turns`

#### Still on `agent/*` because payload shape is compatibility-oriented

- `active_structures`
- `structure_state`
- `symbol_state`
- `depth_change`
- `broker_movement`
- `invalidation_status`
- `sector_flow`
- `macro_event_candidates`
- `macro_events`
- `knowledge_links`

## 4. Blocked-By-Shape Consumers

These consumers should not be switched blindly. They depend on an `agent/*` view that is not a
1:1 alias of the primary surfaces.

| Consumer | Status | Why |
| --- | --- | --- |
| [src/agent/tools.rs](/Users/hayhay2323/Desktop/eden/src/agent/tools.rs) `watchlist` | `blocked-by-shape` | `watchlist` is a ranked analyst view, not an ontology object list. |
| [src/agent/tools.rs](/Users/hayhay2323/Desktop/eden/src/agent/tools.rs) `recommendations` | `blocked-by-shape` | Returns the derived agent recommendation payload, not raw recommendation contracts. |
| [src/agent/tools.rs](/Users/hayhay2323/Desktop/eden/src/agent/tools.rs) `threads` / `turns` | `blocked-by-shape` | These remain analyst/session views rather than ontology objects. |
| [src/agent/tools.rs](/Users/hayhay2323/Desktop/eden/src/agent/tools.rs) `depth_change` / `broker_movement` / `invalidation_status` | `blocked-by-shape` | Still consume compatibility-shaped drill-down payloads rather than direct contract-specific tool specs. |

## 5. Non-Tracked / Out-Of-Scope Consumers

These are visible in the current worktree but are not tracked in git here, so they were not
included in the migration commits:

- `scripts/`
- `frontend/`
- `justfile`

In practice, the most important known example is:

- `scripts/run_codex_analyst.py`

That script should eventually prefer:

1. `/api/ontology/:market/market-session`
2. `/api/ontology/:market/recommendations`
3. `/api/feed/:market/notices`
4. `/api/feed/:market/transitions`
5. object/detail follow-ups on `ontology/*`

## 6. Recommended Next Steps

### Step 1

Define explicit replacements for the remaining compatibility-shaped tool specs in
[src/agent/tools.rs](/Users/hayhay2323/Desktop/eden/src/agent/tools.rs).

This likely means introducing either:

- object-first tool names, or
- a separate derived-view tool category

instead of pointing everything through `agent/*`.

### Step 2

Migrate tracked non-Rust consumers if and when they are added to git.

Priority order:

1. `scripts/run_codex_analyst.py`
2. any frontend API client
3. any operational notebooks or automation wrappers

### Step 3

Once downstream consumers are moved, mark `agent/*` as deprecated in docs and health/reporting
output, while keeping it available as a compatibility layer.
