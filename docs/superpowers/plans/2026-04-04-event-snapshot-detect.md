# EventSnapshot::detect Restoration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restore the HK event detection pipeline (EventSnapshot::detect + broker_events + attribution + propagation absence) and its type dependencies, fixing ~130 pre-existing compilation errors.

**Architecture:** Restore `ontology/links/types.rs` (241 lines, fixes 112 errors) and `pipeline/signals/events.rs` (442 lines) from git history commit `44901e0`. Add attribution provenance injection to every event construction site. Implement two new functions: `enrich_attribution_with_evidence` and `detect_propagation_absences`.

**Tech Stack:** Rust, rust_decimal, time, petgraph

**Spec:** `docs/superpowers/specs/2026-04-04-event-snapshot-detect-design.md`

**Strategy:** This plan restores code from git history verbatim first, verifies compilation improves, then adds new attribution features. The recovered code has been verified to match the current type shapes (DimensionSnapshot, GraphInsights, DecisionSnapshot, BrokerTemporalDelta all unchanged).

---

### Task 1: Restore ontology/links/types.rs (fixes ~112 compilation errors)

**Files:**
- Modify: `src/ontology/links/types.rs` (currently empty)

- [ ] **Step 1: Restore types.rs from git history**

Write the full recovered content (241 lines) to `src/ontology/links/types.rs`. The content defines: `Side`, `BrokerQueueEntry`, `InstitutionActivity`, `CrossStockPresence`, `CapitalFlow`, `YuanAmount`, `CapitalFlowTimeSeries`, `CapitalBreakdown`, `MarketStatus`, `DepthLevel`, `DepthProfile`, `OrderBookObservation`, `QuoteObservation`, `CalcIndexObservation`, `CandlestickObservation`, `MarketTemperatureObservation`, `TradeActivity`, `LinkSnapshot`, and supporting types.

The file begins with `use super::*;` and contains a helper function `candle_range_normalizer()`.

Full content to write — recovered verbatim from `git show 44901e0:src/ontology/links/types.rs`.

- [ ] **Step 2: Verify compilation improvement**

Run: `cargo check --lib 2>&1 | grep -c "^error"`
Expected: error count drops from ~162 to ~50 (fixing ~112 `ontology::links` errors)

- [ ] **Step 3: Commit**

```bash
git add src/ontology/links/types.rs
git commit -m "restore(ontology): recover LinkSnapshot + observation types from git history"
```

---

### Task 2: Restore EventSnapshot::detect + broker_events_from_delta

**Files:**
- Modify: `src/pipeline/signals/events.rs` (currently empty)

- [ ] **Step 1: Write restored events.rs with attribution provenance**

Write the recovered detect implementation (442 lines) to `src/pipeline/signals/events.rs`. The file contains:
- `impl EventSnapshot { pub fn detect(...) -> Self }` — main event detection from market data
- `broker_events_from_delta(...)` — converts broker temporal transitions to events
- Helper functions: `previous_history_tick`, `strict_positive_median_cutoff`, `exceeds_cutoff`, `volume_dislocation_magnitude`, `historical_market_stress_cutoff`, `previous_market_stress`

The file begins with `use super::*;` and uses helpers from `helpers.rs` (`provenance`, `median`, `normalized_ratio`).

**Key modification from original:** Add attribution provenance to every event construction. After each `Event::new(MarketEventRecord { ... }, provenance(...))`, the provenance inputs must include `attr:driver=` and `attr:scope=` entries.

Add a helper at the top of the file:

```rust
fn attribution_inputs(driver: &str, scope: &str) -> Vec<String> {
    vec![
        format!("attr:driver={}", driver),
        format!("attr:scope={}", scope),
    ]
}
```

Then for each event construction, extend the provenance inputs with attribution. The mapping:

| MarketEventKind | driver | scope |
|-----------------|--------|-------|
| OrderBookDislocation | company_specific | local |
| VolumeDislocation | company_specific | local |
| CandlestickBreakout | company_specific | local |
| SmartMoneyPressure | company_specific | local |
| IcebergDetected | company_specific | local |
| BrokerSideFlip | company_specific | local |
| BrokerClusterFormation | company_specific | local |
| CompositeAcceleration | sector_wide | sector |
| InstitutionalFlip | sector_wide | sector |
| SharedHolderAnomaly | sector_wide | sector |
| CatalystActivation | sector_wide | sector |
| MarketStressElevated | macro_wide | market |
| StressRegimeShift | macro_wide | market |
| ManualReviewRequired | macro_wide | market |

For each `Event::new(record, prov)` call, change to:
```rust
Event::new(record, {
    let mut p = prov;
    p.inputs.extend(attribution_inputs("company_specific", "local"));
    p
})
```

Adjust driver/scope per the mapping above.

- [ ] **Step 2: Verify compilation**

Run: `cargo check --lib 2>&1 | grep "signals/events.rs" | head -10`
Expected: no errors from events.rs (the `use super::*` imports come from signals.rs parent module which re-exports types.rs)

- [ ] **Step 3: Commit**

```bash
git add src/pipeline/signals/events.rs
git commit -m "restore(signals): recover EventSnapshot::detect + broker_events with attribution provenance"
```

---

### Task 3: Fix signals.rs re-exports + remaining dead imports

**Files:**
- Modify: `src/pipeline/signals.rs`

- [ ] **Step 1: Restore events.rs re-exports**

In `src/pipeline/signals.rs`, the current attribution re-exports (around line 31-34) need to be augmented with the restored events.rs functions. Replace:

```rust
// Re-export attribution types from types.rs (previously in events.rs, now rebuilt)
pub use types::{
    event_driver_kind, event_propagation_scope, EventDriverKind, EventPropagationScope,
};
```

With:

```rust
pub use events::broker_events_from_delta;
pub use types::{
    event_driver_kind, event_propagation_scope, EventDriverKind, EventPropagationScope,
};
```

- [ ] **Step 2: Fix other dead imports in signals.rs**

Check if `LinkSnapshot` import (line 14) now resolves after types.rs restoration. If `TickHistory` import (line 16) is still dead, check its location and fix.

`TickHistory` is at `crate::temporal::buffer::TickHistory` — verify the import path is correct.

- [ ] **Step 3: Verify compilation**

Run: `cargo check --lib 2>&1 | grep "pipeline/signals.rs" | head -10`
Expected: significantly fewer errors

- [ ] **Step 4: Commit**

```bash
git add src/pipeline/signals.rs
git commit -m "fix(signals): restore events.rs re-exports and fix dead imports"
```

---

### Task 4: Implement enrich_attribution_with_evidence

**Files:**
- Modify: `src/pipeline/signals/events.rs` (append new function)

- [ ] **Step 1: Write test**

Add to the bottom of events.rs (or in a `#[cfg(test)] mod tests` block):

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};

    #[test]
    fn enrich_attribution_upgrades_scope_on_cross_stock() {
        let symbol = Symbol("700.HK".into());
        let mut snapshot = EventSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            events: vec![Event::new(
                MarketEventRecord {
                    scope: SignalScope::Symbol(symbol.clone()),
                    kind: MarketEventKind::SmartMoneyPressure,
                    magnitude: Decimal::ONE,
                    summary: "test".into(),
                },
                ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH)
                    .with_inputs(attribution_inputs("company_specific", "local")),
            )],
        };
        let presences = vec![CrossStockPresence {
            institution_id: InstitutionId(1),
            symbols: vec![symbol.clone(), Symbol("388.HK".into())],
            ask_symbols: vec![],
            bid_symbols: vec![],
        }];
        enrich_attribution_with_evidence(&mut snapshot, &presences, &[]);
        let inputs = &snapshot.events[0].provenance.inputs;
        assert!(inputs.iter().any(|i| i == "attr:scope=sector"));
        assert!(inputs.iter().any(|i| i == "attr:driver=sector_wide"));
    }
}
```

- [ ] **Step 2: Implement enrich_attribution_with_evidence**

Add to `src/pipeline/signals/events.rs`:

```rust
pub fn enrich_attribution_with_evidence(
    snapshot: &mut EventSnapshot,
    cross_stock_presences: &[CrossStockPresence],
    _macro_events: &[crate::ontology::knowledge::AgentMacroEvent],
) {
    let cross_stock_symbols: std::collections::HashSet<Symbol> = cross_stock_presences
        .iter()
        .flat_map(|p| p.symbols.iter().cloned())
        .collect();

    for event in &mut snapshot.events {
        let event_symbol = match &event.value.scope {
            SignalScope::Symbol(s) => Some(s),
            _ => None,
        };
        let Some(symbol) = event_symbol else {
            continue;
        };
        if !cross_stock_symbols.contains(symbol) {
            continue;
        }
        // Symbol appears in cross-stock presence → upgrade from local to sector
        let has_local_scope = event
            .provenance
            .inputs
            .iter()
            .any(|i| i == "attr:scope=local");
        if has_local_scope {
            event.provenance.inputs.retain(|i| {
                !i.starts_with("attr:scope=") && !i.starts_with("attr:driver=")
            });
            event
                .provenance
                .inputs
                .extend(attribution_inputs("sector_wide", "sector"));
        }
    }
}
```

- [ ] **Step 3: Export from signals.rs**

In `src/pipeline/signals.rs`, add:

```rust
pub(crate) use events::enrich_attribution_with_evidence;
```

- [ ] **Step 4: Run test**

Run: `cargo test --lib -- pipeline::signals::events::tests::enrich_attribution -q 2>&1 | tail -5`
Expected: 1 test passes

- [ ] **Step 5: Commit**

```bash
git add src/pipeline/signals/events.rs src/pipeline/signals.rs
git commit -m "feat(signals): implement enrich_attribution_with_evidence"
```

---

### Task 5: Implement detect_propagation_absences

**Files:**
- Modify: `src/pipeline/signals/events.rs` (append new function)

- [ ] **Step 1: Write test**

Add to the tests module in events.rs:

```rust
    #[test]
    fn detect_propagation_absence_when_sector_peers_silent() {
        let mut snapshot = EventSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            events: vec![Event::new(
                MarketEventRecord {
                    scope: SignalScope::Symbol(Symbol("700.HK".into())),
                    kind: MarketEventKind::SmartMoneyPressure,
                    magnitude: Decimal::ONE,
                    summary: "test".into(),
                },
                ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH),
            )],
        };
        // 700.HK is in "tech" sector, 388.HK is also in "tech" but has zero activity
        let mut dimensions = HashMap::new();
        dimensions.insert(
            Symbol("700.HK".into()),
            SymbolDimensions {
                activity_momentum: Decimal::ONE,
                ..SymbolDimensions::default()
            },
        );
        dimensions.insert(
            Symbol("388.HK".into()),
            SymbolDimensions {
                activity_momentum: Decimal::ZERO,
                ..SymbolDimensions::default()
            },
        );
        let dim_snapshot = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions,
        };
        // Map symbols to sectors
        let mut sector_map = HashMap::new();
        sector_map.insert(Symbol("700.HK".into()), SectorId("tech".into()));
        sector_map.insert(Symbol("388.HK".into()), SectorId("tech".into()));

        detect_propagation_absences(&mut snapshot, &dim_snapshot, &sector_map);

        assert!(snapshot
            .events
            .iter()
            .any(|e| e.value.kind == MarketEventKind::PropagationAbsence));
    }
```

- [ ] **Step 2: Implement detect_propagation_absences**

Add to `src/pipeline/signals/events.rs`:

```rust
pub fn detect_propagation_absences(
    snapshot: &mut EventSnapshot,
    dimensions: &DimensionSnapshot,
    sector_map: &std::collections::HashMap<Symbol, crate::ontology::objects::SectorId>,
) {
    // Group existing event symbols by sector
    let mut sectors_with_events: std::collections::HashMap<
        crate::ontology::objects::SectorId,
        Vec<Symbol>,
    > = std::collections::HashMap::new();
    for event in &snapshot.events {
        if let SignalScope::Symbol(symbol) = &event.value.scope {
            if let Some(sector) = sector_map.get(symbol) {
                sectors_with_events
                    .entry(sector.clone())
                    .or_default()
                    .push(symbol.clone());
            }
        }
    }

    // For each sector with events, check if peers are silent
    for (sector, _event_symbols) in &sectors_with_events {
        let sector_symbols: Vec<&Symbol> = sector_map
            .iter()
            .filter(|(_, s)| *s == sector)
            .map(|(sym, _)| sym)
            .collect();
        if sector_symbols.len() < 2 {
            continue;
        }
        let silent_count = sector_symbols
            .iter()
            .filter(|sym| {
                dimensions
                    .dimensions
                    .get(*sym)
                    .map(|d| d.activity_momentum == Decimal::ZERO)
                    .unwrap_or(true)
            })
            .count();
        let silent_ratio =
            Decimal::from(silent_count as i64) / Decimal::from(sector_symbols.len() as i64);
        if silent_ratio > Decimal::new(5, 1) {
            snapshot.events.push(Event::new(
                MarketEventRecord {
                    scope: SignalScope::Sector(sector.clone()),
                    kind: MarketEventKind::PropagationAbsence,
                    magnitude: silent_ratio,
                    summary: format!(
                        "sector {} has events but {:.0}% of peers are silent",
                        sector,
                        silent_ratio * Decimal::from(100)
                    ),
                },
                provenance(
                    ProvenanceSource::Computed,
                    snapshot.timestamp,
                    Some(silent_ratio),
                    [format!("sector_absence:{}", sector)],
                ),
            ));
        }
    }
}
```

- [ ] **Step 3: Export from signals.rs and update HK runtime call site**

In `src/pipeline/signals.rs`, add:

```rust
pub(crate) use events::detect_propagation_absences;
```

In `src/hk/runtime.rs`, the existing call site at line ~463 passes `(&mut event_snapshot, &dim_snapshot)`. The new signature also needs `&sector_map`. The sector_map can be built from `runtime.store.object_store.stocks` — each Stock has a `sector_id`. Update the call site:

```rust
let sector_map: std::collections::HashMap<_, _> = runtime
    .object_store
    .stocks
    .iter()
    .filter_map(|(sym, stock)| stock.sector_id.as_ref().map(|s| (sym.clone(), s.clone())))
    .collect();
crate::pipeline::signals::detect_propagation_absences(
    &mut event_snapshot,
    &dim_snapshot,
    &sector_map,
);
```

Note: The exact field path to `object_store.stocks` may vary — check the runtime's available fields. If `object_store` is not directly accessible, the sector_map can be passed from wherever the ObjectStore is available.

- [ ] **Step 4: Run test**

Run: `cargo test --lib -- pipeline::signals::events::tests::detect_propagation -q 2>&1 | tail -5`
Expected: 1 test passes

- [ ] **Step 5: Commit**

```bash
git add src/pipeline/signals/events.rs src/pipeline/signals.rs src/hk/runtime.rs
git commit -m "feat(signals): implement detect_propagation_absences"
```

---

### Task 6: Fix remaining HK runtime call sites

**Files:**
- Modify: `src/hk/runtime.rs`
- Modify: `src/bin/replay.rs`

- [ ] **Step 1: Fix catalyst_events_from_macro_events call**

The HK runtime calls `crate::pipeline::signals::catalyst_events_from_macro_events` at line ~452. This function needs to be implemented in events.rs (it was in the old code but not in the recovered 442-line version — it was likely in a separate section).

Add to `src/pipeline/signals/events.rs`:

```rust
pub fn catalyst_events_from_macro_events(
    macro_events: &[crate::ontology::knowledge::AgentMacroEvent],
    timestamp: OffsetDateTime,
) -> Vec<Event<MarketEventRecord>> {
    macro_events
        .iter()
        .filter(|e| is_thematic_catalyst(&e.event_type))
        .map(|e| {
            let magnitude = e.confidence.clamp(Decimal::ZERO, Decimal::ONE);
            Event::new(
                MarketEventRecord {
                    scope: SignalScope::Market,
                    kind: MarketEventKind::CatalystActivation,
                    magnitude,
                    summary: e.headline.clone(),
                },
                {
                    let mut p = provenance(
                        ProvenanceSource::Agent,
                        timestamp,
                        Some(magnitude),
                        [format!("macro_event:{}", e.event_id)],
                    );
                    p.inputs
                        .extend(attribution_inputs("sector_wide", "sector"));
                    p
                },
            )
        })
        .collect()
}

fn is_thematic_catalyst(event_type: &str) -> bool {
    matches!(
        event_type,
        "thematic_catalyst"
            | "sector_catalyst"
            | "policy_catalyst"
            | "earnings_catalyst"
            | "macro_catalyst"
    )
}
```

Export from signals.rs:

```rust
pub use events::{broker_events_from_delta, catalyst_events_from_macro_events};
```

- [ ] **Step 2: Handle enrich_attribution_with_evidence call site**

The HK runtime at line ~457 calls `enrich_attribution_with_evidence`. It's now exported. Verify the call site compiles — the arguments are `(&mut event_snapshot, &links.cross_stock_presences, &macro_events)`.

- [ ] **Step 3: Fix replay.rs call site**

In `src/bin/replay.rs`, the `EventSnapshot::detect` call at line ~148 and `broker_events_from_delta` at line ~158 should now compile. Verify and fix any remaining type mismatches.

- [ ] **Step 4: Verify compilation**

Run: `cargo check --lib 2>&1 | grep -c "^error"`
Expected: error count significantly reduced (target: <30)

- [ ] **Step 5: Commit**

```bash
git add src/pipeline/signals/events.rs src/pipeline/signals.rs src/hk/runtime.rs src/bin/replay.rs
git commit -m "fix(signals): restore catalyst_events + fix all HK runtime call sites"
```

---

### Task 7: Add attribution provenance test

**Files:**
- Modify: `src/pipeline/signals/events.rs` (tests module)

- [ ] **Step 1: Write test verifying attribution injection**

Add to the tests module in events.rs:

```rust
    #[test]
    fn detected_events_have_attribution_provenance() {
        // Build minimal fixtures that trigger at least one event
        // (e.g., an order book with significant imbalance)
        // Then verify the generated event has attr:driver= and attr:scope= in provenance.inputs
        //
        // Use the existing test patterns from pipeline/signals.rs tests
        // (history, links, dimensions, insights, decision fixtures)
        // to call EventSnapshot::detect and check attribution on the result.
    }
```

Note: The exact fixture construction depends on what's available after types.rs restoration. The implementing agent should follow the pattern from existing tests in `pipeline/signals.rs` (around line 300-400).

- [ ] **Step 2: Run all tests**

Run: `cargo test --lib -- pipeline::signals 2>&1 | tail -10`
Expected: attribution test + 5 restored detect tests + 2 new tests all pass

- [ ] **Step 3: Commit**

```bash
git add src/pipeline/signals/events.rs
git commit -m "test(signals): verify attribution provenance injection in detected events"
```

---

### Task 8: Final verification

- [ ] **Step 1: Full cargo check**

Run: `cargo check --lib 2>&1 | grep -c "^error"`
Expected: significant reduction from 162 (target: <30, ideally <10)

- [ ] **Step 2: Verify attribution pipeline is live**

Verify the chain works end-to-end:
1. `EventSnapshot::detect` creates events with `attr:driver=` provenance ✓
2. `enrich_attribution_with_evidence` can upgrade scope ✓
3. `detect_propagation_absences` creates `PropagationAbsence` events ✓
4. `propagation_absence_sectors()` reads those events ✓
5. `AbsenceMemory` accumulates across ticks ✓
6. `hypothesis_templates` reads absence_memory for suppression ✓
7. `attribution_allows_template` reads driver_kind for filtering ✓

- [ ] **Step 3: Git log**

Run: `git log --oneline -8`

Expected 6 new commits:
1. `restore(ontology): recover LinkSnapshot + observation types from git history`
2. `restore(signals): recover EventSnapshot::detect + broker_events with attribution provenance`
3. `fix(signals): restore events.rs re-exports and fix dead imports`
4. `feat(signals): implement enrich_attribution_with_evidence`
5. `feat(signals): implement detect_propagation_absences`
6. `fix(signals): restore catalyst_events + fix all HK runtime call sites`
7. `test(signals): verify attribution provenance injection in detected events`
