# Horizon System — Unify Time Language Through Trading Windows

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Collapse Eden's scattered time language (7 different places) into a single Horizon System that answers "in which trading window does this intent live?" — solving the BKNG exit-timing failure where Eden didn't know when an intent was over.

**Architecture:** Four independent time concepts with strict separation. `HorizonBucket` enum is the shared trading-time language. Intent provides a horizon profile, Case picks one primary, Outcome evaluates at primary + supplemental, Memory learns on `Intent × Bucket` as main key.

**Tech Stack:** Rust enums/structs, shared with existing Intent System (`IntentHypothesis`, `TacticalSetup`, `DiscoveredArchetype`)

---

## Core Principle

> TimeScale is compute time. HorizonBucket is trading time. SessionPhase is market time. Urgency is action time.
>
> Intent provides horizon profile, Case picks primary horizon, Outcome uses aligned horizon for main learning, supplemental horizons are diagnostics only.

Four time-related concepts are now cleanly separated and **must not be mixed**:

| Concept | Purpose | Values | Location |
|---------|---------|--------|----------|
| `TimeScale` | Math/decay layers for pressure field | Tick/Minute/Hour/Day | `pipeline/pressure.rs` — **unchanged** |
| `HorizonBucket` | Trading opportunity period (shared enum) | Fast5m/Mid30m/Session/MultiSession | `ontology/horizon.rs` — **new** |
| `SessionPhase` | Market context | PreMarket/Opening/Midday/Closing/AfterHours | `ontology/horizon.rs` — **new** |
| `Urgency` | Action timing (how late is too late) | Immediate/Normal/Relaxed | `ontology/horizon.rs` — **new** |

---

## Core Types

### Shared enums

```rust
/// Trading-time language. Not minute counts — trading opportunity categories.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HorizonBucket {
    Fast5m,        // Short-term opportunity, seconds-to-minutes decision
    Mid30m,        // Medium-term, complete within half hour
    Session,       // Complete within one trading session
    MultiSession,  // Cross-session hold
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Urgency {
    Immediate,  // Act now, late means missed
    Normal,     // Can wait for pullback
    Relaxed,    // Watch only, no hurry
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SessionPhase {
    PreMarket,
    Opening,     // First 60 minutes
    Midday,
    Closing,     // Last 60 minutes
    AfterHours,
}

/// Relative expiry — concrete `expires_at` derived at display/execution time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HorizonExpiry {
    UntilNextBucket,     // Expires at the end of this bucket's natural window
    UntilSessionClose,   // Expires at session close
    FixedTicks(u64),     // Expires after N ticks
    None,                // No expiry
}
```

### Intent layer — horizon profile

```rust
/// One window in an Intent's horizon profile.
/// An IntentHypothesis can have multiple of these — same intent may be
/// viable in multiple buckets with different bias/confidence.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentOpportunityWindow {
    pub bucket: HorizonBucket,
    pub urgency: Urgency,
    pub bias: IntentOpportunityBias,   // existing enum: Enter/Hold/Exit/Watch
    pub confidence: Decimal,
    pub alignment: Decimal,             // how well this window aligns with other observations
    pub rationale: String,
}

// IntentHypothesis gains: opportunities: Vec<IntentOpportunityWindow>
```

### Case layer — single primary

```rust
/// Secondary horizons are context-only. Enough info for display and
/// delayed confirmation, but NOT full IntentOpportunityWindow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SecondaryHorizon {
    pub bucket: HorizonBucket,
    pub bias: IntentOpportunityBias,
    pub confidence: Decimal,
}

/// Case's operational horizon — single choice, one rhythm.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseHorizon {
    /// Single primary — the decision rhythm. Required.
    pub primary: HorizonBucket,
    pub urgency: Urgency,
    pub bias: IntentOpportunityBias,

    /// Context only. Does NOT enter main ranking or main learning key.
    pub secondary: Vec<SecondaryHorizon>,

    /// Market context at case open.
    pub session_phase: SessionPhase,

    /// Relative expiry. Concrete `due_at` materialized in persistence layer.
    pub expiry: HorizonExpiry,
}

// TacticalSetup gains: horizon: CaseHorizon
// TacticalSetup.time_horizon (String) — kept as legacy derived field until Phase 4
```

### Outcome layer — aligned evaluation + supplemental diagnostics

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HorizonResult {
    pub net_return: Decimal,
    pub resolved_at: OffsetDateTime,
    pub follow_through: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EvaluationStatus {
    Pending,      // due_at not yet reached
    Resolved,     // naturally resolved at due_at
    Expired,      // due_at reached without confirmation
    EarlyExited,  // intent exit signal triggered before due_at
}

/// Persistence-layer record. Case open creates one record per horizon
/// (primary + each secondary). Settled independently.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HorizonEvaluationRecord {
    pub setup_id: String,
    pub horizon: HorizonBucket,
    pub primary: bool,             // true for the case's primary horizon
    pub due_at: OffsetDateTime,    // materialized from HorizonExpiry
    pub status: EvaluationStatus,
    pub result: Option<HorizonResult>,
}

/// In-memory view used by learning layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OutcomeEvaluation {
    /// Always equals Case.horizon.primary — this is half the main learning key
    pub primary_horizon: HorizonBucket,
    pub primary_result: HorizonResult,

    /// Extra horizons. Diagnostics until sample gate clears.
    pub supplemental: Vec<(HorizonBucket, HorizonResult)>,
}
```

---

## Data Flow

```
┌─────────────────────────────────────────────────────────┐
│ Step 1: Pressure Field detects vortex                   │
│   (TimeScale layer — not in Horizon System scope)       │
└─────────────────────────────────────────────────────────┘
                        ↓
┌─────────────────────────────────────────────────────────┐
│ Step 2: Intent inference generates opportunity profile  │
│                                                          │
│   IntentHypothesis {                                    │
│     kind: DirectionalAccumulation,                      │
│     opportunities: [                                    │
│       { bucket: Fast5m,  bias: Enter, urgency: Immediate, conf: 0.85 }, │
│       { bucket: Mid30m,  bias: Hold,  urgency: Normal,    conf: 0.70 }, │
│       { bucket: Session, bias: Watch, urgency: Relaxed,   conf: 0.45 }, │
│     ],                                                  │
│   }                                                     │
└─────────────────────────────────────────────────────────┘
                        ↓
┌─────────────────────────────────────────────────────────┐
│ Step 3: Case Builder picks single primary horizon        │
│                                                          │
│   Selection rule (in order):                             │
│     1. Rank by bias: Enter/Exit > Hold > Watch           │
│     2. Break ties by confidence                          │
│     3. Break further by urgency                          │
│     4. Bucket policy as final tie-breaker:               │
│        - Enter/Exit: short bucket wins                   │
│        - Hold/Watch: long bucket wins                    │
│     5. If all are Watch, still pick one — never leave    │
│        Case without a primary horizon                    │
│                                                          │
│   CaseHorizon {                                         │
│     primary: Fast5m, urgency: Immediate, bias: Enter,   │
│     secondary: [{ Mid30m, Hold, 0.70 }, { Session, Watch, 0.45 }], │
│     session_phase: Opening,                             │
│     expiry: UntilNextBucket,                            │
│   }                                                     │
└─────────────────────────────────────────────────────────┘
                        ↓
┌─────────────────────────────────────────────────────────┐
│ Step 4: Persistence materializes HorizonEvaluationRecord │
│                                                          │
│   For each horizon (primary + each secondary):           │
│     create HorizonEvaluationRecord {                     │
│       setup_id, horizon, primary: bool,                  │
│       due_at: derived from HorizonExpiry + current time, │
│       status: Pending, result: None,                     │
│     }                                                    │
│                                                          │
│   Each record settles independently when due_at hits,    │
│   or earlier if IntentExitSignal fires (→ EarlyExited).  │
└─────────────────────────────────────────────────────────┘
                        ↓
┌─────────────────────────────────────────────────────────┐
│ Step 5: Outcome Evaluation                               │
│                                                          │
│   OutcomeEvaluation {                                   │
│     primary_horizon: Fast5m,        ← main learning key │
│     primary_result: { +0.023, 0.85 },                   │
│     supplemental: [                 ← diagnostics only  │
│       (Mid30m, { +0.041, 0.72 }),                       │
│       (Session, { +0.038, 0.55 }),                      │
│     ],                                                  │
│   }                                                     │
└─────────────────────────────────────────────────────────┘
                        ↓
┌─────────────────────────────────────────────────────────┐
│ Step 6: Memory Update                                    │
│                                                          │
│   Main learning key: (Intent, Bucket)                    │
│     DiscoveredArchetype indexed on                       │
│     (DirectionalAccumulation, Fast5m)                    │
│                                                          │
│   Supplemental gate:                                     │
│     samples < 50  → diagnostics only (log, no ranking)   │
│     samples 50-99 → shadow learning (recorded but off)   │
│     samples ≥ 100 → full learning (affects ranking)      │
└─────────────────────────────────────────────────────────┘
```

### Urgency computation (locked rules, not heuristic)

```rust
fn compute_urgency(
    intent_state: IntentState,
    bucket: HorizonBucket,
    bias: IntentOpportunityBias,
    conflict_score: Decimal,
    exit_signal_present: bool,
) -> Urgency {
    match (bucket, bias, intent_state) {
        // Fast + Enter with high conflict = window closing
        (HorizonBucket::Fast5m, IntentOpportunityBias::Enter, IntentState::Forming)
            if conflict_score > dec!(0.6) => Urgency::Immediate,

        // Exit signals are always immediate
        (_, IntentOpportunityBias::Exit, _) if exit_signal_present => Urgency::Immediate,

        // Active hold = normal pace
        (HorizonBucket::Mid30m, IntentOpportunityBias::Hold, IntentState::Active) => Urgency::Normal,

        // Forming medium entry = normal
        (HorizonBucket::Mid30m, IntentOpportunityBias::Enter, IntentState::Forming) => Urgency::Normal,

        // Session watch = relaxed
        (HorizonBucket::Session, IntentOpportunityBias::Watch, _) => Urgency::Relaxed,
        (HorizonBucket::MultiSession, IntentOpportunityBias::Watch, _) => Urgency::Relaxed,

        // Default
        _ => Urgency::Normal,
    }
}
```

### SessionPhase resolution

```rust
/// Trait lets us swap timestamp-based rules for a calendar-aware resolver later.
pub trait SessionPhaseResolver: Send + Sync {
    fn classify(&self, market: MarketId, ts: OffsetDateTime) -> SessionPhase;
}

/// Phase 1/2 implementation: pure timestamp rules.
/// Handles normal US/HK sessions. Does NOT handle half-days, holidays,
/// early closes — that requires a calendar-aware resolver in Phase 3+.
pub struct TimestampSessionResolver;
```

---

## Migration Path

### Wave 1 — Add types only (zero behavior change)

**Scope:**
- New file: `src/ontology/horizon.rs` — all the enums and structs above
- New file: `src/persistence/horizon_evaluation.rs` — `HorizonEvaluationRecord`
- `TimestampSessionResolver` implementation
- Unit tests for the types

**Rules:**
- No existing code changes
- Start with **exactly two new files**: `src/ontology/horizon.rs` and `src/persistence/horizon_evaluation.rs`. Do NOT pre-split into submodules (`case_horizon.rs`, `outcome.rs`).
- Split only when `horizon.rs` exceeds 300-400 lines
- Must include an invariant test: `CaseHorizon.primary` must NEVER appear in `CaseHorizon.secondary`. This is the single rule that keeps Case's primary truly single.

**Exit criteria:**
- `cargo check --lib` green
- Zero consumer breakage
- All new type tests pass
- Single commit

### Wave 2 — Write horizons at Intent and Case layer

**Scope:**
- `IntentHypothesis` gains `opportunities: Vec<IntentOpportunityWindow>`
- `TacticalSetup` gains `horizon: CaseHorizon`
- `TacticalSetup.time_horizon: String` stays but becomes **legacy derived field** — marked deprecated, only written from `CaseHorizon.primary` (never read back)
- `insight_to_tactical_setup` computes `CaseHorizon` via the selection rule
- `compute_urgency` helper wired in
- `LiveTacticalCase` surfaces primary bucket and urgency in JSON snapshot

**Dual-write rules:**
- New `CaseHorizon` is the source of truth
- Old `time_horizon` is derived one-way from primary bucket: `Fast5m/Mid30m → "intraday"`, `Session → "session"`, `MultiSession → "multi_session"`
- Never read `time_horizon` to infer horizon — only for backward-compat JSON output

**Legacy deserialization rule (fixed, no runtime guessing):**

When reading old records that have only `time_horizon: String` and no `CaseHorizon`:

| Legacy string | Maps to `HorizonBucket` |
|--------------|-------------------------|
| `"intraday"` | `Session` *(conservative default — avoids accidentally triggering Fast5m learning buckets)* |
| `"session"` | `Session` |
| `"multi_session"` / `"multi-session"` | `MultiSession` |
| `"multi-hour"` / `"multihour"` | `Mid30m` |
| Any other value | `Session` *(fallback)* |

This is a **deterministic lookup table**, not an inference. The rule lives in `horizon.rs` as `HorizonBucket::from_legacy_string(&str) -> HorizonBucket`.

**Exit criteria:**
- `cargo check --lib` green
- Existing tests still pass
- `live_snapshot.json` format unchanged
- Cases carry horizon info, but nothing depends on it yet

### Wave 3 — Outcome and memory layer

**Scope:**
- `HorizonEvaluationRecord` persistence: case open creates Pending records for primary + each secondary
- `case_realized_outcome` flow settles records at `due_at`, writes results
- `IntentExitSignal` handler flips primary to `EarlyExited` (still writes partial result)
- `DiscoveredArchetype.key` upgraded from `(intent_kind, signature)` to `(intent_kind, bucket, signature)` — existing records are read with `bucket = Session` as default on deserialization (lazy migration, no batch rewrite required). New records write the full key.
- `ReasoningLearningFeedback` gains `horizon_adjustments: Vec<HorizonLearningAdjustment>`
- Sample gate logic is a **hard rule**, not a suggestion:
  - `<50` — diagnostics only. Records are written but **never** enter ranking or feedback deltas.
  - `50-99` — shadow learning. Adjustments are computed and logged but **never** influence live ranking. Purpose: validate the signal before committing.
  - `≥100` — full learning. Adjustments flow into `ReasoningLearningFeedback` and affect ranking.
  - The gate is enforced at a single choke point (`supplemental_horizon_learning_mode`), not scattered across call sites.
- `lineage::aggregate_outcomes_by_family` signature changes: `horizon: &str` → `horizon: HorizonBucket`

**Exit criteria:**
- New `HorizonEvaluationRecord` persistence tests pass
- BKNG-style regression test (see Testing section) passes
- Existing `lineage` tests updated to use enum
- Main learning key is now 2D: `(intent_kind, bucket)`

### Wave 4 — Cleanup

**Scope:**
- Delete `TacticalSetup.time_horizon: String` — all consumers migrated
- Delete string horizons in lineage aggregations
- Replace `"opening" / "midday" / "closing"` strings in `outcomes/evaluation.rs` with `SessionPhase` enum
- Delete legacy derivation helpers

**Exit criteria:**
- No remaining `.time_horizon` references
- `grep -r '"5m"\|"30m"\|"session"'` returns zero results in `src/`
- `SessionPhase` used throughout

### Unaffected — NOT in scope

These stay as-is:

- `PressureField.layers` (`TimeScale`) — math layer, not trading layer
- `SignalMomentumTracker` 60-tick window — lookback window for derivative computation, not horizon
- `EdgeLearningLedger` 7-day decay — memory decay, not horizon
- Persistence counter 3-scan = 6-minute — signal confirmation threshold, different concept from case horizon

---

## Testing Strategy

### Wave 1 tests — type layer

```rust
#[test]
fn horizon_bucket_snake_case_json() { /* ... */ }

#[test]
fn case_horizon_has_exactly_one_primary() { /* ... */ }

#[test]
fn secondary_cannot_contain_primary_bucket() {
    // Validator rejects CaseHorizon where secondary includes primary bucket
}

#[test]
fn urgency_compute_all_branches_covered() {
    // One test case per match arm in compute_urgency
    assert_eq!(
        compute_urgency(IntentState::Forming, HorizonBucket::Fast5m,
                        IntentOpportunityBias::Enter, dec!(0.7), false),
        Urgency::Immediate
    );
    // ... all arms
}

#[test]
fn horizon_expiry_until_next_bucket_fast5m_is_five_min() {
    // UntilNextBucket + Fast5m → ticks equivalent to 5 minutes
}

#[test]
fn timestamp_session_resolver_us_opening() {
    // 09:30-10:30 ET → SessionPhase::Opening
}
```

### Wave 2 tests — Case Builder

```rust
#[test]
fn case_builder_bias_rank_beats_confidence() {
    // opportunities: [Fast5m(Enter,0.6), Mid30m(Enter,0.7), Session(Watch,0.9)]
    // Expected primary: Mid30m — Watch is demoted regardless of confidence
}

#[test]
fn case_builder_all_watch_still_picks_one() {
    // All Watch → primary is still populated, never None
}

#[test]
fn case_builder_enter_prefers_short_bucket() {
    // [Fast5m(Enter,0.8), Mid30m(Enter,0.8)] tie → Fast5m wins
}

#[test]
fn case_builder_hold_prefers_long_bucket() {
    // [Fast5m(Hold,0.8), Mid30m(Hold,0.8)] tie → Mid30m wins
}

#[test]
fn legacy_time_horizon_is_derived_only() {
    // CaseHorizon.primary == Fast5m → time_horizon == "intraday"
    // CaseHorizon.primary == MultiSession → time_horizon == "multi_session"
    // Setting time_horizon directly does NOT affect CaseHorizon
}
```

### Wave 3 tests — Outcome + Memory

```rust
#[test]
fn horizon_evaluation_record_roundtrip() { /* persistence */ }

#[test]
fn pending_records_created_on_case_open() {
    // One Pending record per horizon in {primary} ∪ secondary
    // primary flag set correctly
}

#[test]
fn early_exit_flags_primary_as_early_exited() {
    // IntentExitSignal before due_at → status = EarlyExited
    // result still populated with return at exit time
}

#[test]
fn supplemental_below_50_is_diagnostics_only() {
    // 49 samples in supplemental → no ranking impact, only log
}

#[test]
fn supplemental_gate_50_to_99_shadow_learning() {
    // 75 samples → shadow learning recorded, ranking unchanged
}

#[test]
fn supplemental_gate_100_plus_full_learning() {
    // 100+ samples → full learning active
}

#[test]
fn archetype_key_includes_bucket() {
    // DiscoveredArchetype now indexed on (intent_kind, bucket, signature)
    // Pre-migration records default to Session bucket
}
```

### Wave 4 tests — regression guards

```rust
#[test]
fn no_references_to_legacy_time_horizon_string() {
    // Source-level check: no `.time_horizon` references remain
}

#[test]
fn lineage_uses_horizon_bucket_enum() {
    // aggregate_outcomes_by_family signature uses HorizonBucket
}
```

### BKNG regression test

```rust
#[test]
fn bkng_flow_through_horizon_system() {
    // Simulate BKNG volume 14x entry:
    //   Intent = DirectionalAccumulation, strength = Extreme
    //   opportunities = [
    //     Fast5m (Enter, Immediate, conf=0.85),
    //     Mid30m (Hold,  Normal,    conf=0.70),
    //     Session (Watch, Relaxed,  conf=0.45),
    //   ]
    //
    // Case.horizon.primary = Fast5m (Enter wins bias rank)
    // Case.horizon.secondary = [(Mid30m, Hold, 0.70), (Session, Watch, 0.45)]
    // expiry = UntilNextBucket
    //
    // At 5min: primary settles Resolved +0.02
    // At 35min: supplemental[Mid30m] settles Resolved +0.05
    // At session end: volume decelerates sharply → IntentExitSignal fires
    //   supplemental[Session] settles EarlyExited with partial result
    //
    // Assertions:
    //   - Main learning key = (DirectionalAccumulation, Fast5m)
    //     — NOT (DirectionalAccumulation, Session)
    //   - Mid30m/Session supplemental only enter diagnostics path
    //     (samples below gate)
    //   - Exit timing is driven by primary horizon expiry +
    //     IntentExitSignal — NOT by arbitrary time
}
```

This regression test is the concrete lock against the BKNG failure mode: the system must know which window an intent lives in, and exit when that window closes.

---

## Non-Goals

Not in this spec:

- `Duration` field on `OpportunityWindow` (only bucket + urgency)
- `DecayShape` on horizons (linear/exponential/step)
- Absolute `expires_at` in the core model (only relative `HorizonExpiry`)
- Calendar-aware session resolver (Phase 3+ via trait swap)
- Workflow/governance language (operator layer, separate concern)
- Archetype promotion to first-class reasoning entity (memory only)

---

## Summary

The repo converges to this time language:

- **TimeScale** = compute time (pressure decay math)
- **HorizonBucket** = trading time (main enum for intent/case/outcome)
- **SessionPhase** = market time (context)
- **Urgency** = action time (how late is too late)

Intent produces a horizon profile, Case picks one primary rhythm, Outcome evaluates at aligned horizon with supplemental diagnostics, Memory learns on `(Intent, Bucket)` as main key with gated promotion of supplemental observations.

This resolves the BKNG exit-timing failure by giving every case a named rhythm and a clear expiry condition.
