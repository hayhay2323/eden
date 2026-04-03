# ReasoningContext: Unified Information Conduit for Eden's Reasoning Pipeline

**Date**: 2026-04-03
**Status**: Approved
**Problem**: Eden computes rich multi-dimensional intelligence (convergence components, world state, attribution, family performance, propagation absence) but compresses it to scalars or discards it before reaching decision points. ~85% information loss at key interfaces.

## Core Design

### ReasoningContext Struct

Immutable per-tick snapshot assembled by runtime, consumed by synthesis/policy. Replaces 4 scattered parameters in `derive_with_policy`.

```rust
pub struct ReasoningContext<'a> {
    // Existing (consolidated from current parameters)
    pub lineage_priors: &'a [FamilyContextLineageOutcome],
    pub multi_horizon_gate: Option<&'a MultiHorizonGate>,
    pub symbol_dimensions: Option<&'a HashMap<Symbol, SymbolDimensions>>,
    pub reviewer_doctrine: Option<&'a ReviewerDoctrinePressure>,

    // New: Graph convergence full components
    pub convergence_components: &'a HashMap<Symbol, ConvergenceScore>,

    // New: World state
    pub market_regime: &'a MarketRegimeFilter,
    pub world_state: Option<&'a WorldStateSnapshot>,

    // New: Propagation absence memory
    pub absence_memory: &'a AbsenceMemory,

    // New: Family positive feedback
    pub family_boost: &'a FamilyBoostLedger,
}
```

### Runtime-Owned Stateful Objects

#### AbsenceMemory

Tracks (sector, mechanism) pairs with consecutive propagation absence. Runtime updates each tick.

- `record_absences()`: increment consecutive count for absent sectors
- `record_propagations()`: clear entries for sectors that did propagate
- `should_suppress(sector, family) -> bool`: true if consecutive_count >= 3
- `decay()`: remove entries older than 30 minutes

#### FamilyBoostLedger

Positive feedback mirror of FamilyAlphaGate. Rebuilt from `lineage_priors` each tick.

- `boost_for_family(family) -> Decimal`: returns 1.0 (neutral) to 1.25 (max boost)
- Boost activates when `follow_through_rate >= 55%` AND `mean_net_return > 0`
- Formula: `1.0 + (follow_through_rate - 0.50) * 0.5`, capped at 1.25

#### ConvergenceDetail

Structured subset of ConvergenceScore for TacticalSetup:

```rust
pub struct ConvergenceDetail {
    pub institutional_alignment: Decimal,
    pub sector_coherence: Option<Decimal>,
    pub cross_stock_correlation: Decimal,
    pub component_spread: Option<Decimal>,
    pub edge_stability: Option<Decimal>,
}
```

## 5 Consumption Points

### 1. hypothesis_templates() — Attribution + Absence + World State

- **driver_kind filtering**: If all relevant events are `company_specific`, block cross-scope templates (institution_relay, shared_holder_spillover, propagation, cross_mechanism_chain)
- **absence suppression**: If `absence_memory.should_suppress(sector, family)` for propagation/spillover templates, skip generation
- **world state regime**: Block `stress_feedback_loop` when market regime is "stabilizing"

### 2. derive_tactical_setups() — Convergence Dimension Decompression

- Read full `ConvergenceScore` from `ctx.convergence_components` instead of scalar `suggestion.convergence_score`
- Populate new `TacticalSetup.convergence_detail` field
- Scalar `convergence_score` field retained for backward compatibility

### 3. apply_track_action_policy() — Convergence-Informed Decisions

Two new rules:
- **Strong consensus** (institutional_alignment.abs() > 0.5 AND component_spread < 0.3): promote "observe" → "review"
- **High disagreement** (component_spread > 0.6): demote "enter" → "review" with `ReviewReasonCode::ConvergenceDisagreement`

### 4. apply_track_action_policy() — Family Positive Feedback

```
adjusted_confidence = setup.confidence * family_boost - doctrine_pressure
```

Symmetric with existing negative feedback (ReviewerDoctrinePressure).

### 5. derive_with_policy() — Signature Convergence

10 parameters → 6: events, derived_signals, insights, decision, previous_setups, previous_tracks, ctx.

## HK/US Symmetry

### Shared Modules (new files)

- `src/pipeline/reasoning/context.rs`: ReasoningContext, AbsenceMemory, ConvergenceDetail, FamilyBoostLedger
- `src/pipeline/reasoning/family_gate.rs`: FamilyAlphaGate, best_family_prior, should_block_family_alpha, templates_from_candidate_mechanisms (extracted from HK support.rs)

Both HK and US import from these shared modules. One change, both markets benefit.

## File Change List

| # | File | Change | Type |
|---|------|--------|------|
| 1 | `src/pipeline/reasoning/context.rs` | New — ReasoningContext, AbsenceMemory, ConvergenceDetail, FamilyBoostLedger | New |
| 2 | `src/pipeline/reasoning/family_gate.rs` | New — extract shared family logic from HK support.rs | Extract |
| 3 | `src/pipeline/reasoning.rs` | mod context, mod family_gate; change derive_with_policy signature | Signature |
| 4 | `src/pipeline/reasoning/support.rs` | Remove FamilyAlphaGate (moved); add ctx param to hypothesis_templates; add driver_kind logic to attribution_allows_template | Refactor + Logic |
| 5 | `src/pipeline/reasoning/synthesis.rs` | derive_tactical_setups reads convergence_components; builds ConvergenceDetail | Wiring |
| 6 | `src/pipeline/reasoning/policy.rs` | apply_track_action_policy reads convergence_detail + family_boost | New rules |
| 7 | `src/ontology/reasoning.rs` | TacticalSetup: add convergence_detail field | Field add |
| 8 | `src/hk/runtime.rs` | Hold AbsenceMemory; assemble ReasoningContext per tick | Runtime wiring |
| 9 | `src/us/runtime.rs` | Same as #8 | Symmetric |
| 10 | `src/us/pipeline/reasoning/support.rs` | Use shared family_gate.rs; add ctx param | Alignment |
| 11 | `src/us/pipeline/reasoning.rs` | Same signature change as #3 | Signature |
| 12 | `src/pipeline/reasoning/tests.rs` | Tests for absence, attribution, convergence policy, family boost | Tests |
| 13 | `src/us/pipeline/reasoning_tests.rs` | Symmetric tests | Tests |

**2 new files, 11 modifications. No new external dependencies.**

## Explicitly Out of Scope

- **BrokerTemporalDelta → reasoning**: Requires new semantic mapping (broker pattern → ReasoningEvidence). Separate design.
- **Agent macro event rich fields**: Requires US signals pipeline conversion redesign. Separate change.
- **Attention budget rebalancing**: Requires attention_budget.rs redesign with health feedback. Separate change.

These are new channels, not wiring of existing computations.

## Test Plan

### AbsenceMemory (3 tests)
- `absence_memory_suppresses_after_3_consecutive`
- `absence_memory_clears_on_propagation`
- `absence_memory_decays_after_30_min`

### Family Boost (3 tests)
- `family_boost_neutral_below_55_pct`
- `family_boost_caps_at_1_25`
- `family_boost_requires_positive_net_return`

### Attribution Filtering (3 tests)
- `company_specific_blocks_cross_scope_templates`
- `sector_wide_allows_institution_relay`
- `no_attribution_allows_all` (cold start backward compat)

### Convergence Policy (3 tests)
- `strong_consensus_promotes_observe_to_review`
- `high_spread_demotes_enter_to_review`
- `no_convergence_detail_is_noop` (backward compat)

### World State Regime (2 tests)
- `stress_feedback_blocked_in_stabilizing`
- `stress_feedback_allowed_in_stress_regime`

### Verification
- `cargo check --lib -q` after each file change
- `cargo test -p eden --lib -- reasoning` for all reasoning tests
