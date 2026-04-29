# US Runtime Symmetry — Wire Shared Modules Into US Runtime

## Goal

Close the 6-feature gap between HK and US runtimes by wiring existing shared modules into the US tick loop, and adding US-specific adapter functions where needed. Design principle: "有就用沒有就不用" — features activate based on available data, not market identity.

## Architecture

Three layers, each building on the previous:

### Layer 1: Wire Existing Shared Modules (3 features)

These modules already exist in shared code but US runtime never calls them.

**1a. AbsenceMemory → US Runtime**

- Add `let mut absence_memory = AbsenceMemory::default()` to `UsRuntimeBootstrap`
- After US propagation paths are generated, extract sectors from successful paths → `absence_memory.record_propagation(&sector)`
- After reasoning, extract absence sectors → `absence_memory.record_absence(&sector, family, tick, now)`
- Decay each tick: `absence_memory.decay(now)`
- Pass `&absence_memory` into US `ReasoningContext` (US currently passes a `&AbsenceMemory::default()` or doesn't have the field — check and wire)

Files: `src/us/runtime.rs`, `src/us/runtime/startup.rs`

**1b. EnergyMomentum → US Runtime**

- Add `let mut energy_momentum = EnergyMomentum::default()` to `UsRuntimeBootstrap`
- US already generates propagation paths in `derive_diffusion_propagation_paths` (in `src/us/pipeline/reasoning/propagation.rs`)
- After paths are generated: `let tick_energy = NodeEnergyMap::from_propagation_paths(&propagation_paths)`
- Blend: `energy_momentum.update(&tick_energy, Decimal::new(7, 1))`
- Apply to US convergence scores: `apply_energy_to_convergence(&mut convergence_scores, &energy_momentum)`

Files: `src/us/runtime.rs`, `src/us/runtime/startup.rs`

**1c. accumulate_institutional_memory → US Runtime**

- US has `UsGraph` with stock nodes and edges, but no `BrainGraph`
- Add `accumulate_from_us_graph(&mut self, tick: u64, graph: &UsGraph)` method to `AccumulatedKnowledge` in `src/ontology/store/knowledge.rs`
- This method extracts sector→stock edges from `UsGraph` and builds institutional memory (using sector as the "institution" proxy since US has no broker/institution data)
- Call at end of US tick loop: `store.knowledge_write().accumulate_from_us_graph(tick, &graph)`

Files: `src/ontology/store/knowledge.rs`, `src/us/runtime.rs`

### Layer 2: US Fingerprint/Outcome Functions (1 feature)

**2. Causal Schema Extraction (every 10 ticks)**

Requires two US-specific adapter functions that parallel HK's:

- `compute_us_vortex_successful_fingerprints(history: &UsTickHistory, window: usize) -> Vec<VortexFingerprint>`
  - Extract from US tick history: which vortex patterns (convergence_hypothesis) led to positive outcomes
  - Uses `UsTickRecord.tactical_setups` + `hypothesis_tracks` to match setup→outcome

- `compute_us_case_realized_outcomes_adaptive(history: &UsTickHistory, window: usize) -> Vec<CaseRealizedOutcome>`
  - Extract realized outcomes from US tick history using the same adaptive window logic as HK
  - HK version is at `src/temporal/lineage/outcomes.rs` — create parallel at `src/us/temporal/lineage/outcomes.rs`

- Every 10 ticks in US runtime: call `extract_causal_schema()` (shared function) with US-derived fingerprints and outcomes
- Write schemas to persistence

Files: `src/us/temporal/lineage/outcomes.rs` (new), `src/us/runtime.rs`

### Layer 3: Governance Cycle (2 features)

**3a. SurfaceQualitySnapshot (every 50 ticks)**

- Compute from US outcomes: hits, misses, total_return, live_mechanism_count, live_schema_count
- `live_schema_count` starts at 0 and grows naturally as schemas accumulate
- Store as `baseline_quality: Option<SurfaceQualitySnapshot>` in US runtime state

Files: `src/us/runtime.rs`

**3b. Evolution Cycle (every 50 ticks)**

- Call `run_evolution_cycle()` (shared function at `src/temporal/lineage/evolution.rs`) with:
  - `&mut cached_us_candidate_mechanisms`
  - `&mut cached_us_causal_schemas` (new field)
  - `&shadow_scores` (new: US needs to compute these during echo/verification)
  - `baseline_quality.as_ref()`
  - `Some(&current_quality)`
- Manages promote/demote/rollback of US mechanisms and schemas

Files: `src/us/runtime.rs`, `src/us/runtime/startup.rs`

## New Fields in UsRuntimeBootstrap

```rust
pub(super) absence_memory: AbsenceMemory,
pub(super) energy_momentum: EnergyMomentum,
pub(super) cached_us_causal_schemas: Vec<CausalSchemaRecord>,
pub(super) shadow_scores: HashMap<String, Decimal>,
pub(super) baseline_quality: Option<SurfaceQualitySnapshot>,
```

## Testing

- Layer 1: `cargo check --lib` + existing 780 tests must pass. No new tests needed (modules already tested).
- Layer 2: Add 2 tests for US fingerprint/outcome functions in `src/us/temporal/lineage_tests.rs` or similar.
- Layer 3: Existing `evolution.rs` tests cover the shared function. No new tests needed for wiring.
- All layers: Run `cargo test --lib` → 780+ pass.

## Implementation Order

Layer 1 → Layer 2 → Layer 3. Each layer is independently valuable and can be committed/shipped separately. Layer 1 alone closes 50% of the symmetry gap with minimal risk.
