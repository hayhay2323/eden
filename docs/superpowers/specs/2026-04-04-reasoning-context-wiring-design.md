# ReasoningContext Wiring Completion: AbsenceMemory persistence, hypothesis_templates passthrough, HK driver_kind

**Date**: 2026-04-04
**Status**: Approved
**Prerequisite**: `1cdb354` feat(reasoning): introduce ReasoningContext unified information conduit
**Problem**: ReasoningContext was introduced but 3 paths use `default()` placeholders — AbsenceMemory doesn't persist across ticks, hypothesis_templates receives empty absence/world_state, and HK lacks driver_kind attribution.

## 1. Rebuild HK EventPropagationScope + EventDriverKind

`EventPropagationScope` and `event_propagation_scope()` were deleted in ongoing refactoring but still referenced by `attribution_allows_template()`. Rebuild them in `src/pipeline/signals/types.rs` using the same provenance-input encoding as US.

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum EventPropagationScope {
    Local,
    Sector,
    Market,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EventDriverKind {
    CompanySpecific,
    SectorWide,
    MacroWide,
}
```

Two reader functions parse provenance inputs:
- `event_propagation_scope(event) -> Option<EventPropagationScope>` reads `attr:scope=local|sector|market`
- `event_driver_kind(event) -> Option<EventDriverKind>` reads `attr:driver=company_specific|sector_wide|macro_wide`

HK event construction sites in `events.rs` add these provenance inputs based on event kind:
- `SmartMoneyPressure`, `VolumeDislocation`, `OrderBookDislocation`, `CandlestickBreakout`, `BrokerSideFlip` → `attr:driver=company_specific`, `attr:scope=local`
- `SharedHolderAnomaly`, `InstitutionalFlip`, `CompositeAcceleration` → `attr:driver=sector_wide`, `attr:scope=sector`
- `StressRegimeShift`, `MarketStressElevated`, `ManualReviewRequired` → `attr:driver=macro_wide`, `attr:scope=market`
- `CatalystActivation` → `attr:driver=sector_wide`, `attr:scope=sector`
- `PropagationAbsence` → no attribution (keep existing behavior)

## 2. driver_kind filtering in attribution_allows_template

In addition to existing scope-based filtering, add:

```rust
let all_company_specific = relevant_events.iter().all(|e|
    event_driver_kind(e) == Some(EventDriverKind::CompanySpecific)
);
if all_company_specific && is_cross_scope_template(template_key) {
    return false;
}
```

Cross-scope templates: `shared_holder_spillover`, `institution_relay`, `cross_mechanism_chain`, `propagation`, `sector_rotation_spillover`, `sector_symbol_spillover`, `stress_feedback_loop`, `stress_concentration`.

## 3. Thread absence_memory + world_state into derive_hypotheses

Change `derive_hypotheses` signature:

```rust
pub(super) fn derive_hypotheses(
    events: &EventSnapshot,
    derived_signals: &DerivedSignalSnapshot,
    propagation_paths: &[PropagationPath],
    family_gate: Option<&FamilyAlphaGate>,
    absence_memory: &AbsenceMemory,          // NEW
    world_state: Option<&WorldStateSnapshot>, // NEW
) -> Vec<Hypothesis>
```

Inside, pass these to `hypothesis_templates()` instead of the current `AbsenceMemory::default()` and `None`.

Callers in `reasoning.rs` (`derive_with_policy`, `derive_with_diffusion`) pass `ctx.absence_memory` and `ctx.world_state`.

## 4. HK runtime AbsenceMemory persistence

In `src/hk/runtime.rs`, before the tick loop:
```rust
let mut absence_memory = eden::pipeline::reasoning::AbsenceMemory::default();
```

In the ReasoningContext assembly, use `&absence_memory` instead of `&AbsenceMemory::default()`.

After reasoning snapshot is computed, update:
```rust
let absence_sectors = propagation_absence_sectors(&events);
for sector in &absence_sectors {
    absence_memory.record_absence(sector, "propagation", tick, timestamp);
}
absence_memory.decay(timestamp);
```

## File Change List

| # | File | Change |
|---|------|--------|
| 1 | `src/pipeline/signals/types.rs` | Add EventPropagationScope, EventDriverKind enums + reader functions |
| 2 | `src/pipeline/signals.rs` | Fix re-exports to use types.rs instead of deleted events.rs exports |
| 3 | `src/pipeline/signals/events.rs` | Add attr:scope= and attr:driver= provenance to HK event construction |
| 4 | `src/pipeline/reasoning/support.rs` | Fix attribution_allows_template to use new types + add driver_kind filter |
| 5 | `src/pipeline/reasoning/synthesis.rs` | derive_hypotheses adds absence_memory + world_state params |
| 6 | `src/pipeline/reasoning.rs` | Update derive_hypotheses call sites to pass from ctx |
| 7 | `src/hk/runtime.rs` | Persist AbsenceMemory across ticks |

## Test Plan

### types.rs (2 tests)
- `hk_event_propagation_scope_from_provenance`: construct event with `attr:scope=sector` in provenance, verify `event_propagation_scope()` returns `Sector`
- `hk_event_driver_kind_from_provenance`: construct event with `attr:driver=company_specific`, verify returns `CompanySpecific`

### reasoning/tests.rs (4 tests)
- `company_specific_blocks_cross_scope_templates`: all events company_specific → institution_relay NOT in templates
- `sector_wide_allows_institution_relay`: at least one sector_wide event → institution_relay IS in templates
- `no_attribution_allows_all`: no provenance → all templates allowed (cold start)
- `absence_memory_suppresses_in_hypothesis_templates`: AbsenceMemory with 3+ consecutive absences → propagation templates removed for that sector

## Not in scope
- US pipeline changes (already has UsEventDriverKind)
- replay.rs AbsenceMemory (stays default — replay doesn't need cross-tick memory)
- apply_vortex_success_pattern_feedback signature cleanup (separate change)
