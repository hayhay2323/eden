# Perception Graph ↔ Master KG Sync Contract

**Status:** descriptive (codifies what the code currently does + flags
known gaps). Update when boundaries change.

**Audience:** anyone touching `src/perception/`, `src/graph/`,
`src/persistence/`, or runtime wiring in `src/hk/` / `src/us/`.

## Two graphs, two roles

Eden runs **two distinct graphs** that are easy to confuse. Keep them
straight:

| | **Master KG** | **PerceptionGraph** |
|---|---|---|
| Module | `src/graph/` | `src/perception/` |
| Lifetime | persistent across sessions | ephemeral per-tick (with energy decay) |
| Holds | topology (Stock/Institution/Sector nodes, learned edge weights) | sensation (KL surprise, sensory flux, world intent reflection, …) |
| Mutated by | `outcome_feedback`, `edge_learning_ledger`, structural events | per-tick detectors via `apply_to_perception_graph(...)` |
| Read by | `graph::insights`, `graph::tracker`, decision modules | `agent::build_perception_report`, `NodeView`/`WorldView` facades |
| Persistence | SQLite (`data/eden.db`) + edge_learning ledger | partial — see Persistence section |

**Mental model:** Master KG is the *long-term memory of who is connected
to whom and how strongly*. PerceptionGraph is the *what eden is feeling
right now*. Both exist; both are needed; neither subsumes the other.

## Write boundaries

Each component writes to exactly one graph. No component writes to both.

### Master KG writers
- `graph/edge_learning.rs::EdgeLearningLedger` — accumulates per-edge
  credit from outcomes, weight range `[0.5, 1.5]`. Profitable edges
  amplified.
- `pipeline/outcome_feedback.rs` — feeds realized outcomes back into
  edge weights.
- Structural events (institution buys/sells, sector membership changes)
  via `graph/temporal/`.

### PerceptionGraph writers
Each "Perceiver" owns one sub-graph slice. The pattern is a stateless
free function:

```
apply_to_perception_graph(events, graph, tick) -> ()
```

Current Perceivers (one per sub-graph):

| Sub-graph | Writer (`pipeline/`) | Status |
|---|---|---|
| `kl_surprise` | `kl_surprise.rs` | landed (commit `1af1000`) |
| `sector_kinematics` | `sector_kinematics.rs` | landed (`5cf0ab2`, fix `5643b19`) |
| `sector_contrast` | `cross_sector_contrast.rs` | landed (`e38c650`) |
| `world_intent` | `latent_world_state.rs` | landed (`9d498c2`, persist `589f82b`) |
| `emergence` | `cluster_sync.rs`, `sub_kg_emergence.rs` | landed |
| `lead_lag` | `lead_lag_index.rs` | landed |
| `symbol_contrast` | `structural_contrast.rs` | landed |
| `sensory_flux` | `pressure_events/aggregator.rs` | landed (commit `64271a5`) |
| `thematic_flux` | `pressure_events/aggregator.rs` | landed (`64271a5`) |
| `synthetic_sectors` | `loopy_bp.rs::detect_synthetic_sectors` | in-flight diff |
| `sensory_gain` | `active_probe.rs:310-326` | landed (`64271a5`) — see Persistence gap below |

## Read boundaries

PerceptionGraph is read through **facades**, not by mutating callers
walking the sub-graph internals:

- `PerceptionGraph::node(symbol)` → `NodeView` — per-symbol modality
  assembly (mod.rs:752).
- `PerceptionGraph::world(market)` → `WorldView` — market-level intent
  (mod.rs:762).
- `PerceptionGraph::to_report(market, tick, ts, cfg)` — projects the
  graph into the Y-facing `EdenPerception` report. Filtering thresholds
  (`contrast >= 1.0`, `surprise >= 1.0 bits`, `flux > 0.1`,
  `correlation >= 0.3`) are applied **here**, not at write time. This
  is deliberate — different Y readers can apply different thresholds to
  the same raw sub-graph.

Master KG is read directly via `graph::insights` and similar query
modules. There is no facade layer because Y does not yet read the
master KG (it reads the perception-derived `EdenPerception` instead).

## Cross-graph data flow

There is **one** legitimate cross-graph dependency, and it is one-way:

```
Master KG edge weights  ──read──>  loopy_bp inference
loopy_bp marginals      ──write──> PerceptionGraph (via downstream detectors)
PerceptionGraph         ──read──>  agent::build_perception_report
```

PerceptionGraph never writes to Master KG. Master KG never reads from
PerceptionGraph. This separation is intentional: it keeps the
"persistent topology" layer free of per-tick volatility.

The closed loop runs *outside* PerceptionGraph: realized outcomes →
`outcome_feedback` → Master KG edge weights → next tick's BP priors →
next tick's PerceptionGraph readings.

## Persistence

What survives session restart:

| Layer | Survives? | Mechanism |
|---|---|---|
| Master KG topology | yes | SQLite (`data/eden.db`) |
| Master KG edge weights | yes | `edge_learning_ledger` (SQLite) |
| `world_intent` reflection | yes | NDJSON tail at `world_reflection_ledger_path(market)` (~50k records, ~4 MB), loaded on `LatentWorldState::persistent()` |
| `belief_snapshot`, `tactical_setup`, `hypothesis_track` | yes | SQLite |
| `case_resolution`, `case_reasoning_assessment` | yes | SQLite |
| `kl_surprise`, `sector_kinematics`, `sector_contrast`, `emergence`, `lead_lag`, `symbol_contrast`, `sensory_flux`, `thematic_flux`, `synthetic_sectors` | **no** | ephemeral with `decay_energy(0.90)` per tick |
| `sensory_gain` | yes | single JSON snapshot at `sensory_gain_ledger_path(market_slug)` (`.run/sensory-gain-{slug}.json`); `PerceptionGraph::persistent(slug)` loads on startup, `active_probe::evaluate_due` saves after each closed-loop update |

## Concurrency contract

Both graphs are accessed from multiple call paths within the same tick.
Current locking:

- Master KG: per-table locks in `persistence/store/` (sync_lock
  introduced in `bb87832`).
- PerceptionGraph: `Arc<std::sync::RwLock<PerceptionGraph>>` in
  `PreparedRuntimeContext` (`core/runtime/context.rs:210`).

**Known follow-up:** when an async Y reader (CLI / API) lands, the
`std::sync::RwLock` will block the runtime worker. Switch the read
boundary to `tokio::task::spawn_blocking` rather than swapping the
whole lock to `tokio::sync::RwLock` — the writer paths are sync and
deeply integrated. Tracked in `memory/perception_graph_progress.md`.

## When to add a new sub-graph vs. a new module

Adding a new Perceiver sub-graph to `PerceptionGraph`:
- The output is a per-tick observation Y wants to read.
- The data fits a typed `HashMap<Key, Snapshot>` shape.
- The detector is willing to be stateless (`apply_to_perception_graph`).

Otherwise, keep it as its own module under `pipeline/`. Not every
detector needs to graduate to PerceptionGraph; sub-graphs are the
public surface, pipeline modules are private machinery.

## Invariants (enforce in review)

1. No PerceptionGraph code writes to Master KG.
2. No Master KG code reads PerceptionGraph state.
3. Filtering thresholds live at read boundary (`to_report`), not at
   sub-graph write time.
4. Sub-graphs use the `(upsert, get, iter, len, is_empty)` shape so the
   facade pattern remains uniform.
5. New Perceivers use the stateless free-function form, not a method on
   a stateful detector struct, so `apply_to_perception_graph(...)`
   composition stays trivial.
6. `last_tick` on a sub-graph snapshot reflects observation tick, not
   apply-call tick (regression in `5cf0ab2` fixed by `5643b19`).
