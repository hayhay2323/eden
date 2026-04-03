# ReasoningContext Wiring Completion Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Complete the ReasoningContext wiring — define HK attribution types, thread absence_memory + world_state into derive_hypotheses, persist AbsenceMemory across HK ticks, and fix dead imports.

**Architecture:** Define `EventPropagationScope` and `EventDriverKind` in `types.rs` with provenance-based readers (matching US pattern). Thread `absence_memory` and `world_state` from `ReasoningContext` through `derive_hypotheses` into `hypothesis_templates`. Move `AbsenceMemory` lifecycle to HK runtime tick loop. Fix broken `signals.rs` re-exports.

**Tech Stack:** Rust, rust_decimal, time, existing Eden ontology/pipeline crates

**Spec:** `docs/superpowers/specs/2026-04-04-reasoning-context-wiring-design.md`

**Note:** `EventSnapshot::detect` was deleted in ongoing branch refactoring. The event construction sites no longer exist in this branch. This plan defines the reader infrastructure and driver_kind filtering logic — the provenance injection will happen when event construction is rebuilt. All new filtering falls back gracefully when no provenance exists (cold start compatible).

---

### Task 1: Define EventPropagationScope + EventDriverKind in types.rs

**Files:**
- Modify: `src/pipeline/signals/types.rs`

- [ ] **Step 1: Write tests for provenance readers**

At the bottom of `src/pipeline/signals/types.rs`, add:

```rust
#[cfg(test)]
mod attribution_tests {
    use super::*;
    use crate::ontology::domain::{Event, ProvenanceMetadata, ProvenanceSource};
    use time::OffsetDateTime;

    fn event_with_provenance(inputs: Vec<&str>) -> Event<MarketEventRecord> {
        Event {
            value: MarketEventRecord {
                scope: SignalScope::Symbol(crate::ontology::objects::Symbol("700.HK".into())),
                kind: MarketEventKind::SmartMoneyPressure,
                magnitude: rust_decimal::Decimal::ONE,
                summary: "test".into(),
            },
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            )
            .with_inputs(inputs.into_iter().map(String::from).collect()),
        }
    }

    #[test]
    fn hk_event_propagation_scope_from_provenance() {
        let event = event_with_provenance(vec!["attr:scope=sector"]);
        assert_eq!(
            event_propagation_scope(&event),
            Some(EventPropagationScope::Sector)
        );
    }

    #[test]
    fn hk_event_propagation_scope_none_when_missing() {
        let event = event_with_provenance(vec!["other:data"]);
        assert_eq!(event_propagation_scope(&event), None);
    }

    #[test]
    fn hk_event_driver_kind_from_provenance() {
        let event = event_with_provenance(vec!["attr:driver=company_specific"]);
        assert_eq!(
            event_driver_kind(&event),
            Some(EventDriverKind::CompanySpecific)
        );
    }

    #[test]
    fn hk_event_driver_kind_none_when_missing() {
        let event = event_with_provenance(vec![]);
        assert_eq!(event_driver_kind(&event), None);
    }
}
```

- [ ] **Step 2: Implement the enums and reader functions**

Add before the `#[cfg(test)]` block in `src/pipeline/signals/types.rs`:

```rust
// ── Event Attribution ──

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

const ATTR_SCOPE_PREFIX: &str = "attr:scope=";
const ATTR_DRIVER_PREFIX: &str = "attr:driver=";

pub fn event_propagation_scope(
    event: &crate::ontology::domain::Event<MarketEventRecord>,
) -> Option<EventPropagationScope> {
    event.provenance.inputs.iter().find_map(|input| {
        input
            .strip_prefix(ATTR_SCOPE_PREFIX)
            .and_then(|value| match value {
                "local" => Some(EventPropagationScope::Local),
                "sector" => Some(EventPropagationScope::Sector),
                "market" => Some(EventPropagationScope::Market),
                _ => None,
            })
    })
}

pub fn event_driver_kind(
    event: &crate::ontology::domain::Event<MarketEventRecord>,
) -> Option<EventDriverKind> {
    event.provenance.inputs.iter().find_map(|input| {
        input
            .strip_prefix(ATTR_DRIVER_PREFIX)
            .and_then(|value| match value {
                "company_specific" => Some(EventDriverKind::CompanySpecific),
                "sector_wide" => Some(EventDriverKind::SectorWide),
                "macro_wide" => Some(EventDriverKind::MacroWide),
                _ => None,
            })
    })
}
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check --lib -q 2>&1 | grep "signals/types.rs" | head -5`
Expected: no errors from types.rs

- [ ] **Step 4: Commit**

```bash
git add src/pipeline/signals/types.rs
git commit -m "feat(signals): add EventPropagationScope + EventDriverKind with provenance readers"
```

---

### Task 2: Fix dead imports in signals.rs

**Files:**
- Modify: `src/pipeline/signals.rs`

- [ ] **Step 1: Remove dead re-exports from events module**

In `src/pipeline/signals.rs`, replace lines 31-35:

```rust
pub use events::{broker_events_from_delta, catalyst_events_from_macro_events};
pub(crate) use events::{
    detect_propagation_absences, enrich_attribution_with_evidence, event_propagation_scope,
    EventPropagationScope,
};
```

With:

```rust
// Re-export attribution types from types.rs (previously in events.rs, now rebuilt)
pub use types::{event_driver_kind, event_propagation_scope, EventDriverKind, EventPropagationScope};
```

The deleted functions (`broker_events_from_delta`, `catalyst_events_from_macro_events`, `detect_propagation_absences`, `enrich_attribution_with_evidence`) are genuinely gone — they were removed in the ongoing ontology refactoring. Removing their imports fixes compilation errors.

- [ ] **Step 2: Verify compilation**

Run: `cargo check --lib -q 2>&1 | grep "signals.rs" | head -5`
Expected: the `events::broker_events_from_delta` and `events::EventPropagationScope` errors are gone.

- [ ] **Step 3: Commit**

```bash
git add src/pipeline/signals.rs
git commit -m "fix(signals): remove dead events.rs re-exports, re-export attribution types from types.rs"
```

---

### Task 3: Fix attribution_allows_template + add driver_kind filtering

**Files:**
- Modify: `src/pipeline/reasoning/support.rs`

- [ ] **Step 1: Update imports**

In `src/pipeline/reasoning/support.rs`, change line 10-12:

```rust
use crate::pipeline::signals::{
    event_propagation_scope, DerivedSignalKind, EventPropagationScope, MarketEventKind, SignalScope,
};
```

To:

```rust
use crate::pipeline::signals::{
    event_driver_kind, event_propagation_scope, DerivedSignalKind, EventDriverKind,
    EventPropagationScope, MarketEventKind, SignalScope,
};
```

- [ ] **Step 2: Add driver_kind filtering to attribution_allows_template**

In `src/pipeline/reasoning/support.rs`, find `fn attribution_allows_template` (around line 304). After the existing `match template_key` block that returns `true/false` based on scope, add a second check before the final return. Replace the entire function with:

```rust
fn attribution_allows_template(
    relevant_events: &[&crate::ontology::Event<crate::pipeline::signals::MarketEventRecord>],
    template_key: &str,
) -> bool {
    let strongest_scope = relevant_events
        .iter()
        .filter_map(|event| event_propagation_scope(event))
        .max();

    let Some(scope) = strongest_scope else {
        // No attribution data → allow everything (cold start).
        return true;
    };

    let scope_allowed = match template_key {
        // Pure local templates: always allowed regardless of attribution.
        "flow"
        | "liquidity"
        | "risk"
        | "catalyst_repricing"
        | "institution_reversal"
        | "breakout_contagion" => true,

        // Sector-level templates: need at least Sector attribution.
        "sector_rotation_spillover"
        | "sector_symbol_spillover"
        | "stress_concentration"
        | "stress_feedback_loop" => {
            matches!(
                scope,
                EventPropagationScope::Sector | EventPropagationScope::Market
            )
        }

        // Cross-scope / institutional templates: need at least Sector attribution.
        "shared_holder_spillover"
        | "institution_relay"
        | "cross_mechanism_chain"
        | "propagation" => {
            matches!(
                scope,
                EventPropagationScope::Sector | EventPropagationScope::Market
            )
        }

        _ => true,
    };

    if !scope_allowed {
        return false;
    }

    // Driver-kind gate: if ALL events are company_specific, block cross-scope templates.
    let all_company_specific = relevant_events
        .iter()
        .filter_map(|e| event_driver_kind(e))
        .all(|dk| dk == EventDriverKind::CompanySpecific);
    let has_any_driver = relevant_events
        .iter()
        .any(|e| event_driver_kind(e).is_some());

    if has_any_driver && all_company_specific {
        let is_cross_scope = matches!(
            template_key,
            "shared_holder_spillover"
                | "institution_relay"
                | "cross_mechanism_chain"
                | "propagation"
                | "sector_rotation_spillover"
                | "sector_symbol_spillover"
                | "stress_feedback_loop"
                | "stress_concentration"
        );
        if is_cross_scope {
            return false;
        }
    }

    true
}
```

Note: `has_any_driver` check ensures we only apply the driver_kind gate when provenance exists (cold start compatible).

- [ ] **Step 3: Verify compilation**

Run: `cargo check --lib -q 2>&1 | grep "reasoning/support.rs" | head -5`
Expected: no errors.

- [ ] **Step 4: Commit**

```bash
git add src/pipeline/reasoning/support.rs
git commit -m "feat(reasoning): fix attribution_allows_template + add driver_kind filtering"
```

---

### Task 4: Thread absence_memory + world_state into derive_hypotheses

**Files:**
- Modify: `src/pipeline/reasoning/synthesis.rs`
- Modify: `src/pipeline/reasoning.rs`

- [ ] **Step 1: Add parameters to derive_hypotheses**

In `src/pipeline/reasoning/synthesis.rs`, change the `derive_hypotheses` signature (line 95):

```rust
pub(super) fn derive_hypotheses(
    events: &EventSnapshot,
    derived_signals: &DerivedSignalSnapshot,
    propagation_paths: &[PropagationPath],
    family_gate: Option<&FamilyAlphaGate>,
    absence_memory: &crate::pipeline::reasoning::AbsenceMemory,
    world_state: Option<&crate::ontology::world::WorldStateSnapshot>,
) -> Vec<Hypothesis> {
```

- [ ] **Step 2: Pass new params to hypothesis_templates**

Inside `derive_hypotheses`, replace the `hypothesis_templates` call (around line 133):

```rust
        let templates = hypothesis_templates(
            &relevant_events,
            &relevant_signals,
            &relevant_paths,
            family_gate,
            absence_memory,
            world_state,
            &scope,
        );
```

This replaces the current call that passes `AbsenceMemory::default()` and `None`.

- [ ] **Step 3: Update callers in reasoning.rs**

In `src/pipeline/reasoning.rs`, update both `derive_hypotheses` call sites.

First call (in `derive_with_policy`, around line 114):

```rust
        let hypotheses = derive_hypotheses(
            events,
            derived_signals,
            &propagation_paths,
            family_gate.as_ref(),
            ctx.absence_memory,
            ctx.world_state,
        );
```

Second call (in `derive_with_diffusion`, around line 202):

```rust
        let hypotheses = derive_hypotheses(
            events,
            derived_signals,
            &propagation_paths,
            family_gate.as_ref(),
            ctx.absence_memory,
            ctx.world_state,
        );
```

- [ ] **Step 4: Update derive() default path**

In the `derive()` method (around line 66), the `derive_with_policy` call already passes `&ctx` which has `absence_memory` and `world_state` — no change needed since `derive_hypotheses` reads from the params passed by `derive_with_policy`.

- [ ] **Step 5: Verify compilation**

Run: `cargo check --lib -q 2>&1 | grep -E "reasoning/(synthesis|reasoning)\.rs" | head -5`
Expected: no errors.

- [ ] **Step 6: Commit**

```bash
git add src/pipeline/reasoning/synthesis.rs src/pipeline/reasoning.rs
git commit -m "feat(reasoning): thread absence_memory + world_state into derive_hypotheses"
```

---

### Task 5: Persist AbsenceMemory across HK ticks

**Files:**
- Modify: `src/pipeline/reasoning.rs` (make propagation_absence_sectors pub(crate))
- Modify: `src/hk/runtime.rs`

- [ ] **Step 1: Make propagation_absence_sectors accessible**

In `src/pipeline/reasoning.rs`, change line 303:

```rust
fn propagation_absence_sectors(events: &EventSnapshot) -> Vec<crate::ontology::objects::SectorId> {
```

To:

```rust
pub(crate) fn propagation_absence_sectors(events: &EventSnapshot) -> Vec<crate::ontology::objects::SectorId> {
```

- [ ] **Step 2: Add AbsenceMemory to HK runtime tick loop**

In `src/hk/runtime.rs`, find `let mut hidden_force_state` (around line 203, just before `loop {`). Add after it:

```rust
    let mut absence_memory = eden::pipeline::reasoning::AbsenceMemory::default();
```

- [ ] **Step 3: Use the persistent absence_memory in ReasoningContext**

In `src/hk/runtime.rs`, in the `reasoning_ctx` construction (around line 562), change:

```rust
            absence_memory: &eden::pipeline::reasoning::AbsenceMemory::default(),
```

To:

```rust
            absence_memory: &absence_memory,
```

- [ ] **Step 4: Update AbsenceMemory after reasoning completes**

In `src/hk/runtime.rs`, after the `reasoning_snapshot` is fully built (after the hidden force injection block, around line 610-620), add:

```rust
        // Update absence memory for next tick
        {
            let absence_sectors = eden::pipeline::reasoning::propagation_absence_sectors(
                &deep_reasoning_event_snapshot,
            );
            for sector in &absence_sectors {
                absence_memory.record_absence(sector, "propagation", tick, deep_reasoning_decision.timestamp);
            }
            absence_memory.decay(deep_reasoning_decision.timestamp);
        }
```

- [ ] **Step 5: Verify compilation**

Run: `cargo check --lib -q 2>&1 | grep "hk/runtime.rs" | grep -v "pre-existing" | head -5`

Check that no NEW errors appear in hk/runtime.rs related to absence_memory.

- [ ] **Step 6: Commit**

```bash
git add src/pipeline/reasoning.rs src/hk/runtime.rs
git commit -m "feat(runtime): persist AbsenceMemory across HK ticks"
```

---

### Task 6: Final verification

**Files:** (no changes — verification only)

- [ ] **Step 1: Full cargo check**

Run: `cargo check --lib 2>&1 | grep -E "context\.rs|family_gate|absence_memory|driver_kind|EventPropagationScope|EventDriverKind|attribution_allows|derive_hypotheses|propagation_absence_sectors" | head -10`

Expected: no errors from any of the modified files.

- [ ] **Step 2: Verify new tests compile and pass**

Run: `cargo test --lib -- pipeline::signals::types::attribution_tests 2>&1 | tail -10`

Expected: 4 tests pass (scope from provenance, scope none, driver from provenance, driver none).

Run: `cargo test --lib -- pipeline::reasoning::context::tests 2>&1 | tail -10`

Expected: 6 tests pass (3 absence + 3 family boost from previous commit).

- [ ] **Step 3: Verify git log**

Run: `git log --oneline -6`

Expected 5 new commits:
1. `feat(signals): add EventPropagationScope + EventDriverKind with provenance readers`
2. `fix(signals): remove dead events.rs re-exports, re-export attribution types from types.rs`
3. `feat(reasoning): fix attribution_allows_template + add driver_kind filtering`
4. `feat(reasoning): thread absence_memory + world_state into derive_hypotheses`
5. `feat(runtime): persist AbsenceMemory across HK ticks`
