# Resolution System — Unify Case Outcome Language

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Collapse Eden's 12 scattered outcome vocabularies (CaseRealizedOutcome booleans, EvaluationStatus, IntentExitKind, ExpectationViolationKind, HypothesisTrackStatus, AgentRecommendationResolution, UsResolvedTopologyOutcome, LineageOutcome, OutcomeLearningContext, HorizonResult, expectancy triplets, free-text invalidation conditions) into a single dual-layer Resolution System. Give operators a single "what did this case amount to" answer and give the learning loop enough structure to distinguish *intent wrong* from *horizon wrong* from *luck*.

**Architecture:** Two layers with orthogonal responsibilities.
- `HorizonResolution` (per-horizon, 4 kinds) answers "what did this window add up to"
- `CaseResolution` (per-case, 7 kinds) answers "what did this case add up to"
- Both share a common `ResolutionFinality` (Provisional / Final)
- Case-level resolution is mutable via single-direction upgrades, with an append-only `resolution_history` trail
- Wave 3's `EvaluationStatus` state machine is preserved (lifecycle, not semantics)

**Tech Stack:** Rust, `serde`, `rust_decimal`, existing SurrealDB persistence via `EdenStore`, existing Horizon System types from `src/ontology/horizon.rs`, existing Intent System types (`IntentExitKind`, `ExpectationViolation`)

---

## Core Principle

> `EvaluationStatus` is lifecycle. `HorizonResolution` is per-window semantics. `CaseResolution` is per-case semantics. The three layers never share vocabulary.
>
> Primary horizon writes the first case resolution. Each supplemental settlement can upgrade it — but only monotonically, and only if the current resolution is `Provisional`. Every upgrade appends a transition to an append-only history. `Final` is a terminal lock.

Four separated concepts:

| Concept | Purpose | Values |
|---------|---------|--------|
| `EvaluationStatus` | Technical lifecycle of a horizon evaluation record | Pending / Due / Resolved / EarlyExited |
| `HorizonResolutionKind` | Per-horizon semantic outcome | Confirmed / Exhausted / Invalidated / Fulfilled |
| `CaseResolutionKind` | Per-case aggregated semantic outcome | Confirmed / PartiallyConfirmed / Invalidated / Exhausted / ProfitableButLate / EarlyExited / StructurallyRightButUntradeable |
| `ResolutionFinality` | Can this resolution still be upgraded? | Provisional / Final |

**`EvaluationStatus::Expired` is renamed to `Due` in this spec** to avoid confusion with `HorizonResolutionKind::Exhausted`. The rename is **mandatory**, not cosmetic, and lands in Wave 1 of the migration.

---

## Core Types

### Shared finality

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionFinality {
    /// Can still be upgraded by later supplemental evidence.
    Provisional,
    /// Terminal. Either a hard falsifier triggered, all horizons have
    /// settled, or the resolution was set by operator override.
    Final,
}
```

### Resolution source (audit)

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResolutionSource {
    /// Classifier + aggregator produced this resolution automatically.
    Auto,
    /// Operator explicitly set this resolution via override API.
    /// Bypasses upgrade rules. Used for StructurallyRightButUntradeable.
    OperatorOverride,
}
```

### Horizon layer

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HorizonResolutionKind {
    /// Expected market move materialized within this horizon with
    /// strong follow-through. Matches the intent's original thesis.
    Confirmed,
    /// Horizon's window closed without meaningful movement.
    /// NOT a negative judgment — the intent simply didn't play out here.
    /// This is the conservative default when numeric rules don't
    /// strongly support confirmation.
    Exhausted,
    /// Within-window falsifier: high-magnitude expectation violation,
    /// intent exit signal declaring reversal/invalidation, or numeric
    /// evidence that the market moved strongly against the thesis.
    Invalidated,
    /// Intent's explicit completion signal fired. Strongest positive.
    Fulfilled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HorizonResolution {
    pub kind: HorizonResolutionKind,
    /// Whether a higher-level aggregator can override the verdict this
    /// horizon carries into the case layer. Hard falsifier → Final.
    /// Numeric fallback / window-only failure → Provisional.
    pub finality: ResolutionFinality,
    /// Human-readable classification tag. Includes source tag prefix:
    /// "hard_falsifier:...", "window_violation:...", "exit_signal_...",
    /// "numeric_confirmed", "numeric_no_follow_through", "numeric_default".
    pub rationale: String,
    /// What specifically triggered this resolution (violation falsifier
    /// id, exit signal trigger text). None for pure numeric fallback.
    pub trigger: Option<String>,
}
```

### Extended `HorizonEvaluationRecord`

Wave 3's record gains one field:

```rust
pub struct HorizonEvaluationRecord {
    pub record_id: String,
    pub setup_id: String,
    pub market: String,
    pub horizon: HorizonBucket,
    pub primary: bool,
    pub due_at: OffsetDateTime,
    pub status: EvaluationStatus,   // Wave 3, Expired renamed to Due
    pub result: Option<HorizonResult>,
    /// New in Resolution System.
    #[serde(default)]
    pub resolution: Option<HorizonResolution>,
}
```

**Consistency invariant** (enforced in settle helper):

| status | resolution | result |
|--------|-----------|--------|
| Pending | None | None |
| Due | None | Some (numerics computed, classifier not yet run) |
| Resolved | **Some** | Some |
| EarlyExited | **Some** | Some |

### Case layer

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CaseResolutionKind {
    /// All relevant horizons confirmed. Strongest positive.
    Confirmed,
    /// Some horizons confirmed, some exhausted. Intent was partially right.
    PartiallyConfirmed,
    /// Hard falsifier triggered OR all supplemental also failed.
    Invalidated,
    /// Nothing happened across the horizons. Neutral outcome.
    Exhausted,
    /// Primary horizon exhausted/failed, but a supplemental horizon
    /// later confirmed with positive return. Critical signal that
    /// horizon selection (not intent) was wrong.
    ProfitableButLate,
    /// Case was closed early (by operator or exit signal) before any
    /// horizon could settle on its own.
    EarlyExited,
    /// Intent direction was correct but market microstructure (liquidity,
    /// spread, execution slippage) made it untradeable. Only settable
    /// via operator override — classifier/aggregator never emit this.
    StructurallyRightButUntradeable,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseResolution {
    pub kind: CaseResolutionKind,
    /// Provisional until all horizons settle or a Final is locked in.
    pub finality: ResolutionFinality,
    /// One-line operator summary. Opaque string for now; a structured
    /// `reason_codes: Vec<String>` may be added in a future iteration.
    pub narrative: String,
    /// Aggregated net return across the case's horizons. Updates on upgrade.
    pub net_return: Decimal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CaseResolutionTransition {
    /// None on the first write (case had no prior resolution).
    pub from_kind: Option<CaseResolutionKind>,
    /// None on the first write, matching `from_kind`.
    pub from_finality: Option<ResolutionFinality>,
    pub to_kind: CaseResolutionKind,
    pub to_finality: ResolutionFinality,
    pub triggered_by_horizon: HorizonBucket,
    pub at: OffsetDateTime,
    pub reason: String,
}

pub struct CaseResolutionRecord {
    pub record_id: String,
    pub setup_id: String,
    pub market: String,
    pub symbol: Option<String>,
    pub primary_horizon: HorizonBucket,

    /// Current resolution. Upgraded in place (see upgrade rules).
    pub resolution: CaseResolution,

    /// How this current resolution was produced.
    pub resolution_source: ResolutionSource,

    /// Denormalized snapshot of the horizon resolutions used by the
    /// latest aggregator run. NOT source of truth — `horizon_evaluation`
    /// is. Kept for fast postmortem reads and diagnostics.
    pub horizon_resolution_snapshot: Vec<HorizonResolution>,

    /// Append-only history. Every upgrade (kind OR finality change)
    /// appends exactly one transition. Never rewritten or collapsed.
    pub resolution_history: Vec<CaseResolutionTransition>,

    pub created_at: OffsetDateTime,
    pub updated_at: OffsetDateTime,
}
```

### Upgrade rules

Only `Provisional` resolutions can be upgraded. `Final` is a terminal lock.

```
Exhausted              → ProfitableButLate    (supplemental Confirmed + net_return > 0)
Exhausted              → PartiallyConfirmed   (mixed supplementals)
Exhausted              → Confirmed            (all horizons eventually Confirmed)
PartiallyConfirmed     → Confirmed            (last supplemental Confirmed)
Invalidated(Prov)      → ProfitableButLate    (supplemental Confirmed + positive return)
Invalidated(Prov)      → PartiallyConfirmed   (supplemental mixed)
EarlyExited            → ProfitableButLate    (post-exit evidence shows confirmation)

(any kind, Provisional) → (same kind, Final)  — finality transition, always recorded
```

**Terminal locks:**

- `Invalidated(Final)` — **cannot upgrade**. Set when a hard falsifier (magnitude > 0.5) triggered or when IntentExitKind was Invalidated/Reversal.
- `Confirmed(Final)` — cannot change.
- `Fulfilled` at horizon layer maps to `CaseResolutionKind::Confirmed + Final` (Case layer has no `Fulfilled` kind).
- `StructurallyRightButUntradeable` — only via operator override, always Final.

**Upgrade gate invariant:**

```rust
// Both kind and finality must differ from current to register as an upgrade
if new.kind == current.kind && new.finality == current.finality {
    return Skip; // no-op
}
if current.finality == Final {
    return Reject; // terminal lock
}
if !is_valid_monotonic_upgrade(current.kind, new.kind) {
    return Reject; // downgrade attempt
}
// Accepted — append transition
```

### Legacy vocabulary migration (derive-only, no reverse write)

```
// CaseRealizedOutcome booleans → CaseResolutionKind
followed_through=true  + invalidated=false + net_return>0  → Confirmed (Provisional)
followed_through=false + invalidated=true  (any magnitude) → Invalidated (finality depends on magnitude)
followed_through=false + invalidated=false                 → Exhausted
structure_retained=true  + net_return>0                    → candidate for ProfitableButLate
                                                             (only if supplemental evidence corroborates)

// AgentRecommendationResolution.status: String → CaseResolutionKind
"hit"         → Confirmed
"miss"        → Invalidated (Provisional)
"wait_regret" → StructurallyRightButUntradeable
(Other strings → Exhausted as conservative default.)

// IntentExitKind → horizon classifier input (not direct mapping)
Fulfilled     → HorizonResolution::Fulfilled (Final)
Invalidated   → HorizonResolution::Invalidated (Final)
Reversal      → HorizonResolution::Invalidated (Final)
Absorbed      → HorizonResolution::Exhausted (Provisional)
Exhaustion    → HorizonResolution::Exhausted (Provisional)
Decay         → HorizonResolution::Exhausted (Provisional)
              (Decay never directly maps to ProfitableButLate. That's the
              aggregator's job — it needs cross-horizon evidence.)
```

---

## Data Flow

```
┌─────────────────────────────────────────────────────────┐
│ T0: Case created (Wave 3 already wires this)           │
│                                                          │
│   TacticalSetup + CaseHorizon ready                    │
│   persist_horizon_evaluations creates one Pending       │
│   HorizonEvaluationRecord per (primary + secondary)    │
│                                                          │
│   case_resolution table: no record yet                 │
└─────────────────────────────────────────────────────────┘
                        ↓
┌─────────────────────────────────────────────────────────┐
│ T1: Primary horizon due_at reached                     │
│                                                          │
│   settle_horizon_evaluation(primary_record):           │
│     1. Compute HorizonResult from tick history         │
│        (net_return, follow_through, resolved_at)       │
│     2. status = Resolved                                │
│     3. resolution = classify_horizon_resolution(       │
│          result, exit_signal, violations)              │
│     4. Write back HorizonEvaluationRecord              │
│                                                          │
│   Trigger case-level aggregator:                        │
│     case_resolution = aggregate_case_resolution(       │
│       primary: primary_horizon_resolution,             │
│       supplementals: &[],   // none settled yet        │
│       horizon_results,                                  │
│     )                                                   │
│                                                          │
│   upsert_case_resolution(setup_id, case_resolution):    │
│     Writes new CaseResolutionRecord:                   │
│       resolution_source = Auto                          │
│       resolution_history = [                            │
│         CaseResolutionTransition {                      │
│           from_kind: None, from_finality: None,        │
│           to_kind, to_finality,                         │
│           triggered_by_horizon: primary_bucket,        │
│           at, reason: "primary settled",               │
│         }                                               │
│       ]                                                 │
└─────────────────────────────────────────────────────────┘
                        ↓
┌─────────────────────────────────────────────────────────┐
│ T2+: Each supplemental horizon due_at reached          │
│                                                          │
│   Same settle pipeline as primary.                      │
│                                                          │
│   Re-run aggregator with all settled horizons:          │
│     new_case_resolution = aggregate_case_resolution(   │
│       primary, supplementals, horizon_results)         │
│                                                          │
│   apply_case_resolution_update(                         │
│     record, new_case_resolution,                        │
│     triggered_by: settling_horizon,                     │
│     reason: "supplemental settled",                     │
│   ):                                                    │
│     - If (new.kind == current.kind && same finality)    │
│       → skip                                            │
│     - If current.finality == Final                      │
│       → reject                                          │
│     - If downgrade attempt                              │
│       → reject                                          │
│     - Otherwise                                          │
│       → append transition, update resolution,          │
│         update horizon_resolution_snapshot,            │
│         update updated_at                               │
│                                                          │
│   Finality locks at case level:                         │
│     - Any supplemental comes in as Invalidated+Final   │
│       (hard falsifier) → lock Final Invalidated        │
│     - All horizons settled, no Pending remains         │
│       → finality = Final                                │
└─────────────────────────────────────────────────────────┘
                        ↓
┌─────────────────────────────────────────────────────────┐
│ Tn (optional): Operator intervenes                      │
│                                                          │
│   override_case_resolution(                             │
│     setup_id,                                           │
│     new_kind: StructurallyRightButUntradeable,          │
│     reason: "spread too wide to fill",                  │
│   ):                                                    │
│     - Bypass upgrade rules                              │
│     - resolution_source = OperatorOverride              │
│     - finality = Final                                  │
│     - Append transition with reason                     │
│                                                          │
│   This is the ONLY path that can produce                │
│   StructurallyRightButUntradeable.                      │
└─────────────────────────────────────────────────────────┘
                        ↓
┌─────────────────────────────────────────────────────────┐
│ Learning loop consumption                               │
│                                                          │
│   Read path: case_resolution table first.               │
│     If no case_resolution record → fallback to old      │
│     CaseRealizedOutcomeRecord boolean path.             │
│     NEVER merge both. No dual-read max.                 │
│                                                          │
│   Resolution kind → delta policy:                       │
│     Confirmed + Final             → +full credit        │
│     Confirmed + Provisional       → +half credit        │
│     Invalidated + Final           → −full debit         │
│     Invalidated + Provisional     → neutral (wait)     │
│     Exhausted                     → 0                   │
│     ProfitableButLate             → −bucket debit,      │
│                                     +intent credit     │
│                                     (horizon selection  │
│                                     wrong, intent fine) │
│     PartiallyConfirmed            → +partial credit     │
│     EarlyExited                   → 0                   │
│     StructurallyRightButUntradeable → 0                │
│                                                          │
│   DiscoveredArchetype main key: unchanged               │
│     (Intent, Bucket, Signature)                         │
│                                                          │
│   Archetype record gains outcome distribution fields:   │
│     confirmed_count, invalidated_count,                 │
│     profitable_but_late_count,                          │
│     partially_confirmed_count, exhausted_count,         │
│     early_exited_count, structurally_right_count       │
│                                                          │
│   On every case_resolution update, recompute the       │
│   distribution for ONLY the affected                    │
│   (Intent, Bucket, Signature) shard. Read from         │
│   source-of-truth (case_resolution table), not         │
│   naive increment — because upgrades can flip counts. │
└─────────────────────────────────────────────────────────┘
```

### Horizon classifier (locked rules)

Priority-ordered single-function classifier. Every branch is tested:

```rust
pub fn classify_horizon_resolution(
    result: &HorizonResult,
    exit: Option<IntentExitKind>,
    violations: &[ExpectationViolation],
) -> HorizonResolution {
    // Priority 1: Hard falsifier — high-magnitude expectation violation
    if let Some(hard) = violations.iter().find(|v| v.magnitude > dec!(0.5)) {
        return HorizonResolution {
            kind: HorizonResolutionKind::Invalidated,
            finality: ResolutionFinality::Final,
            rationale: format!("hard_falsifier: {}", hard.description),
            trigger: Some(hard.falsifier.clone()),
        };
    }

    // Priority 2: Intent exit signal
    match exit {
        Some(IntentExitKind::Fulfilled) => return HorizonResolution {
            kind: HorizonResolutionKind::Fulfilled,
            finality: ResolutionFinality::Final,
            rationale: "exit_signal_fulfilled".into(),
            trigger: None,
        },
        Some(IntentExitKind::Invalidated) | Some(IntentExitKind::Reversal) => {
            return HorizonResolution {
                kind: HorizonResolutionKind::Invalidated,
                finality: ResolutionFinality::Final,
                rationale: format!("exit_signal_{:?}", exit.unwrap()).to_lowercase(),
                trigger: None,
            };
        }
        Some(kind @ (IntentExitKind::Absorbed | IntentExitKind::Exhaustion | IntentExitKind::Decay)) => {
            return HorizonResolution {
                kind: HorizonResolutionKind::Exhausted,
                finality: ResolutionFinality::Provisional,
                rationale: format!("exit_signal_{:?}", kind).to_lowercase(),
                trigger: None,
            };
        }
        None => {}
    }

    // Priority 3: Weak violation fallback (window-level, can be upgraded)
    if let Some(soft) = violations.iter().find(|v| v.magnitude > dec!(0.2)) {
        return HorizonResolution {
            kind: HorizonResolutionKind::Invalidated,
            finality: ResolutionFinality::Provisional,
            rationale: format!("window_violation: {}", soft.description),
            trigger: Some(soft.falsifier.clone()),
        };
    }

    // Priority 4: Pure numeric rule
    if result.follow_through >= dec!(0.6) && result.net_return > dec!(0) {
        return HorizonResolution {
            kind: HorizonResolutionKind::Confirmed,
            finality: ResolutionFinality::Provisional,
            rationale: "numeric_confirmed".into(),
            trigger: None,
        };
    }

    if result.follow_through < dec!(0.2) {
        return HorizonResolution {
            kind: HorizonResolutionKind::Exhausted,
            finality: ResolutionFinality::Provisional,
            rationale: "numeric_no_follow_through".into(),
            trigger: None,
        };
    }

    // Priority 5: Conservative default — Exhausted, NOT Invalidated
    HorizonResolution {
        kind: HorizonResolutionKind::Exhausted,
        finality: ResolutionFinality::Provisional,
        rationale: "numeric_default".into(),
        trigger: None,
    }
}
```

### Case aggregator (locked rules)

```rust
pub fn aggregate_case_resolution(
    primary: &HorizonResolution,
    supplementals: &[HorizonResolution],
    horizon_results: &[(HorizonBucket, HorizonResult)],
    all_settled: bool,
) -> CaseResolution {
    let net_return = compute_aggregate_net_return(horizon_results);

    // 1. Hard falsifier anywhere → Final Invalidated
    if primary.finality == ResolutionFinality::Final
        && primary.kind == HorizonResolutionKind::Invalidated
    {
        return invalidated_final(primary, net_return);
    }
    if supplementals.iter().any(|s| {
        s.finality == ResolutionFinality::Final && s.kind == HorizonResolutionKind::Invalidated
    }) {
        return invalidated_final(primary, net_return);
    }

    // 2. Any Fulfilled → Final Confirmed (Case layer has no Fulfilled kind)
    if primary.kind == HorizonResolutionKind::Fulfilled
        || supplementals.iter().any(|s| s.kind == HorizonResolutionKind::Fulfilled)
    {
        return CaseResolution {
            kind: CaseResolutionKind::Confirmed,
            finality: ResolutionFinality::Final,
            narrative: "intent explicitly fulfilled".into(),
            net_return,
        };
    }

    // 3. All horizons Confirmed → Final Confirmed
    let confirmed_count = std::iter::once(primary)
        .chain(supplementals.iter())
        .filter(|h| h.kind == HorizonResolutionKind::Confirmed)
        .count();
    let total = 1 + supplementals.len();
    if confirmed_count == total && all_settled {
        return CaseResolution {
            kind: CaseResolutionKind::Confirmed,
            finality: ResolutionFinality::Final,
            narrative: format!("all {total} horizons confirmed"),
            net_return,
        };
    }

    // 4. Primary Exhausted/Provisional-Invalidated but supplemental
    //    Confirmed with positive return → ProfitableButLate
    let primary_can_be_overridden = matches!(
        primary.kind,
        HorizonResolutionKind::Exhausted | HorizonResolutionKind::Invalidated,
    ) && primary.finality == ResolutionFinality::Provisional;

    if primary_can_be_overridden {
        let supp_confirmed_positive = supplementals.iter().zip(horizon_results.iter().skip(1))
            .any(|(sup, (_, result))| {
                sup.kind == HorizonResolutionKind::Confirmed && result.net_return > Decimal::ZERO
            });
        if supp_confirmed_positive {
            return CaseResolution {
                kind: CaseResolutionKind::ProfitableButLate,
                finality: if all_settled { ResolutionFinality::Final } else { ResolutionFinality::Provisional },
                narrative: "primary exhausted, supplemental later confirmed".into(),
                net_return,
            };
        }
    }

    // 5. Mix of Confirmed + Exhausted → PartiallyConfirmed
    if confirmed_count > 0 {
        return CaseResolution {
            kind: CaseResolutionKind::PartiallyConfirmed,
            finality: if all_settled { ResolutionFinality::Final } else { ResolutionFinality::Provisional },
            narrative: format!("{confirmed_count}/{total} horizons confirmed"),
            net_return,
        };
    }

    // 6. All Exhausted or Provisional Invalidated → Exhausted
    CaseResolution {
        kind: CaseResolutionKind::Exhausted,
        finality: if all_settled { ResolutionFinality::Final } else { ResolutionFinality::Provisional },
        narrative: "no horizon confirmed".into(),
        net_return,
    }
}
```

### Key invariants

1. **Classifier and aggregator are pure functions.** Same inputs always produce same output. No clock, no I/O, no shared state. Unit-testable.
2. **Only the classifier can emit `Final Invalidated`** (hard violation or exit signal). The aggregator can propagate Final into the case layer but cannot create it from nothing.
3. **Only the aggregator can emit `ProfitableButLate` / `PartiallyConfirmed`.** The classifier sees one horizon at a time and cannot infer cross-horizon patterns.
4. **`apply_case_resolution_update` is the single choke point** for all case-resolution writes. It enforces finality lock, downgrade rejection, no-op skip, transition append, and timestamp update atomically.
5. **`resolution_history` is append-only.** Never rewritten, never collapsed. Postmortem and horizon-mismatch analysis depend on this.
6. **`StructurallyRightButUntradeable` is operator-only.** Automated path never produces it.

---

## Migration Path

### Wave 1 — Type foundation + `Expired → Due` rename

**Two separate commits:**

**Commit A: Rename `EvaluationStatus::Expired` to `Due`**
- Full-repo search + replace
- Verify with `grep -rn 'Expired' src/ --type rust` — only doc comments should remain
- `cargo check --lib` + `cargo check --bin eden` + full test suite green
- No behavior change

**Commit B: Introduce Resolution types**
- New file `src/ontology/resolution.rs` (~500 lines, single file, follows Horizon Wave 1 pattern)
- New file `src/persistence/case_resolution.rs` (~250 lines, persistence record + round-trip tests)
- Register new modules in `src/ontology/mod.rs` and `src/persistence/mod.rs`
- `HorizonResolutionKind`, `CaseResolutionKind`, `ResolutionFinality`, `ResolutionSource`, `HorizonResolution`, `CaseResolution`, `CaseResolutionTransition`, `CaseResolutionRecord`
- `classify_horizon_resolution` — pure function, all branches tested
- `aggregate_case_resolution` — pure function, all paths tested
- `apply_case_resolution_update` — upgrade gate, all rules tested

**Rules:**
- Zero existing behavior touched (no reads, no writes in production code paths)
- All new types compile, all new tests pass
- Single file per module — do not pre-split

**Exit criteria:**
- `cargo check --lib` clean
- `cargo check --bin eden` clean
- `grep -rn 'Expired' src/ --type rust` empty except doc comments
- ~30 new resolution tests pass
- Single rename commit + single type introduction commit

### Wave 2 — Horizon-layer wiring

**Scope:**
- `HorizonEvaluationRecord.resolution: Option<HorizonResolution>` new field with `#[serde(default)]`
- Horizon settle path calls `classify_horizon_resolution` when status transitions Due → Resolved or into EarlyExited
- Consistency invariant enforced: `Resolved`/`EarlyExited` must have `Some(resolution)`
- Legacy records (no `resolution` field in JSON) deserialize with `None` — lazy migration, no rewrite

**Rules:**
- Learning loop does **not** read `HorizonEvaluationRecord.resolution` yet
- Old `CaseRealizedOutcomeRecord` path still active
- New field purely additive, no code reads it at case level yet

**Exit criteria:**
- New horizon settles produce records with `resolution = Some(...)`
- Legacy records still deserialize
- `cargo test --lib persistence::horizon_evaluation` green
- Wave 3 data flow from the Horizon System unbroken

### Wave 3 — Case-layer + new `case_resolution` table

**Scope:**
- `upsert_case_resolution` called after each horizon settle (primary writes first version, supplementals trigger upgrade attempts)
- New `EdenStore::write_case_resolutions` / `load_case_resolution_for_setup`
- `src/persistence/schema.rs` registers `case_resolution` SurrealDB table (schemaless)
- `apply_case_resolution_update` is the single choke point for all case-resolution writes
- Integration test: BKNG-style flow (Fast5m primary → Mid30m supplemental later confirms → ProfitableButLate) passes end-to-end

**Rules:**
- All new cases get a `CaseResolutionRecord` on primary settle
- Every upgrade appends one transition with both `from_kind` and `from_finality` recorded
- `resolution_history` is immutable (regression test: a rewriting test fails)
- `StructurallyRightButUntradeable` does not appear in this wave's automated path
- Race-condition safety: concurrent supplemental settles serialize through the choke point

**Exit criteria:**
- New cases have `case_resolution` records visible in SurrealDB
- BKNG regression test passes
- `resolution_history` append-only regression test passes
- Finality lock test passes (Final + Confirmed cannot be changed)

### Wave 4 — Learning loop switch + archetype distribution

**Scope:**
- `ReasoningLearningFeedback` reads `case_resolution` table. If no record exists for a setup, fall back to the old `CaseRealizedOutcomeRecord` boolean path.
- **Never merge both paths.** No `max()`, no average. Read one source.
- `DiscoveredArchetypeRecord` gains outcome distribution fields:

  ```rust
  pub confirmed_count: u64,
  pub invalidated_count: u64,
  pub profitable_but_late_count: u64,
  pub partially_confirmed_count: u64,
  pub exhausted_count: u64,
  pub early_exited_count: u64,
  pub structurally_right_count: u64,
  ```

  All `#[serde(default)]` so existing archetypes load as zero.
- **Main key unchanged**: `(Intent, Bucket, Signature)`. Resolution is a distribution statistic, not a key dimension.
- On every `case_resolution` update, recompute the distribution for ONLY the affected `(Intent, Bucket, Signature)` shard. Re-read the source of truth (`case_resolution` table). Do **not** naive-increment — upgrades flip counts.
- Resolution kind → delta policy is locked in a single dispatch function.

**Rules:**
- Learning loop always prefers new path when both exist
- Archetype recomputation scoped to a single shard per update
- `horizon_adjustments` sample gate (50/100 from Horizon Wave 3) still applies
- `ProfitableButLate` splits the delta: bucket side is debited, intent side is credited

**Exit criteria:**
- Learning reads case_resolution as primary source
- Archetype distribution fields populated correctly after case updates
- Upgrade flipping test passes (Exhausted → ProfitableButLate flips counts by −1 / +1)
- `ProfitableButLate` learning test verifies bucket debit + intent credit

### Wave 5 — Operator override + legacy cleanup

**Scope:**
- `override_case_resolution(setup_id, kind, reason)` function + HTTP/CLI handler
- Bypasses upgrade rules, sets `source = OperatorOverride`, sets `finality = Final`
- Non-empty `reason` required — validated before write
- Audit log: every override writes a transition with `reason` prefixed `"operator_override: ..."`
- `CaseRealizedOutcomeRecord.followed_through / invalidated / structure_retained` marked deprecated in doc comments (not deleted — MFE/MAE/return stay)
- `AgentRecommendationResolution.status: String` migrated to `Option<CaseResolutionKind>` via helper

**Exit criteria:**
- Operator can manually set `StructurallyRightButUntradeable`
- Empty `reason` is rejected
- Audit transitions visible in `resolution_history`
- Full test suite green
- `git tag resolution-complete`

### Unaffected by this spec

| System | Why |
|--------|-----|
| `CaseRealizedOutcomeRecord.net_return / MFE / MAE / return_pct` | Numeric facts layer — Resolution does not replace it |
| `EvaluationStatus` state machine | Wave 3 lifecycle, only the `Expired → Due` rename touches it |
| `IntentExitKind` enum | Input to classifier, not part of Resolution |
| `ExpectationViolation` enum | Input to classifier, not part of Resolution |
| `HypothesisTrackStatus` | Per-tick tracking, orthogonal to case-level Resolution |
| `HorizonBucket` / `SessionPhase` / `CaseHorizon` | Horizon System (Wave 4 done), unchanged |
| `DiscoveredArchetype` main key shape | Stays `(Intent, Bucket, Signature)` — Resolution is distribution, not key |

---

## Testing Strategy

### Wave 1 tests — pure type layer

```rust
// Type-level sanity
#[test] fn horizon_resolution_kind_snake_case_json() { /* ... */ }
#[test] fn case_resolution_kind_has_seven_variants() { /* ... */ }
#[test] fn resolution_finality_snake_case_json() { /* ... */ }
#[test] fn resolution_source_snake_case_json() { /* ... */ }

#[test]
fn case_resolution_transition_from_kind_can_be_none() {
    let t = CaseResolutionTransition {
        from_kind: None,
        from_finality: None,
        to_kind: CaseResolutionKind::Exhausted,
        to_finality: ResolutionFinality::Provisional,
        triggered_by_horizon: HorizonBucket::Fast5m,
        at: OffsetDateTime::UNIX_EPOCH,
        reason: "test".into(),
    };
    let json = serde_json::to_string(&t).unwrap();
    let parsed: CaseResolutionTransition = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed.from_kind, None);
    assert_eq!(parsed.from_finality, None);
}

// Classifier — one test per priority branch
#[test] fn classify_hard_falsifier_returns_final_invalidated() { /* ... */ }
#[test] fn classify_exit_fulfilled_returns_final_fulfilled() { /* ... */ }
#[test] fn classify_exit_invalidated_returns_final_invalidated() { /* ... */ }
#[test] fn classify_exit_reversal_returns_final_invalidated() { /* ... */ }
#[test] fn classify_exit_absorbed_returns_provisional_exhausted() { /* ... */ }
#[test] fn classify_exit_decay_returns_provisional_exhausted() { /* ... */ }
#[test] fn classify_weak_violation_returns_provisional_invalidated() { /* ... */ }
#[test] fn classify_numeric_confirmed_returns_provisional_confirmed() { /* ... */ }
#[test] fn classify_numeric_no_follow_through_returns_provisional_exhausted() { /* ... */ }
#[test]
fn classify_default_fallback_is_exhausted_not_invalidated() {
    // Conservative — default must be Exhausted
    let result = HorizonResult {
        net_return: dec!(0.0),
        follow_through: dec!(0.4),
        resolved_at: OffsetDateTime::UNIX_EPOCH,
    };
    let r = classify_horizon_resolution(&result, None, &[]);
    assert_eq!(r.kind, HorizonResolutionKind::Exhausted);
    assert_eq!(r.finality, ResolutionFinality::Provisional);
}

// Aggregator — one test per locked path
#[test]
fn aggregate_single_horizon_confirmed_all_settled_is_final() {
    // Case with only a primary horizon (no supplemental), Confirmed,
    // all_settled=true → Final. This is the degenerate case.
}
#[test] fn aggregate_all_horizons_confirmed_all_settled_is_final_confirmed() { /* ... */ }
#[test]
fn aggregate_all_confirmed_but_not_all_settled_stays_provisional() {
    // 2/3 settled and all 2 are Confirmed, but one Pending → still Provisional
}
#[test] fn aggregate_primary_exhausted_supplemental_confirmed_is_profitable_but_late() { /* ... */ }
#[test] fn aggregate_mix_confirmed_exhausted_is_partially_confirmed() { /* ... */ }
#[test] fn aggregate_primary_hard_invalidated_is_final_invalidated() { /* ... */ }
#[test] fn aggregate_primary_provisional_invalidated_supplemental_confirmed_is_profitable_but_late() { /* ... */ }
#[test]
fn aggregate_any_fulfilled_maps_to_final_confirmed() {
    // Case layer has no Fulfilled kind — must map to Confirmed + Final + narrative
}
#[test] fn aggregate_all_exhausted_all_settled_is_final_exhausted() { /* ... */ }
#[test] fn aggregate_all_exhausted_with_pending_is_provisional_exhausted() { /* ... */ }

// Upgrade gate
#[test] fn apply_update_rejects_downgrade() { /* Confirmed → Exhausted */ }
#[test] fn apply_update_rejects_final_change() { /* Final anything → anything */ }
#[test]
fn apply_update_allows_provisional_to_final_same_kind() {
    // Confirmed(Provisional) → Confirmed(Final)
    // Must register as transition, not a no-op
}
#[test] fn apply_update_appends_transition_on_every_change() { /* ... */ }
#[test] fn apply_update_never_rewrites_history() {
    // Regression test: after multiple updates, history.len() is monotonically
    // increasing and prior entries are byte-for-byte identical
}

// Rename gate
#[test]
fn evaluation_status_has_due_not_expired() {
    let status = EvaluationStatus::Due;
    let json = serde_json::to_string(&status).unwrap();
    assert_eq!(json, "\"due\"");
}
```

### Wave 2 tests — horizon settle + classifier wiring

```rust
#[test] fn settle_horizon_writes_resolution_when_resolved() { /* ... */ }
#[test] fn settle_horizon_early_exit_writes_resolution() { /* ... */ }
#[test]
fn legacy_record_without_resolution_field_deserializes_with_none() {
    // Raw JSON with no `resolution` field still loads
}
#[test] fn pending_record_has_no_resolution() { /* consistency invariant */ }
#[test] fn due_record_has_no_resolution() { /* consistency invariant */ }
```

### Wave 3 tests — case aggregator + new table

```rust
#[test] fn primary_settle_creates_case_resolution_record() {
    // First write, from_kind=None transition
}
#[test] fn supplemental_settle_upgrades_case_resolution() {
    // Fast5m Exhausted → Mid30m Confirmed → case = ProfitableButLate
    // history has 2 transitions
}
#[test] fn case_resolution_record_roundtrip_with_source() { /* ... */ }
#[test]
fn case_resolution_history_append_only_regression() {
    // Load record, mutate in memory, save, load again, verify
    // history was not collapsed
}
#[test]
fn race_condition_concurrent_supplementals_serialize_atomically() {
    // Two settles hitting apply_case_resolution_update simultaneously
    // must produce exactly two transitions, not clobber each other
}
```

### Wave 4 tests — learning loop switch + archetype distribution

```rust
#[test] fn learning_reads_case_resolution_first() { /* ... */ }
#[test] fn learning_falls_back_to_legacy_when_no_case_resolution() { /* ... */ }
#[test]
fn learning_never_merges_two_paths() {
    // Setup with BOTH legacy and new records:
    // Assert learning only uses one, not a combination
}
#[test]
fn profitable_but_late_debits_bucket_credits_intent() {
    // (intent, wrong_bucket) delta < 0
    // (intent, right_bucket) delta > 0
    // No double-counting
}
#[test] fn confirmed_final_gives_full_credit() { /* ... */ }
#[test] fn confirmed_provisional_gives_half_credit() { /* ... */ }
#[test] fn invalidated_provisional_is_neutral() { /* ... */ }
#[test] fn invalidated_final_is_full_debit() { /* ... */ }
#[test] fn exhausted_gives_zero_delta() { /* ... */ }

#[test]
fn archetype_recompute_only_affected_shard() {
    // Update one case's resolution — only (intent_A, Fast5m, sig_A) recomputes
    // (intent_B, *, *) archetypes unchanged
}
#[test]
fn archetype_recompute_handles_upgrade_correctly() {
    // Case upgraded Exhausted → ProfitableButLate
    // Affected archetype: exhausted_count goes from N → N−1,
    //                     profitable_but_late_count goes from M → M+1
    // Because recompute reads from source of truth, not increments
}
#[test]
fn archetype_main_key_still_three_dimensional() {
    // Regression: Resolution must not appear in the archetype key
}
```

### Wave 5 tests — operator override + cleanup

```rust
#[test]
fn operator_override_bypasses_upgrade_rules() {
    // Start with Final Confirmed
    // Override to StructurallyRightButUntradeable
    // Assert the change is accepted despite Final lock
}
#[test] fn operator_override_sets_finality_final() { /* ... */ }
#[test] fn operator_override_appends_transition_with_reason() { /* ... */ }
#[test] fn operator_override_rejects_empty_reason() { /* ... */ }
#[test]
fn only_operator_override_produces_structurally_right_untradeable() {
    // Exhaustive: run classifier and aggregator across many inputs,
    // assert kind is never StructurallyRightButUntradeable
}
#[test] fn legacy_boolean_fields_still_readable() {
    // CaseRealizedOutcomeRecord still compiles and loads
}
```

### End-to-end BKNG regression

```rust
#[test]
fn end_to_end_bkng_regression_through_resolution_system() {
    // T0: Pressure field detects BKNG-style vortex, builds TacticalSetup
    //     CaseHorizon = { primary: Fast5m, secondary: [Mid30m, Session] }
    //     persist_horizon_evaluations creates 3 Pending records
    //
    // T1 (5 min): Fast5m settles
    //     HorizonResult { net_return=+0.015, follow_through=0.75 }
    //     classify → Confirmed(Provisional), rationale=numeric_confirmed
    //     aggregate → Confirmed(Provisional)
    //     write CaseResolutionRecord
    //     history = [None → Confirmed(Provisional)]
    //
    // T2 (35 min): Mid30m settles
    //     HorizonResult { net_return=+0.022, follow_through=0.8 }
    //     classify → Confirmed(Provisional)
    //     aggregate → still Confirmed(Provisional) — no change yet
    //     upgrade gate skips (same kind, same finality)
    //     history = [None → Confirmed(Provisional)] (unchanged)
    //
    // T3 (6h): Session settles
    //     HorizonResult { net_return=+0.005, follow_through=0.3 }
    //     classify → Exhausted(Provisional), rationale=numeric_no_follow_through
    //     aggregate → PartiallyConfirmed (2/3 confirmed)
    //     all_settled == true → finality = Final
    //     apply_update appends transition
    //     history = [
    //       None → Confirmed(Provisional),
    //       Confirmed(Provisional) → PartiallyConfirmed(Final),
    //     ]
    //
    // T4: Learning loop reads case_resolution
    //     resolution.kind = PartiallyConfirmed(Final)
    //     delta = partial credit on (DirectionalAccumulation, Fast5m)
    //     archetype_recompute_shard triggered
    //     archetype (DA, Fast5m, bkng_sig):
    //       partially_confirmed_count: N → N+1
    //
    // Assertions:
    //   - No ProfitableButLate (primary was Confirmed, not Exhausted)
    //   - history includes a Provisional → Final transition
    //   - archetype key is (DirectionalAccumulation, Fast5m, bkng_sig)
    //     — Resolution is not in the key
    //   - archetype distribution field updated via shard recompute
    //
    // This locks in the Horizon + Resolution combined flow for BKNG.
}
```

---

## Non-Goals

Not in this spec:

- Auto-detection of `StructurallyRightButUntradeable` (operator-only in Wave 5, possibly auto in a future iteration once execution data is rich)
- Structured `reason_codes: Vec<String>` on `CaseResolution` (single `String` narrative for now, may add later)
- Rewriting `CaseRealizedOutcomeRecord` — numeric facts layer stays
- Resolution as a first-class archetype key dimension (distribution only, for now)
- Actor / Source Process layer (deeper intent typing — separate future spec)
- Expression layer (how to execute an intent — separate future spec)

---

## Summary

Eden's outcome language now has three orthogonal layers:

- **`EvaluationStatus`** answers *"what lifecycle stage is this horizon in?"* (Pending / Due / Resolved / EarlyExited)
- **`HorizonResolution`** answers *"what did this window amount to?"* (Confirmed / Exhausted / Invalidated / Fulfilled, each with finality)
- **`CaseResolution`** answers *"what did this case amount to?"* (7 kinds, mutable via monotonic upgrade, append-only history)

The primary horizon writes the first case resolution. Each supplemental settlement can upgrade — but only if the current resolution is `Provisional`, only monotonically, and only via the single choke-point `apply_case_resolution_update` which enforces both kind and finality transitions. `Final` is a terminal lock.

Learning reads `case_resolution` as primary source with a fallback to the legacy boolean path. Never merges both. Resolution kind determines credit/debit direction: `ProfitableButLate` specifically blames the bucket and credits the intent. `DiscoveredArchetype` keeps its `(Intent, Bucket, Signature)` main key and gains outcome distribution counts, recomputed from source of truth per affected shard on every case update.

This resolves the gap left by Horizon System Wave 3: the learning loop was built but had no semantic structure to learn from. After Resolution System lands, the learning loop can distinguish *intent wrong* from *horizon wrong* from *luck* — which is the real edge of having a reasoning system in the first place.
