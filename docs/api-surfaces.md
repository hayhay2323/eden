# Eden API Surfaces

This document defines the intended API entry points for Eden after the ontology/object-contract refactor.

The short version:

- `agent/*` is a compatibility surface.
- `feed/*` is the operational event/feed surface.
- `ontology/*` is the primary object/query surface.
- `stream/feed/*` and `stream/ontology/*` are the preferred streaming paths.

## 1. Surface Roles

### `agent/*`

Use this only for legacy consumers that still expect the older agent-shaped API.

Examples:

- `/api/agent/:market/session`
- `/api/agent/:market/watchlist`
- `/api/agent/:market/recommendations`
- `/api/agent/:market/history/*`
- `/api/agent/:market/graph/*`

Rules:

- Treated as compatibility routes.
- Allowed to proxy or derive from newer ontology/feed layers.
- New product work should avoid depending on these paths as the primary contract.

### `feed/*`

Use this for event-like, operator-facing feed data that is not an ontology object.

Examples:

- `/api/feed/:market/notices`
- `/api/feed/:market/transitions`
- `/api/stream/feed/:market/notices`
- `/api/stream/feed/:market/transitions`

Rules:

- Feed payloads represent recent activity, operator alerts, or evolving transitions.
- Feed payloads are not the canonical object contract.
- These routes are the preferred replacement for feed-like `agent/*` paths.

### `ontology/*`

Use this as the main application/query surface.

Examples:

- `/api/ontology/:market/operational-snapshot`
- `/api/ontology/:market/navigation/:kind/:id`
- `/api/ontology/:market/neighborhood/:kind/:id`
- `/api/ontology/:market/market-session`
- `/api/ontology/:market/symbols`
- `/api/ontology/:market/cases`
- `/api/ontology/:market/recommendations`
- `/api/ontology/:market/workflows`
- `/api/ontology/:market/world`
- `/api/ontology/:market/graph/*`

Rules:

- This is the preferred surface for new application code.
- Object contracts live here.
- History refs and graph/world query paths also belong here.
- Navigation and neighborhood traversal also belong here.

## 2. Object Surface

The primary application objects are:

- `MarketSession`
- `SymbolState`
- `Case`
- `Recommendation`
- `MacroEvent`
- `Thread`
- `Workflow`

Primary routes:

- `/api/ontology/:market/market-session`
- `/api/ontology/:market/symbols`
- `/api/ontology/:market/symbols/:symbol`
- `/api/ontology/:market/cases`
- `/api/ontology/:market/cases/:case_id`
- `/api/ontology/:market/recommendations`
- `/api/ontology/:market/recommendations/:recommendation_id`
- `/api/ontology/:market/macro-events`
- `/api/ontology/:market/macro-events/:event_id`
- `/api/ontology/:market/threads`
- `/api/ontology/:market/threads/:thread_id`
- `/api/ontology/:market/workflows`
- `/api/ontology/:market/workflows/:workflow_id`

Contract note:

- Primary consumers should prefer each object's `summary`, `navigation`, and `relationships`
  before falling back to legacy payload-shaped fields.
- `navigation` is now the preferred traversal entry.
- `neighborhood` is the expanded traversal view.
- `graph_ref` and `history_refs` remain available, but should be treated as inputs into
  `navigation`, not as the first thing a new consumer reads.

## 3. History Surface

History should hang off ontology objects, not off ad hoc agent payloads.

Preferred routes:

- `/api/ontology/:market/navigation/:kind/:id`
- `/api/ontology/:market/cases/:case_id/history/workflow`
- `/api/ontology/:market/cases/:case_id/history/reasoning`
- `/api/ontology/:market/cases/:case_id/history/outcomes`
- `/api/ontology/:market/recommendations/:recommendation_id/history`
- `/api/ontology/:market/workflows/:workflow_id/history`

Contract note:

- `history_refs` on object contracts are the discovery mechanism.
- Consumers should follow those refs instead of inventing URL patterns.

## 4. World And Graph Query Surface

World state and graph queries belong to the ontology/query layer.

Preferred routes:

- `/api/ontology/:market/macro-event-candidates`
- `/api/ontology/:market/knowledge-links`
- `/api/ontology/:market/world`
- `/api/ontology/:market/graph/node/:node_id`
- `/api/ontology/:market/graph/links`
- `/api/ontology/:market/graph/history/macro-events`
- `/api/ontology/:market/graph/history/knowledge-links`
- `/api/ontology/:market/graph/state/macro-events`
- `/api/ontology/:market/graph/state/knowledge-links`

Compatibility note:

- `/api/agent/:market/world`
- `/api/agent/:market/history/*`
- `/api/agent/:market/state/*`
- `/api/agent/:market/graph/*`

These remain available for older consumers, but they are not the preferred contract.

## 5. Streaming Surface

Preferred streaming routes:

- `/api/stream/feed/:market/notices`
- `/api/stream/feed/:market/transitions`
- `/api/stream/ontology/:market/world`

Legacy agent streams still exist for compatibility:

- `/api/stream/agent/:market/*`

Rule:

- New stream consumers should start from `feed/*` or `ontology/*`, depending on whether they need operational feed data or object/query data.

## 6. Consumer Guidance

If you are building:

- A dashboard or operator console:
  Prefer `ontology/*` for objects and `feed/*` for event rails.

- A detail page for a symbol/case/workflow:
  Start from the corresponding `ontology/*` object route, then follow `history_refs`.

- A legacy agent-style panel:
  `agent/*` is still usable, but it should be treated as a compatibility adapter over the newer surfaces.

## 7. Migration Rule

For new code:

1. Start from `ontology/*` if the UI is object-centric.
2. Start from `feed/*` if the UI is event/rail-centric.
3. Use `agent/*` only when matching an existing legacy consumer shape is required.
