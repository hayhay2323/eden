# Energy Propagation: Diffusion Path Energy → ConvergenceScore

**Date**: 2026-04-05
**Status**: Approved
**Prerequisite**: Edge Learning (Sub-project 1) completed
**Problem**: `derive_diffusion_propagation_paths` computes multi-hop energy flows but the results are only used for narrative. ConvergenceScore::compute uses local neighborhood voting only, ignoring upstream energy from propagation. Additionally, contradiction damping is inverted (amplifies instead of dampens).

## Core Design

### NodeEnergyMap

New struct that accumulates energy flux per symbol from diffusion propagation paths.

```rust
pub struct NodeEnergyMap {
    flux: HashMap<Symbol, Decimal>,
}
```

Built from `Vec<PropagationPath>`: for each path, the last step's target scope receives energy = `path.confidence * polarity_sign`. Multiple paths to the same target accumulate.

### Consumption in ConvergenceScore

`ConvergenceScore::compute()` accepts `Option<&NodeEnergyMap>`. Energy becomes the 4th component alongside institutional_alignment, sector_coherence, and cross_stock_correlation:

```rust
if let Some(energy_map) = energy_map {
    let energy = energy_map.energy_for(symbol);
    if energy != Decimal::ZERO {
        components.push(energy.clamp(-1.0, 1.0));
    }
}
composite = mean(all nonzero components)
```

Clamped to [-1, 1] to match the range of other components.

### Contradiction Damping Fix

In `propagation.rs`, `diffusion_lag_factor` currently sets `lag_factor = 1.15` when target has opposite-sign motion (amplifying contradiction). Fix to `0.85` (dampening contradiction).

### Lifecycle

NodeEnergyMap is rebuilt each tick from that tick's diffusion paths (pure function, no cross-tick state). Cross-tick energy accumulation/momentum is deferred to Sub-project 3.

## File Change List

| # | File | Change |
|---|------|--------|
| 1 | `src/graph/energy.rs` | New — NodeEnergyMap + from_propagation_paths |
| 2 | `src/graph/mod.rs` | pub mod energy |
| 3 | `src/graph/convergence.rs` | compute() accepts Option<&NodeEnergyMap>, adds energy as 4th component |
| 4 | `src/graph/decision.rs` | Pass energy_map to compute() |
| 5 | `src/pipeline/reasoning.rs` | Build NodeEnergyMap from diffusion paths, pass to decision |
| 6 | `src/pipeline/reasoning/propagation.rs` | Fix contradiction damping 1.15 → 0.85 |
| 7 | `src/hk/runtime.rs` | Pass energy_map through |

## Test Plan

### energy.rs (3 tests)
- `energy_map_accumulates_from_paths`: two paths to same symbol → flux is sum
- `energy_map_returns_zero_for_unknown_symbol`: unknown symbol → 0
- `energy_clamped_to_unit_interval`: energy > 1.0 after accumulation → clamped in convergence

### propagation.rs (1 test)
- `contradiction_dampens_not_amplifies`: opposite-sign target → lag_factor < 1.0

### convergence.rs (1 test)
- `convergence_composite_includes_upstream_energy`: same graph, with energy_map vs without → different composite

## Not in Scope
- Cross-tick energy accumulation / momentum (Sub-project 3)
- Resonance / interference (Sub-project 3)
- Template retirement (Sub-project 4)
