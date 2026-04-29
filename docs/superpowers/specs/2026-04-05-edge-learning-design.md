# Edge Learning: Outcome-Adaptive Edge Weights for BrainGraph

**Date**: 2026-04-05
**Status**: Approved
**Problem**: BrainGraph edge weights are purely microstructure-derived and rebuilt each tick. Edges that historically led to profitable outcomes carry the same weight as edges that led to losses. The graph cannot learn.

## Core Design

### EdgeLearningLedger

Runtime-owned, cross-tick accumulator. Maps edge fingerprints to accumulated credit from resolved outcomes.

```rust
pub struct EdgeLearningLedger {
    entries: HashMap<String, EdgeCredit>,
}

pub struct EdgeCredit {
    pub total_credit: Decimal,
    pub sample_count: u32,
    pub mean_credit: Decimal,
    pub last_updated: OffsetDateTime,
}
```

Edge fingerprint format: `"inst:{id}→stock:{symbol}"`, `"stock:{a}↔stock:{b}"`, `"stock:{symbol}→sector:{id}"`.

### Credit Attribution from Outcomes

When a `CaseRealizedOutcomeRecord` resolves:

1. Find the resolved setup's `ConvergenceDetail` (institutional_alignment, sector_coherence, cross_stock_correlation)
2. Identify dominant component (largest absolute value)
3. Compute contribution_ratio = `dominant_abs / (inst_abs + sector_abs + cross_abs)`
4. credit_per_edge = `outcome.net_return * contribution_ratio`
5. Distribute credit to edges of the dominant type:
   - institutional_alignment dominant → all Institution→Stock edges for that symbol
   - cross_stock_correlation dominant → all Stock↔Stock edges for that symbol
   - sector_coherence dominant → Stock→Sector edge for that symbol

### Weight Multiplier

```rust
fn weight_multiplier(mean_credit: Decimal) -> Decimal {
    // Positive credit: amplify up to 50%
    // Negative credit: dampen up to 50%
    // No data: neutral (1.0)
    Decimal::ONE + mean_credit.clamp(Decimal::new(-5, 1), Decimal::new(5, 1))
}
```

Range: [0.5, 1.5]. Neutral at 1.0.

### Consumption in ConvergenceScore

`ConvergenceScore::compute()` accepts `Option<&EdgeLearningLedger>`. For each edge aggregation:

- institutional_alignment: `weighted_sum += e.direction * seat_count * ledger.weight_multiplier(edge_id)`
- cross_stock_correlation: `corr_sum += e.similarity * neighbor.mean_direction * ledger.weight_multiplier(edge_id)`
- sector_coherence: `sector_coherence *= ledger.weight_multiplier(edge_id)` (single edge, direct multiplier)

When ledger is None, all multipliers are 1.0 (backward compatible).

### Decay

`ledger.decay(now)`: entries with `last_updated` older than 7 days get `total_credit *= 0.95` per tick. Entries with `sample_count == 0` or `total_credit.abs() < 0.001` are removed.

### Startup Backfill

At runtime startup, load historical `CaseRealizedOutcomeRecord` (up to 500) and replay credit attribution to seed the ledger. This requires access to the corresponding `ConvergenceDetail` for each outcome — stored in `TacticalSetup.convergence_detail` which is persisted.

If convergence_detail is not available for historical outcomes (they predate the field), skip those outcomes. The ledger starts empty and accumulates from live outcomes.

### Runtime Lifecycle

- Before tick loop: `let mut edge_ledger = EdgeLearningLedger::default();`
- Startup backfill from historical outcomes (if convergence_detail available)
- Each tick: pass `Some(&edge_ledger)` to `ConvergenceScore::compute()`
- After outcome resolution: `edge_ledger.credit_from_outcome(outcome, convergence_detail, symbol, brain)`
- Each tick end: `edge_ledger.decay(now)`

## File Change List

| # | File | Change |
|---|------|--------|
| 1 | `src/graph/edge_learning.rs` | New — EdgeLearningLedger, EdgeCredit, credit attribution, decay, weight_multiplier |
| 2 | `src/graph/convergence.rs` | compute() accepts Option<&EdgeLearningLedger>, applies weight_multiplier to edge aggregations |
| 3 | `src/graph/decision.rs` | DecisionSnapshot::compute() passes ledger to ConvergenceScore::compute() |
| 4 | `src/graph/mod.rs` or graph.rs | pub mod edge_learning |
| 5 | `src/hk/runtime.rs` | Hold EdgeLearningLedger, backfill, credit on outcome, decay each tick |
| 6 | `src/us/runtime.rs` | Symmetric changes |

## Test Plan

### edge_learning.rs (6 tests)
- `credit_attribution_selects_dominant_component`: inst_alignment=0.6, sector=0.2, cross=0.1 → credit goes to inst edges
- `weight_multiplier_positive_credit_amplifies`: mean_credit=0.3 → multiplier=1.3
- `weight_multiplier_negative_credit_dampens`: mean_credit=-0.3 → multiplier=0.7
- `weight_multiplier_capped_at_50_pct`: mean_credit=0.9 → multiplier=1.5 (not 1.9)
- `decay_reduces_stale_entries`: entry 8 days old → total_credit reduced
- `no_learning_data_returns_neutral`: unknown edge_id → multiplier=1.0

### convergence.rs (1 test)
- `convergence_score_uses_learned_edge_weights`: same graph, with vs without ledger → different composite

## Not in Scope
- Energy propagation (sub-project 2)
- Multi-tick resonance (sub-project 3)
- Template retirement (sub-project 4)
- VortexSuccessPattern changes (complementary, not replaced)
