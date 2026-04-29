# Horizon System Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Collapse Eden's scattered time language into a single Horizon System centered on `HorizonBucket` enum, resolving BKNG-style exit-timing failures by giving every case a named rhythm with a clear expiry.

**Architecture:** Four waves. Wave 1 adds types with zero behavior change. Wave 2 wires types into Intent/Case producers. Wave 3 adds persistence and outcome/memory learning. Wave 4 removes legacy `time_horizon: String`. Throughout, `HorizonBucket` is the shared trading-time enum; `CaseHorizon` has exactly one primary; supplemental learning is gated at 50/100 samples.

**Tech Stack:** Rust, `serde`, `rust_decimal`, existing SurrealDB persistence via `EdenStore`, existing `IntentHypothesis` / `TacticalSetup` / `DiscoveredArchetype` / `ReasoningLearningFeedback` types in `src/ontology/` and `src/persistence/`.

**Spec:** `docs/superpowers/specs/2026-04-12-horizon-system-design.md`

---

## File Structure

### New files (Wave 1)

- `src/ontology/horizon.rs` (~350 lines) — all horizon types, urgency computation, legacy deserialization, `SessionPhaseResolver` trait + `TimestampSessionResolver`. Single file per spec Wave 1 constraint.
- `src/persistence/horizon_evaluation.rs` (~200 lines) — `HorizonEvaluationRecord`, `EvaluationStatus`, persistence helpers.

### Files modified (Wave 2)

- `src/ontology/mod.rs` — register `pub mod horizon;`
- `src/ontology/reasoning.rs` — `IntentOpportunityWindow` gains `bucket`/`urgency` fields, `TacticalSetup` gains `horizon: CaseHorizon` field, legacy `time_horizon` marked as derived
- `src/pipeline/pressure/bridge.rs` — `insight_to_tactical_setup` populates `CaseHorizon` via selection rule
- `src/live_snapshot.rs` — `LiveTacticalCase` surfaces bucket/urgency strings

### Files modified (Wave 3)

- `src/persistence/mod.rs` — register `pub mod horizon_evaluation;`
- `src/persistence/store.rs` — expose read/write for `HorizonEvaluationRecord`
- `src/persistence/discovered_archetype.rs` — `archetype_key` includes bucket
- `src/pipeline/learning_loop/types.rs` — `ReasoningLearningFeedback` gains `horizon_adjustments`
- `src/pipeline/learning_loop/feedback.rs` — gate logic at 50/100
- `src/temporal/lineage.rs` — `aggregate_outcomes_by_family` signature uses `HorizonBucket`

### Files modified (Wave 4)

- `src/ontology/reasoning.rs` — remove `TacticalSetup.time_horizon: String`
- All call sites that referenced `time_horizon` — migrate to `CaseHorizon`
- `src/temporal/lineage/outcomes/evaluation.rs` — string session phases → `SessionPhase` enum
- `src/persistence/tactical_setup.rs` — persistence schema update

---

## Wave 1 — Types Only, Zero Behavior Change

Goal: establish the type foundation in a single self-contained commit. No existing code reads or writes these types yet.

### Task 1: Create `horizon.rs` with core enums

**Files:**
- Create: `src/ontology/horizon.rs`
- Modify: `src/ontology/mod.rs`

- [ ] **Step 1: Write the failing test for enum serialization**

Create `src/ontology/horizon.rs`:

```rust
//! Horizon System — unified trading-time language.
//!
//! Four independent time concepts are kept strictly separate:
//! - TimeScale (in pipeline/pressure.rs) = compute time, not in this module
//! - HorizonBucket = trading time (this module's main enum)
//! - SessionPhase = market context
//! - Urgency = action timing
//!
//! See docs/superpowers/specs/2026-04-12-horizon-system-design.md.

use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

/// Trading-time language. These are trading opportunity categories,
/// not minute counts. A "Fast5m" bucket means "short-term opportunity
/// where decisions live at the seconds-to-minutes scale."
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HorizonBucket {
    Fast5m,
    Mid30m,
    Session,
    MultiSession,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    Immediate,
    Normal,
    Relaxed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    PreMarket,
    Opening,
    Midday,
    Closing,
    AfterHours,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizon_bucket_serializes_to_snake_case() {
        let json = serde_json::to_string(&HorizonBucket::Fast5m).unwrap();
        assert_eq!(json, "\"fast5m\"");
        let json = serde_json::to_string(&HorizonBucket::MultiSession).unwrap();
        assert_eq!(json, "\"multi_session\"");
    }

    #[test]
    fn urgency_serializes_to_snake_case() {
        let json = serde_json::to_string(&Urgency::Immediate).unwrap();
        assert_eq!(json, "\"immediate\"");
    }

    #[test]
    fn session_phase_serializes_to_snake_case() {
        let json = serde_json::to_string(&SessionPhase::PreMarket).unwrap();
        assert_eq!(json, "\"pre_market\"");
    }
}
```

Register the module in `src/ontology/mod.rs` by adding this line in alphabetical order with the other `pub mod` declarations:

```rust
pub mod horizon;
```

- [ ] **Step 2: Run test to verify it fails (module not found initially)**

Run: `cargo test --lib ontology::horizon::tests::horizon_bucket_serializes_to_snake_case`

Expected: PASS (this is a pure type test that should compile and pass immediately once the file exists).

- [ ] **Step 3: Run all three tests**

Run: `cargo test --lib ontology::horizon::tests`

Expected output:
```
running 3 tests
test ontology::horizon::tests::horizon_bucket_serializes_to_snake_case ... ok
test ontology::horizon::tests::urgency_serializes_to_snake_case ... ok
test ontology::horizon::tests::session_phase_serializes_to_snake_case ... ok
```

- [ ] **Step 4: Commit**

```bash
git add src/ontology/horizon.rs src/ontology/mod.rs
git commit -m "feat(horizon): add HorizonBucket, Urgency, SessionPhase enums"
```

---

### Task 2: Add `HorizonExpiry` enum and legacy string lookup table

**Files:**
- Modify: `src/ontology/horizon.rs`

- [ ] **Step 1: Write failing tests for expiry and legacy lookup**

Add to `src/ontology/horizon.rs` (before `mod tests`):

```rust
/// Relative expiry — concrete `due_at` derived at display/execution time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HorizonExpiry {
    UntilNextBucket,
    UntilSessionClose,
    FixedTicks(u64),
    None,
}

impl HorizonBucket {
    /// Deterministic lookup table for legacy `time_horizon: String` values.
    /// No runtime inference — this is a fixed mapping.
    ///
    /// "intraday" maps to `Session` as a conservative default so that old
    /// records never accidentally poison Fast5m learning buckets.
    pub fn from_legacy_string(s: &str) -> HorizonBucket {
        match s {
            "intraday" => HorizonBucket::Session,
            "session" => HorizonBucket::Session,
            "multi_session" | "multi-session" => HorizonBucket::MultiSession,
            "multi-hour" | "multihour" => HorizonBucket::Mid30m,
            _ => HorizonBucket::Session,
        }
    }

    /// Forward derivation used for dual-writing the legacy `time_horizon`
    /// string field during Wave 2. Source of truth is always the bucket.
    pub fn to_legacy_string(self) -> &'static str {
        match self {
            HorizonBucket::Fast5m | HorizonBucket::Mid30m => "intraday",
            HorizonBucket::Session => "session",
            HorizonBucket::MultiSession => "multi_session",
        }
    }
}
```

Add to the `tests` module:

```rust
    #[test]
    fn legacy_string_intraday_maps_to_session() {
        assert_eq!(HorizonBucket::from_legacy_string("intraday"), HorizonBucket::Session);
    }

    #[test]
    fn legacy_string_session_maps_to_session() {
        assert_eq!(HorizonBucket::from_legacy_string("session"), HorizonBucket::Session);
    }

    #[test]
    fn legacy_string_multi_session_maps_to_multi_session() {
        assert_eq!(
            HorizonBucket::from_legacy_string("multi_session"),
            HorizonBucket::MultiSession,
        );
        assert_eq!(
            HorizonBucket::from_legacy_string("multi-session"),
            HorizonBucket::MultiSession,
        );
    }

    #[test]
    fn legacy_string_multi_hour_maps_to_mid30m() {
        assert_eq!(HorizonBucket::from_legacy_string("multi-hour"), HorizonBucket::Mid30m);
    }

    #[test]
    fn legacy_string_unknown_falls_back_to_session() {
        assert_eq!(HorizonBucket::from_legacy_string("whatever"), HorizonBucket::Session);
        assert_eq!(HorizonBucket::from_legacy_string(""), HorizonBucket::Session);
    }

    #[test]
    fn forward_derivation_is_unique() {
        assert_eq!(HorizonBucket::Fast5m.to_legacy_string(), "intraday");
        assert_eq!(HorizonBucket::Mid30m.to_legacy_string(), "intraday");
        assert_eq!(HorizonBucket::Session.to_legacy_string(), "session");
        assert_eq!(HorizonBucket::MultiSession.to_legacy_string(), "multi_session");
    }

    #[test]
    fn horizon_expiry_serialization_roundtrip() {
        let expiry = HorizonExpiry::FixedTicks(300);
        let json = serde_json::to_string(&expiry).unwrap();
        let parsed: HorizonExpiry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, HorizonExpiry::FixedTicks(300));
    }
```

- [ ] **Step 2: Run tests to verify they pass**

Run: `cargo test --lib ontology::horizon::tests`

Expected: all 10 tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/ontology/horizon.rs
git commit -m "feat(horizon): add HorizonExpiry and legacy string lookup"
```

---

### Task 3: Add `IntentOpportunityWindow` (new shape), `SecondaryHorizon`, `CaseHorizon`

**Files:**
- Modify: `src/ontology/horizon.rs`

- [ ] **Step 1: Write failing test for CaseHorizon invariant**

Add to `src/ontology/horizon.rs` (before `mod tests`):

```rust
/// One window in an Intent's horizon profile. An intent can have multiple —
/// the same underlying process may be viable in multiple buckets with
/// different bias/confidence. This lives on `IntentHypothesis.opportunities`.
///
/// Note: the `bias` field uses `IntentOpportunityBias` from `ontology::reasoning`,
/// but to keep this module free of reverse dependencies during Wave 1, we
/// accept a `bias` parameter as a generic placeholder. In Wave 2 we inline
/// the concrete enum type in `ontology::reasoning` directly.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HorizonWindow {
    pub bucket: HorizonBucket,
    pub urgency: Urgency,
    pub confidence: Decimal,
    pub alignment: Decimal,
    pub rationale: String,
}

/// Secondary horizons on a Case — context only. Carries just enough info
/// for display and delayed confirmation. Must never contain the primary bucket.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SecondaryHorizon {
    pub bucket: HorizonBucket,
    pub confidence: Decimal,
}

/// A Case's operational horizon — single primary choice, one rhythm.
///
/// Invariant: `primary` must never appear in `secondary`. The validator
/// `CaseHorizon::new` enforces this at construction time.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseHorizon {
    pub primary: HorizonBucket,
    pub urgency: Urgency,
    pub secondary: Vec<SecondaryHorizon>,
    pub session_phase: SessionPhase,
    pub expiry: HorizonExpiry,
}

impl CaseHorizon {
    /// Construct a `CaseHorizon`, enforcing the single-primary invariant.
    /// Any secondary entry whose bucket equals `primary` is silently
    /// filtered out — this is the single choke point for the invariant.
    pub fn new(
        primary: HorizonBucket,
        urgency: Urgency,
        session_phase: SessionPhase,
        expiry: HorizonExpiry,
        secondary: Vec<SecondaryHorizon>,
    ) -> Self {
        let secondary = secondary
            .into_iter()
            .filter(|s| s.bucket != primary)
            .collect();
        Self {
            primary,
            urgency,
            secondary,
            session_phase,
            expiry,
        }
    }
}
```

Add to the `tests` module:

```rust
    #[test]
    fn case_horizon_invariant_primary_not_in_secondary() {
        let ch = CaseHorizon::new(
            HorizonBucket::Fast5m,
            Urgency::Immediate,
            SessionPhase::Opening,
            HorizonExpiry::UntilNextBucket,
            vec![
                SecondaryHorizon { bucket: HorizonBucket::Fast5m, confidence: dec!(0.5) },
                SecondaryHorizon { bucket: HorizonBucket::Mid30m, confidence: dec!(0.7) },
            ],
        );
        // Fast5m should have been filtered from secondary
        assert_eq!(ch.secondary.len(), 1);
        assert_eq!(ch.secondary[0].bucket, HorizonBucket::Mid30m);
        assert!(!ch.secondary.iter().any(|s| s.bucket == ch.primary));
    }

    #[test]
    fn case_horizon_primary_is_single() {
        let ch = CaseHorizon::new(
            HorizonBucket::Session,
            Urgency::Relaxed,
            SessionPhase::Midday,
            HorizonExpiry::UntilSessionClose,
            vec![],
        );
        // This is a compile-time guarantee: primary is HorizonBucket, not Vec
        assert_eq!(ch.primary, HorizonBucket::Session);
    }
```

- [ ] **Step 2: Run tests**

Run: `cargo test --lib ontology::horizon::tests`

Expected: all 12 tests pass (10 previous + 2 new).

- [ ] **Step 3: Commit**

```bash
git add src/ontology/horizon.rs
git commit -m "feat(horizon): add HorizonWindow, SecondaryHorizon, CaseHorizon with primary invariant"
```

---

### Task 4: Add `compute_urgency` helper with locked rules

**Files:**
- Modify: `src/ontology/horizon.rs`

- [ ] **Step 1: Write failing tests covering every match arm**

Add to `src/ontology/horizon.rs` (before `mod tests`):

```rust
/// Minimal bias enum used by the urgency computation.
/// The full `IntentOpportunityBias` lives in `ontology::reasoning`;
/// we accept a lowered copy to keep this module free of reverse deps.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrgencyBias {
    Enter,
    Hold,
    Watch,
    Exit,
}

/// Minimal intent-state enum used by the urgency computation.
/// The full `IntentState` lives in `ontology::reasoning`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UrgencyIntentState {
    Forming,
    Active,
    Other,
}

/// Locked rules for computing `Urgency` from the context. This is NOT a
/// heuristic — every branch is explicit and tested.
///
/// See `urgency_compute_all_branches_covered` in the tests module.
pub fn compute_urgency(
    intent_state: UrgencyIntentState,
    bucket: HorizonBucket,
    bias: UrgencyBias,
    conflict_score: Decimal,
    exit_signal_present: bool,
) -> Urgency {
    // Exit signals are always immediate.
    if bias == UrgencyBias::Exit && exit_signal_present {
        return Urgency::Immediate;
    }

    match (bucket, bias, intent_state) {
        // Fast + Enter with high conflict while forming = window closing
        (HorizonBucket::Fast5m, UrgencyBias::Enter, UrgencyIntentState::Forming)
            if conflict_score > dec!(0.6) =>
        {
            Urgency::Immediate
        }

        // Active hold in mid bucket = normal pace
        (HorizonBucket::Mid30m, UrgencyBias::Hold, UrgencyIntentState::Active) => Urgency::Normal,

        // Forming mid entry = normal
        (HorizonBucket::Mid30m, UrgencyBias::Enter, UrgencyIntentState::Forming) => Urgency::Normal,

        // Session/MultiSession watch = relaxed regardless of state
        (HorizonBucket::Session, UrgencyBias::Watch, _) => Urgency::Relaxed,
        (HorizonBucket::MultiSession, UrgencyBias::Watch, _) => Urgency::Relaxed,

        // Default
        _ => Urgency::Normal,
    }
}
```

Add to the `tests` module:

```rust
    #[test]
    fn urgency_fast5m_enter_forming_high_conflict_is_immediate() {
        let u = compute_urgency(
            UrgencyIntentState::Forming,
            HorizonBucket::Fast5m,
            UrgencyBias::Enter,
            dec!(0.7),
            false,
        );
        assert_eq!(u, Urgency::Immediate);
    }

    #[test]
    fn urgency_fast5m_enter_forming_low_conflict_is_normal() {
        let u = compute_urgency(
            UrgencyIntentState::Forming,
            HorizonBucket::Fast5m,
            UrgencyBias::Enter,
            dec!(0.3),
            false,
        );
        assert_eq!(u, Urgency::Normal);
    }

    #[test]
    fn urgency_exit_signal_is_always_immediate() {
        let u = compute_urgency(
            UrgencyIntentState::Active,
            HorizonBucket::Session,
            UrgencyBias::Exit,
            dec!(0.0),
            true,
        );
        assert_eq!(u, Urgency::Immediate);
    }

    #[test]
    fn urgency_exit_without_signal_defaults_normal() {
        let u = compute_urgency(
            UrgencyIntentState::Active,
            HorizonBucket::Mid30m,
            UrgencyBias::Exit,
            dec!(0.0),
            false,
        );
        assert_eq!(u, Urgency::Normal);
    }

    #[test]
    fn urgency_mid30m_hold_active_is_normal() {
        let u = compute_urgency(
            UrgencyIntentState::Active,
            HorizonBucket::Mid30m,
            UrgencyBias::Hold,
            dec!(0.0),
            false,
        );
        assert_eq!(u, Urgency::Normal);
    }

    #[test]
    fn urgency_session_watch_is_relaxed() {
        let u = compute_urgency(
            UrgencyIntentState::Forming,
            HorizonBucket::Session,
            UrgencyBias::Watch,
            dec!(0.0),
            false,
        );
        assert_eq!(u, Urgency::Relaxed);
    }

    #[test]
    fn urgency_multi_session_watch_is_relaxed() {
        let u = compute_urgency(
            UrgencyIntentState::Other,
            HorizonBucket::MultiSession,
            UrgencyBias::Watch,
            dec!(0.0),
            false,
        );
        assert_eq!(u, Urgency::Relaxed);
    }

    #[test]
    fn urgency_unknown_combination_defaults_normal() {
        let u = compute_urgency(
            UrgencyIntentState::Other,
            HorizonBucket::Session,
            UrgencyBias::Hold,
            dec!(0.0),
            false,
        );
        assert_eq!(u, Urgency::Normal);
    }
```

- [ ] **Step 2: Run tests**

Run: `cargo test --lib ontology::horizon::tests`

Expected: all 20 tests pass (12 previous + 8 new).

- [ ] **Step 3: Commit**

```bash
git add src/ontology/horizon.rs
git commit -m "feat(horizon): add compute_urgency with locked branch rules"
```

---

### Task 5: Add `SessionPhaseResolver` trait and timestamp-based implementation

**Files:**
- Modify: `src/ontology/horizon.rs`

- [ ] **Step 1: Write failing tests for US opening/midday/closing classification**

Add to `src/ontology/horizon.rs` (before `mod tests`):

```rust
use crate::core::market::MarketId;

/// Classifier for `SessionPhase` given a timestamp and market.
/// Phase 1/2 uses `TimestampSessionResolver`; Phase 3+ can swap in a
/// calendar-aware resolver that handles half-days and holidays.
pub trait SessionPhaseResolver: Send + Sync {
    fn classify(&self, market: MarketId, ts: OffsetDateTime) -> SessionPhase;
}

/// Pure timestamp rule-based resolver. Handles normal US and HK sessions.
/// Does NOT handle half-days, early closes, or market holidays — those
/// require a calendar-aware resolver swapped in later.
///
/// US session (Eastern time, DST ignored for simplicity, normalized to UTC):
/// - 09:30-10:30 ET → Opening
/// - 10:30-15:00 ET → Midday
/// - 15:00-16:00 ET → Closing
/// - 04:00-09:30 ET → PreMarket
/// - 16:00-20:00 ET → AfterHours
///
/// HK session (Hong Kong time, normalized to UTC):
/// - 09:30-10:30 HKT → Opening
/// - 10:30-15:00 HKT → Midday
/// - 15:00-16:00 HKT → Closing
/// - before 09:30 HKT → PreMarket
/// - after 16:00 HKT → AfterHours
pub struct TimestampSessionResolver;

impl SessionPhaseResolver for TimestampSessionResolver {
    fn classify(&self, market: MarketId, ts: OffsetDateTime) -> SessionPhase {
        // Convert to minutes-since-midnight in the market's local time.
        // We compute local time by adding a fixed UTC offset — simple
        // and correct for normal sessions.
        let offset_hours: i8 = match market {
            MarketId::Hk => 8,    // HKT
            MarketId::Us => -5,   // ET standard time (DST ignored in Phase 1)
        };
        let local = ts + time::Duration::hours(offset_hours as i64);
        let minutes = local.hour() as i32 * 60 + local.minute() as i32;

        // Shared rule boundaries (minutes-from-midnight in local time)
        let opening_start = 9 * 60 + 30;   // 09:30
        let opening_end = 10 * 60 + 30;    // 10:30
        let closing_start = 15 * 60;       // 15:00
        let closing_end = 16 * 60;         // 16:00
        let pre_start = 4 * 60;            // 04:00 US pre-market
        let after_end = 20 * 60;           // 20:00 US after-hours

        if minutes >= opening_start && minutes < opening_end {
            SessionPhase::Opening
        } else if minutes >= opening_end && minutes < closing_start {
            SessionPhase::Midday
        } else if minutes >= closing_start && minutes < closing_end {
            SessionPhase::Closing
        } else if minutes >= pre_start && minutes < opening_start {
            SessionPhase::PreMarket
        } else if minutes >= closing_end && minutes < after_end {
            SessionPhase::AfterHours
        } else {
            SessionPhase::AfterHours
        }
    }
}
```

Add to the `tests` module:

```rust
    use time::macros::datetime;

    #[test]
    fn timestamp_resolver_us_opening() {
        let r = TimestampSessionResolver;
        // 14:45 UTC = 09:45 ET (standard time) — inside opening window
        let ts = datetime!(2026-04-13 14:45 UTC);
        assert_eq!(r.classify(MarketId::Us, ts), SessionPhase::Opening);
    }

    #[test]
    fn timestamp_resolver_us_midday() {
        let r = TimestampSessionResolver;
        // 17:00 UTC = 12:00 ET — midday
        let ts = datetime!(2026-04-13 17:00 UTC);
        assert_eq!(r.classify(MarketId::Us, ts), SessionPhase::Midday);
    }

    #[test]
    fn timestamp_resolver_us_closing() {
        let r = TimestampSessionResolver;
        // 20:30 UTC = 15:30 ET — closing
        let ts = datetime!(2026-04-13 20:30 UTC);
        assert_eq!(r.classify(MarketId::Us, ts), SessionPhase::Closing);
    }

    #[test]
    fn timestamp_resolver_hk_opening() {
        let r = TimestampSessionResolver;
        // 01:45 UTC = 09:45 HKT — opening
        let ts = datetime!(2026-04-14 01:45 UTC);
        assert_eq!(r.classify(MarketId::Hk, ts), SessionPhase::Opening);
    }

    #[test]
    fn timestamp_resolver_us_premarket() {
        let r = TimestampSessionResolver;
        // 13:00 UTC = 08:00 ET — pre-market
        let ts = datetime!(2026-04-13 13:00 UTC);
        assert_eq!(r.classify(MarketId::Us, ts), SessionPhase::PreMarket);
    }
```

- [ ] **Step 2: Run tests**

Run: `cargo test --lib ontology::horizon::tests`

Expected: all 25 tests pass (20 previous + 5 new). If `MarketId` type doesn't exist or has a different path, adjust the import line accordingly — check `src/core/market.rs` first.

- [ ] **Step 3: Commit**

```bash
git add src/ontology/horizon.rs
git commit -m "feat(horizon): add SessionPhaseResolver trait with timestamp implementation"
```

---

### Task 6: Add `HorizonEvaluationRecord` persistence type

**Files:**
- Create: `src/persistence/horizon_evaluation.rs`
- Modify: `src/persistence/mod.rs`

- [ ] **Step 1: Write the failing test for record roundtrip**

Create `src/persistence/horizon_evaluation.rs`:

```rust
//! Horizon evaluation persistence — materializes `HorizonExpiry` into
//! concrete `due_at` timestamps and tracks settlement status.
//!
//! One record per horizon per case: a case with primary `Fast5m` and
//! secondary `[Mid30m, Session]` produces three records, all settled
//! independently when their `due_at` hits (or earlier on exit signal).

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::ontology::horizon::HorizonBucket;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationStatus {
    Pending,
    Resolved,
    Expired,
    EarlyExited,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HorizonResult {
    pub net_return: Decimal,
    pub resolved_at: OffsetDateTime,
    pub follow_through: Decimal,
}

/// Persistence record for one horizon evaluation. Written at case open
/// (status=Pending, result=None) and updated when settled.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HorizonEvaluationRecord {
    pub record_id: String,
    pub setup_id: String,
    pub market: String,
    pub horizon: HorizonBucket,
    pub primary: bool,
    pub due_at: OffsetDateTime,
    pub status: EvaluationStatus,
    pub result: Option<HorizonResult>,
}

impl HorizonEvaluationRecord {
    pub fn build_id(setup_id: &str, horizon: HorizonBucket) -> String {
        format!("horizon-eval:{setup_id}:{horizon:?}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use time::macros::datetime;

    #[test]
    fn record_roundtrip_pending() {
        let r = HorizonEvaluationRecord {
            record_id: "horizon-eval:setup-1:Fast5m".into(),
            setup_id: "setup-1".into(),
            market: "us".into(),
            horizon: HorizonBucket::Fast5m,
            primary: true,
            due_at: datetime!(2026-04-13 14:35 UTC),
            status: EvaluationStatus::Pending,
            result: None,
        };
        let json = serde_json::to_string(&r).unwrap();
        let parsed: HorizonEvaluationRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn record_roundtrip_resolved_with_result() {
        let r = HorizonEvaluationRecord {
            record_id: "horizon-eval:setup-1:Mid30m".into(),
            setup_id: "setup-1".into(),
            market: "us".into(),
            horizon: HorizonBucket::Mid30m,
            primary: false,
            due_at: datetime!(2026-04-13 15:00 UTC),
            status: EvaluationStatus::Resolved,
            result: Some(HorizonResult {
                net_return: dec!(0.023),
                resolved_at: datetime!(2026-04-13 15:00 UTC),
                follow_through: dec!(0.85),
            }),
        };
        let json = serde_json::to_string(&r).unwrap();
        let parsed: HorizonEvaluationRecord = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, r);
    }

    #[test]
    fn build_id_is_deterministic() {
        let id = HorizonEvaluationRecord::build_id("setup-7", HorizonBucket::Session);
        assert_eq!(id, "horizon-eval:setup-7:Session");
    }
}
```

Register the module in `src/persistence/mod.rs` by adding this line in alphabetical order with the other `pub mod` declarations:

```rust
pub mod horizon_evaluation;
```

- [ ] **Step 2: Run tests**

Run: `cargo test --lib persistence::horizon_evaluation::tests`

Expected: all 3 tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/persistence/horizon_evaluation.rs src/persistence/mod.rs
git commit -m "feat(horizon): add HorizonEvaluationRecord persistence type"
```

---

### Task 7: Wave 1 exit — verify zero behavior change and full compile

**Files:** none modified; verification only.

- [ ] **Step 1: Run full library compile**

Run: `cargo check --lib`

Expected: clean build with no errors. Warnings unrelated to horizon are acceptable.

- [ ] **Step 2: Run full library tests**

Run: `cargo test --lib`

Expected: all existing tests still pass. All 28 new horizon-related tests pass.

- [ ] **Step 3: Verify no existing file outside the two new files was modified**

Run:
```bash
git diff HEAD~6 HEAD --stat -- ':!src/ontology/horizon.rs' ':!src/persistence/horizon_evaluation.rs' ':!src/ontology/mod.rs' ':!src/persistence/mod.rs' ':!docs/superpowers/plans/2026-04-12-horizon-system-plan.md'
```

Expected: empty output. Wave 1 must touch only the four listed files.

- [ ] **Step 4: Tag the wave**

```bash
git tag horizon-wave-1
```

---

## Wave 2 — Wire Types into Intent and Case Producers

Goal: `IntentHypothesis.opportunities` starts carrying `HorizonBucket` (not `String`), and every `TacticalSetup` gains a populated `CaseHorizon`. Legacy `time_horizon: String` becomes a derived field only.

### Task 8: Evolve `IntentOpportunityWindow` to carry `bucket`

**Files:**
- Modify: `src/ontology/reasoning.rs:327-334`

- [ ] **Step 1: Read the existing type**

Run: `grep -n "pub struct IntentOpportunityWindow" src/ontology/reasoning.rs`

Expected: one hit around line 327.

- [ ] **Step 2: Write the failing test**

Add to the existing tests module in `src/ontology/reasoning.rs` (find `#[cfg(test)]` near the bottom):

```rust
    #[test]
    fn intent_opportunity_window_has_bucket_field() {
        use crate::ontology::horizon::{HorizonBucket, Urgency};
        use rust_decimal_macros::dec;

        let w = IntentOpportunityWindow {
            bucket: HorizonBucket::Fast5m,
            urgency: Urgency::Immediate,
            horizon: "intraday".into(),
            bias: IntentOpportunityBias::Enter,
            confidence: dec!(0.85),
            alignment: dec!(0.7),
            rationale: "test".into(),
        };
        assert_eq!(w.bucket, HorizonBucket::Fast5m);
        assert_eq!(w.urgency, Urgency::Immediate);
    }
```

- [ ] **Step 3: Run test to verify it fails**

Run: `cargo test --lib ontology::reasoning::tests::intent_opportunity_window_has_bucket_field`

Expected: FAIL with "no field `bucket`" or similar.

- [ ] **Step 4: Update the struct**

Edit `src/ontology/reasoning.rs` around line 327. Current:

```rust
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentOpportunityWindow {
    pub horizon: String,
    pub bias: IntentOpportunityBias,
    pub confidence: Decimal,
    pub alignment: Decimal,
    pub rationale: String,
}
```

Change to:

```rust
use crate::ontology::horizon::{HorizonBucket, Urgency};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct IntentOpportunityWindow {
    /// Primary trading-time language. New field (Wave 2).
    pub bucket: HorizonBucket,
    /// Action timing. New field (Wave 2).
    pub urgency: Urgency,
    /// Legacy string horizon. Derived from `bucket` via
    /// `HorizonBucket::to_legacy_string()`. Kept until Wave 4 for
    /// JSON backward compatibility. Never use as source of truth.
    #[serde(default)]
    pub horizon: String,
    pub bias: IntentOpportunityBias,
    pub confidence: Decimal,
    pub alignment: Decimal,
    pub rationale: String,
}

impl IntentOpportunityWindow {
    /// Build a window, auto-filling `horizon` from `bucket` so callers
    /// never have to set the legacy string manually.
    pub fn new(
        bucket: HorizonBucket,
        urgency: Urgency,
        bias: IntentOpportunityBias,
        confidence: Decimal,
        alignment: Decimal,
        rationale: String,
    ) -> Self {
        Self {
            bucket,
            urgency,
            horizon: bucket.to_legacy_string().to_string(),
            bias,
            confidence,
            alignment,
            rationale,
        }
    }
}
```

If the `use crate::ontology::horizon...` line would conflict (file already has a `use` block), add it to the existing `use` block instead.

- [ ] **Step 5: Run test to verify it passes**

Run: `cargo test --lib ontology::reasoning::tests::intent_opportunity_window_has_bucket_field`

Expected: PASS.

- [ ] **Step 6: Fix all existing call sites that construct `IntentOpportunityWindow`**

Run to find them:
```bash
grep -rn "IntentOpportunityWindow {" src/ --include="*.rs"
```

For each call site, either use the new `IntentOpportunityWindow::new(...)` constructor, or add `bucket` and `urgency` fields to the struct literal. For existing code that used `horizon: "intraday".into()`, the corresponding bucket is `HorizonBucket::from_legacy_string("intraday")` → `Session`. Apply that default unless the context makes a better bucket obvious.

- [ ] **Step 7: Full library compile**

Run: `cargo check --lib`

Expected: clean build. If there are remaining call sites, fix them.

- [ ] **Step 8: Run all library tests**

Run: `cargo test --lib`

Expected: all tests pass.

- [ ] **Step 9: Commit**

```bash
git add src/ontology/reasoning.rs
git commit -m "feat(horizon): IntentOpportunityWindow carries HorizonBucket + Urgency"
```

---

### Task 9: Add `TacticalSetup.horizon: CaseHorizon` field with legacy derivation

**Files:**
- Modify: `src/ontology/reasoning.rs` (TacticalSetup struct around line 442)

- [ ] **Step 1: Locate the struct**

Run: `grep -n "pub struct TacticalSetup" src/ontology/reasoning.rs`

- [ ] **Step 2: Write the failing test**

Add to the tests module:

```rust
    #[test]
    fn tactical_setup_has_case_horizon() {
        use crate::ontology::horizon::{
            CaseHorizon, HorizonBucket, HorizonExpiry, SessionPhase, Urgency,
        };
        let setup = TacticalSetup {
            horizon: CaseHorizon::new(
                HorizonBucket::Fast5m,
                Urgency::Immediate,
                SessionPhase::Opening,
                HorizonExpiry::UntilNextBucket,
                vec![],
            ),
            time_horizon: "intraday".into(),
            // ... other fields
            ..TacticalSetup::default()
        };
        assert_eq!(setup.horizon.primary, HorizonBucket::Fast5m);
        // Legacy field is derived from primary bucket
        assert_eq!(setup.time_horizon, setup.horizon.primary.to_legacy_string());
    }
```

If `TacticalSetup` doesn't have a `Default` impl, this test will need explicit construction. Check existing tests around `TacticalSetup` to see which pattern the repo uses and match it.

- [ ] **Step 3: Run test to verify failure**

Run: `cargo test --lib ontology::reasoning::tests::tactical_setup_has_case_horizon`

Expected: FAIL with "no field `horizon`".

- [ ] **Step 4: Add the field**

In `src/ontology/reasoning.rs`, in the `TacticalSetup` struct, add after `time_horizon: String`:

```rust
    /// New in Wave 2. `CaseHorizon` is the source of truth for this
    /// setup's trading rhythm. The `time_horizon` field above is now
    /// a legacy derived projection and will be removed in Wave 4.
    #[serde(default = "default_case_horizon")]
    pub horizon: crate::ontology::horizon::CaseHorizon,
```

Add this helper at module level (near other `fn default_*` helpers if they exist, otherwise at the top of the file):

```rust
fn default_case_horizon() -> crate::ontology::horizon::CaseHorizon {
    use crate::ontology::horizon::{CaseHorizon, HorizonBucket, HorizonExpiry, SessionPhase, Urgency};
    CaseHorizon::new(
        HorizonBucket::Session,
        Urgency::Normal,
        SessionPhase::Midday,
        HorizonExpiry::UntilSessionClose,
        vec![],
    )
}
```

This default is used only for deserializing pre-Wave-2 records, consistent with the "intraday" → Session legacy mapping.

- [ ] **Step 5: Fix every `TacticalSetup { ... }` construction site**

Run:
```bash
grep -rn "TacticalSetup {" src/ --include="*.rs" | grep -v "pub struct"
```

For each hit, add `horizon: default_case_horizon(),` to the struct literal. This is safe because the default matches the legacy intraday → Session mapping. Call sites that have specific horizon information can override this in Task 10.

- [ ] **Step 6: Run all library tests**

Run: `cargo test --lib`

Expected: all tests pass, including the new `tactical_setup_has_case_horizon`.

- [ ] **Step 7: Commit**

```bash
git add src/ontology/reasoning.rs
git commit -m "feat(horizon): TacticalSetup carries CaseHorizon with legacy default"
```

---

### Task 10: Wire Case Builder selection rule in `bridge.rs`

**Files:**
- Modify: `src/pipeline/pressure/bridge.rs`

- [ ] **Step 1: Write failing test for selection rule ordering**

Add to `src/pipeline/pressure/bridge.rs` at the bottom, inside a `#[cfg(test)] mod tests` block (create one if not present):

```rust
#[cfg(test)]
mod horizon_tests {
    use super::*;
    use crate::ontology::horizon::{HorizonBucket, Urgency};
    use crate::ontology::reasoning::{IntentOpportunityBias, IntentOpportunityWindow};
    use rust_decimal_macros::dec;

    fn window(
        bucket: HorizonBucket,
        bias: IntentOpportunityBias,
        conf: Decimal,
    ) -> IntentOpportunityWindow {
        IntentOpportunityWindow::new(
            bucket,
            Urgency::Normal,
            bias,
            conf,
            dec!(0.5),
            "test".into(),
        )
    }

    #[test]
    fn selection_bias_rank_beats_confidence() {
        let opps = vec![
            window(HorizonBucket::Fast5m, IntentOpportunityBias::Enter, dec!(0.6)),
            window(HorizonBucket::Mid30m, IntentOpportunityBias::Enter, dec!(0.7)),
            window(HorizonBucket::Session, IntentOpportunityBias::Watch, dec!(0.9)),
        ];
        let ch = select_case_horizon(&opps);
        // Watch is demoted regardless of its 0.9 confidence.
        // Tie-break between two Enters: Mid30m has higher confidence.
        assert_eq!(ch.primary, HorizonBucket::Mid30m);
    }

    #[test]
    fn selection_all_watch_still_picks_one() {
        let opps = vec![
            window(HorizonBucket::Fast5m, IntentOpportunityBias::Watch, dec!(0.3)),
            window(HorizonBucket::Session, IntentOpportunityBias::Watch, dec!(0.6)),
        ];
        let ch = select_case_horizon(&opps);
        // Higher confidence Watch wins when nothing else exists.
        assert_eq!(ch.primary, HorizonBucket::Session);
    }

    #[test]
    fn selection_enter_prefers_short_bucket_on_tie() {
        let opps = vec![
            window(HorizonBucket::Fast5m, IntentOpportunityBias::Enter, dec!(0.8)),
            window(HorizonBucket::Mid30m, IntentOpportunityBias::Enter, dec!(0.8)),
        ];
        let ch = select_case_horizon(&opps);
        assert_eq!(ch.primary, HorizonBucket::Fast5m);
    }

    #[test]
    fn selection_hold_prefers_long_bucket_on_tie() {
        let opps = vec![
            window(HorizonBucket::Fast5m, IntentOpportunityBias::Hold, dec!(0.7)),
            window(HorizonBucket::Mid30m, IntentOpportunityBias::Hold, dec!(0.7)),
        ];
        let ch = select_case_horizon(&opps);
        assert_eq!(ch.primary, HorizonBucket::Mid30m);
    }

    #[test]
    fn selection_primary_not_in_secondary() {
        let opps = vec![
            window(HorizonBucket::Fast5m, IntentOpportunityBias::Enter, dec!(0.8)),
            window(HorizonBucket::Mid30m, IntentOpportunityBias::Hold, dec!(0.7)),
            window(HorizonBucket::Session, IntentOpportunityBias::Watch, dec!(0.4)),
        ];
        let ch = select_case_horizon(&opps);
        assert!(!ch.secondary.iter().any(|s| s.bucket == ch.primary));
    }
}
```

- [ ] **Step 2: Run tests to verify failure**

Run: `cargo test --lib pipeline::pressure::bridge::horizon_tests`

Expected: FAIL with "function `select_case_horizon` not found".

- [ ] **Step 3: Implement the selection rule**

Add to `src/pipeline/pressure/bridge.rs` (place near the top of the file after `use` statements):

```rust
use crate::ontology::horizon::{
    CaseHorizon, HorizonBucket, HorizonExpiry, SecondaryHorizon, SessionPhase, Urgency,
};
use crate::ontology::reasoning::{IntentOpportunityBias, IntentOpportunityWindow};

/// Select a single primary `CaseHorizon` from an intent's `opportunities`
/// profile, applying the documented selection rule:
///
/// 1. Rank by bias: Enter/Exit > Hold > Watch
/// 2. Break ties by confidence (descending)
/// 3. Break further by urgency: Immediate > Normal > Relaxed
/// 4. Bucket policy as final tie-breaker:
///    - Enter/Exit: short bucket wins (Fast5m > Mid30m > Session > MultiSession)
///    - Hold/Watch: long bucket wins (MultiSession > Session > Mid30m > Fast5m)
/// 5. If `opportunities` is empty, return a conservative default CaseHorizon
///    (Session, Normal, Midday, UntilSessionClose, no secondary).
pub fn select_case_horizon(opportunities: &[IntentOpportunityWindow]) -> CaseHorizon {
    if opportunities.is_empty() {
        return CaseHorizon::new(
            HorizonBucket::Session,
            Urgency::Normal,
            SessionPhase::Midday,
            HorizonExpiry::UntilSessionClose,
            vec![],
        );
    }

    // Sort: bias rank → confidence desc → urgency desc → bucket policy.
    let mut ranked: Vec<&IntentOpportunityWindow> = opportunities.iter().collect();
    ranked.sort_by(|a, b| {
        bias_rank(a.bias)
            .cmp(&bias_rank(b.bias))
            .then_with(|| b.confidence.cmp(&a.confidence))
            .then_with(|| urgency_rank(a.urgency).cmp(&urgency_rank(b.urgency)))
            .then_with(|| bucket_tiebreak(a.bias, a.bucket).cmp(&bucket_tiebreak(b.bias, b.bucket)))
    });

    let primary_window = ranked[0];
    let primary_bucket = primary_window.bucket;

    // Secondary = everyone else whose bucket is different, carried as
    // `(bucket, confidence)`. The `CaseHorizon::new` constructor filters
    // any accidental primary duplicates as a safety net.
    let secondary: Vec<SecondaryHorizon> = ranked[1..]
        .iter()
        .map(|w| SecondaryHorizon {
            bucket: w.bucket,
            confidence: w.confidence,
        })
        .collect();

    // Expiry derived from the primary bucket — short bucket → next bucket,
    // longer buckets → session close.
    let expiry = match primary_bucket {
        HorizonBucket::Fast5m | HorizonBucket::Mid30m => HorizonExpiry::UntilNextBucket,
        HorizonBucket::Session => HorizonExpiry::UntilSessionClose,
        HorizonBucket::MultiSession => HorizonExpiry::None,
    };

    // Session phase defaults to Midday — Wave 2 does not know the current
    // market time here. Wave 3 plugs in `SessionPhaseResolver`.
    CaseHorizon::new(
        primary_bucket,
        primary_window.urgency,
        SessionPhase::Midday,
        expiry,
        secondary,
    )
}

fn bias_rank(bias: IntentOpportunityBias) -> u8 {
    match bias {
        IntentOpportunityBias::Enter | IntentOpportunityBias::Exit => 0,
        IntentOpportunityBias::Hold => 1,
        IntentOpportunityBias::Watch => 2,
    }
}

fn urgency_rank(u: Urgency) -> u8 {
    match u {
        Urgency::Immediate => 0,
        Urgency::Normal => 1,
        Urgency::Relaxed => 2,
    }
}

fn bucket_tiebreak(bias: IntentOpportunityBias, bucket: HorizonBucket) -> u8 {
    let short_first = matches!(
        bias,
        IntentOpportunityBias::Enter | IntentOpportunityBias::Exit
    );
    let order = match bucket {
        HorizonBucket::Fast5m => 0u8,
        HorizonBucket::Mid30m => 1,
        HorizonBucket::Session => 2,
        HorizonBucket::MultiSession => 3,
    };
    if short_first {
        order
    } else {
        3 - order
    }
}
```

- [ ] **Step 4: Run tests to verify pass**

Run: `cargo test --lib pipeline::pressure::bridge::horizon_tests`

Expected: all 5 tests pass.

- [ ] **Step 5: Wire `select_case_horizon` into `insight_to_tactical_setup`**

Find the function in `src/pipeline/pressure/bridge.rs`:

```bash
grep -n "fn insight_to_tactical_setup" src/pipeline/pressure/bridge.rs
```

In the `TacticalSetup { ... }` construction inside that function, replace `horizon: default_case_horizon(),` (added in Task 9) with:

```rust
        horizon: select_case_horizon(&insight.intent.opportunities),
        time_horizon: select_case_horizon(&insight.intent.opportunities)
            .primary
            .to_legacy_string()
            .to_string(),
```

Note: if `insight.intent.opportunities` is not the exact path, adjust to match. Check the `VortexInsight` or equivalent struct that `insight_to_tactical_setup` takes.

**Important:** eliminate the duplicate `select_case_horizon` call by extracting to a local:

```rust
        let case_horizon = select_case_horizon(&insight.intent.opportunities);
        let legacy_horizon = case_horizon.primary.to_legacy_string().to_string();
        // ... then in the struct:
        horizon: case_horizon.clone(),
        time_horizon: legacy_horizon,
```

If `insight_to_tactical_setup` has no access to `opportunities` because it takes a simpler type, keep the default `CaseHorizon::new(HorizonBucket::Session, Urgency::Normal, ...)` from Task 9 at that call site — the Wave 3 work will wire intent-level data more deeply.

- [ ] **Step 6: Full compile and test**

Run: `cargo test --lib`

Expected: all tests pass.

- [ ] **Step 7: Commit**

```bash
git add src/pipeline/pressure/bridge.rs
git commit -m "feat(horizon): Case Builder selection rule in pressure bridge"
```

---

### Task 11: Surface horizon in `LiveTacticalCase` JSON

**Files:**
- Modify: `src/live_snapshot.rs`

- [ ] **Step 1: Locate existing fields**

Run: `grep -n "lifecycle_phase\|tension_driver\|is_isolated" src/live_snapshot.rs`

- [ ] **Step 2: Write failing test**

Add near existing `live_snapshot` tests (or create `#[cfg(test)] mod tests` if none):

```rust
#[test]
fn live_tactical_case_includes_horizon_fields() {
    let case = LiveTacticalCase {
        // ... existing required fields ...
        horizon_bucket: Some("fast5m".to_string()),
        horizon_urgency: Some("immediate".to_string()),
        horizon_secondary: vec!["mid30m".to_string(), "session".to_string()],
        ..LiveTacticalCase::default()
    };
    let json = serde_json::to_string(&case).unwrap();
    assert!(json.contains("\"horizon_bucket\":\"fast5m\""));
    assert!(json.contains("\"horizon_urgency\":\"immediate\""));
}
```

If `LiveTacticalCase` does not have `Default`, populate all required fields explicitly.

- [ ] **Step 3: Verify failure**

Run: `cargo test --lib live_snapshot`

Expected: FAIL with "no field `horizon_bucket`".

- [ ] **Step 4: Add fields to `LiveTacticalCase`**

In `src/live_snapshot.rs`, find the `LiveTacticalCase` struct (look for existing `lifecycle_phase` field) and add after the existing horizon-related optional fields:

```rust
    /// Primary horizon bucket (fast5m/mid30m/session/multi_session).
    /// Written from `CaseHorizon.primary` in Wave 2.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub horizon_bucket: Option<String>,

    /// Action urgency (immediate/normal/relaxed).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub horizon_urgency: Option<String>,

    /// Secondary horizon buckets (context only, no ranking impact).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub horizon_secondary: Vec<String>,
```

Also populate these fields wherever `LiveTacticalCase` is built from `TacticalSetup`. Find those sites:

```bash
grep -rn "LiveTacticalCase {" src/ --include="*.rs"
```

At each construction site, add:

```rust
        horizon_bucket: Some(format!("{:?}", setup.horizon.primary).to_lowercase()),
        horizon_urgency: Some(format!("{:?}", setup.horizon.urgency).to_lowercase()),
        horizon_secondary: setup
            .horizon
            .secondary
            .iter()
            .map(|s| format!("{:?}", s.bucket).to_lowercase())
            .collect(),
```

For test fixtures that don't have a real `setup`, use:
```rust
        horizon_bucket: None,
        horizon_urgency: None,
        horizon_secondary: vec![],
```

- [ ] **Step 5: Run all tests**

Run: `cargo test --lib`

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/live_snapshot.rs src/hk/runtime/snapshot/live.rs src/us/runtime/view.rs src/bin/replay.rs src/cases/tests.rs src/pipeline/mechanism_integration_tests.rs src/pipeline/predicate_engine_tests.rs
git commit -m "feat(horizon): surface bucket + urgency in LiveTacticalCase JSON"
```

(Include only the files actually modified.)

---

### Task 12: Wave 2 exit — compile and behavioral smoke test

- [ ] **Step 1: Full library compile**

Run: `cargo check --lib`

Expected: clean.

- [ ] **Step 2: Full library tests**

Run: `cargo test --lib`

Expected: all pass. New horizon-related tests: 28 (Wave 1) + ~10 (Wave 2) = ~38.

- [ ] **Step 3: Tag**

```bash
git tag horizon-wave-2
```

---

## Wave 3 — Outcome & Memory Layer

Goal: every case open creates `HorizonEvaluationRecord` rows (Pending), they settle at `due_at` or on exit signal, learning feedback gains a horizon dimension, and `DiscoveredArchetype` key includes bucket.

### Task 13: Materialize `HorizonExpiry` to `due_at` at case creation

**Files:**
- Modify: `src/persistence/horizon_evaluation.rs` — add a constructor helper
- Modify: the place that persists tactical setups (`src/persistence/tactical_setup.rs` or the store write path)

- [ ] **Step 1: Locate the tactical-setup persistence path**

Run:
```bash
grep -rn "TacticalSetupRecord\|fn write.*tactical\|upsert.*setup" src/persistence/ --include="*.rs"
```

Identify the function that writes a `TacticalSetup` to the store.

- [ ] **Step 2: Write failing test for `materialize_due_at`**

Add to `src/persistence/horizon_evaluation.rs`:

```rust
use crate::ontology::horizon::{CaseHorizon, HorizonBucket, HorizonExpiry};

impl HorizonEvaluationRecord {
    /// Convert a `HorizonExpiry` + reference timestamp into a concrete
    /// `due_at`. This is the single choke point for expiry materialization.
    pub fn materialize_due_at(
        expiry: HorizonExpiry,
        bucket: HorizonBucket,
        reference: OffsetDateTime,
    ) -> OffsetDateTime {
        match expiry {
            HorizonExpiry::UntilNextBucket => {
                let minutes = match bucket {
                    HorizonBucket::Fast5m => 5,
                    HorizonBucket::Mid30m => 30,
                    HorizonBucket::Session => 6 * 60,
                    HorizonBucket::MultiSession => 24 * 60,
                };
                reference + time::Duration::minutes(minutes)
            }
            HorizonExpiry::UntilSessionClose => {
                // Simple approximation: 6 hours from reference. Good enough
                // for Wave 3; calendar-aware resolver can refine later.
                reference + time::Duration::hours(6)
            }
            HorizonExpiry::FixedTicks(n) => {
                // One tick ≈ 1 second in Eden's runtime.
                reference + time::Duration::seconds(n as i64)
            }
            HorizonExpiry::None => {
                // Far-future sentinel.
                reference + time::Duration::days(365)
            }
        }
    }

    /// Build pending records for a case's primary + secondary horizons.
    pub fn pending_for_case(
        setup_id: &str,
        market: &str,
        horizon: &CaseHorizon,
        now: OffsetDateTime,
    ) -> Vec<HorizonEvaluationRecord> {
        let mut records = Vec::with_capacity(1 + horizon.secondary.len());
        records.push(HorizonEvaluationRecord {
            record_id: Self::build_id(setup_id, horizon.primary),
            setup_id: setup_id.to_string(),
            market: market.to_string(),
            horizon: horizon.primary,
            primary: true,
            due_at: Self::materialize_due_at(horizon.expiry, horizon.primary, now),
            status: EvaluationStatus::Pending,
            result: None,
        });
        for sec in &horizon.secondary {
            records.push(HorizonEvaluationRecord {
                record_id: Self::build_id(setup_id, sec.bucket),
                setup_id: setup_id.to_string(),
                market: market.to_string(),
                horizon: sec.bucket,
                primary: false,
                due_at: Self::materialize_due_at(horizon.expiry, sec.bucket, now),
                status: EvaluationStatus::Pending,
                result: None,
            });
        }
        records
    }
}
```

Add tests:

```rust
    #[test]
    fn materialize_fast5m_until_next_bucket_is_five_min() {
        let ref_ts = datetime!(2026-04-13 14:00 UTC);
        let due = HorizonEvaluationRecord::materialize_due_at(
            HorizonExpiry::UntilNextBucket,
            HorizonBucket::Fast5m,
            ref_ts,
        );
        assert_eq!(due, datetime!(2026-04-13 14:05 UTC));
    }

    #[test]
    fn materialize_session_until_session_close_is_six_hours() {
        let ref_ts = datetime!(2026-04-13 14:00 UTC);
        let due = HorizonEvaluationRecord::materialize_due_at(
            HorizonExpiry::UntilSessionClose,
            HorizonBucket::Session,
            ref_ts,
        );
        assert_eq!(due, datetime!(2026-04-13 20:00 UTC));
    }

    #[test]
    fn pending_for_case_creates_primary_plus_each_secondary() {
        use crate::ontology::horizon::{CaseHorizon, SecondaryHorizon, SessionPhase, Urgency};
        use rust_decimal_macros::dec;
        let horizon = CaseHorizon::new(
            HorizonBucket::Fast5m,
            Urgency::Immediate,
            SessionPhase::Opening,
            HorizonExpiry::UntilNextBucket,
            vec![
                SecondaryHorizon { bucket: HorizonBucket::Mid30m, confidence: dec!(0.7) },
                SecondaryHorizon { bucket: HorizonBucket::Session, confidence: dec!(0.4) },
            ],
        );
        let now = datetime!(2026-04-13 14:00 UTC);
        let records = HorizonEvaluationRecord::pending_for_case("setup-1", "us", &horizon, now);
        assert_eq!(records.len(), 3);
        assert!(records[0].primary);
        assert!(!records[1].primary);
        assert!(!records[2].primary);
        assert_eq!(records[0].horizon, HorizonBucket::Fast5m);
        assert_eq!(records[1].horizon, HorizonBucket::Mid30m);
        assert_eq!(records[2].horizon, HorizonBucket::Session);
        // Each record has its own due_at derived from its bucket.
        assert_eq!(records[0].due_at, datetime!(2026-04-13 14:05 UTC));
        assert_eq!(records[1].due_at, datetime!(2026-04-13 14:30 UTC));
        assert_eq!(records[2].due_at, datetime!(2026-04-13 20:00 UTC));
    }
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib persistence::horizon_evaluation::tests`

Expected: all tests pass (3 prior + 3 new = 6).

- [ ] **Step 4: Commit**

```bash
git add src/persistence/horizon_evaluation.rs
git commit -m "feat(horizon): materialize HorizonExpiry into due_at at case open"
```

---

### Task 14: Wire pending-record writes into the persistence store

**Files:**
- Modify: `src/persistence/store.rs` — add `write_horizon_evaluations` method
- Modify: `src/persistence/schema.rs` — register schema for the new table

- [ ] **Step 1: Locate the store write API pattern**

Run: `grep -n "pub async fn write_\|fn upsert_batch_checked" src/persistence/store.rs | head -20`

Study one existing `write_*` method (e.g. `write_tactical_setups`) to copy the pattern.

- [ ] **Step 2: Write failing test**

Add to `src/persistence/store/tests.rs` (or wherever existing store tests live — check with `grep -n "mod tests" src/persistence/store.rs`):

```rust
#[tokio::test]
async fn write_and_read_horizon_evaluation_records() {
    let store = EdenStore::new_in_memory().await.unwrap();
    use crate::ontology::horizon::HorizonBucket;
    use crate::persistence::horizon_evaluation::{
        EvaluationStatus, HorizonEvaluationRecord,
    };
    use time::macros::datetime;

    let records = vec![HorizonEvaluationRecord {
        record_id: "horizon-eval:test-1:Fast5m".into(),
        setup_id: "test-1".into(),
        market: "us".into(),
        horizon: HorizonBucket::Fast5m,
        primary: true,
        due_at: datetime!(2026-04-13 14:05 UTC),
        status: EvaluationStatus::Pending,
        result: None,
    }];

    store.write_horizon_evaluations(&records).await.unwrap();
    let loaded = store.load_horizon_evaluations_for_setup("test-1").await.unwrap();
    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].horizon, HorizonBucket::Fast5m);
    assert!(loaded[0].primary);
}
```

If `EdenStore::new_in_memory` does not exist, use whatever constructor existing tests use — check a nearby test in the same file.

- [ ] **Step 3: Run test to verify failure**

Run: `cargo test --lib --features persistence persistence::store::tests::write_and_read_horizon_evaluation_records`

Expected: FAIL with "no method `write_horizon_evaluations`".

- [ ] **Step 4: Implement store methods**

In `src/persistence/store.rs`, add to the `impl EdenStore` block (near other `write_*` methods):

```rust
    pub async fn write_horizon_evaluations(
        &self,
        records: &[crate::persistence::horizon_evaluation::HorizonEvaluationRecord],
    ) -> Result<(), StoreError> {
        upsert_batch_checked(
            &self.db,
            "horizon_evaluation",
            records.iter().map(|r| (r.record_id.as_str(), r)),
        )
        .await
    }

    pub async fn load_horizon_evaluations_for_setup(
        &self,
        setup_id: &str,
    ) -> Result<Vec<crate::persistence::horizon_evaluation::HorizonEvaluationRecord>, StoreError> {
        fetch_records_by_field_order(
            &self.db,
            "horizon_evaluation",
            "setup_id",
            setup_id,
            "due_at",
        )
        .await
    }
```

Verify the signatures of `upsert_batch_checked` and `fetch_records_by_field_order` in `src/persistence/store_helpers.rs` — if they differ from this shape, adjust the call accordingly.

- [ ] **Step 5: Register schema**

In `src/persistence/schema.rs`, find where other tables are defined and add:

```rust
    sql.push_str("DEFINE TABLE horizon_evaluation SCHEMALESS;\n");
```

If the file uses a structured `TableDef` list instead of raw SQL, match that pattern — check nearby declarations.

- [ ] **Step 6: Run test**

Run: `cargo test --lib --features persistence persistence::store::tests::write_and_read_horizon_evaluation_records`

Expected: PASS.

- [ ] **Step 7: Commit**

```bash
git add src/persistence/store.rs src/persistence/schema.rs
git commit -m "feat(horizon): persistence store read/write for HorizonEvaluationRecord"
```

---

### Task 15: Write pending records when a case is created

**Files:**
- Modify: the runtime path that persists newly created `TacticalSetup`s. Likely `src/core/persistence_sink.rs` or one of the `runtime` modules.

- [ ] **Step 1: Locate the setup persistence trigger**

Run:
```bash
grep -rn "write_tactical_setups\|persist.*setup" src/ --include="*.rs" | grep -v "test\|fn write_"
```

Find where a newly surfaced `TacticalSetup` gets written to the store.

- [ ] **Step 2: Write the integration-style test**

Because the runtime is async and integration-heavy, use a unit-level assertion on a helper function instead of a full runtime test. Add this helper to `src/core/persistence_sink.rs` (or equivalent):

```rust
/// Build horizon evaluation records for a newly created tactical setup.
/// Split out for testability.
pub fn horizon_records_for_setup(
    setup: &crate::ontology::reasoning::TacticalSetup,
    market: &str,
    now: time::OffsetDateTime,
) -> Vec<crate::persistence::horizon_evaluation::HorizonEvaluationRecord> {
    crate::persistence::horizon_evaluation::HorizonEvaluationRecord::pending_for_case(
        &setup.setup_id,
        market,
        &setup.horizon,
        now,
    )
}
```

Add a unit test in the same file:

```rust
#[cfg(test)]
mod horizon_records_tests {
    use super::*;
    use crate::ontology::horizon::{
        CaseHorizon, HorizonBucket, HorizonExpiry, SessionPhase, Urgency,
    };
    use time::macros::datetime;

    fn sample_setup() -> crate::ontology::reasoning::TacticalSetup {
        let mut s = crate::ontology::reasoning::TacticalSetup::default();
        s.setup_id = "test-1".into();
        s.horizon = CaseHorizon::new(
            HorizonBucket::Fast5m,
            Urgency::Immediate,
            SessionPhase::Opening,
            HorizonExpiry::UntilNextBucket,
            vec![],
        );
        s
    }

    #[test]
    fn builds_one_record_for_primary_only_case() {
        let setup = sample_setup();
        let now = datetime!(2026-04-13 14:00 UTC);
        let records = horizon_records_for_setup(&setup, "us", now);
        assert_eq!(records.len(), 1);
        assert!(records[0].primary);
        assert_eq!(records[0].horizon, HorizonBucket::Fast5m);
    }
}
```

- [ ] **Step 3: Verify failure, then implement**

Run the test. If `horizon_records_for_setup` already compiles (you added it in step 2), the test should PASS immediately. If `TacticalSetup::default()` is not available, use an explicit construction that matches other tests in the file.

- [ ] **Step 4: Wire into the write path**

In the place where `write_tactical_setups` is called, add immediately after:

```rust
        if let Some(store) = store_ref {
            let horizon_records: Vec<_> = new_setups
                .iter()
                .flat_map(|s| horizon_records_for_setup(s, market_label, now))
                .collect();
            if !horizon_records.is_empty() {
                if let Err(e) = store.write_horizon_evaluations(&horizon_records).await {
                    eprintln!("[horizon] failed to write evaluation records: {e}");
                }
            }
        }
```

Adjust variable names to match the surrounding code (`store_ref`, `new_setups`, `market_label`, `now`).

- [ ] **Step 5: Full compile and test**

Run: `cargo test --lib`

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/core/persistence_sink.rs
git commit -m "feat(horizon): write pending evaluation records on case open"
```

---

### Task 16: Upgrade `DiscoveredArchetype` key to include `HorizonBucket`

**Files:**
- Modify: `src/persistence/discovered_archetype.rs`

- [ ] **Step 1: Read the existing struct**

Run: `grep -n "archetype_key\|DiscoveredArchetypeRecord" src/persistence/discovered_archetype.rs | head -20`

- [ ] **Step 2: Write failing test**

Add to that file:

```rust
#[cfg(test)]
mod horizon_key_tests {
    use super::*;
    use crate::ontology::horizon::HorizonBucket;

    #[test]
    fn archetype_key_includes_bucket() {
        let key = build_archetype_key("failed_propagation", HorizonBucket::Fast5m, "sig-abc");
        assert_eq!(key, "failed_propagation:fast5m:sig-abc");
    }

    #[test]
    fn archetype_key_legacy_defaults_to_session() {
        // Records that don't carry a bucket (pre-Wave 3) should deserialize
        // with bucket = Session by default.
        let json = r#"{
            "archetype_id": "id-1",
            "market": "us",
            "archetype_key": "failed_propagation::sig-abc",
            "label": "legacy",
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
            "updated_at": "2026-04-12T00:00:00Z"
        }"#;
        let record: DiscoveredArchetypeRecord = serde_json::from_str(json).unwrap();
        assert_eq!(record.bucket, HorizonBucket::Session);
    }
}
```

- [ ] **Step 3: Run tests — expect failure**

Run: `cargo test --lib persistence::discovered_archetype::horizon_key_tests`

Expected: FAIL.

- [ ] **Step 4: Add the `bucket` field and the key helper**

In `DiscoveredArchetypeRecord`:

```rust
    #[serde(default = "default_bucket_session")]
    pub bucket: crate::ontology::horizon::HorizonBucket,
```

Add the default helper:

```rust
fn default_bucket_session() -> crate::ontology::horizon::HorizonBucket {
    crate::ontology::horizon::HorizonBucket::Session
}
```

Add the key builder:

```rust
pub fn build_archetype_key(
    intent_kind: &str,
    bucket: crate::ontology::horizon::HorizonBucket,
    signature: &str,
) -> String {
    let bucket_str = match bucket {
        crate::ontology::horizon::HorizonBucket::Fast5m => "fast5m",
        crate::ontology::horizon::HorizonBucket::Mid30m => "mid30m",
        crate::ontology::horizon::HorizonBucket::Session => "session",
        crate::ontology::horizon::HorizonBucket::MultiSession => "multi_session",
    };
    format!("{intent_kind}:{bucket_str}:{signature}")
}
```

Update `build_discovered_archetypes` (the existing builder near the top of the file) to populate `bucket`:
- Look up the bucket from the associated assessment/outcome record if available.
- Default to `HorizonBucket::Session` when not present.

- [ ] **Step 5: Run tests**

Run: `cargo test --lib persistence::discovered_archetype`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/persistence/discovered_archetype.rs
git commit -m "feat(horizon): DiscoveredArchetype key includes HorizonBucket"
```

---

### Task 17: Add `horizon_adjustments` to `ReasoningLearningFeedback` with sample gate

**Files:**
- Modify: `src/pipeline/learning_loop/types.rs`
- Modify: `src/pipeline/learning_loop/feedback.rs`

- [ ] **Step 1: Write failing test for the gate**

Add to `src/pipeline/learning_loop/feedback.rs` at the bottom:

```rust
#[cfg(test)]
mod horizon_gate_tests {
    use super::*;
    use crate::ontology::horizon::HorizonBucket;

    #[test]
    fn gate_below_50_is_diagnostics_only() {
        let mode = supplemental_horizon_learning_mode(49);
        assert_eq!(mode, HorizonLearningMode::Diagnostics);
    }

    #[test]
    fn gate_50_to_99_is_shadow() {
        assert_eq!(
            supplemental_horizon_learning_mode(50),
            HorizonLearningMode::Shadow
        );
        assert_eq!(
            supplemental_horizon_learning_mode(99),
            HorizonLearningMode::Shadow
        );
    }

    #[test]
    fn gate_100_plus_is_full() {
        assert_eq!(
            supplemental_horizon_learning_mode(100),
            HorizonLearningMode::Full
        );
        assert_eq!(
            supplemental_horizon_learning_mode(250),
            HorizonLearningMode::Full
        );
    }
}
```

- [ ] **Step 2: Run — expect failure**

Run: `cargo test --lib pipeline::learning_loop::feedback::horizon_gate_tests`

Expected: FAIL with "not found".

- [ ] **Step 3: Add types in `types.rs`**

Add to `src/pipeline/learning_loop/types.rs`:

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HorizonLearningAdjustment {
    pub intent_kind: String,
    pub bucket: crate::ontology::horizon::HorizonBucket,
    pub delta: Decimal,
    pub samples: usize,
    /// `true` when this adjustment came from supplemental horizons that
    /// haven't cleared the 100-sample gate. Shadow adjustments are recorded
    /// but never applied to live ranking.
    pub shadow: bool,
}
```

And extend `ReasoningLearningFeedback`:

```rust
pub struct ReasoningLearningFeedback {
    // ... existing fields ...
    #[serde(default)]
    pub horizon_adjustments: Vec<HorizonLearningAdjustment>,
}
```

Add a default helper method:

```rust
impl ReasoningLearningFeedback {
    pub fn horizon_delta(&self, intent: &str, bucket: crate::ontology::horizon::HorizonBucket) -> Decimal {
        self.horizon_adjustments
            .iter()
            .filter(|item| !item.shadow)  // shadow adjustments never contribute
            .find(|item| item.intent_kind == intent && item.bucket == bucket)
            .map(|item| item.delta)
            .unwrap_or(Decimal::ZERO)
    }
}
```

- [ ] **Step 4: Add gate in `feedback.rs`**

At the top of `src/pipeline/learning_loop/feedback.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HorizonLearningMode {
    Diagnostics,  // < 50 samples: log only, no feedback effect
    Shadow,       // 50-99 samples: adjustment recorded with shadow=true
    Full,         // >= 100 samples: adjustment recorded with shadow=false
}

/// Single choke point for the supplemental-horizon sample gate.
pub fn supplemental_horizon_learning_mode(samples: usize) -> HorizonLearningMode {
    match samples {
        0..=49 => HorizonLearningMode::Diagnostics,
        50..=99 => HorizonLearningMode::Shadow,
        _ => HorizonLearningMode::Full,
    }
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib pipeline::learning_loop::feedback::horizon_gate_tests`

Expected: PASS.

- [ ] **Step 6: Commit**

```bash
git add src/pipeline/learning_loop/types.rs src/pipeline/learning_loop/feedback.rs
git commit -m "feat(horizon): horizon_adjustments with 50/100 supplemental gate"
```

---

### Task 18: BKNG regression integration test

**Files:**
- Create: `src/ontology/horizon/regression_tests.rs` (or add to existing `horizon.rs` tests)

- [ ] **Step 1: Write the regression test**

Add to `src/ontology/horizon.rs` `mod tests`:

```rust
    #[test]
    fn bkng_flow_through_horizon_system() {
        use rust_decimal_macros::dec;
        use crate::ontology::reasoning::{IntentOpportunityBias, IntentOpportunityWindow};
        use crate::pipeline::pressure::bridge::select_case_horizon;

        // Step 1: Intent emits opportunities profile mimicking BKNG 14x vol.
        let opportunities = vec![
            IntentOpportunityWindow::new(
                HorizonBucket::Fast5m,
                Urgency::Immediate,
                IntentOpportunityBias::Enter,
                dec!(0.85),
                dec!(0.9),
                "volume 14x, isolated, no conflict".into(),
            ),
            IntentOpportunityWindow::new(
                HorizonBucket::Mid30m,
                Urgency::Normal,
                IntentOpportunityBias::Hold,
                dec!(0.70),
                dec!(0.8),
                "sustained momentum".into(),
            ),
            IntentOpportunityWindow::new(
                HorizonBucket::Session,
                Urgency::Relaxed,
                IntentOpportunityBias::Watch,
                dec!(0.45),
                dec!(0.6),
                "session-level confirmation pending".into(),
            ),
        ];

        // Step 2: Case Builder picks primary.
        let case_horizon = select_case_horizon(&opportunities);
        assert_eq!(case_horizon.primary, HorizonBucket::Fast5m);
        assert_eq!(case_horizon.urgency, Urgency::Immediate);
        assert_eq!(case_horizon.expiry, HorizonExpiry::UntilNextBucket);
        // Secondary carries the other two buckets, NOT the primary.
        assert_eq!(case_horizon.secondary.len(), 2);
        assert!(!case_horizon.secondary.iter().any(|s| s.bucket == HorizonBucket::Fast5m));

        // Step 3: Main learning key = (intent, Fast5m), not Session.
        // We assert this via the archetype key builder.
        use crate::persistence::discovered_archetype::build_archetype_key;
        let main_key = build_archetype_key(
            "DirectionalAccumulation",
            case_horizon.primary,
            "high_volume_isolated_no_conflict",
        );
        assert!(main_key.contains(":fast5m:"), "main key must be Fast5m, got: {}", main_key);
    }
```

- [ ] **Step 2: Run**

Run: `cargo test --lib ontology::horizon::tests::bkng_flow_through_horizon_system`

Expected: PASS. The test locks in that BKNG's "short-bucket entry, long-bucket confirmation" pattern flows correctly through Wave 1-3.

- [ ] **Step 3: Commit**

```bash
git add src/ontology/horizon.rs
git commit -m "test(horizon): BKNG regression — Fast5m entry is primary learning key"
```

---

### Task 19: Wave 3 exit — compile, test, tag

- [ ] **Step 1: Full compile**

Run: `cargo check --lib`

Expected: clean.

- [ ] **Step 2: Full tests**

Run: `cargo test --lib`

Expected: all pass.

- [ ] **Step 3: Tag**

```bash
git tag horizon-wave-3
```

---

## Wave 4 — Remove Legacy String Horizon

Goal: delete `TacticalSetup.time_horizon: String` and all legacy horizon string handling. After this wave, the only horizon language in the codebase is `HorizonBucket`.

### Task 20: Find and eliminate `time_horizon` references

**Files:** all code sites referencing `.time_horizon`.

- [ ] **Step 1: List all references**

Run:
```bash
grep -rn "time_horizon" src/ --include="*.rs"
```

Catalog every hit. Separate into:
- **Writer sites** (assign `time_horizon: "intraday".into()` or similar): delete the assignment, the field is gone.
- **Reader sites** (read `setup.time_horizon` to make a decision): replace with reading `setup.horizon.primary.to_legacy_string()` if the string is still needed for compat, or use `setup.horizon.primary` directly.
- **Test sites**: same as writers; just remove the field from struct literals.

- [ ] **Step 2: Remove the field from `TacticalSetup`**

In `src/ontology/reasoning.rs`, delete:

```rust
    pub time_horizon: String,
```

- [ ] **Step 3: Fix each call site**

Work site-by-site through the grep output. For each, apply the catalog rule from Step 1.

- [ ] **Step 4: Run compile after each file**

Run: `cargo check --lib` and fix errors until clean.

- [ ] **Step 5: Verify no references remain**

Run: `grep -rn "time_horizon" src/ --include="*.rs" | grep -v "case_horizon\|horizon_bucket"`

Expected: empty output.

- [ ] **Step 6: Run all tests**

Run: `cargo test --lib`

Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add -u
git commit -m "refactor(horizon): remove legacy time_horizon: String field

All call sites migrated to TacticalSetup.horizon: CaseHorizon.
Legacy string derivation remains via HorizonBucket::to_legacy_string
for JSON backward compat only."
```

---

### Task 21: Migrate lineage horizon strings to `HorizonBucket`

**Files:**
- Modify: `src/temporal/lineage.rs`
- Modify: `src/us/temporal/lineage.rs` (if symmetric)

- [ ] **Step 1: Locate the string usage**

Run: `grep -n '"5m"\|"30m"\|"session"' src/temporal/lineage.rs`

- [ ] **Step 2: Write failing test that asserts the enum API**

Add near existing lineage tests:

```rust
#[test]
fn aggregate_outcomes_by_family_takes_horizon_bucket() {
    use crate::ontology::horizon::HorizonBucket;
    // Smoke test: signature check only.
    let outcomes: Vec<CaseRealizedOutcome> = vec![];
    let _items = aggregate_outcomes_by_family(HorizonBucket::Fast5m, outcomes);
}
```

- [ ] **Step 3: Run — expect failure**

Run: `cargo test --lib temporal::lineage::tests::aggregate_outcomes_by_family_takes_horizon_bucket`

Expected: FAIL with type mismatch.

- [ ] **Step 4: Change the function signature**

Find `fn aggregate_outcomes_by_family(horizon: &str, ...)`. Change to:

```rust
fn aggregate_outcomes_by_family(
    horizon: HorizonBucket,
    outcomes: Vec<CaseRealizedOutcome>,
) -> Vec<HorizonLineageMetric> {
    // Inside: convert horizon to the label string only at the emit boundary.
    let horizon_label = match horizon {
        HorizonBucket::Fast5m => "5m",
        HorizonBucket::Mid30m => "30m",
        HorizonBucket::Session => "session",
        HorizonBucket::MultiSession => "multi_session",
    };
    // ... existing body but use horizon_label where the old `horizon: &str` was ...
}
```

Update the three call sites inside `compute_horizon_outcomes` (around lines 672-688):

```rust
    if let Some(lag_5m) = estimate_tick_lag_for_minutes(history, 5) {
        items.extend(aggregate_outcomes_by_family(
            HorizonBucket::Fast5m,
            compute_case_realized_outcomes(history, limit, lag_5m),
        ));
    }
    if let Some(lag_30m) = estimate_tick_lag_for_minutes(history, 30) {
        items.extend(aggregate_outcomes_by_family(
            HorizonBucket::Mid30m,
            compute_case_realized_outcomes(history, limit, lag_30m),
        ));
    }
    if let Some(lag_session) = estimate_tick_lag_for_minutes(history, session_minutes) {
        items.extend(aggregate_outcomes_by_family(
            HorizonBucket::Session,
            compute_case_realized_outcomes(history, limit, lag_session),
        ));
    }
```

If `HorizonLineageMetric.horizon` is still `String`, keep it as `String` — the `horizon_label` variable populates it. A later cleanup can change it to the enum if needed.

Mirror changes in `src/us/temporal/lineage.rs` if it has the same structure.

- [ ] **Step 5: Run tests**

Run: `cargo test --lib temporal::lineage`

Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/temporal/lineage.rs src/us/temporal/lineage.rs
git commit -m "refactor(horizon): lineage aggregate_outcomes_by_family takes HorizonBucket"
```

---

### Task 22: Replace `opening/midday/closing` strings with `SessionPhase`

**Files:**
- Modify: `src/temporal/lineage/outcomes/evaluation.rs:224-230` or wherever the strings live.

- [ ] **Step 1: Locate**

Run: `grep -rn '"opening"\|"midday"\|"closing"' src/ --include="*.rs"`

- [ ] **Step 2: Write failing test**

Add a test near the current location asserting the new signature returns `SessionPhase` instead of `&str`.

```rust
#[test]
fn classify_session_phase_returns_enum() {
    use crate::ontology::horizon::SessionPhase;
    // 570 minutes from midnight = 09:30 = opening
    let phase = classify_session_phase_from_minutes(570);
    assert_eq!(phase, SessionPhase::Opening);
    let phase = classify_session_phase_from_minutes(720);
    assert_eq!(phase, SessionPhase::Midday);
    let phase = classify_session_phase_from_minutes(900);
    assert_eq!(phase, SessionPhase::Closing);
}
```

- [ ] **Step 3: Rename and retype**

Find the existing function (probably `fn classify_session_phase(...) -> &'static str`). Rename to `classify_session_phase_from_minutes` and change its return to `SessionPhase`:

```rust
pub fn classify_session_phase_from_minutes(minutes_from_midnight: i32) -> crate::ontology::horizon::SessionPhase {
    use crate::ontology::horizon::SessionPhase;
    match minutes_from_midnight {
        570..=630 => SessionPhase::Opening,
        631..=870 => SessionPhase::Midday,
        871..=970 => SessionPhase::Closing,
        _ => SessionPhase::AfterHours,
    }
}
```

Update all callers.

- [ ] **Step 4: Run**

Run: `cargo test --lib temporal::lineage::outcomes`

Expected: pass.

- [ ] **Step 5: Commit**

```bash
git add src/temporal/lineage/outcomes/evaluation.rs
git commit -m "refactor(horizon): session phase classifier returns SessionPhase enum"
```

---

### Task 23: Wave 4 exit — final verification

- [ ] **Step 1: Grep for legacy strings**

Run: `grep -rn 'time_horizon' src/ --include="*.rs" | grep -v "HorizonBucket\|case_horizon\|to_legacy_string\|from_legacy_string"`

Expected: empty.

Run: `grep -rn '"intraday"\|"5m"\|"30m"\|"session"\|"opening"\|"midday"\|"closing"' src/ --include="*.rs" | grep -v "test\|tests\|to_legacy_string\|from_legacy_string\|horizon_label"`

Expected: empty or only inside `to_legacy_string` / `from_legacy_string` / `horizon_label` helpers.

- [ ] **Step 2: Full compile**

Run: `cargo check --lib`

Expected: clean.

- [ ] **Step 3: Full tests**

Run: `cargo test --lib`

Expected: all pass.

- [ ] **Step 4: Tag**

```bash
git tag horizon-wave-4
```

- [ ] **Step 5: Final commit and roll-up**

```bash
git commit --allow-empty -m "chore(horizon): Wave 4 complete — legacy time language removed"
```

---

## Rollback plan

Each wave is a separate tag: `horizon-wave-1`, `horizon-wave-2`, `horizon-wave-3`, `horizon-wave-4`. Reset to any of them if a wave breaks something unrelated.

Waves 1 and 2 are reversible without data loss because new fields are additive and the source of truth remains `CaseHorizon`. Wave 3 writes new SurrealDB records (`horizon_evaluation` table) but does not alter old ones — reverting Wave 3 simply stops writing new records. Wave 4 is destructive (removes `time_horizon`) so confirm Waves 1-3 have been running stably before merging Wave 4.
