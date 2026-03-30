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
| [src/ontology/contracts/types.rs](/Users/hayhay2323/Desktop/eden/src/ontology/contracts/types.rs) | `aligned` | Contracts now expose `summary`, `navigation`, `relationships`, `graph_ref`, and `history_refs`. |

## 3. Partial Migrations

| Component | Status | Notes |
| --- | --- | --- |
| [src/agent/tools.rs](/Users/hayhay2323/Desktop/eden/src/agent/tools.rs) | `partial` | Object-first and graph-first replacement tools now exist, but derived analyst views still remain on `agent/*`. |

### `src/agent/tools.rs` route breakdown

#### Already on primary surfaces

- `market_session` -> `/api/ontology/:market/market-session`
- `symbol_contract` -> `/api/ontology/:market/symbols/:symbol`
- `transitions_since` -> `/api/feed/:market/transitions`
- `notices` -> `/api/feed/:market/notices`
- `macro_event_contracts` -> `/api/ontology/:market/macro-events`
- `graph_macro_event_candidates` -> `/api/ontology/:market/macro-event-candidates`
- `graph_knowledge_links` -> `/api/ontology/:market/knowledge-links`
- `world_state` -> `/api/ontology/:market/world`
- `backward_investigation` -> `/api/ontology/:market/backward/:symbol`
- `sector_flow` -> `/api/ontology/:market/sector-flows`

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
- `macro_event_candidates`
- `macro_events`
- `knowledge_links`

### Explicit replacement map

- `session` -> `market_session`
- `structure_state` -> `symbol_contract`
- `symbol_state` -> `symbol_contract`
- `invalidation_status` -> `symbol_contract`
- `macro_event_candidates` -> `graph_macro_event_candidates`
- `macro_events` -> `macro_event_contracts`
- `knowledge_links` -> `graph_knowledge_links`

## 4. Blocked-By-Shape Consumers

These consumers should not be switched blindly. They depend on an `agent/*` view that is not a
1:1 alias of the primary surfaces.

| Consumer | Status | Why |
| --- | --- | --- |
| [src/agent/tools.rs](/Users/hayhay2323/Desktop/eden/src/agent/tools.rs) `watchlist` | `blocked-by-shape` | `watchlist` is a ranked analyst view, not an ontology object list. |
| [src/agent/tools.rs](/Users/hayhay2323/Desktop/eden/src/agent/tools.rs) `recommendations` | `blocked-by-shape` | Returns the derived agent recommendation payload, not raw recommendation contracts. |
| [src/agent/tools.rs](/Users/hayhay2323/Desktop/eden/src/agent/tools.rs) `threads` / `turns` | `blocked-by-shape` | These remain analyst/session views rather than ontology objects. |
| [src/agent/tools.rs](/Users/hayhay2323/Desktop/eden/src/agent/tools.rs) `depth_change` / `broker_movement` / `invalidation_status` | `blocked-by-shape` | Still consume compatibility-shaped drill-down payloads rather than direct contract-specific tool specs. |
| [src/agent/tools.rs](/Users/hayhay2323/Desktop/eden/src/agent/tools.rs) `active_structures` | `blocked-by-shape` | Still a compatibility list view; the ontology-native replacement should be relationship-aware case/recommendation traversal, not a 1:1 route swap. |

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

Deprecate the old flat navigation fields in docs and downstream consumers so that:

- `navigation` becomes the primary traversal entry
- `relationships` become the primary object linkage entry
- `graph_ref` / `history_refs` become supporting navigation data

### Step 2

Migrate tracked non-Rust consumers if and when they are added to git.

Priority order:

1. `scripts/run_codex_analyst.py`
2. any frontend API client
3. any operational notebooks or automation wrappers

### Step 3

Once downstream consumers are moved, mark `agent/*` as deprecated in docs and health/reporting
output, while keeping it available as a compatibility layer.
