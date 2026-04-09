# Rebuild EventSnapshot::detect — Event Detection Pipeline Restoration

**Date**: 2026-04-04
**Status**: Approved
**Prerequisite**: `e5da08a` feat(reasoning): complete ReasoningContext wiring
**Problem**: `EventSnapshot::detect` and 3 related functions were deleted in commit `44901e0` during ongoing ontology refactoring. Without them: no events are generated → no hypotheses → pipeline is dead. Also: driver_kind attribution provenance has no writer, and PropagationAbsence events are never created — making the last two commits' wiring (AbsenceMemory, driver_kind filtering) inert.

## 1. EventSnapshot::detect — Restore + Attribution Injection

Restore the 442-line implementation from git history (`44901e0~1:src/pipeline/signals/events.rs`). The function signature was:

```rust
impl EventSnapshot {
    pub fn detect(
        history: &TickHistory,
        current_tick_number: u64,
        links: &LinkSnapshot,
        dimensions: &DimensionSnapshot,
        insights: &GraphInsights,
        decision: &DecisionSnapshot,
    ) -> Self
}
```

**Type migration**: `LinkSnapshot` was renamed to `RawSnapshot` in the ontology refactoring. The restored code must use the current type names. Other types (`TickHistory`, `DimensionSnapshot`) also need verification.

**Attribution provenance injection**: Each event construction site adds `attr:scope=` and `attr:driver=` to `provenance.inputs`:

| Event Kind | driver | scope |
|------------|--------|-------|
| SmartMoneyPressure, VolumeDislocation, OrderBookDislocation, CandlestickBreakout, BrokerSideFlip, IcebergDetected, BrokerClusterFormation | company_specific | local |
| SharedHolderAnomaly, InstitutionalFlip, CompositeAcceleration, CatalystActivation | sector_wide | sector |
| StressRegimeShift, MarketStressElevated, ManualReviewRequired | macro_wide | market |
| PropagationAbsence | (none) | (none) |

Helper function to inject attribution:
```rust
fn attribution_inputs(driver: &str, scope: &str) -> Vec<String> {
    vec![
        format!("attr:driver={}", driver),
        format!("attr:scope={}", scope),
    ]
}
```

## 2. broker_events_from_delta — Restore

Restore from git history. Converts `BrokerTemporalDelta` transitions into:
- `IcebergDetected` — iceberg_confidence threshold
- `BrokerSideFlip` — side change detection
- `BrokerClusterFormation` — multiple brokers clustering

Add `attr:driver=company_specific` + `attr:scope=local` provenance to each.

## 3. enrich_attribution_with_evidence — New Implementation

```rust
pub fn enrich_attribution_with_evidence(
    event_snapshot: &mut EventSnapshot,
    cross_stock_presences: &[CrossStockPresence],
    macro_events: &[AgentMacroEvent],
)
```

For each event in snapshot:
- If event's symbol appears in cross_stock_presences (multi-stock coordination) → upgrade scope from `local` → `sector`, driver from `company_specific` → `sector_wide`
- If macro_events contain a catalyst matching the event's sector → upgrade driver from `company_specific` → `sector_wide`

Implementation: iterate events, check symbol membership in cross_stock_presences symbols set, mutate provenance.inputs in-place (remove old attr:scope/attr:driver, add upgraded values).

## 4. detect_propagation_absences — New Implementation

```rust
pub fn detect_propagation_absences(
    event_snapshot: &mut EventSnapshot,
    dimensions: &DimensionSnapshot,
)
```

Logic:
- Group existing events by sector (via scope → SectorId)
- For each sector that has events, check if other symbols in the same sector (from dimensions) have zero activity_momentum
- If a sector has events but >50% of its symbols show no activity → insert a `PropagationAbsence` event for that sector

This creates the events that `propagation_absence_sectors()` reads → feeds `AbsenceMemory`.

## 5. signals.rs Re-exports

Restore:
```rust
pub use events::{broker_events_from_delta, catalyst_events_from_macro_events};
pub(crate) use events::{detect_propagation_absences, enrich_attribution_with_evidence};
```

Note: `catalyst_events_from_macro_events` needs to be implemented in HK events.rs (the US version uses different types). Minimal implementation: convert `AgentMacroEvent` with `event_type` matching thematic catalysts → `MarketEventRecord` with `CatalystActivation` kind.

## 6. LinkSnapshot → RawSnapshot Migration

The restored code references `LinkSnapshot` which was renamed. Need to:
- Check `RawSnapshot` field names match what detect() accessed (order_books, calc_indexes, etc.)
- Update all type references in restored code
- Verify `TickHistory` and `DimensionSnapshot` still exist with compatible shapes

## File Change List

| # | File | Change |
|---|------|--------|
| 1 | `src/pipeline/signals/events.rs` | Restore detect, broker_events_from_delta, catalyst_events_from_macro_events from git + add attribution provenance + implement enrich_attribution_with_evidence + detect_propagation_absences |
| 2 | `src/pipeline/signals.rs` | Restore re-exports; fix dead imports (LinkSnapshot → RawSnapshot, TickHistory) |
| 3 | `src/pipeline/signals/helpers.rs` | Restore any missing helpers (volume_dislocation_magnitude etc.) if not already present |
| 4 | `src/hk/runtime.rs` | Fix detect call site type params if needed |
| 5 | `src/bin/replay.rs` | Fix detect call site type params if needed |

## Test Plan

### Restored tests (5)
- `detect_order_book_dislocation`
- `detect_volume_dislocation`
- `detect_candlestick_breakout`
- `detect_composite_acceleration_threshold`
- `detect_composite_acceleration`

### New tests (3)
- `detected_events_have_attribution_provenance` — verify attr:driver= and attr:scope= present in provenance.inputs for generated events
- `enrich_attribution_upgrades_scope_on_cross_stock` — event with company_specific driver upgrades to sector_wide when symbol in cross_stock_presences
- `detect_propagation_absence_when_sector_peers_silent` — sector has events but peers have zero activity → PropagationAbsence event inserted

## Risk: Type Shape Changes

The biggest risk is that `RawSnapshot` (ex-LinkSnapshot) has different field names or types than what the old code expected. The implementation must:
1. Read `RawSnapshot` struct definition first
2. Map old field accesses to new field names
3. If fields were removed (e.g., `order_books` no longer exists), adapt the detection logic or skip that event type

## Not in Scope
- US EventSnapshot changes (UsEventSnapshot::detect is separate and working)
- New event types beyond the original 10
- Broker behavioral profiling (separate design)
