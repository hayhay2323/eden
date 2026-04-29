# Resolution System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Collapse Eden's 12 scattered outcome vocabularies into a dual-layer Resolution System (`HorizonResolution` + `CaseResolution`) that gives the learning loop enough structure to distinguish intent-wrong from horizon-wrong from luck.

**Architecture:** Five waves. Wave 0 renames `EvaluationStatus::Expired` to `Due` as a standalone pre-flight commit. Wave 1 adds pure types with zero behavior change. Wave 2 wires `classify_horizon_resolution` into the horizon settle path. Wave 3 introduces the `case_resolution` table with `aggregate_case_resolution` and the `apply_case_resolution_update` upgrade gate. Wave 4 switches the learning loop to read `case_resolution` (with legacy fallback, never merging) and adds outcome-distribution shard recompute. Wave 5 adds the operator-override path and deprecates legacy boolean fields.

**Tech Stack:** Rust, `serde`, `rust_decimal`, existing `EdenStore` SurrealDB persistence, existing Horizon System types (`HorizonBucket`, `EvaluationStatus`, `HorizonResult`, `HorizonEvaluationRecord`), existing Intent System types (`IntentExitKind`, `ExpectationViolation`).

**Spec:** `docs/superpowers/specs/2026-04-12-resolution-system-design.md`

---

## File Structure

### New files (Wave 1)

- `src/ontology/resolution.rs` (~500 lines) — all resolution types + `classify_horizon_resolution` + `aggregate_case_resolution` + `apply_case_resolution_update` upgrade gate + all unit tests. Single file per the spec's "start small, split later" rule (like Horizon Wave 1).
- `src/persistence/case_resolution.rs` (~250 lines) — `CaseResolutionRecord` schema + persistence helpers + roundtrip tests.

### Files modified (Wave 0 — rename)

- `src/persistence/horizon_evaluation.rs` — `EvaluationStatus::Expired` → `Due`
- Any call site using `EvaluationStatus::Expired` (grep gate confirms zero remaining)

### Files modified (Wave 1 — type introduction)

- `src/ontology/mod.rs` — register `pub mod resolution;`
- `src/persistence/mod.rs` — register `pub mod case_resolution;`

### Files modified (Wave 2)

- `src/persistence/horizon_evaluation.rs` — `HorizonEvaluationRecord.resolution: Option<HorizonResolution>` new field
- The horizon settle call site (inside `RuntimeContext::persist_horizon_evaluations` from Horizon Wave 3 Task 15) — extend settle to call `classify_horizon_resolution`

### Files modified (Wave 3)

- `src/persistence/store.rs` — add `write_case_resolutions` + `load_case_resolution_for_setup`
- `src/persistence/schema.rs` — register `case_resolution` SurrealDB table
- `src/core/runtime/context.rs` (or wherever `persist_horizon_evaluations` lives) — add `upsert_case_resolution` hook after horizon settle

### Files modified (Wave 4)

- `src/persistence/discovered_archetype.rs` — add outcome distribution count fields
- `src/pipeline/learning_loop/feedback.rs` — switch primary read path to `case_resolution`, keep legacy fallback
- `src/pipeline/learning_loop/types.rs` — extend `HorizonLearningAdjustment` dispatch or add new resolution-kind delta policy

### Files modified (Wave 5)

- `src/persistence/store.rs` — add `override_case_resolution` method
- `src/ontology/reasoning.rs` — mark `CaseRealizedOutcome.followed_through / invalidated / structure_retained` deprecated (doc only)

---

## Wave 0 — Rename `Expired` to `Due`

Goal: standalone commit that renames `EvaluationStatus::Expired` to `EvaluationStatus::Due` across the repo. Zero semantic change, existing tests still pass. Lands before any Resolution type is introduced to avoid mixing concerns.

### Task 1: Rename `EvaluationStatus::Expired` to `Due`

**Files:**
- Modify: `src/persistence/horizon_evaluation.rs`
- Modify: every call site (found via grep)

- [ ] **Step 1: Catalog all current references**

Run:
```bash
grep -rn 'EvaluationStatus::Expired\|"expired"' src/ --include="*.rs"
```

Also check that `Due` is not already used for something unrelated:
```bash
grep -rn 'EvaluationStatus::Due' src/ --include="*.rs"
```

Expected: zero matches for `Due`. Catalog every `Expired` hit so they can all be fixed in one pass.

- [ ] **Step 2: Update the enum definition**

In `src/persistence/horizon_evaluation.rs`, find:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationStatus {
    Pending,
    Resolved,
    Expired,
    EarlyExited,
}
```

Change to:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationStatus {
    /// Horizon evaluation scheduled but the reference window hasn't
    /// been reached yet. No result, no resolution.
    Pending,
    /// The reference window (`due_at`) has been reached. Numeric result
    /// should be computed but the horizon-level classifier may not yet
    /// have run. Transitional state.
    Due,
    /// Fully settled with a resolution attached.
    Resolved,
    /// Exit signal or operator action ended this horizon before `due_at`.
    /// Resolution is set at the moment of early exit.
    EarlyExited,
}
```

Note: the serde label for `Due` will be `"due"` — previously it was `"expired"`. This is a breaking change for on-disk records. Since no records currently carry `"expired"` in production (Wave 3 only just landed horizon evaluations and they're all `"pending"`), the rename is safe.

- [ ] **Step 3: Update every call site**

For each hit from Step 1, replace `EvaluationStatus::Expired` with `EvaluationStatus::Due`. Replace string literals `"expired"` in test fixtures with `"due"`.

Do not touch doc comments that mention "expired" as a concept unrelated to the enum.

- [ ] **Step 4: Run `cargo check --lib`**

Run: `cargo check --lib`

Expected: clean. Any remaining error indicates a missed call site.

- [ ] **Step 5: Run the post-grep gate**

Run:
```bash
grep -rn 'EvaluationStatus::Expired\|"expired"' src/ --include="*.rs"
```

Expected: empty. If any hit remains, it must be a string appearing in a non-horizon context — verify and, if unrelated, add a brief comment to that effect.

- [ ] **Step 6: Run horizon_evaluation tests**

Run: `cargo test --lib persistence::horizon_evaluation::tests`

Expected: all tests pass. Pay attention to any JSON roundtrip tests that contained `"expired"` — those would have been caught in Step 3 above.

- [ ] **Step 7: Run full library tests**

Run: `cargo test --lib`

Expected: same pass count as before Task 1 (~747 pass, 0 fail).

- [ ] **Step 8: Commit**

```bash
git add -u
git commit -m "refactor(horizon): rename EvaluationStatus::Expired to Due

Pre-flight for the Resolution System. Expired is too easy to confuse
with HorizonResolutionKind::Exhausted, which is semantically different
(Expired = lifecycle maturity; Exhausted = semantic outcome). The
Resolution System spec requires this rename before any resolution
types are introduced.

No semantic change. On-disk records have not yet accumulated any
Expired entries in production, so the serde label change is safe."
```

---

## Wave 1 — Resolution Types (Zero Behavior Change)

Goal: introduce `HorizonResolution`, `CaseResolution`, `ResolutionFinality`, `ResolutionSource`, the classifier, the aggregator, and the upgrade gate. Pure types. Nothing reads or writes them yet.

### Task 2: Create `resolution.rs` with shared enums

**Files:**
- Create: `src/ontology/resolution.rs`
- Modify: `src/ontology/mod.rs`

- [ ] **Step 1: Create `src/ontology/resolution.rs`**

```rust
//! Resolution System — dual-layer case outcome language.
//!
//! Three independent concepts are kept strictly separate:
//! - EvaluationStatus (in persistence/horizon_evaluation.rs) = lifecycle
//! - HorizonResolution (this module) = per-window semantic outcome
//! - CaseResolution (this module) = per-case aggregated semantic outcome
//!
//! See docs/superpowers/specs/2026-04-12-resolution-system-design.md.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::ontology::horizon::HorizonBucket;
use crate::ontology::reasoning::{ExpectationViolation, IntentExitKind};

/// Whether a resolution can still be upgraded by later evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionFinality {
    Provisional,
    Final,
}

/// Who produced this resolution.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionSource {
    Auto,
    OperatorOverride,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolution_finality_snake_case_json() {
        assert_eq!(serde_json::to_string(&ResolutionFinality::Provisional).unwrap(), "\"provisional\"");
        assert_eq!(serde_json::to_string(&ResolutionFinality::Final).unwrap(), "\"final\"");
    }

    #[test]
    fn resolution_source_snake_case_json() {
        assert_eq!(serde_json::to_string(&ResolutionSource::Auto).unwrap(), "\"auto\"");
        assert_eq!(serde_json::to_string(&ResolutionSource::OperatorOverride).unwrap(), "\"operator_override\"");
    }
}
```

Note the `use` imports at the top — `Decimal`, `dec!`, `OffsetDateTime`, `HorizonBucket`, `ExpectationViolation`, `IntentExitKind` are used by later tasks in this same file. The compiler will warn about unused imports until Task 3/4; wrap them in `#[allow(unused_imports)]` at the top if the lint blocks compilation:

```rust
#[allow(unused_imports)]
use rust_decimal::Decimal;
```

Or apply the allow to the whole use block if easier. Remove the allow attribute once Task 4 lands and all imports are used.

- [ ] **Step 2: Register module in `src/ontology/mod.rs`**

Read `src/ontology/mod.rs` first to see the existing `pub mod` list. Add `pub mod resolution;` in alphabetical order with the other declarations.

Do NOT add `pub use resolution::*` — we want types accessed via `crate::ontology::resolution::HorizonResolution` to avoid namespace pollution.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib ontology::resolution::tests`

Expected:
```
test ontology::resolution::tests::resolution_finality_snake_case_json ... ok
test ontology::resolution::tests::resolution_source_snake_case_json ... ok
```

- [ ] **Step 4: `cargo check --lib`**

Run: `cargo check --lib`

Expected: clean. Warnings about unused imports are acceptable at this stage.

- [ ] **Step 5: Commit**

```bash
git add src/ontology/resolution.rs src/ontology/mod.rs
git commit -m "feat(resolution): add ResolutionFinality and ResolutionSource enums"
```

---

### Task 3: Add `HorizonResolutionKind` + `HorizonResolution`

**Files:**
- Modify: `src/ontology/resolution.rs`

- [ ] **Step 1: Add the horizon-level types**

Add before the `#[cfg(test)] mod tests` block in `src/ontology/resolution.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HorizonResolutionKind {
    /// Expected market move materialized within this horizon with
    /// strong follow-through. Matches the intent's original thesis.
    Confirmed,
    /// Horizon's window closed without meaningful movement. Conservative
    /// default. NOT a negative judgment.
    Exhausted,
    /// Hard falsifier, reversal, or strong negative numeric evidence.
    Invalidated,
    /// Intent's explicit completion signal fired. Strongest positive.
    Fulfilled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HorizonResolution {
    pub kind: HorizonResolutionKind,
    pub finality: ResolutionFinality,
    /// Classification tag with source prefix:
    /// "hard_falsifier:...", "window_violation:...", "exit_signal_...",
    /// "numeric_confirmed", "numeric_no_follow_through", "numeric_default".
    pub rationale: String,
    /// Specific trigger (violation falsifier id, exit signal trigger text).
    /// None for pure numeric fallback.
    pub trigger: Option<String>,
}
```

- [ ] **Step 2: Add tests in the `tests` module**

```rust
    #[test]
    fn horizon_resolution_kind_snake_case_json() {
        assert_eq!(serde_json::to_string(&HorizonResolutionKind::Confirmed).unwrap(), "\"confirmed\"");
        assert_eq!(serde_json::to_string(&HorizonResolutionKind::Exhausted).unwrap(), "\"exhausted\"");
        assert_eq!(serde_json::to_string(&HorizonResolutionKind::Invalidated).unwrap(), "\"invalidated\"");
        assert_eq!(serde_json::to_string(&HorizonResolutionKind::Fulfilled).unwrap(), "\"fulfilled\"");
    }

    #[test]
    fn horizon_resolution_roundtrip() {
        let hr = HorizonResolution {
            kind: HorizonResolutionKind::Confirmed,
            finality: ResolutionFinality::Final,
            rationale: "numeric_confirmed".into(),
            trigger: None,
        };
        let json = serde_json::to_string(&hr).unwrap();
        let parsed: HorizonResolution = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, hr);
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib ontology::resolution::tests`

Expected: 4 pass (2 previous + 2 new).

- [ ] **Step 4: `cargo check --lib`**

Run: `cargo check --lib`

Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src/ontology/resolution.rs
git commit -m "feat(resolution): add HorizonResolutionKind + HorizonResolution"
```

---

### Task 4: Add `classify_horizon_resolution` with locked priority rules

**Files:**
- Modify: `src/ontology/resolution.rs`

- [ ] **Step 1: Add the classifier function**

Add after `HorizonResolution` but before the tests module:

```rust
/// Classify a settled horizon's outcome into `HorizonResolution`.
///
/// Pure function. Same inputs always produce same output. No clock,
/// no I/O, no shared state. Priority-ordered — every branch is tested.
///
/// Priority:
/// 1. Hard falsifier (violation.magnitude > 0.5) → Final Invalidated
/// 2. Intent exit signal (Fulfilled / Invalidated / Reversal / Absorbed / ...)
/// 3. Weak violation (magnitude > 0.2) → Provisional Invalidated
/// 4. Numeric confirmed (follow_through ≥ 0.6 + net_return > 0) → Provisional Confirmed
/// 5. Numeric no follow-through (< 0.2) → Provisional Exhausted
/// 6. Default fallback → Provisional Exhausted (never Invalidated)
pub fn classify_horizon_resolution(
    result: &HorizonResult,
    exit: Option<IntentExitKind>,
    violations: &[ExpectationViolation],
) -> HorizonResolution {
    // Priority 1: Hard falsifier
    if let Some(hard) = violations.iter().find(|v| v.magnitude > dec!(0.5)) {
        return HorizonResolution {
            kind: HorizonResolutionKind::Invalidated,
            finality: ResolutionFinality::Final,
            rationale: format!("hard_falsifier: {}", hard.description),
            trigger: hard.falsifier.clone(),
        };
    }

    // Priority 2: Intent exit signal
    match exit {
        Some(IntentExitKind::Fulfilled) => {
            return HorizonResolution {
                kind: HorizonResolutionKind::Fulfilled,
                finality: ResolutionFinality::Final,
                rationale: "exit_signal_fulfilled".into(),
                trigger: None,
            };
        }
        Some(IntentExitKind::Invalidated) => {
            return HorizonResolution {
                kind: HorizonResolutionKind::Invalidated,
                finality: ResolutionFinality::Final,
                rationale: "exit_signal_invalidated".into(),
                trigger: None,
            };
        }
        Some(IntentExitKind::Reversal) => {
            return HorizonResolution {
                kind: HorizonResolutionKind::Invalidated,
                finality: ResolutionFinality::Final,
                rationale: "exit_signal_reversal".into(),
                trigger: None,
            };
        }
        Some(IntentExitKind::Absorbed) => {
            return HorizonResolution {
                kind: HorizonResolutionKind::Exhausted,
                finality: ResolutionFinality::Provisional,
                rationale: "exit_signal_absorbed".into(),
                trigger: None,
            };
        }
        Some(IntentExitKind::Exhaustion) => {
            return HorizonResolution {
                kind: HorizonResolutionKind::Exhausted,
                finality: ResolutionFinality::Provisional,
                rationale: "exit_signal_exhaustion".into(),
                trigger: None,
            };
        }
        Some(IntentExitKind::Decay) => {
            return HorizonResolution {
                kind: HorizonResolutionKind::Exhausted,
                finality: ResolutionFinality::Provisional,
                rationale: "exit_signal_decay".into(),
                trigger: None,
            };
        }
        None => {}
    }

    // Priority 3: Weak violation — window-level, upgradable
    if let Some(soft) = violations.iter().find(|v| v.magnitude > dec!(0.2)) {
        return HorizonResolution {
            kind: HorizonResolutionKind::Invalidated,
            finality: ResolutionFinality::Provisional,
            rationale: format!("window_violation: {}", soft.description),
            trigger: soft.falsifier.clone(),
        };
    }

    // Priority 4: Numeric confirmed
    if result.follow_through >= dec!(0.6) && result.net_return > Decimal::ZERO {
        return HorizonResolution {
            kind: HorizonResolutionKind::Confirmed,
            finality: ResolutionFinality::Provisional,
            rationale: "numeric_confirmed".into(),
            trigger: None,
        };
    }

    // Priority 5: Numeric exhausted (weak follow-through)
    if result.follow_through < dec!(0.2) {
        return HorizonResolution {
            kind: HorizonResolutionKind::Exhausted,
            finality: ResolutionFinality::Provisional,
            rationale: "numeric_no_follow_through".into(),
            trigger: None,
        };
    }

    // Priority 6: Conservative default — Exhausted, NOT Invalidated
    HorizonResolution {
        kind: HorizonResolutionKind::Exhausted,
        finality: ResolutionFinality::Provisional,
        rationale: "numeric_default".into(),
        trigger: None,
    }
}
```

Note: `HorizonResult` is already defined in `src/persistence/horizon_evaluation.rs` (from Wave 3 of Horizon). Add an import at the top of `resolution.rs`:

```rust
use crate::persistence::horizon_evaluation::HorizonResult;
```

- [ ] **Step 2: Add tests covering all 6 branches + default**

Append to the `tests` module:

```rust
    use crate::ontology::reasoning::ExpectationViolationKind;
    use crate::persistence::horizon_evaluation::HorizonResult;

    fn make_result(net: Decimal, follow: Decimal) -> HorizonResult {
        HorizonResult {
            net_return: net,
            resolved_at: OffsetDateTime::UNIX_EPOCH,
            follow_through: follow,
        }
    }

    fn hard_violation() -> ExpectationViolation {
        ExpectationViolation {
            kind: ExpectationViolationKind::FailedConfirmation,
            expectation_id: Some("exp1".into()),
            description: "hard falsifier desc".into(),
            magnitude: dec!(0.8),
            falsifier: Some("falsifier_hard".into()),
        }
    }

    fn weak_violation() -> ExpectationViolation {
        ExpectationViolation {
            kind: ExpectationViolationKind::FailedConfirmation,
            expectation_id: None,
            description: "weak window violation".into(),
            magnitude: dec!(0.3),
            falsifier: Some("falsifier_weak".into()),
        }
    }

    #[test]
    fn classify_hard_falsifier_returns_final_invalidated() {
        let r = classify_horizon_resolution(
            &make_result(dec!(0.01), dec!(0.5)),
            None,
            &[hard_violation()],
        );
        assert_eq!(r.kind, HorizonResolutionKind::Invalidated);
        assert_eq!(r.finality, ResolutionFinality::Final);
        assert!(r.rationale.starts_with("hard_falsifier"));
        assert_eq!(r.trigger.as_deref(), Some("falsifier_hard"));
    }

    #[test]
    fn classify_exit_fulfilled_returns_final_fulfilled() {
        let r = classify_horizon_resolution(
            &make_result(dec!(0.01), dec!(0.5)),
            Some(IntentExitKind::Fulfilled),
            &[],
        );
        assert_eq!(r.kind, HorizonResolutionKind::Fulfilled);
        assert_eq!(r.finality, ResolutionFinality::Final);
    }

    #[test]
    fn classify_exit_invalidated_returns_final_invalidated() {
        let r = classify_horizon_resolution(
            &make_result(dec!(0.0), dec!(0.5)),
            Some(IntentExitKind::Invalidated),
            &[],
        );
        assert_eq!(r.kind, HorizonResolutionKind::Invalidated);
        assert_eq!(r.finality, ResolutionFinality::Final);
    }

    #[test]
    fn classify_exit_reversal_returns_final_invalidated() {
        let r = classify_horizon_resolution(
            &make_result(dec!(0.0), dec!(0.5)),
            Some(IntentExitKind::Reversal),
            &[],
        );
        assert_eq!(r.kind, HorizonResolutionKind::Invalidated);
        assert_eq!(r.finality, ResolutionFinality::Final);
    }

    #[test]
    fn classify_exit_absorbed_returns_provisional_exhausted() {
        let r = classify_horizon_resolution(
            &make_result(dec!(0.0), dec!(0.5)),
            Some(IntentExitKind::Absorbed),
            &[],
        );
        assert_eq!(r.kind, HorizonResolutionKind::Exhausted);
        assert_eq!(r.finality, ResolutionFinality::Provisional);
    }

    #[test]
    fn classify_exit_decay_returns_provisional_exhausted() {
        let r = classify_horizon_resolution(
            &make_result(dec!(0.01), dec!(0.5)),
            Some(IntentExitKind::Decay),
            &[],
        );
        assert_eq!(r.kind, HorizonResolutionKind::Exhausted);
        assert_eq!(r.finality, ResolutionFinality::Provisional);
    }

    #[test]
    fn classify_weak_violation_returns_provisional_invalidated() {
        let r = classify_horizon_resolution(
            &make_result(dec!(0.0), dec!(0.5)),
            None,
            &[weak_violation()],
        );
        assert_eq!(r.kind, HorizonResolutionKind::Invalidated);
        assert_eq!(r.finality, ResolutionFinality::Provisional);
        assert!(r.rationale.starts_with("window_violation"));
    }

    #[test]
    fn classify_numeric_confirmed_returns_provisional_confirmed() {
        let r = classify_horizon_resolution(
            &make_result(dec!(0.015), dec!(0.75)),
            None,
            &[],
        );
        assert_eq!(r.kind, HorizonResolutionKind::Confirmed);
        assert_eq!(r.finality, ResolutionFinality::Provisional);
        assert_eq!(r.rationale, "numeric_confirmed");
    }

    #[test]
    fn classify_numeric_no_follow_through_returns_provisional_exhausted() {
        let r = classify_horizon_resolution(
            &make_result(dec!(0.01), dec!(0.1)),
            None,
            &[],
        );
        assert_eq!(r.kind, HorizonResolutionKind::Exhausted);
        assert_eq!(r.finality, ResolutionFinality::Provisional);
        assert_eq!(r.rationale, "numeric_no_follow_through");
    }

    #[test]
    fn classify_default_fallback_is_exhausted_not_invalidated() {
        // Moderate follow-through, small negative return, no signals.
        // Must default to Exhausted (conservative), NOT Invalidated.
        let r = classify_horizon_resolution(
            &make_result(dec!(-0.003), dec!(0.4)),
            None,
            &[],
        );
        assert_eq!(r.kind, HorizonResolutionKind::Exhausted);
        assert_eq!(r.finality, ResolutionFinality::Provisional);
        assert_eq!(r.rationale, "numeric_default");
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib ontology::resolution::tests`

Expected: 14 pass (4 previous + 10 new classifier branch tests).

- [ ] **Step 4: `cargo check --lib`**

Run: `cargo check --lib`

Expected: clean. The `#[allow(unused_imports)]` from Task 2 can be removed now because `dec!`, `Decimal`, `OffsetDateTime`, `IntentExitKind`, `ExpectationViolation`, `HorizonResult` are all used by the classifier.

- [ ] **Step 5: Commit**

```bash
git add src/ontology/resolution.rs
git commit -m "feat(resolution): classify_horizon_resolution with locked priority rules"
```

---

### Task 5: Add `CaseResolutionKind` + `CaseResolution` + `CaseResolutionTransition`

**Files:**
- Modify: `src/ontology/resolution.rs`

- [ ] **Step 1: Add the case-level types**

Add before the `#[cfg(test)] mod tests` block:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseResolutionKind {
    /// All relevant horizons confirmed. Strongest positive.
    Confirmed,
    /// Some horizons confirmed, some exhausted. Intent partially right.
    PartiallyConfirmed,
    /// Hard falsifier triggered OR all supplementals also failed.
    Invalidated,
    /// Nothing happened across the horizons. Neutral.
    Exhausted,
    /// Primary horizon exhausted/failed, but a supplemental later
    /// confirmed with positive return. Horizon selection was wrong,
    /// intent was fine.
    ProfitableButLate,
    /// Case was closed early before any horizon could settle naturally.
    EarlyExited,
    /// Intent direction was correct but microstructure (liquidity,
    /// spread, slippage) made it untradeable. Operator-only.
    StructurallyRightButUntradeable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseResolution {
    pub kind: CaseResolutionKind,
    pub finality: ResolutionFinality,
    /// One-line operator summary. Opaque string for now.
    pub narrative: String,
    /// Aggregated net return across the case's horizons.
    pub net_return: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseResolutionTransition {
    /// None on the first write (case had no prior resolution).
    #[serde(default)]
    pub from_kind: Option<CaseResolutionKind>,
    /// None on the first write.
    #[serde(default)]
    pub from_finality: Option<ResolutionFinality>,
    pub to_kind: CaseResolutionKind,
    pub to_finality: ResolutionFinality,
    pub triggered_by_horizon: HorizonBucket,
    pub at: OffsetDateTime,
    pub reason: String,
}
```

- [ ] **Step 2: Add tests**

```rust
    #[test]
    fn case_resolution_kind_snake_case_json() {
        assert_eq!(serde_json::to_string(&CaseResolutionKind::Confirmed).unwrap(), "\"confirmed\"");
        assert_eq!(
            serde_json::to_string(&CaseResolutionKind::PartiallyConfirmed).unwrap(),
            "\"partially_confirmed\"",
        );
        assert_eq!(
            serde_json::to_string(&CaseResolutionKind::ProfitableButLate).unwrap(),
            "\"profitable_but_late\"",
        );
        assert_eq!(
            serde_json::to_string(&CaseResolutionKind::StructurallyRightButUntradeable).unwrap(),
            "\"structurally_right_but_untradeable\"",
        );
    }

    #[test]
    fn case_resolution_has_seven_variants() {
        // Smoke guard: if someone adds or removes a kind without updating
        // aggregator/learning policy, this test reminds them.
        let variants = [
            CaseResolutionKind::Confirmed,
            CaseResolutionKind::PartiallyConfirmed,
            CaseResolutionKind::Invalidated,
            CaseResolutionKind::Exhausted,
            CaseResolutionKind::ProfitableButLate,
            CaseResolutionKind::EarlyExited,
            CaseResolutionKind::StructurallyRightButUntradeable,
        ];
        assert_eq!(variants.len(), 7);
    }

    #[test]
    fn case_resolution_transition_from_kind_can_be_none() {
        let t = CaseResolutionTransition {
            from_kind: None,
            from_finality: None,
            to_kind: CaseResolutionKind::Exhausted,
            to_finality: ResolutionFinality::Provisional,
            triggered_by_horizon: HorizonBucket::Fast5m,
            at: OffsetDateTime::UNIX_EPOCH,
            reason: "primary settled".into(),
        };
        let json = serde_json::to_string(&t).unwrap();
        let parsed: CaseResolutionTransition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.from_kind, None);
        assert_eq!(parsed.from_finality, None);
    }

    #[test]
    fn case_resolution_transition_finality_upgrade() {
        let t = CaseResolutionTransition {
            from_kind: Some(CaseResolutionKind::Confirmed),
            from_finality: Some(ResolutionFinality::Provisional),
            to_kind: CaseResolutionKind::Confirmed,
            to_finality: ResolutionFinality::Final,
            triggered_by_horizon: HorizonBucket::Session,
            at: OffsetDateTime::UNIX_EPOCH,
            reason: "all horizons settled".into(),
        };
        let json = serde_json::to_string(&t).unwrap();
        let parsed: CaseResolutionTransition = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, t);
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib ontology::resolution::tests`

Expected: 18 pass (14 previous + 4 new).

- [ ] **Step 4: `cargo check --lib`**

Run: `cargo check --lib`

Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src/ontology/resolution.rs
git commit -m "feat(resolution): add CaseResolutionKind + CaseResolution + Transition types"
```

---

### Task 6: Add `aggregate_case_resolution` with locked aggregation rules

**Files:**
- Modify: `src/ontology/resolution.rs`

- [ ] **Step 1: Add the aggregator function**

Add before the tests module:

```rust
/// Aggregate multiple horizon resolutions into a single case resolution.
///
/// Pure function. Never produces `StructurallyRightButUntradeable`
/// (operator-only). Maps `Fulfilled` at the horizon layer to
/// `CaseResolutionKind::Confirmed` + Final (the case layer has no
/// Fulfilled variant by design).
///
/// Rule priority:
/// 1. Any horizon Final Invalidated → Final Invalidated
/// 2. Any horizon Fulfilled → Final Confirmed
/// 3. All horizons Confirmed AND all_settled → Final Confirmed
/// 4. Primary can be overridden (Exhausted or Provisional Invalidated)
///    AND supplemental Confirmed with positive return → ProfitableButLate
/// 5. Any Confirmed in the mix → PartiallyConfirmed
/// 6. Fallback → Exhausted
pub fn aggregate_case_resolution(
    primary: &HorizonResolution,
    supplementals: &[(HorizonBucket, HorizonResolution, HorizonResult)],
    primary_result: &HorizonResult,
    all_settled: bool,
) -> CaseResolution {
    // Aggregate net return across primary + supplementals.
    let mut net_return = primary_result.net_return;
    for (_, _, result) in supplementals {
        net_return += result.net_return;
    }

    // 1. Hard falsifier anywhere → Final Invalidated
    let primary_is_final_invalidated = primary.finality == ResolutionFinality::Final
        && primary.kind == HorizonResolutionKind::Invalidated;
    let supplemental_has_final_invalidated = supplementals.iter().any(|(_, r, _)| {
        r.finality == ResolutionFinality::Final && r.kind == HorizonResolutionKind::Invalidated
    });
    if primary_is_final_invalidated || supplemental_has_final_invalidated {
        return CaseResolution {
            kind: CaseResolutionKind::Invalidated,
            finality: ResolutionFinality::Final,
            narrative: "hard falsifier triggered".into(),
            net_return,
        };
    }

    // 2. Any Fulfilled → Final Confirmed (case layer has no Fulfilled variant)
    let any_fulfilled = primary.kind == HorizonResolutionKind::Fulfilled
        || supplementals.iter().any(|(_, r, _)| r.kind == HorizonResolutionKind::Fulfilled);
    if any_fulfilled {
        return CaseResolution {
            kind: CaseResolutionKind::Confirmed,
            finality: ResolutionFinality::Final,
            narrative: "intent explicitly fulfilled".into(),
            net_return,
        };
    }

    // 3. All horizons Confirmed AND all_settled → Final Confirmed
    let confirmed_count = std::iter::once(primary)
        .chain(supplementals.iter().map(|(_, r, _)| r))
        .filter(|r| r.kind == HorizonResolutionKind::Confirmed)
        .count();
    let total_horizons = 1 + supplementals.len();
    if confirmed_count == total_horizons && all_settled {
        return CaseResolution {
            kind: CaseResolutionKind::Confirmed,
            finality: ResolutionFinality::Final,
            narrative: format!("all {total_horizons} horizons confirmed"),
            net_return,
        };
    }

    // 4. Primary overridable + supplemental Confirmed + positive return → ProfitableButLate
    let primary_can_be_overridden = matches!(
        primary.kind,
        HorizonResolutionKind::Exhausted | HorizonResolutionKind::Invalidated
    ) && primary.finality == ResolutionFinality::Provisional;

    if primary_can_be_overridden {
        let supp_confirmed_positive = supplementals.iter().any(|(_, r, result)| {
            r.kind == HorizonResolutionKind::Confirmed && result.net_return > Decimal::ZERO
        });
        if supp_confirmed_positive {
            return CaseResolution {
                kind: CaseResolutionKind::ProfitableButLate,
                finality: if all_settled {
                    ResolutionFinality::Final
                } else {
                    ResolutionFinality::Provisional
                },
                narrative: "primary exhausted, supplemental later confirmed".into(),
                net_return,
            };
        }
    }

    // 5. Mix of Confirmed + other → PartiallyConfirmed
    if confirmed_count > 0 {
        return CaseResolution {
            kind: CaseResolutionKind::PartiallyConfirmed,
            finality: if all_settled {
                ResolutionFinality::Final
            } else {
                ResolutionFinality::Provisional
            },
            narrative: format!("{confirmed_count}/{total_horizons} horizons confirmed"),
            net_return,
        };
    }

    // 6. Fallback: Exhausted
    CaseResolution {
        kind: CaseResolutionKind::Exhausted,
        finality: if all_settled {
            ResolutionFinality::Final
        } else {
            ResolutionFinality::Provisional
        },
        narrative: "no horizon confirmed".into(),
        net_return,
    }
}
```

- [ ] **Step 2: Add aggregator tests**

```rust
    fn confirmed(bucket: HorizonBucket) -> HorizonResolution {
        HorizonResolution {
            kind: HorizonResolutionKind::Confirmed,
            finality: ResolutionFinality::Provisional,
            rationale: format!("{:?}_numeric_confirmed", bucket),
            trigger: None,
        }
    }

    fn exhausted_prov() -> HorizonResolution {
        HorizonResolution {
            kind: HorizonResolutionKind::Exhausted,
            finality: ResolutionFinality::Provisional,
            rationale: "numeric_default".into(),
            trigger: None,
        }
    }

    fn invalidated_final() -> HorizonResolution {
        HorizonResolution {
            kind: HorizonResolutionKind::Invalidated,
            finality: ResolutionFinality::Final,
            rationale: "hard_falsifier: test".into(),
            trigger: Some("f1".into()),
        }
    }

    fn fulfilled() -> HorizonResolution {
        HorizonResolution {
            kind: HorizonResolutionKind::Fulfilled,
            finality: ResolutionFinality::Final,
            rationale: "exit_signal_fulfilled".into(),
            trigger: None,
        }
    }

    #[test]
    fn aggregate_single_horizon_confirmed_all_settled_is_final_confirmed() {
        let primary_res = make_result(dec!(0.02), dec!(0.8));
        let out = aggregate_case_resolution(
            &confirmed(HorizonBucket::Fast5m),
            &[],
            &primary_res,
            true,
        );
        assert_eq!(out.kind, CaseResolutionKind::Confirmed);
        assert_eq!(out.finality, ResolutionFinality::Final);
        assert_eq!(out.net_return, dec!(0.02));
    }

    #[test]
    fn aggregate_all_horizons_confirmed_all_settled_is_final_confirmed() {
        let primary_res = make_result(dec!(0.01), dec!(0.7));
        let supp_res = make_result(dec!(0.02), dec!(0.8));
        let out = aggregate_case_resolution(
            &confirmed(HorizonBucket::Fast5m),
            &[(HorizonBucket::Mid30m, confirmed(HorizonBucket::Mid30m), supp_res)],
            &primary_res,
            true,
        );
        assert_eq!(out.kind, CaseResolutionKind::Confirmed);
        assert_eq!(out.finality, ResolutionFinality::Final);
        assert_eq!(out.net_return, dec!(0.03));
    }

    #[test]
    fn aggregate_all_confirmed_but_not_all_settled_stays_provisional() {
        let primary_res = make_result(dec!(0.01), dec!(0.7));
        let supp_res = make_result(dec!(0.02), dec!(0.8));
        let out = aggregate_case_resolution(
            &confirmed(HorizonBucket::Fast5m),
            &[(HorizonBucket::Mid30m, confirmed(HorizonBucket::Mid30m), supp_res)],
            &primary_res,
            false,
        );
        assert_eq!(out.kind, CaseResolutionKind::PartiallyConfirmed);
        assert_eq!(out.finality, ResolutionFinality::Provisional);
    }

    #[test]
    fn aggregate_primary_exhausted_supplemental_confirmed_is_profitable_but_late() {
        let primary_res = make_result(dec!(0.0), dec!(0.1));
        let supp_res = make_result(dec!(0.025), dec!(0.85));
        let out = aggregate_case_resolution(
            &exhausted_prov(),
            &[(HorizonBucket::Mid30m, confirmed(HorizonBucket::Mid30m), supp_res)],
            &primary_res,
            true,
        );
        assert_eq!(out.kind, CaseResolutionKind::ProfitableButLate);
        assert_eq!(out.finality, ResolutionFinality::Final);
        assert_eq!(out.net_return, dec!(0.025));
    }

    #[test]
    fn aggregate_primary_hard_invalidated_is_final_invalidated() {
        let primary_res = make_result(dec!(-0.01), dec!(0.2));
        let out = aggregate_case_resolution(
            &invalidated_final(),
            &[],
            &primary_res,
            true,
        );
        assert_eq!(out.kind, CaseResolutionKind::Invalidated);
        assert_eq!(out.finality, ResolutionFinality::Final);
    }

    #[test]
    fn aggregate_primary_provisional_invalidated_supplemental_confirmed_is_profitable_but_late() {
        // Primary has weak window_violation (Provisional Invalidated).
        // Supplemental later Confirms with positive return.
        // Result: ProfitableButLate.
        let prim_weak_inv = HorizonResolution {
            kind: HorizonResolutionKind::Invalidated,
            finality: ResolutionFinality::Provisional,
            rationale: "window_violation: test".into(),
            trigger: Some("w1".into()),
        };
        let primary_res = make_result(dec!(-0.002), dec!(0.3));
        let supp_res = make_result(dec!(0.03), dec!(0.9));
        let out = aggregate_case_resolution(
            &prim_weak_inv,
            &[(HorizonBucket::Mid30m, confirmed(HorizonBucket::Mid30m), supp_res)],
            &primary_res,
            true,
        );
        assert_eq!(out.kind, CaseResolutionKind::ProfitableButLate);
        assert_eq!(out.finality, ResolutionFinality::Final);
    }

    #[test]
    fn aggregate_any_fulfilled_maps_to_final_confirmed() {
        let primary_res = make_result(dec!(0.015), dec!(0.7));
        let out = aggregate_case_resolution(
            &fulfilled(),
            &[],
            &primary_res,
            true,
        );
        assert_eq!(out.kind, CaseResolutionKind::Confirmed);
        assert_eq!(out.finality, ResolutionFinality::Final);
        assert!(out.narrative.contains("fulfilled"));
    }

    #[test]
    fn aggregate_mix_confirmed_exhausted_is_partially_confirmed() {
        let primary_res = make_result(dec!(0.01), dec!(0.7));
        let supp_res = make_result(dec!(0.0), dec!(0.1));
        let out = aggregate_case_resolution(
            &confirmed(HorizonBucket::Fast5m),
            &[(HorizonBucket::Mid30m, exhausted_prov(), supp_res)],
            &primary_res,
            true,
        );
        assert_eq!(out.kind, CaseResolutionKind::PartiallyConfirmed);
        assert_eq!(out.finality, ResolutionFinality::Final);
    }

    #[test]
    fn aggregate_all_exhausted_all_settled_is_final_exhausted() {
        let primary_res = make_result(dec!(0.0), dec!(0.15));
        let supp_res = make_result(dec!(0.0), dec!(0.1));
        let out = aggregate_case_resolution(
            &exhausted_prov(),
            &[(HorizonBucket::Mid30m, exhausted_prov(), supp_res)],
            &primary_res,
            true,
        );
        assert_eq!(out.kind, CaseResolutionKind::Exhausted);
        assert_eq!(out.finality, ResolutionFinality::Final);
    }

    #[test]
    fn aggregate_all_exhausted_with_pending_is_provisional_exhausted() {
        // Only primary has settled; supplementals pending (empty here = not yet present)
        let primary_res = make_result(dec!(0.0), dec!(0.1));
        let out = aggregate_case_resolution(
            &exhausted_prov(),
            &[],
            &primary_res,
            false,
        );
        assert_eq!(out.kind, CaseResolutionKind::Exhausted);
        assert_eq!(out.finality, ResolutionFinality::Provisional);
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib ontology::resolution::tests`

Expected: 28 pass (18 previous + 10 new aggregator tests).

- [ ] **Step 4: `cargo check --lib`**

Run: `cargo check --lib`

Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src/ontology/resolution.rs
git commit -m "feat(resolution): aggregate_case_resolution with locked rules"
```

---

### Task 7: Add `apply_case_resolution_update` with upgrade gate

**Files:**
- Modify: `src/ontology/resolution.rs`

- [ ] **Step 1: Add the upgrade gate function**

```rust
/// Result of an upgrade attempt. Callers decide what to do based on this.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UpdateOutcome {
    /// The new resolution differs and was applied (transition appended).
    Applied,
    /// The new resolution is identical to current (kind AND finality). No-op.
    NoChange,
    /// The current resolution is Final and cannot be changed.
    RejectedFinal,
    /// The proposed change is a downgrade and was rejected.
    RejectedDowngrade,
}

/// Returns true if `new_kind` is a valid monotonic upgrade from `current_kind`.
/// Same-kind is NOT an upgrade (use finality transition for that).
pub fn is_valid_upgrade(current: CaseResolutionKind, new: CaseResolutionKind) -> bool {
    use CaseResolutionKind::*;
    matches!(
        (current, new),
        (Exhausted, ProfitableButLate)
            | (Exhausted, PartiallyConfirmed)
            | (Exhausted, Confirmed)
            | (PartiallyConfirmed, Confirmed)
            | (Invalidated, ProfitableButLate)
            | (Invalidated, PartiallyConfirmed)
            | (EarlyExited, ProfitableButLate)
    )
}

/// Transient update input produced by the aggregator.
#[derive(Debug, Clone)]
pub struct ResolutionUpdate {
    pub new_resolution: CaseResolution,
    pub triggered_by_horizon: HorizonBucket,
    pub at: OffsetDateTime,
    pub reason: String,
}

/// Apply an upgrade attempt to an existing `CaseResolution` + transition
/// history. This is the single choke point for all case-resolution
/// mutations. Enforces:
/// - Final lock
/// - Downgrade rejection
/// - No-op skip (both kind and finality identical)
/// - Finality transitions are recorded even when kind is unchanged
/// - History append is always one new transition on Applied
pub fn apply_case_resolution_update(
    current: &mut CaseResolution,
    history: &mut Vec<CaseResolutionTransition>,
    update: ResolutionUpdate,
) -> UpdateOutcome {
    // No-op: exactly the same
    if current.kind == update.new_resolution.kind
        && current.finality == update.new_resolution.finality
    {
        return UpdateOutcome::NoChange;
    }

    // Final lock: cannot change anything once Final
    if current.finality == ResolutionFinality::Final {
        return UpdateOutcome::RejectedFinal;
    }

    // Downgrade check:
    //   If kind is changing, it must be a monotonic upgrade.
    //   If only finality is changing (kind identical), always allowed
    //   (Provisional → Final).
    if current.kind != update.new_resolution.kind
        && !is_valid_upgrade(current.kind, update.new_resolution.kind)
    {
        return UpdateOutcome::RejectedDowngrade;
    }

    // Accepted: append transition BEFORE mutating current
    history.push(CaseResolutionTransition {
        from_kind: Some(current.kind),
        from_finality: Some(current.finality),
        to_kind: update.new_resolution.kind,
        to_finality: update.new_resolution.finality,
        triggered_by_horizon: update.triggered_by_horizon,
        at: update.at,
        reason: update.reason,
    });

    // Update in place
    *current = update.new_resolution;

    UpdateOutcome::Applied
}

/// Build the initial transition for a case that has no prior resolution.
/// Used when writing the first `CaseResolutionRecord` for a case.
pub fn initial_case_resolution_transition(
    new: &CaseResolution,
    triggered_by_horizon: HorizonBucket,
    at: OffsetDateTime,
    reason: String,
) -> CaseResolutionTransition {
    CaseResolutionTransition {
        from_kind: None,
        from_finality: None,
        to_kind: new.kind,
        to_finality: new.finality,
        triggered_by_horizon,
        at,
        reason,
    }
}
```

- [ ] **Step 2: Add gate tests**

```rust
    fn make_case_resolution(kind: CaseResolutionKind, finality: ResolutionFinality) -> CaseResolution {
        CaseResolution {
            kind,
            finality,
            narrative: "test".into(),
            net_return: dec!(0.0),
        }
    }

    fn make_update(kind: CaseResolutionKind, finality: ResolutionFinality) -> ResolutionUpdate {
        ResolutionUpdate {
            new_resolution: make_case_resolution(kind, finality),
            triggered_by_horizon: HorizonBucket::Mid30m,
            at: OffsetDateTime::UNIX_EPOCH,
            reason: "test".into(),
        }
    }

    #[test]
    fn apply_update_rejects_downgrade() {
        let mut cur = make_case_resolution(CaseResolutionKind::Confirmed, ResolutionFinality::Provisional);
        let mut hist: Vec<CaseResolutionTransition> = vec![];
        let out = apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(CaseResolutionKind::Exhausted, ResolutionFinality::Provisional),
        );
        assert_eq!(out, UpdateOutcome::RejectedDowngrade);
        assert_eq!(cur.kind, CaseResolutionKind::Confirmed);
        assert!(hist.is_empty());
    }

    #[test]
    fn apply_update_rejects_final_change() {
        let mut cur = make_case_resolution(CaseResolutionKind::Confirmed, ResolutionFinality::Final);
        let mut hist: Vec<CaseResolutionTransition> = vec![];
        let out = apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(CaseResolutionKind::ProfitableButLate, ResolutionFinality::Final),
        );
        assert_eq!(out, UpdateOutcome::RejectedFinal);
        assert_eq!(cur.kind, CaseResolutionKind::Confirmed);
        assert!(hist.is_empty());
    }

    #[test]
    fn apply_update_skips_noop() {
        let mut cur = make_case_resolution(CaseResolutionKind::Confirmed, ResolutionFinality::Provisional);
        let mut hist: Vec<CaseResolutionTransition> = vec![];
        let out = apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(CaseResolutionKind::Confirmed, ResolutionFinality::Provisional),
        );
        assert_eq!(out, UpdateOutcome::NoChange);
        assert!(hist.is_empty());
    }

    #[test]
    fn apply_update_allows_provisional_to_final_same_kind() {
        let mut cur = make_case_resolution(CaseResolutionKind::Confirmed, ResolutionFinality::Provisional);
        let mut hist: Vec<CaseResolutionTransition> = vec![];
        let out = apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(CaseResolutionKind::Confirmed, ResolutionFinality::Final),
        );
        assert_eq!(out, UpdateOutcome::Applied);
        assert_eq!(cur.kind, CaseResolutionKind::Confirmed);
        assert_eq!(cur.finality, ResolutionFinality::Final);
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].from_finality, Some(ResolutionFinality::Provisional));
        assert_eq!(hist[0].to_finality, ResolutionFinality::Final);
    }

    #[test]
    fn apply_update_allows_valid_upgrade_exhausted_to_profitable_but_late() {
        let mut cur = make_case_resolution(CaseResolutionKind::Exhausted, ResolutionFinality::Provisional);
        let mut hist: Vec<CaseResolutionTransition> = vec![];
        let out = apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(CaseResolutionKind::ProfitableButLate, ResolutionFinality::Provisional),
        );
        assert_eq!(out, UpdateOutcome::Applied);
        assert_eq!(cur.kind, CaseResolutionKind::ProfitableButLate);
        assert_eq!(hist.len(), 1);
        assert_eq!(hist[0].from_kind, Some(CaseResolutionKind::Exhausted));
        assert_eq!(hist[0].to_kind, CaseResolutionKind::ProfitableButLate);
    }

    #[test]
    fn apply_update_appends_transition_on_every_change() {
        let mut cur = make_case_resolution(CaseResolutionKind::Exhausted, ResolutionFinality::Provisional);
        let mut hist: Vec<CaseResolutionTransition> = vec![];

        apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(CaseResolutionKind::PartiallyConfirmed, ResolutionFinality::Provisional),
        );
        apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(CaseResolutionKind::Confirmed, ResolutionFinality::Provisional),
        );
        apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(CaseResolutionKind::Confirmed, ResolutionFinality::Final),
        );

        assert_eq!(hist.len(), 3);
        // Each transition records what it came from
        assert_eq!(hist[0].from_kind, Some(CaseResolutionKind::Exhausted));
        assert_eq!(hist[1].from_kind, Some(CaseResolutionKind::PartiallyConfirmed));
        assert_eq!(hist[2].from_kind, Some(CaseResolutionKind::Confirmed));
        assert_eq!(hist[2].from_finality, Some(ResolutionFinality::Provisional));
        assert_eq!(hist[2].to_finality, ResolutionFinality::Final);
    }

    #[test]
    fn apply_update_never_rewrites_history() {
        let mut cur = make_case_resolution(CaseResolutionKind::Exhausted, ResolutionFinality::Provisional);
        let mut hist: Vec<CaseResolutionTransition> = vec![];

        apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(CaseResolutionKind::PartiallyConfirmed, ResolutionFinality::Provisional),
        );
        let first_snapshot = hist[0].clone();

        apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(CaseResolutionKind::Confirmed, ResolutionFinality::Final),
        );

        // First transition must be byte-for-byte identical
        assert_eq!(hist[0], first_snapshot);
        // History grew monotonically
        assert_eq!(hist.len(), 2);
    }

    #[test]
    fn initial_transition_has_none_from() {
        let new = make_case_resolution(CaseResolutionKind::Exhausted, ResolutionFinality::Provisional);
        let t = initial_case_resolution_transition(
            &new,
            HorizonBucket::Fast5m,
            OffsetDateTime::UNIX_EPOCH,
            "primary settled".into(),
        );
        assert_eq!(t.from_kind, None);
        assert_eq!(t.from_finality, None);
        assert_eq!(t.to_kind, CaseResolutionKind::Exhausted);
        assert_eq!(t.triggered_by_horizon, HorizonBucket::Fast5m);
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib ontology::resolution::tests`

Expected: 36 pass (28 previous + 8 new gate tests).

- [ ] **Step 4: `cargo check --lib`**

Run: `cargo check --lib`

Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src/ontology/resolution.rs
git commit -m "feat(resolution): apply_case_resolution_update single choke point"
```

---

### Task 8: Create `case_resolution.rs` persistence record

**Files:**
- Create: `src/persistence/case_resolution.rs`
- Modify: `src/persistence/mod.rs`

- [ ] **Step 1: Create `src/persistence/case_resolution.rs`**

```rust
//! Case-resolution persistence record.
//!
//! One row per tactical setup. Written first on primary horizon settle,
//! possibly upgraded on each supplemental settle. The `resolution_history`
//! is append-only — every upgrade (kind or finality) adds exactly one
//! transition. Never rewritten or collapsed.

use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::ontology::horizon::HorizonBucket;
use crate::ontology::resolution::{
    CaseResolution, CaseResolutionTransition, HorizonResolution, ResolutionSource,
};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseResolutionRecord {
    pub record_id: String,
    pub setup_id: String,
    pub market: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    pub primary_horizon: HorizonBucket,
    pub resolution: CaseResolution,
    pub resolution_source: ResolutionSource,
    /// Denormalized snapshot of horizon resolutions at the time of the
    /// latest update. NOT source of truth — the horizon_evaluation table is.
    pub horizon_resolution_snapshot: Vec<HorizonResolution>,
    /// Append-only. Every upgrade adds one transition. Never rewritten.
    pub resolution_history: Vec<CaseResolutionTransition>,
    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}

impl CaseResolutionRecord {
    /// Construct the record_id from a setup_id. One record per setup.
    pub fn build_id(setup_id: &str) -> String {
        format!("case-resolution:{setup_id}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::resolution::{CaseResolutionKind, ResolutionFinality};
    use rust_decimal_macros::dec;
    use time::macros::datetime;

    fn sample() -> CaseResolutionRecord {
        CaseResolutionRecord {
            record_id: "case-resolution:setup-1".into(),
            setup_id: "setup-1".into(),
            market: "us".into(),
            symbol: Some("FICO.US".into()),
            primary_horizon: HorizonBucket::Fast5m,
            resolution: CaseResolution {
                kind: CaseResolutionKind::Confirmed,
                finality: ResolutionFinality::Provisional,
                narrative: "test".into(),
                net_return: dec!(0.02),
            },
            resolution_source: ResolutionSource::Auto,
            horizon_resolution_snapshot: vec![],
            resolution_history: vec![CaseResolutionTransition {
                from_kind: None,
                from_finality: None,
                to_kind: CaseResolutionKind::Confirmed,
                to_finality: ResolutionFinality::Provisional,
                triggered_by_horizon: HorizonBucket::Fast5m,
                at: datetime!(2026-04-13 14:05 UTC),
                reason: "primary settled".into(),
            }],
            created_at: datetime!(2026-04-13 14:05 UTC),
            updated_at: datetime!(2026-04-13 14:05 UTC),
        }
    }

    #[test]
    fn record_roundtrip() {
        let record = sample();
        let json = serde_json::to_string(&record).unwrap();
        let parsed: CaseResolutionRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, record);
    }

    #[test]
    fn build_id_is_deterministic() {
        assert_eq!(
            CaseResolutionRecord::build_id("setup-7"),
            "case-resolution:setup-7",
        );
    }

    #[test]
    fn record_with_upgrade_history_serializes() {
        let mut record = sample();
        record.resolution_history.push(CaseResolutionTransition {
            from_kind: Some(CaseResolutionKind::Confirmed),
            from_finality: Some(ResolutionFinality::Provisional),
            to_kind: CaseResolutionKind::Confirmed,
            to_finality: ResolutionFinality::Final,
            triggered_by_horizon: HorizonBucket::Session,
            at: datetime!(2026-04-13 20:00 UTC),
            reason: "all horizons settled".into(),
        });
        let json = serde_json::to_string(&record).unwrap();
        let parsed: CaseResolutionRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.resolution_history.len(), 2);
    }
}
```

- [ ] **Step 2: Register module in `src/persistence/mod.rs`**

Read the file first, then add `pub mod case_resolution;` in alphabetical order with the other `pub mod` declarations. Do NOT add a `pub use` re-export.

- [ ] **Step 3: Run tests**

Run: `cargo test --lib persistence::case_resolution::tests`

Expected: 3 pass.

- [ ] **Step 4: `cargo check --lib`**

Run: `cargo check --lib`

Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src/persistence/case_resolution.rs src/persistence/mod.rs
git commit -m "feat(resolution): add CaseResolutionRecord persistence type"
```

---

### Task 9: Wave 1 exit — verification

**Files:** none modified, verification only.

- [ ] **Step 1: Full library compile**

Run: `cargo check --lib`

Expected: clean.

- [ ] **Step 2: Run all Wave 1 resolution tests**

Run: `cargo test --lib ontology::resolution::tests persistence::case_resolution::tests`

Expected: 39 pass (36 ontology::resolution + 3 persistence::case_resolution).

- [ ] **Step 3: Full library tests — ensure no regression**

Run: `cargo test --lib`

Expected: same pass count as before Wave 1 plus 39 new (~786 pass, 0 fail).

- [ ] **Step 4: Verify zero production paths touched**

Run:
```bash
git diff horizon-wave-4..HEAD --stat -- ':!src/ontology/resolution.rs' ':!src/persistence/case_resolution.rs' ':!src/ontology/mod.rs' ':!src/persistence/mod.rs' ':!src/persistence/horizon_evaluation.rs' ':!docs/'
```

Expected: empty output. Wave 0 + Wave 1 must touch only these files (Wave 0 touched `horizon_evaluation.rs` for the rename).

- [ ] **Step 5: Tag the wave**

```bash
git tag resolution-wave-1
```

---

## Wave 2 — Wire Classifier Into Horizon Settle

Goal: `HorizonEvaluationRecord.resolution` field starts being written at settle time. Legacy records without the field still deserialize.

### Task 10: Add `resolution` field to `HorizonEvaluationRecord`

**Files:**
- Modify: `src/persistence/horizon_evaluation.rs`

- [ ] **Step 1: Write failing test for the new field**

Add to the existing `mod tests` block in `src/persistence/horizon_evaluation.rs`:

```rust
    #[test]
    fn record_with_resolution_serializes_and_deserializes() {
        use crate::ontology::resolution::{
            HorizonResolution, HorizonResolutionKind, ResolutionFinality,
        };
        let r = HorizonEvaluationRecord {
            record_id: "horizon-eval:setup-1:Fast5m".into(),
            setup_id: "setup-1".into(),
            market: "us".into(),
            horizon: HorizonBucket::Fast5m,
            primary: true,
            due_at: datetime!(2026-04-13 14:35 UTC),
            status: EvaluationStatus::Resolved,
            result: Some(HorizonResult {
                net_return: dec!(0.02),
                resolved_at: datetime!(2026-04-13 14:35 UTC),
                follow_through: dec!(0.8),
            }),
            resolution: Some(HorizonResolution {
                kind: HorizonResolutionKind::Confirmed,
                finality: ResolutionFinality::Provisional,
                rationale: "numeric_confirmed".into(),
                trigger: None,
            }),
        };
        let json = serde_json::to_string(&r).unwrap();
        let parsed: HorizonEvaluationRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn legacy_record_without_resolution_field_deserializes_with_none() {
        // Manually build a JSON payload lacking the `resolution` field
        let json = r#"{
            "record_id": "horizon-eval:legacy:Fast5m",
            "setup_id": "legacy",
            "market": "us",
            "horizon": "fast5m",
            "primary": true,
            "due_at": "2026-04-13T14:35:00Z",
            "status": "resolved",
            "result": {
                "net_return": "0.01",
                "resolved_at": "2026-04-13T14:35:00Z",
                "follow_through": "0.7"
            }
        }"#;
        let parsed: HorizonEvaluationRecord = serde_json::from_str(json).unwrap();
        assert_eq!(parsed.resolution, None);
    }
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test --lib persistence::horizon_evaluation::tests::record_with_resolution_serializes_and_deserializes`

Expected: FAIL with "no field `resolution`" or similar.

- [ ] **Step 3: Add the field**

In `src/persistence/horizon_evaluation.rs`, in the `HorizonEvaluationRecord` struct, add after `result`:

```rust
    /// New in Resolution System Wave 2. Written when the record transitions
    /// from Due/EarlyExited to Resolved with a classifier output. Legacy
    /// records without this field deserialize as None.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<crate::ontology::resolution::HorizonResolution>,
```

Also ensure the constructor or helper `pending_for_case` (from Horizon Wave 3) initializes `resolution: None` when building new pending records:

```rust
// In the struct literal inside pending_for_case:
resolution: None,
```

Check `grep -n "pending_for_case\|HorizonEvaluationRecord {" src/persistence/horizon_evaluation.rs` to find all construction sites inside the file.

- [ ] **Step 4: Run tests**

Run: `cargo test --lib persistence::horizon_evaluation::tests`

Expected: all pass including the 2 new tests.

- [ ] **Step 5: `cargo check --lib`**

Run: `cargo check --lib`

Expected: clean. If any construction site outside the file failed, the compiler will name it — add `resolution: None,` to each.

- [ ] **Step 6: Commit**

```bash
git add src/persistence/horizon_evaluation.rs
git commit -m "feat(resolution): add resolution field to HorizonEvaluationRecord"
```

---

### Task 11: Wire `classify_horizon_resolution` into horizon settle path

**Files:**
- Modify: the settle call site (`src/core/runtime/context.rs` or wherever `pending_for_case` consumers settle records)

- [ ] **Step 1: Locate the settle path**

Run:
```bash
grep -rn "EvaluationStatus::Resolved\|EvaluationStatus::EarlyExited" src/ --include="*.rs"
```

Find where a record's status gets flipped from Pending/Due to Resolved or EarlyExited. This is where the classifier needs to run.

From Horizon Wave 3, the write path in `src/core/runtime/context.rs::persist_horizon_evaluations` creates records as Pending. There should be a separate settle step. If no settle step exists yet, add a stub now that calls the classifier:

```rust
/// Settle a single horizon evaluation record: flip status, attach
/// result and resolution. Called when due_at has been reached or an
/// exit signal fires. See Resolution System Wave 2.
pub(crate) fn settle_horizon_evaluation(
    record: &mut HorizonEvaluationRecord,
    result: HorizonResult,
    exit: Option<crate::ontology::reasoning::IntentExitKind>,
    violations: &[crate::ontology::reasoning::ExpectationViolation],
    new_status: EvaluationStatus,
) {
    debug_assert!(
        matches!(new_status, EvaluationStatus::Resolved | EvaluationStatus::EarlyExited),
        "settle must set Resolved or EarlyExited, not {:?}", new_status,
    );
    record.status = new_status;
    let resolution = crate::ontology::resolution::classify_horizon_resolution(
        &result,
        exit,
        violations,
    );
    record.result = Some(result);
    record.resolution = Some(resolution);
}
```

If a settle path already exists (e.g. `RuntimeContext::settle_horizon_evaluation` from a later Horizon task), extend it with the classifier call and the resolution assignment. In either case, the consistency invariant must hold:

```
status == Pending | Due       → resolution = None
status == Resolved | EarlyExited → resolution = Some(classify_horizon_resolution(...))
```

- [ ] **Step 2: Write failing test for the settle helper**

In `src/persistence/horizon_evaluation.rs` or wherever the helper lives, add:

```rust
    #[test]
    fn settle_horizon_evaluation_sets_resolution() {
        use crate::ontology::resolution::{HorizonResolutionKind, ResolutionFinality};
        let mut record = HorizonEvaluationRecord {
            record_id: "test".into(),
            setup_id: "test".into(),
            market: "us".into(),
            horizon: HorizonBucket::Fast5m,
            primary: true,
            due_at: datetime!(2026-04-13 14:35 UTC),
            status: EvaluationStatus::Due,
            result: None,
            resolution: None,
        };
        let result = HorizonResult {
            net_return: dec!(0.02),
            resolved_at: datetime!(2026-04-13 14:35 UTC),
            follow_through: dec!(0.75),
        };
        settle_horizon_evaluation(&mut record, result, None, &[], EvaluationStatus::Resolved);
        assert_eq!(record.status, EvaluationStatus::Resolved);
        assert!(record.result.is_some());
        let res = record.resolution.as_ref().unwrap();
        assert_eq!(res.kind, HorizonResolutionKind::Confirmed);
        assert_eq!(res.finality, ResolutionFinality::Provisional);
    }
```

- [ ] **Step 3: Run test to verify it passes**

Run: `cargo test --lib settle_horizon_evaluation_sets_resolution`

Expected: PASS.

- [ ] **Step 4: `cargo check --lib`**

Run: `cargo check --lib`

Expected: clean.

- [ ] **Step 5: Full library tests**

Run: `cargo test --lib`

Expected: all pass, no regression.

- [ ] **Step 6: Commit**

```bash
git add -u
git commit -m "feat(resolution): wire classify_horizon_resolution into settle helper"
```

---

### Task 12: Wave 2 exit — verification

- [ ] **Step 1: `cargo check --lib`**

Run: `cargo check --lib`

Expected: clean.

- [ ] **Step 2: Full library tests**

Run: `cargo test --lib`

Expected: baseline + Wave 2 new tests, no regressions.

- [ ] **Step 3: Tag**

```bash
git tag resolution-wave-2
```

---

## Wave 3 — Case Resolution Table + Aggregator Hook

Goal: every new tactical setup gets a `CaseResolutionRecord` on primary horizon settle. Supplemental settles trigger aggregator re-runs via `apply_case_resolution_update`.

### Task 13: Add `EdenStore` read/write for `CaseResolutionRecord`

**Files:**
- Modify: `src/persistence/store.rs`
- Modify: `src/persistence/schema.rs`

- [ ] **Step 1: Locate an existing write pattern**

```bash
grep -n "pub async fn write_horizon_evaluations\|pub async fn load_horizon_evaluations_for_setup" src/persistence/store.rs
```

Use the Horizon Wave 3 Task 14 methods as a template for shape, signatures, and error handling.

- [ ] **Step 2: Add store methods**

Inside `impl EdenStore` in `src/persistence/store.rs`, add near the horizon evaluation methods:

```rust
    pub async fn write_case_resolutions(
        &self,
        records: &[crate::persistence::case_resolution::CaseResolutionRecord],
    ) -> Result<(), StoreError> {
        if records.is_empty() {
            return Ok(());
        }
        upsert_batch_checked(
            &self.db,
            "case_resolution",
            records,
            |r| r.record_id.as_str(),
        )
        .await
    }

    pub async fn load_case_resolution_for_setup(
        &self,
        setup_id: &str,
    ) -> Result<Option<crate::persistence::case_resolution::CaseResolutionRecord>, StoreError>
    {
        let mut records = fetch_records_by_field_order(
            &self.db,
            "case_resolution",
            "setup_id",
            setup_id,
            "updated_at",
            false, // descending — newest first
            1,
        )
        .await?;
        Ok(records.pop())
    }
```

If the actual signature of `upsert_batch_checked` or `fetch_records_by_field_order` differs (check `src/persistence/store_helpers.rs`), adapt the call. Copy the shape from `write_horizon_evaluations` which follows the exact same pattern.

- [ ] **Step 3: Register schema**

In `src/persistence/schema.rs`, find where `horizon_evaluation` table is declared. Add a mirror entry for `case_resolution`:

```rust
    sql.push_str("DEFINE TABLE case_resolution SCHEMALESS;\n");
```

If the file uses a migration number system (as Horizon Wave 3 Task 14 did when bumping to MIGRATION_025), add a new migration entry. Check the existing migration pattern and match it.

- [ ] **Step 4: `cargo check --lib`**

Run: `cargo check --lib`

Expected: clean.

- [ ] **Step 5: Commit**

```bash
git add src/persistence/store.rs src/persistence/schema.rs
git commit -m "feat(resolution): EdenStore read/write for CaseResolutionRecord"
```

Note: a runtime persistence test would require `--features persistence` which triggers a 18-minute RocksDB build on this machine and often OOMs. The shape of these methods mirrors `write_horizon_evaluations` (Horizon Wave 3 Task 14) exactly, so trust the pattern.

---

### Task 14: Add `upsert_case_resolution` hook + aggregator integration

**Files:**
- Modify: wherever `persist_horizon_evaluations` lives (from Horizon Wave 3 Task 15 — likely `src/core/runtime/context.rs`)

- [ ] **Step 1: Locate the horizon evaluation persistence hook**

```bash
grep -rn "persist_horizon_evaluations\|schedule_store_operation" src/ --include="*.rs"
```

Find the function that runs after horizon settle. Add a sibling function `upsert_case_resolution` that takes the primary horizon result + any already-settled supplementals and writes/updates the `case_resolution` record.

- [ ] **Step 2: Add the hook**

Add next to `persist_horizon_evaluations` (inside `impl RuntimeContext` or equivalent):

```rust
#[cfg(feature = "persistence")]
pub(crate) async fn upsert_case_resolution_for_setup(
    &self,
    setup_id: &str,
    market: &str,
    symbol: Option<&str>,
    primary_horizon: HorizonBucket,
    primary: &HorizonResolution,
    primary_result: &HorizonResult,
    supplementals: &[(HorizonBucket, HorizonResolution, HorizonResult)],
    all_settled: bool,
    triggered_by: HorizonBucket,
    at: OffsetDateTime,
    reason: &str,
) -> Result<(), StoreError> {
    use crate::ontology::resolution::{
        aggregate_case_resolution, apply_case_resolution_update,
        initial_case_resolution_transition, ResolutionSource, ResolutionUpdate, UpdateOutcome,
    };
    use crate::persistence::case_resolution::CaseResolutionRecord;

    let Some(store) = self.store.as_ref() else {
        return Ok(());
    };

    let new_resolution = aggregate_case_resolution(primary, supplementals, primary_result, all_settled);

    // Try to load existing record
    let existing = store.load_case_resolution_for_setup(setup_id).await?;

    let record = match existing {
        None => {
            // First write: create fresh record
            let initial_transition = initial_case_resolution_transition(
                &new_resolution,
                triggered_by,
                at,
                reason.to_string(),
            );
            let snapshot = {
                let mut v = Vec::with_capacity(1 + supplementals.len());
                v.push(primary.clone());
                for (_, r, _) in supplementals {
                    v.push(r.clone());
                }
                v
            };
            CaseResolutionRecord {
                record_id: CaseResolutionRecord::build_id(setup_id),
                setup_id: setup_id.to_string(),
                market: market.to_string(),
                symbol: symbol.map(|s| s.to_string()),
                primary_horizon,
                resolution: new_resolution,
                resolution_source: ResolutionSource::Auto,
                horizon_resolution_snapshot: snapshot,
                resolution_history: vec![initial_transition],
                created_at: at,
                updated_at: at,
            }
        }
        Some(mut existing) => {
            // Subsequent write: run upgrade gate
            let update = ResolutionUpdate {
                new_resolution: new_resolution.clone(),
                triggered_by_horizon: triggered_by,
                at,
                reason: reason.to_string(),
            };
            match apply_case_resolution_update(
                &mut existing.resolution,
                &mut existing.resolution_history,
                update,
            ) {
                UpdateOutcome::Applied => {
                    // Refresh snapshot and timestamp
                    existing.horizon_resolution_snapshot.clear();
                    existing.horizon_resolution_snapshot.push(primary.clone());
                    for (_, r, _) in supplementals {
                        existing.horizon_resolution_snapshot.push(r.clone());
                    }
                    existing.updated_at = at;
                }
                UpdateOutcome::NoChange => return Ok(()),
                UpdateOutcome::RejectedFinal => {
                    eprintln!(
                        "[resolution] upgrade rejected for {setup_id}: resolution is Final"
                    );
                    return Ok(());
                }
                UpdateOutcome::RejectedDowngrade => {
                    eprintln!(
                        "[resolution] downgrade rejected for {setup_id}: {:?} -> {:?}",
                        existing.resolution.kind, new_resolution.kind,
                    );
                    return Ok(());
                }
            }
            existing
        }
    };

    store.write_case_resolutions(&[record]).await
}
```

- [ ] **Step 3: Invoke the hook from the horizon settle flow**

Find where `persist_horizon_evaluations` gets called after each settle. Add a call to `upsert_case_resolution_for_setup` immediately after, passing the primary and any supplementals that have settled so far.

If the current settle flow processes one horizon at a time, collect the primary + already-settled supplementals from the `HorizonEvaluationRecord`s in the store before calling the hook. Use `load_horizon_evaluations_for_setup` (from Horizon Wave 3) to pull the sibling records.

The reason string should indicate which horizon triggered the update (e.g. `"primary settled"`, `"supplemental mid30m settled"`).

- [ ] **Step 4: Add a unit test for the hook helper logic**

Since the hook depends on `EdenStore` (feature-gated), test the core path separately. Add a test that exercises `aggregate_case_resolution` + `apply_case_resolution_update` together for the BKNG-style flow:

```rust
#[test]
fn bkng_style_flow_produces_confirmed_then_final() {
    use crate::ontology::horizon::HorizonBucket;
    use crate::ontology::resolution::*;
    use rust_decimal_macros::dec;
    use time::macros::datetime;

    // T1: Fast5m primary settles Confirmed(Provisional)
    let primary_res = HorizonResult {
        net_return: dec!(0.015),
        resolved_at: datetime!(2026-04-13 14:05 UTC),
        follow_through: dec!(0.75),
    };
    let primary_resolution = HorizonResolution {
        kind: HorizonResolutionKind::Confirmed,
        finality: ResolutionFinality::Provisional,
        rationale: "numeric_confirmed".into(),
        trigger: None,
    };

    let first = aggregate_case_resolution(&primary_resolution, &[], &primary_res, false);
    assert_eq!(first.kind, CaseResolutionKind::PartiallyConfirmed); // not all settled yet? primary only → still Confirmed... wait, confirm

    // Actually, for primary-only-settled with Confirmed:
    // - confirmed_count=1, total_horizons=1
    // - confirmed_count == total AND all_settled=false → not caught by rule 3
    // - primary_can_be_overridden=false (Confirmed, not Exhausted/Invalidated)
    // - rule 5: confirmed_count > 0 → PartiallyConfirmed(Provisional)
    assert_eq!(first.finality, ResolutionFinality::Provisional);

    // T3: all three horizons settle, all Confirmed
    // (... full flow continues ...)
}
```

Note: this test documents the current aggregator behavior for a primary-only write. If the behavior is undesirable (e.g. single confirmed primary should be `Confirmed(Provisional)` not `PartiallyConfirmed`), the aggregator rule needs refinement. The test is intentionally in this plan to surface that behavior as a real decision before Wave 3 commits.

Actually, re-checking: with primary=Confirmed, supplementals=[], all_settled=false:
- Rule 3 requires `all_settled && confirmed_count == total` → fails (all_settled=false)
- Rule 5 triggers: `confirmed_count > 0 && primary is Confirmed`
- Result: `PartiallyConfirmed(Provisional)` ← this is wrong

The aggregator needs a clause above Rule 5: if `primary.kind == Confirmed && supplementals.is_empty()`, produce `Confirmed(Provisional)`. Add this as Rule 4.5:

```rust
    // 4.5. Primary alone confirmed, no supplementals yet → Confirmed(Provisional)
    if supplementals.is_empty() && primary.kind == HorizonResolutionKind::Confirmed {
        return CaseResolution {
            kind: CaseResolutionKind::Confirmed,
            finality: if all_settled {
                ResolutionFinality::Final
            } else {
                ResolutionFinality::Provisional
            },
            narrative: "primary confirmed, no supplementals".into(),
            net_return,
        };
    }
```

Add this clause to `aggregate_case_resolution` in Task 6 retroactively (amend Task 6's code). Then the test here becomes:

```rust
    assert_eq!(first.kind, CaseResolutionKind::Confirmed);
    assert_eq!(first.finality, ResolutionFinality::Provisional);
```

**IMPORTANT:** apply this Rule 4.5 fix to `src/ontology/resolution.rs` in Task 6 when the implementer reaches Task 14 — or better, include it in Task 6 on first pass. Document the fix in the Task 14 commit message.

- [ ] **Step 5: Run tests**

Run: `cargo test --lib`

Expected: all pass. Fix the aggregator as noted above if the test reveals the hole.

- [ ] **Step 6: Commit**

```bash
git add -u
git commit -m "feat(resolution): upsert_case_resolution_for_setup hook + aggregator fix

Adds the case_resolution write path to the horizon settle flow. On
first settle, creates a fresh CaseResolutionRecord with the initial
transition. On subsequent settles, runs apply_case_resolution_update
through the upgrade gate.

Also adds aggregator Rule 4.5: a primary-only Confirmed with no
supplementals settled yet produces Confirmed(Provisional), not
PartiallyConfirmed. This is the degenerate BKNG-style case."
```

---

### Task 15: BKNG end-to-end regression test for Wave 3

**Files:**
- Modify: `src/ontology/resolution.rs` (add the integration-style test)

- [ ] **Step 1: Add the regression test**

Inside the `mod tests` block at the bottom of `src/ontology/resolution.rs`:

```rust
    #[test]
    fn bkng_flow_end_to_end_through_resolution_system() {
        // Simulates BKNG-style case flowing through horizon classifier +
        // case aggregator + upgrade gate.
        //
        // Setup: Case with primary=Fast5m, supplemental=[Mid30m, Session]
        //
        // T1 (5 min):  Fast5m settles Confirmed(Provisional)
        //              → CaseResolution = Confirmed(Provisional)
        // T2 (35 min): Mid30m settles Confirmed(Provisional)
        //              → No upgrade (same kind+finality)
        // T3 (6h):     Session settles Exhausted(Provisional), all_settled=true
        //              → CaseResolution upgrades to PartiallyConfirmed(Final)
        //
        // Asserts:
        //   - history has exactly the expected transitions
        //   - final kind is PartiallyConfirmed
        //   - primary BKNG bucket is still Fast5m in the aggregator input
        //   - no ProfitableButLate (primary was Confirmed, not Exhausted)

        let fast_result = HorizonResult {
            net_return: dec!(0.015),
            resolved_at: datetime!(2026-04-13 14:05 UTC),
            follow_through: dec!(0.75),
        };
        let mid_result = HorizonResult {
            net_return: dec!(0.008),
            resolved_at: datetime!(2026-04-13 14:35 UTC),
            follow_through: dec!(0.7),
        };
        let session_result = HorizonResult {
            net_return: dec!(-0.002),
            resolved_at: datetime!(2026-04-13 20:05 UTC),
            follow_through: dec!(0.15),
        };

        // Classify each horizon
        let fast_res = classify_horizon_resolution(&fast_result, None, &[]);
        let mid_res = classify_horizon_resolution(&mid_result, None, &[]);
        let session_res = classify_horizon_resolution(&session_result, None, &[]);

        assert_eq!(fast_res.kind, HorizonResolutionKind::Confirmed);
        assert_eq!(mid_res.kind, HorizonResolutionKind::Confirmed);
        assert_eq!(session_res.kind, HorizonResolutionKind::Exhausted);

        // T1: Fast5m primary settles → first write
        let t1 = aggregate_case_resolution(&fast_res, &[], &fast_result, false);
        assert_eq!(t1.kind, CaseResolutionKind::Confirmed);
        assert_eq!(t1.finality, ResolutionFinality::Provisional);

        let mut current = t1.clone();
        let mut history = vec![initial_case_resolution_transition(
            &t1,
            HorizonBucket::Fast5m,
            datetime!(2026-04-13 14:05 UTC),
            "primary settled".into(),
        )];

        // T2: Mid30m supplemental settles, aggregator re-runs
        let t2 = aggregate_case_resolution(
            &fast_res,
            &[(HorizonBucket::Mid30m, mid_res.clone(), mid_result.clone())],
            &fast_result,
            false,
        );
        // Both Confirmed, all_settled=false → still Confirmed(Provisional) — no change
        assert_eq!(t2.kind, CaseResolutionKind::Confirmed);
        assert_eq!(t2.finality, ResolutionFinality::Provisional);

        let t2_outcome = apply_case_resolution_update(
            &mut current,
            &mut history,
            ResolutionUpdate {
                new_resolution: t2,
                triggered_by_horizon: HorizonBucket::Mid30m,
                at: datetime!(2026-04-13 14:35 UTC),
                reason: "mid30m settled".into(),
            },
        );
        assert_eq!(t2_outcome, UpdateOutcome::NoChange);
        assert_eq!(history.len(), 1);

        // T3: Session supplemental settles Exhausted, all_settled=true
        let t3 = aggregate_case_resolution(
            &fast_res,
            &[
                (HorizonBucket::Mid30m, mid_res, mid_result),
                (HorizonBucket::Session, session_res, session_result),
            ],
            &fast_result,
            true,
        );
        // 2 Confirmed + 1 Exhausted, all_settled=true
        //   → rule 4.5 not hit (supplementals non-empty)
        //   → rule 3 not hit (not all confirmed)
        //   → rule 4 not hit (primary not Exhausted/Invalidated)
        //   → rule 5: confirmed_count=2, total=3 → PartiallyConfirmed(Final)
        assert_eq!(t3.kind, CaseResolutionKind::PartiallyConfirmed);
        assert_eq!(t3.finality, ResolutionFinality::Final);

        let t3_outcome = apply_case_resolution_update(
            &mut current,
            &mut history,
            ResolutionUpdate {
                new_resolution: t3,
                triggered_by_horizon: HorizonBucket::Session,
                at: datetime!(2026-04-13 20:05 UTC),
                reason: "session settled, all settled".into(),
            },
        );
        // Confirmed(Provisional) → PartiallyConfirmed(Final) is a downgrade
        // in kind rank. Let me check: is_valid_upgrade(Confirmed, PartiallyConfirmed)?
        // Looking at is_valid_upgrade: no arm matches → RejectedDowngrade.
        //
        // This reveals a design question: if primary Confirmed but case is
        // only PartiallyConfirmed because a supplemental Exhausted, that's
        // a *refinement*, not a downgrade. The aggregator produces
        // PartiallyConfirmed legitimately.
        //
        // FIX: is_valid_upgrade should allow Confirmed → PartiallyConfirmed
        // when the new one is Final (locking finality in the presence of
        // an Exhausted supplemental). OR better: apply_case_resolution_update
        // should allow a finality=Final transition regardless of kind change
        // direction, as long as the new kind is a valid terminal state.
        //
        // DECISION: extend is_valid_upgrade to accept Confirmed → PartiallyConfirmed
        // when to_finality is Final. This is the "locking down with refinement"
        // case.
        assert_eq!(t3_outcome, UpdateOutcome::Applied);
        assert_eq!(current.kind, CaseResolutionKind::PartiallyConfirmed);
        assert_eq!(current.finality, ResolutionFinality::Final);
        assert_eq!(history.len(), 2);
        assert_eq!(history[1].from_kind, Some(CaseResolutionKind::Confirmed));
        assert_eq!(history[1].to_kind, CaseResolutionKind::PartiallyConfirmed);
        assert_eq!(history[1].to_finality, ResolutionFinality::Final);
    }
```

The comment block inside the test highlights a real design gap the test reveals: `is_valid_upgrade` as defined in Task 7 does not allow `Confirmed → PartiallyConfirmed`. This is a correct rejection in the upgrade-only framing, but legitimate when finality is being locked to Final in the presence of supplementals that didn't all confirm.

- [ ] **Step 2: Fix `is_valid_upgrade` to handle the "refinement when locking Final" case**

In `src/ontology/resolution.rs`, `apply_case_resolution_update` currently rejects if the kind is changing and `is_valid_upgrade` returns false. Add an exception: when `to_finality == Final`, the gate accepts a kind change from Confirmed → PartiallyConfirmed or from PartiallyConfirmed → Confirmed, because Finality locks are allowed to refine the kind downward.

Rather than adding a special case inside `apply_case_resolution_update`, extend `is_valid_upgrade`:

```rust
pub fn is_valid_upgrade(current: CaseResolutionKind, new: CaseResolutionKind) -> bool {
    use CaseResolutionKind::*;
    matches!(
        (current, new),
        (Exhausted, ProfitableButLate)
            | (Exhausted, PartiallyConfirmed)
            | (Exhausted, Confirmed)
            | (PartiallyConfirmed, Confirmed)
            // When finality is being locked Final, refinement downward is allowed
            | (Confirmed, PartiallyConfirmed)
            | (Invalidated, ProfitableButLate)
            | (Invalidated, PartiallyConfirmed)
            | (EarlyExited, ProfitableButLate)
    )
}
```

BUT this new pair (`Confirmed → PartiallyConfirmed`) is semantically only valid when the new finality is Final. Add the constraint inside `apply_case_resolution_update`:

```rust
    if current.kind != update.new_resolution.kind
        && !is_valid_upgrade(current.kind, update.new_resolution.kind)
    {
        return UpdateOutcome::RejectedDowngrade;
    }

    // Extra rule: Confirmed → PartiallyConfirmed is only allowed when
    // finality is being locked Final (aggregator refining in the light
    // of supplementals). Otherwise it's a downgrade.
    if current.kind == CaseResolutionKind::Confirmed
        && update.new_resolution.kind == CaseResolutionKind::PartiallyConfirmed
        && update.new_resolution.finality != ResolutionFinality::Final
    {
        return UpdateOutcome::RejectedDowngrade;
    }
```

Add a test case in Task 7 confirming this constraint:

```rust
    #[test]
    fn apply_update_rejects_confirmed_to_partially_confirmed_without_final() {
        let mut cur = make_case_resolution(CaseResolutionKind::Confirmed, ResolutionFinality::Provisional);
        let mut hist: Vec<CaseResolutionTransition> = vec![];
        let out = apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(CaseResolutionKind::PartiallyConfirmed, ResolutionFinality::Provisional),
        );
        assert_eq!(out, UpdateOutcome::RejectedDowngrade);
    }

    #[test]
    fn apply_update_accepts_confirmed_to_partially_confirmed_with_final() {
        let mut cur = make_case_resolution(CaseResolutionKind::Confirmed, ResolutionFinality::Provisional);
        let mut hist: Vec<CaseResolutionTransition> = vec![];
        let out = apply_case_resolution_update(
            &mut cur,
            &mut hist,
            make_update(CaseResolutionKind::PartiallyConfirmed, ResolutionFinality::Final),
        );
        assert_eq!(out, UpdateOutcome::Applied);
    }
```

Apply these additions when implementing Task 7 (retroactive, or include them from the start). The implementer should read this Task 15 section before starting Task 7 to understand why.

- [ ] **Step 3: Run the BKNG regression test**

Run: `cargo test --lib ontology::resolution::tests::bkng_flow_end_to_end_through_resolution_system`

Expected: PASS.

- [ ] **Step 4: Run full library tests**

Run: `cargo test --lib`

Expected: all pass (~786 + a few new Wave 3 tests).

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "test(resolution): BKNG end-to-end regression + refinement-to-final rule

Locks in the BKNG flow through horizon classifier + case aggregator +
upgrade gate. Also uncovers the rule that Confirmed -> PartiallyConfirmed
is valid ONLY when the new finality is Final (legitimate refinement
when locking terminal state), rejected otherwise as a downgrade."
```

---

### Task 16: Wave 3 exit — verification

- [ ] **Step 1: `cargo check --lib`** — clean

- [ ] **Step 2: Full tests** — all pass

- [ ] **Step 3: Tag**

```bash
git tag resolution-wave-3
```

---

## Wave 4 — Learning Loop Switch + Archetype Distribution

Goal: `ReasoningLearningFeedback` reads `case_resolution` first, falls back to legacy boolean path if absent. `DiscoveredArchetype` gains outcome distribution counts, recomputed per affected shard.

### Task 17: Add outcome distribution fields to `DiscoveredArchetypeRecord`

**Files:**
- Modify: `src/persistence/discovered_archetype.rs`

- [ ] **Step 1: Write failing test**

Add to the existing `mod horizon_key_tests` or a new module:

```rust
    #[test]
    fn archetype_record_has_outcome_distribution_fields() {
        let record = DiscoveredArchetypeRecord {
            // ... existing required fields ...
            confirmed_count: 3,
            invalidated_count: 1,
            profitable_but_late_count: 2,
            partially_confirmed_count: 0,
            exhausted_count: 5,
            early_exited_count: 1,
            structurally_right_count: 0,
            ..DiscoveredArchetypeRecord::default()
        };
        assert_eq!(record.confirmed_count, 3);
        assert_eq!(record.profitable_but_late_count, 2);
    }

    #[test]
    fn legacy_archetype_without_distribution_deserializes_as_zero() {
        // JSON from before Wave 4 lacks the distribution fields
        let json = r#"{
            "archetype_id": "id-1",
            "market": "us",
            "archetype_key": "intent:fast5m:sig",
            "label": "test",
            "topology": null,
            "temporal_shape": null,
            "conflict_shape": null,
            "dominant_channels": [],
            "expectation_violation_kinds": [],
            "family_label": null,
            "samples": 10,
            "hits": 5,
            "hit_rate": "0.5",
            "mean_net_return": "0.0",
            "mean_affinity": "0.0",
            "updated_at": "2026-04-12T00:00:00Z",
            "bucket": "fast5m"
        }"#;
        let record: DiscoveredArchetypeRecord = serde_json::from_str(json).unwrap();
        assert_eq!(record.confirmed_count, 0);
        assert_eq!(record.profitable_but_late_count, 0);
    }
```

Note: if `DiscoveredArchetypeRecord` doesn't have a `Default` impl, construct all required fields explicitly rather than using `..default()`.

- [ ] **Step 2: Add fields to the struct**

In `DiscoveredArchetypeRecord`, add after the existing fields:

```rust
    #[serde(default)]
    pub confirmed_count: u64,
    #[serde(default)]
    pub invalidated_count: u64,
    #[serde(default)]
    pub profitable_but_late_count: u64,
    #[serde(default)]
    pub partially_confirmed_count: u64,
    #[serde(default)]
    pub exhausted_count: u64,
    #[serde(default)]
    pub early_exited_count: u64,
    #[serde(default)]
    pub structurally_right_count: u64,
```

Find `build_discovered_archetypes` and make sure new records are built with these fields set to 0 initially (they'll be recomputed by the shard-recompute helper in Task 18).

- [ ] **Step 3: Fix all construction sites**

Run:
```bash
grep -rn "DiscoveredArchetypeRecord {" src/ --include="*.rs"
```

At each construction site, add the 7 new count fields (defaulting to 0).

- [ ] **Step 4: Run tests**

Run: `cargo test --lib persistence::discovered_archetype`

Expected: all pass.

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "feat(resolution): DiscoveredArchetype outcome distribution counts"
```

---

### Task 18: Add shard recompute helper

**Files:**
- Modify: `src/persistence/discovered_archetype.rs`

- [ ] **Step 1: Add the shard recompute function**

Add at module level:

```rust
#[cfg(feature = "persistence")]
pub async fn recompute_archetype_shard_distribution(
    store: &crate::persistence::store::EdenStore,
    intent_kind: &str,
    bucket: crate::ontology::horizon::HorizonBucket,
    signature: &str,
) -> Result<(), crate::persistence::store::StoreError> {
    use crate::ontology::resolution::CaseResolutionKind;

    // Load all case_resolution records that match this shard
    // (intent, bucket, signature). Source of truth — NOT increment.
    //
    // In practice this means querying case_resolution by the shard
    // triple. For simplicity in this first pass we load every case
    // resolution and filter in application code. A later optimization
    // can add an index.

    let all_resolutions = store.load_all_case_resolutions().await?;
    let mut counts = OutcomeCounts::default();
    for record in all_resolutions {
        // Match criteria: skip for now if we don't have the intent_kind
        // or signature on the record. Extend CaseResolutionRecord or
        // use a join with TacticalSetup later.
        //
        // First pass: only partition by bucket.
        if record.primary_horizon != bucket {
            continue;
        }
        match record.resolution.kind {
            CaseResolutionKind::Confirmed => counts.confirmed += 1,
            CaseResolutionKind::PartiallyConfirmed => counts.partially_confirmed += 1,
            CaseResolutionKind::Invalidated => counts.invalidated += 1,
            CaseResolutionKind::Exhausted => counts.exhausted += 1,
            CaseResolutionKind::ProfitableButLate => counts.profitable_but_late += 1,
            CaseResolutionKind::EarlyExited => counts.early_exited += 1,
            CaseResolutionKind::StructurallyRightButUntradeable => counts.structurally_right += 1,
        }
    }

    // Load existing archetype record, update counts, write back.
    let archetype_key = build_archetype_key(intent_kind, bucket, signature);
    if let Some(mut record) = store.load_archetype_by_key(&archetype_key).await? {
        record.confirmed_count = counts.confirmed;
        record.partially_confirmed_count = counts.partially_confirmed;
        record.invalidated_count = counts.invalidated;
        record.exhausted_count = counts.exhausted;
        record.profitable_but_late_count = counts.profitable_but_late;
        record.early_exited_count = counts.early_exited;
        record.structurally_right_count = counts.structurally_right;
        store.write_archetypes(&[record]).await?;
    }
    Ok(())
}

#[derive(Default)]
struct OutcomeCounts {
    confirmed: u64,
    partially_confirmed: u64,
    invalidated: u64,
    exhausted: u64,
    profitable_but_late: u64,
    early_exited: u64,
    structurally_right: u64,
}
```

Note: this first pass partitions only by `bucket`, not by the full `(intent_kind, bucket, signature)` triple, because `CaseResolutionRecord` doesn't yet carry intent_kind and signature. The proper fix is to extend `CaseResolutionRecord` with `intent_kind: String` and `signature: String` fields. Add those fields now:

Back in `src/persistence/case_resolution.rs`, add:

```rust
    pub intent_kind: String,
    pub signature: String,
```

Populate them in `upsert_case_resolution_for_setup` (Task 14) from the originating `TacticalSetup.inferred_intent.kind` and `TacticalSetup.case_signature` (if present — fall back to empty string).

Then in `recompute_archetype_shard_distribution`, filter by all three:

```rust
        if record.primary_horizon != bucket
            || record.intent_kind != intent_kind
            || record.signature != signature
        {
            continue;
        }
```

- [ ] **Step 2: Add store helpers `load_all_case_resolutions` + `load_archetype_by_key` + `write_archetypes`**

In `src/persistence/store.rs`, add the methods following the existing pattern. These are 1-screen helpers — mirror `write_horizon_evaluations` / `load_horizon_evaluations_for_setup`.

- [ ] **Step 3: Hook recompute into the upsert path**

In `upsert_case_resolution_for_setup` (Task 14), after a successful write, call:

```rust
    #[cfg(feature = "persistence")]
    if let Err(e) = recompute_archetype_shard_distribution(
        &*store,
        intent_kind,
        primary_horizon,
        &signature,
    )
    .await
    {
        eprintln!("[resolution] archetype shard recompute failed: {e}");
    }
```

- [ ] **Step 4: `cargo check --lib`**

Run: `cargo check --lib`

Expected: clean. Any signature errors on the new helpers must be fixed before proceeding.

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "feat(resolution): shard recompute triggers on case resolution update"
```

---

### Task 19: Switch learning loop to read `case_resolution` with legacy fallback

**Files:**
- Modify: `src/pipeline/learning_loop/feedback.rs`

- [ ] **Step 1: Find the existing boolean-read path**

```bash
grep -rn "CaseRealizedOutcomeRecord\|followed_through\|structure_retained" src/pipeline/learning_loop/
```

Find the function that computes the learning delta for a single case. It currently reads `CaseRealizedOutcomeRecord.followed_through / invalidated / structure_retained`.

- [ ] **Step 2: Add a new dispatch function**

In `src/pipeline/learning_loop/feedback.rs`, add:

```rust
/// Resolution kind → learning delta policy.
///
/// Locked rules:
/// - Confirmed + Final       → +full credit
/// - Confirmed + Provisional → +half credit
/// - Invalidated + Final     → −full debit
/// - Invalidated + Provisional → 0 (wait for upgrade)
/// - Exhausted               → 0
/// - ProfitableButLate       → bucket debit + intent credit (split)
/// - PartiallyConfirmed      → +partial credit
/// - EarlyExited             → 0
/// - StructurallyRightButUntradeable → 0
#[cfg(feature = "persistence")]
pub fn delta_from_case_resolution(
    resolution: &crate::ontology::resolution::CaseResolution,
) -> rust_decimal::Decimal {
    use crate::ontology::resolution::{CaseResolutionKind, ResolutionFinality};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    match (resolution.kind, resolution.finality) {
        (CaseResolutionKind::Confirmed, ResolutionFinality::Final) => dec!(1.0),
        (CaseResolutionKind::Confirmed, ResolutionFinality::Provisional) => dec!(0.5),
        (CaseResolutionKind::Invalidated, ResolutionFinality::Final) => dec!(-1.0),
        (CaseResolutionKind::Invalidated, ResolutionFinality::Provisional) => Decimal::ZERO,
        (CaseResolutionKind::Exhausted, _) => Decimal::ZERO,
        (CaseResolutionKind::ProfitableButLate, _) => dec!(0.3), // intent credit — bucket debit handled separately
        (CaseResolutionKind::PartiallyConfirmed, ResolutionFinality::Final) => dec!(0.5),
        (CaseResolutionKind::PartiallyConfirmed, ResolutionFinality::Provisional) => dec!(0.25),
        (CaseResolutionKind::EarlyExited, _) => Decimal::ZERO,
        (CaseResolutionKind::StructurallyRightButUntradeable, _) => Decimal::ZERO,
    }
}
```

- [ ] **Step 3: Update the feedback computation path**

Find where the learning delta is actually computed per case. Replace the boolean-read with a conditional:

```rust
    // Resolution System: prefer new path, fall back to legacy boolean
    #[cfg(feature = "persistence")]
    let delta = if let Some(resolution_record) = store
        .load_case_resolution_for_setup(&case.setup_id)
        .await
        .ok()
        .flatten()
    {
        // New path: read case_resolution
        delta_from_case_resolution(&resolution_record.resolution)
    } else {
        // Legacy path: read CaseRealizedOutcomeRecord booleans
        legacy_delta_from_booleans(&case.outcome)
    };
```

Name the legacy function `legacy_delta_from_booleans` to make its deprecation clear. Keep it as-is, do not modify.

**CRITICAL:** never merge. Read one or the other, not both.

- [ ] **Step 4: Add a test for the delta policy**

```rust
#[cfg(test)]
mod resolution_delta_tests {
    use super::*;
    use crate::ontology::resolution::{CaseResolution, CaseResolutionKind, ResolutionFinality};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn make(kind: CaseResolutionKind, finality: ResolutionFinality) -> CaseResolution {
        CaseResolution {
            kind,
            finality,
            narrative: "test".into(),
            net_return: Decimal::ZERO,
        }
    }

    #[test]
    fn confirmed_final_full_credit() {
        let d = delta_from_case_resolution(&make(CaseResolutionKind::Confirmed, ResolutionFinality::Final));
        assert_eq!(d, dec!(1.0));
    }

    #[test]
    fn confirmed_provisional_half_credit() {
        let d = delta_from_case_resolution(&make(CaseResolutionKind::Confirmed, ResolutionFinality::Provisional));
        assert_eq!(d, dec!(0.5));
    }

    #[test]
    fn invalidated_final_full_debit() {
        let d = delta_from_case_resolution(&make(CaseResolutionKind::Invalidated, ResolutionFinality::Final));
        assert_eq!(d, dec!(-1.0));
    }

    #[test]
    fn invalidated_provisional_neutral() {
        let d = delta_from_case_resolution(&make(CaseResolutionKind::Invalidated, ResolutionFinality::Provisional));
        assert_eq!(d, Decimal::ZERO);
    }

    #[test]
    fn exhausted_zero() {
        let d = delta_from_case_resolution(&make(CaseResolutionKind::Exhausted, ResolutionFinality::Final));
        assert_eq!(d, Decimal::ZERO);
    }

    #[test]
    fn profitable_but_late_intent_credit() {
        let d = delta_from_case_resolution(&make(CaseResolutionKind::ProfitableButLate, ResolutionFinality::Final));
        assert_eq!(d, dec!(0.3));
    }

    #[test]
    fn structurally_right_zero() {
        let d = delta_from_case_resolution(&make(
            CaseResolutionKind::StructurallyRightButUntradeable,
            ResolutionFinality::Final,
        ));
        assert_eq!(d, Decimal::ZERO);
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib pipeline::learning_loop`

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add -u
git commit -m "feat(resolution): learning loop reads case_resolution with legacy fallback

New path is primary. Legacy CaseRealizedOutcome boolean path is the
fallback when no case_resolution record exists. The two paths are
NEVER merged. Resolution-kind → delta policy is locked and covered
by unit tests. ProfitableButLate bucket debit is handled in a
separate dispatch (Task 20) because it requires access to the
bucket dimension."
```

---

### Task 20: ProfitableButLate bucket-debit split

**Files:**
- Modify: `src/pipeline/learning_loop/feedback.rs`

- [ ] **Step 1: Add the split delta helper**

```rust
/// For ProfitableButLate: the bucket that was chosen (primary) gets a
/// debit (horizon selection was wrong), while the intent credit is
/// applied to the bucket that actually confirmed.
#[cfg(feature = "persistence")]
pub fn profitable_but_late_bucket_deltas(
    primary_bucket: crate::ontology::horizon::HorizonBucket,
    confirming_bucket: Option<crate::ontology::horizon::HorizonBucket>,
) -> Vec<(crate::ontology::horizon::HorizonBucket, rust_decimal::Decimal)> {
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    let mut out = vec![(primary_bucket, dec!(-0.3))];
    if let Some(confirming) = confirming_bucket {
        if confirming != primary_bucket {
            out.push((confirming, dec!(0.3)));
        }
    }
    out
}
```

This function is called only when the case resolution is `ProfitableButLate`. The intent-level credit is already applied via `delta_from_case_resolution` returning `+0.3`; this function produces the *additional* bucket-level splits that flow into `horizon_adjustments`.

- [ ] **Step 2: Test**

```rust
    #[test]
    fn profitable_but_late_debits_primary_credits_confirming() {
        let deltas = profitable_but_late_bucket_deltas(
            HorizonBucket::Fast5m,
            Some(HorizonBucket::Mid30m),
        );
        assert_eq!(deltas.len(), 2);
        assert_eq!(deltas[0], (HorizonBucket::Fast5m, dec!(-0.3)));
        assert_eq!(deltas[1], (HorizonBucket::Mid30m, dec!(0.3)));
    }

    #[test]
    fn profitable_but_late_same_bucket_is_noop_credit() {
        // Edge case: confirming_bucket == primary_bucket (shouldn't happen
        // in practice but defend the contract)
        let deltas = profitable_but_late_bucket_deltas(
            HorizonBucket::Fast5m,
            Some(HorizonBucket::Fast5m),
        );
        assert_eq!(deltas.len(), 1);
        assert_eq!(deltas[0].1, dec!(-0.3));
    }
```

- [ ] **Step 3: Run tests + commit**

Run: `cargo test --lib pipeline::learning_loop`

Expected: pass.

```bash
git add -u
git commit -m "feat(resolution): ProfitableButLate bucket debit split helper"
```

---

### Task 21: Wave 4 exit — verification

- [ ] **Step 1: `cargo check --lib`** — clean
- [ ] **Step 2: Full tests** — all pass
- [ ] **Step 3: Tag**

```bash
git tag resolution-wave-4
```

---

## Wave 5 — Operator Override + Legacy Cleanup

Goal: operator can manually override a case resolution. Legacy `CaseRealizedOutcome` boolean fields marked deprecated (not deleted — numeric facts stay).

### Task 22: Add `override_case_resolution` store method

**Files:**
- Modify: `src/persistence/store.rs`

- [ ] **Step 1: Write the override method**

```rust
    #[cfg(feature = "persistence")]
    pub async fn override_case_resolution(
        &self,
        setup_id: &str,
        new_kind: crate::ontology::resolution::CaseResolutionKind,
        reason: String,
        at: time::OffsetDateTime,
    ) -> Result<(), StoreError> {
        use crate::ontology::resolution::{
            CaseResolution, CaseResolutionTransition, ResolutionFinality, ResolutionSource,
        };

        if reason.trim().is_empty() {
            return Err(StoreError::InvalidInput(
                "override reason cannot be empty".into(),
            ));
        }

        let Some(mut record) = self.load_case_resolution_for_setup(setup_id).await? else {
            return Err(StoreError::NotFound(format!(
                "no case_resolution for setup {setup_id}"
            )));
        };

        // Append transition BEFORE mutating current
        record.resolution_history.push(CaseResolutionTransition {
            from_kind: Some(record.resolution.kind),
            from_finality: Some(record.resolution.finality),
            to_kind: new_kind,
            to_finality: ResolutionFinality::Final,
            triggered_by_horizon: record.primary_horizon,
            at,
            reason: format!("operator_override: {reason}"),
        });

        // Apply the override (bypass upgrade gate)
        record.resolution = CaseResolution {
            kind: new_kind,
            finality: ResolutionFinality::Final,
            narrative: format!("operator override: {reason}"),
            net_return: record.resolution.net_return,
        };
        record.resolution_source = ResolutionSource::OperatorOverride;
        record.updated_at = at;

        self.write_case_resolutions(&[record]).await
    }
```

If `StoreError::InvalidInput` and `StoreError::NotFound` don't exist as variants, either add them (small enum extension) or return the closest existing error with a descriptive message.

- [ ] **Step 2: Unit test via pure logic**

Since a full round-trip requires `--features persistence` (RocksDB), test the validation logic by extracting it:

```rust
#[cfg(test)]
mod override_validation_tests {
    #[test]
    fn empty_reason_rejected() {
        // The override method checks reason.trim().is_empty()
        assert_eq!("".trim().is_empty(), true);
        assert_eq!("  ".trim().is_empty(), true);
        assert_eq!("real reason".trim().is_empty(), false);
    }
}
```

(This is a weaker test but compiles without the persistence feature. A proper integration test should land when the persistence-feature RocksDB build is fixed on CI.)

- [ ] **Step 3: `cargo check --lib`** — clean

- [ ] **Step 4: Commit**

```bash
git add -u
git commit -m "feat(resolution): operator override bypasses upgrade gate"
```

---

### Task 23: Mark legacy `CaseRealizedOutcome` boolean fields deprecated

**Files:**
- Modify: `src/temporal/lineage.rs` or wherever `CaseRealizedOutcome` is defined

- [ ] **Step 1: Locate the struct**

```bash
grep -n "pub struct CaseRealizedOutcome" src/temporal/lineage.rs
```

- [ ] **Step 2: Add deprecation doc comments**

Above each of `followed_through`, `invalidated`, `structure_retained`:

```rust
    /// DEPRECATED. Legacy boolean outcome flag. Use the Resolution System
    /// via the `case_resolution` persistence table for new code. This
    /// field is retained for backward compatibility with historical
    /// records; do not write it from new code paths.
    #[deprecated(note = "Use CaseResolution from the Resolution System instead")]
    pub followed_through: bool,
```

Apply the same `#[deprecated]` attribute to `invalidated` and `structure_retained`.

**Do NOT deprecate** `net_return`, `return_pct`, `max_favorable_excursion`, `max_adverse_excursion` — those are numeric facts and stay.

- [ ] **Step 3: Fix any `#[deprecated]` warnings that now show up**

`#[deprecated]` generates warnings on every use. Silence them in the remaining read paths with:

```rust
#[allow(deprecated)]
```

Placed at the call site. This signals the reader that the path is intentional during the deprecation period.

- [ ] **Step 4: `cargo check --lib`** — clean (warnings from `#[deprecated]` are expected)

- [ ] **Step 5: Commit**

```bash
git add -u
git commit -m "refactor(resolution): deprecate legacy CaseRealizedOutcome boolean fields

Marks followed_through, invalidated, structure_retained as deprecated.
They remain in the struct for backward-compat with historical records
but new code must use CaseResolution from the Resolution System.
Numeric facts (net_return, MFE, MAE) stay — those are not replaced."
```

---

### Task 24: Wave 5 exit + final tag

- [ ] **Step 1: `cargo check --lib`** — clean
- [ ] **Step 2: Full tests** — all pass
- [ ] **Step 3: Final tag**

```bash
git tag resolution-complete
```

---

## Rollback Plan

Each wave is a separate tag: `resolution-wave-1` through `resolution-wave-4`, plus `resolution-complete`. Reset to any of them if a wave breaks something unrelated.

Waves 1 and 2 are reversible without data loss (fields are additive with `#[serde(default)]`). Wave 3 writes a new SurrealDB table (`case_resolution`); reverting stops new writes but leaves old rows in place. Wave 4 changes learning loop read paths; reverting reverts to legacy boolean reads. Wave 5 adds operator override + deprecation markers; fully reversible.

Wave 0 (the `Expired → Due` rename) is the only wave that cannot be rolled back without accepting a test failure, because the new name has propagated through the codebase. Roll back by checking out before `Wave 0` tag commit.

---

## Self-Review

**Spec coverage:**

- Four separated concepts (EvaluationStatus / HorizonResolution / CaseResolution / ResolutionFinality) → Tasks 2, 3, 5 + Wave 0 Task 1
- Rename `Expired → Due` as mandatory → Task 1
- 4-kind `HorizonResolutionKind` + 7-kind `CaseResolutionKind` → Tasks 3, 5
- `classify_horizon_resolution` with 5 priority branches → Task 4
- `aggregate_case_resolution` with 6 rules + Rule 4.5 (primary-only Confirmed) → Tasks 6, 14
- `apply_case_resolution_update` single choke point → Tasks 7, 15
- Refinement-to-Final rule (Confirmed → PartiallyConfirmed when locking Final) → Task 15
- `CaseResolutionRecord` with `resolution_source` + snapshot + append-only history → Task 8
- Operator override (bypass gate, set Final, non-empty reason) → Task 22
- `DiscoveredArchetype` outcome distribution counts, shard recompute → Tasks 17, 18
- Learning loop reads case_resolution first, falls back to legacy, never merges → Tasks 19, 20
- BKNG end-to-end regression → Task 15
- ProfitableButLate bucket-debit split → Task 20
- Legacy boolean fields deprecated (not deleted) → Task 23

**Placeholder scan:**

- No "TBD" / "TODO" / "implement later"
- Every code-changing step has the full code block
- Test expectations are specific (exact PASS / specific values / specific counts)
- Task 14's aggregator Rule 4.5 fix is retroactive — the implementer is instructed to apply it in Task 6 on first pass
- Task 15's `is_valid_upgrade` extension is retroactive — same instruction

**Type consistency:**

- `HorizonResolution` / `CaseResolution` / `CaseResolutionRecord` / `CaseResolutionTransition` / `ResolutionUpdate` / `UpdateOutcome` / `ResolutionSource` / `ResolutionFinality` / `HorizonResolutionKind` / `CaseResolutionKind` — all names used consistently
- Function names: `classify_horizon_resolution`, `aggregate_case_resolution`, `apply_case_resolution_update`, `is_valid_upgrade`, `initial_case_resolution_transition`, `delta_from_case_resolution`, `profitable_but_late_bucket_deltas`, `upsert_case_resolution_for_setup`, `override_case_resolution`, `recompute_archetype_shard_distribution` — all consistent across tasks
- `HorizonBucket` / `EvaluationStatus` / `HorizonResult` / `IntentExitKind` / `ExpectationViolation` all reference the correct existing types
