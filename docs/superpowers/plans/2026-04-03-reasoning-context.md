# ReasoningContext Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Wire Eden's computed intelligence (convergence components, attribution, absence memory, world state, family boost) into the reasoning pipeline through a unified `ReasoningContext` struct.

**Architecture:** Extract a shared `ReasoningContext<'a>` (immutable per-tick snapshot) that replaces 4+ scattered parameters. Runtime assembles it each tick from stateful owners (`AbsenceMemory`, `FamilyBoostLedger`) and existing data (`DecisionSnapshot.convergence_scores`, `WorldStateSnapshot`, `ReviewerDoctrinePressure`). Five consumption points in the pipeline read from it.

**Tech Stack:** Rust, rust_decimal, time, existing Eden ontology/pipeline crates

**Spec:** `docs/superpowers/specs/2026-04-03-reasoning-context-design.md`

---

### Task 1: Create `context.rs` — ReasoningContext, AbsenceMemory, ConvergenceDetail, FamilyBoostLedger

**Files:**
- Create: `src/pipeline/reasoning/context.rs`
- Modify: `src/pipeline/reasoning.rs` (add `mod context` + re-exports)

- [ ] **Step 1: Write AbsenceMemory tests**

In `src/pipeline/reasoning/context.rs`, add:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use time::OffsetDateTime;

    #[test]
    fn absence_memory_suppresses_after_3_consecutive() {
        let mut mem = AbsenceMemory::default();
        let sector = crate::ontology::objects::SectorId("tech".into());
        let now = OffsetDateTime::now_utc();
        // Three consecutive recordings
        mem.record_absence(&sector, "Propagation Chain", 1, now);
        mem.record_absence(&sector, "Propagation Chain", 2, now);
        assert!(!mem.should_suppress(&sector, "Propagation Chain"));
        mem.record_absence(&sector, "Propagation Chain", 3, now);
        assert!(mem.should_suppress(&sector, "Propagation Chain"));
    }

    #[test]
    fn absence_memory_clears_on_propagation() {
        let mut mem = AbsenceMemory::default();
        let sector = crate::ontology::objects::SectorId("tech".into());
        let now = OffsetDateTime::now_utc();
        mem.record_absence(&sector, "Propagation Chain", 1, now);
        mem.record_absence(&sector, "Propagation Chain", 2, now);
        mem.record_absence(&sector, "Propagation Chain", 3, now);
        assert!(mem.should_suppress(&sector, "Propagation Chain"));
        mem.record_propagation(&sector);
        assert!(!mem.should_suppress(&sector, "Propagation Chain"));
    }

    #[test]
    fn absence_memory_decays_after_30_min() {
        let mut mem = AbsenceMemory::default();
        let sector = crate::ontology::objects::SectorId("tech".into());
        let now = OffsetDateTime::now_utc();
        let old = now - time::Duration::minutes(31);
        mem.record_absence(&sector, "Propagation Chain", 1, old);
        mem.record_absence(&sector, "Propagation Chain", 2, old);
        mem.record_absence(&sector, "Propagation Chain", 3, old);
        mem.decay(now);
        assert!(!mem.should_suppress(&sector, "Propagation Chain"));
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p eden --lib -- reasoning::context::tests -q`
Expected: compilation error — `AbsenceMemory` not defined yet.

- [ ] **Step 3: Implement AbsenceMemory**

At the top of `src/pipeline/reasoning/context.rs`:

```rust
use std::collections::HashMap;

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::graph::convergence::ConvergenceScore;
use crate::graph::decision::MarketRegimeFilter;
use crate::ontology::objects::{SectorId, Symbol};
use crate::ontology::world::WorldStateSnapshot;
use crate::pipeline::dimensions::SymbolDimensions;
use crate::temporal::lineage::{FamilyContextLineageOutcome, MultiHorizonGate};

use super::ReviewerDoctrinePressure;

// ── Absence Memory ──

#[derive(Debug, Clone)]
struct AbsenceKey {
    sector: SectorId,
    family: String,
}

#[derive(Debug, Clone)]
pub struct AbsenceEntry {
    pub consecutive_count: u32,
    pub last_seen: OffsetDateTime,
}

#[derive(Debug, Clone, Default)]
pub struct AbsenceMemory {
    entries: HashMap<(String, String), AbsenceEntry>,
}

impl AbsenceMemory {
    /// Record that a (sector, family) pair showed propagation absence this tick.
    pub fn record_absence(
        &mut self,
        sector: &SectorId,
        family: &str,
        _tick: u64,
        now: OffsetDateTime,
    ) {
        let key = (sector.0.clone(), family.to_ascii_lowercase());
        let entry = self.entries.entry(key).or_insert(AbsenceEntry {
            consecutive_count: 0,
            last_seen: now,
        });
        entry.consecutive_count += 1;
        entry.last_seen = now;
    }

    /// Clear absence tracking for a sector that DID propagate this tick.
    pub fn record_propagation(&mut self, sector: &SectorId) {
        self.entries
            .retain(|(sector_key, _), _| *sector_key != sector.0);
    }

    /// Should we suppress hypothesis generation for this (sector, family)?
    pub fn should_suppress(&self, sector: &SectorId, family: &str) -> bool {
        let key = (sector.0.clone(), family.to_ascii_lowercase());
        self.entries
            .get(&key)
            .map(|entry| entry.consecutive_count >= 3)
            .unwrap_or(false)
    }

    /// Remove entries older than 30 minutes.
    pub fn decay(&mut self, now: OffsetDateTime) {
        let cutoff = now - time::Duration::minutes(30);
        self.entries.retain(|_, entry| entry.last_seen >= cutoff);
    }
}
```

- [ ] **Step 4: Run AbsenceMemory tests**

Run: `cargo test -p eden --lib -- reasoning::context::tests::absence -q`
Expected: 3 tests pass.

- [ ] **Step 5: Write FamilyBoostLedger tests**

Append to the `tests` module in `context.rs`:

```rust
    #[test]
    fn family_boost_neutral_below_55_pct() {
        let priors = vec![make_prior("Directed Flow", dec!(0.50), dec!(0.01))];
        let ledger = FamilyBoostLedger::from_lineage_priors(&priors, "midday", "neutral");
        assert_eq!(ledger.boost_for_family("Directed Flow"), Decimal::ONE);
    }

    #[test]
    fn family_boost_caps_at_1_25() {
        let priors = vec![make_prior("Directed Flow", dec!(0.90), dec!(0.05))];
        let ledger = FamilyBoostLedger::from_lineage_priors(&priors, "midday", "neutral");
        assert_eq!(
            ledger.boost_for_family("Directed Flow"),
            Decimal::new(125, 2)
        );
    }

    #[test]
    fn family_boost_requires_positive_net_return() {
        let priors = vec![make_prior("Directed Flow", dec!(0.60), dec!(-0.01))];
        let ledger = FamilyBoostLedger::from_lineage_priors(&priors, "midday", "neutral");
        assert_eq!(ledger.boost_for_family("Directed Flow"), Decimal::ONE);
    }

    fn make_prior(
        family: &str,
        follow_through_rate: Decimal,
        mean_net_return: Decimal,
    ) -> FamilyContextLineageOutcome {
        FamilyContextLineageOutcome {
            family: family.into(),
            session: "midday".into(),
            market_regime: "neutral".into(),
            resolved: 30,
            mean_return: Decimal::ZERO,
            mean_net_return,
            mean_mfe: Decimal::ZERO,
            mean_mae: Decimal::ZERO,
            follow_through_rate,
            invalidation_rate: Decimal::ZERO,
            structure_retention_rate: Decimal::ZERO,
            mean_convergence_score: Decimal::ZERO,
            mean_external_delta: Decimal::ZERO,
            external_follow_through_rate: Decimal::ZERO,
            follow_expectancy: Decimal::ZERO,
            fade_expectancy: Decimal::ZERO,
            wait_expectancy: Decimal::ZERO,
        }
    }
```

Also add at top of test module: `use rust_decimal_macros::dec;`

- [ ] **Step 6: Implement FamilyBoostLedger**

Add after AbsenceMemory in `context.rs`:

```rust
// ── Family Boost Ledger ──

#[derive(Debug, Clone, Default)]
pub struct FamilyBoostLedger {
    boosts: HashMap<String, Decimal>,
}

impl FamilyBoostLedger {
    pub fn from_lineage_priors(
        priors: &[FamilyContextLineageOutcome],
        session: &str,
        regime: &str,
    ) -> Self {
        let mut boosts = HashMap::new();
        let families: std::collections::HashSet<String> =
            priors.iter().map(|p| p.family.clone()).collect();
        for family in families {
            let prior = super::family_gate::best_family_prior(priors, &family, session, regime);
            if let Some(prior) = prior {
                let boost = compute_family_boost(prior);
                if boost != Decimal::ONE {
                    boosts.insert(family.to_ascii_lowercase(), boost);
                }
            }
        }
        Self { boosts }
    }

    /// Returns boost factor: 1.0 = neutral, >1.0 = boosted. Never < 1.0.
    pub fn boost_for_family(&self, family: &str) -> Decimal {
        self.boosts
            .get(&family.to_ascii_lowercase())
            .copied()
            .unwrap_or(Decimal::ONE)
    }
}

fn compute_family_boost(prior: &FamilyContextLineageOutcome) -> Decimal {
    if prior.follow_through_rate < Decimal::new(55, 2) || prior.mean_net_return <= Decimal::ZERO {
        return Decimal::ONE;
    }
    let raw = Decimal::ONE
        + (prior.follow_through_rate - Decimal::new(50, 2)) * Decimal::new(5, 1);
    raw.min(Decimal::new(125, 2))
}
```

- [ ] **Step 7: Run FamilyBoostLedger tests**

Run: `cargo test -p eden --lib -- reasoning::context::tests::family_boost -q`
Expected: 3 tests pass.

- [ ] **Step 8: Add ConvergenceDetail and ReasoningContext**

Add after FamilyBoostLedger in `context.rs`:

```rust
// ── Convergence Detail ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConvergenceDetail {
    pub institutional_alignment: Decimal,
    pub sector_coherence: Option<Decimal>,
    pub cross_stock_correlation: Decimal,
    pub component_spread: Option<Decimal>,
    pub edge_stability: Option<Decimal>,
}

impl ConvergenceDetail {
    pub fn from_convergence_score(score: &ConvergenceScore) -> Self {
        Self {
            institutional_alignment: score.institutional_alignment,
            sector_coherence: score.sector_coherence,
            cross_stock_correlation: score.cross_stock_correlation,
            component_spread: score.component_spread,
            edge_stability: score.edge_stability,
        }
    }
}

// ── Reasoning Context ──

pub struct ReasoningContext<'a> {
    pub lineage_priors: &'a [FamilyContextLineageOutcome],
    pub multi_horizon_gate: Option<&'a MultiHorizonGate>,
    pub symbol_dimensions: Option<&'a HashMap<Symbol, SymbolDimensions>>,
    pub reviewer_doctrine: Option<&'a ReviewerDoctrinePressure>,
    pub convergence_components: &'a HashMap<Symbol, ConvergenceScore>,
    pub market_regime: &'a MarketRegimeFilter,
    pub world_state: Option<&'a WorldStateSnapshot>,
    pub absence_memory: &'a AbsenceMemory,
    pub family_boost: &'a FamilyBoostLedger,
}
```

Add `use serde::{Deserialize, Serialize};` to the top imports.

- [ ] **Step 9: Wire mod context into reasoning.rs**

In `src/pipeline/reasoning.rs`, after the existing `mod` declarations (around line 23-32), add:

```rust
#[path = "reasoning/context.rs"]
mod context;
pub use context::{AbsenceMemory, ConvergenceDetail, FamilyBoostLedger, ReasoningContext};
```

- [ ] **Step 10: Verify compilation**

Run: `cargo check --lib -q`
Expected: clean compilation (or warnings only).

- [ ] **Step 11: Run all context tests**

Run: `cargo test -p eden --lib -- reasoning::context::tests -q`
Expected: 6 tests pass.

- [ ] **Step 12: Commit**

```bash
git add src/pipeline/reasoning/context.rs src/pipeline/reasoning.rs
git commit -m "feat(reasoning): add ReasoningContext, AbsenceMemory, FamilyBoostLedger, ConvergenceDetail"
```

---

### Task 2: Extract shared `family_gate.rs` from HK support.rs

**Files:**
- Create: `src/pipeline/reasoning/family_gate.rs`
- Modify: `src/pipeline/reasoning.rs` (add `mod family_gate`)
- Modify: `src/pipeline/reasoning/support.rs` (remove moved code, import from family_gate)

- [ ] **Step 1: Create family_gate.rs with functions extracted from support.rs**

Create `src/pipeline/reasoning/family_gate.rs` with the following content, extracted verbatim from `src/pipeline/reasoning/support.rs` lines 710-787:

```rust
use std::collections::HashSet;

use rust_decimal::Decimal;

use crate::persistence::candidate_mechanism::CandidateMechanismRecord;
use crate::temporal::lineage::FamilyContextLineageOutcome;

// ── Family Alpha Gate (negative feedback) ──

pub(crate) struct FamilyAlphaGate {
    blocked: HashSet<String>,
}

impl FamilyAlphaGate {
    pub fn from_lineage_priors(
        priors: &[FamilyContextLineageOutcome],
        session: &str,
        regime: &str,
    ) -> Self {
        let families = priors
            .iter()
            .map(|prior| prior.family.clone())
            .collect::<HashSet<_>>();
        let blocked = families
            .into_iter()
            .filter(|family| {
                best_family_prior(priors, family, session, regime)
                    .map(should_block_family_alpha)
                    .unwrap_or(false)
            })
            .map(|family| family.to_ascii_lowercase())
            .collect();
        Self { blocked }
    }

    pub fn allows(&self, family: &str) -> bool {
        !self.blocked.contains(&family.to_ascii_lowercase())
    }
}

// ── Shared helper: best family prior lookup ──

pub(crate) fn best_family_prior<'a>(
    priors: &'a [FamilyContextLineageOutcome],
    family: &str,
    session: &str,
    regime: &str,
) -> Option<&'a FamilyContextLineageOutcome> {
    let best = |items: Vec<&'a FamilyContextLineageOutcome>| {
        items.into_iter().max_by(|left, right| {
            left.resolved
                .cmp(&right.resolved)
                .then_with(|| left.mean_net_return.cmp(&right.mean_net_return))
                .then_with(|| left.follow_through_rate.cmp(&right.follow_through_rate))
        })
    };

    best(
        priors
            .iter()
            .filter(|item| {
                item.family.eq_ignore_ascii_case(family)
                    && item.session.eq_ignore_ascii_case(session)
                    && item.market_regime.eq_ignore_ascii_case(regime)
            })
            .collect(),
    )
    .or_else(|| {
        best(
            priors
                .iter()
                .filter(|item| {
                    item.family.eq_ignore_ascii_case(family)
                        && item.session.eq_ignore_ascii_case(session)
                })
                .collect(),
        )
    })
    .or_else(|| {
        best(
            priors
                .iter()
                .filter(|item| item.family.eq_ignore_ascii_case(family))
                .collect(),
        )
    })
}

fn should_block_family_alpha(prior: &FamilyContextLineageOutcome) -> bool {
    if prior.resolved < 15 {
        return false;
    }

    if prior.follow_through_rate == Decimal::ZERO
        && prior.mean_net_return <= Decimal::ZERO
        && prior.resolved >= 15
    {
        return true;
    }

    if prior.resolved < 20 {
        return false;
    }

    let net_penalty = prior.mean_net_return * Decimal::new(200, 0);
    let follow_bonus = prior.follow_through_rate * Decimal::new(30, 0);
    let invalidation_penalty = prior.invalidation_rate * Decimal::new(30, 0);
    let score = net_penalty + follow_bonus - invalidation_penalty;

    let threshold = if prior.resolved >= 100 {
        Decimal::new(-2, 0)
    } else if prior.resolved >= 50 {
        Decimal::new(-3, 0)
    } else {
        Decimal::new(-5, 0)
    };
    score < threshold
}

// ── Candidate mechanism templates ──

pub fn templates_from_candidate_mechanisms(
    mechanisms: &[CandidateMechanismRecord],
) -> Vec<super::support::HypothesisTemplate> {
    mechanisms
        .iter()
        .filter(|mech| mech.mode == "live")
        .map(|mech| {
            let channels_label = mech.dominant_channels.join("+");
            super::support::HypothesisTemplate {
                key: format!("emergent:{}", mech.channel_signature),
                family_label: format!("Emergent({})", channels_label),
                thesis: format!(
                    "emergent {} pattern via {} (historically {:.1}% net return over {} samples)",
                    mech.center_kind,
                    channels_label,
                    mech.mean_net_return * Decimal::from(100),
                    mech.samples,
                ),
            }
        })
        .collect()
}
```

- [ ] **Step 2: Wire mod family_gate into reasoning.rs**

In `src/pipeline/reasoning.rs`, add after the context mod:

```rust
#[path = "reasoning/family_gate.rs"]
pub(crate) mod family_gate;
pub use family_gate::{templates_from_candidate_mechanisms, FamilyAlphaGate};
```

Remove the existing `pub use support::templates_from_candidate_mechanisms` line and `use support::FamilyAlphaGate` line.

- [ ] **Step 3: Update support.rs to import from family_gate**

In `src/pipeline/reasoning/support.rs`:

1. Remove the `FamilyAlphaGate` struct and impl (lines 115-144).
2. Remove `best_family_prior` function (lines 710-754).
3. Remove `should_block_family_alpha` function (lines 756-787).
4. Remove `templates_from_candidate_mechanisms` function (lines 302-323).
5. Remove `use crate::persistence::candidate_mechanism::CandidateMechanismRecord;` import.
6. Remove `use crate::temporal::lineage::FamilyContextLineageOutcome;` import.
7. Add at top: `pub(super) use super::family_gate::FamilyAlphaGate;`

The `hypothesis_templates` function still takes `family_gate: Option<&FamilyAlphaGate>` — this still works because FamilyAlphaGate is re-imported.

- [ ] **Step 4: Verify compilation**

Run: `cargo check --lib -q`
Expected: clean compilation.

- [ ] **Step 5: Run existing reasoning tests**

Run: `cargo test -p eden --lib -- reasoning -q`
Expected: all existing tests still pass (no behavior change).

- [ ] **Step 6: Commit**

```bash
git add src/pipeline/reasoning/family_gate.rs src/pipeline/reasoning.rs src/pipeline/reasoning/support.rs
git commit -m "refactor(reasoning): extract FamilyAlphaGate + helpers into shared family_gate.rs"
```

---

### Task 3: Add `ConvergenceDetail` to TacticalSetup

**Files:**
- Modify: `src/ontology/reasoning.rs` (add field to TacticalSetup)

- [ ] **Step 1: Add convergence_detail field to TacticalSetup**

In `src/ontology/reasoning.rs`, in the `TacticalSetup` struct (around line 201-228), add after `convergence_score`:

```rust
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub convergence_detail: Option<crate::pipeline::reasoning::ConvergenceDetail>,
```

- [ ] **Step 2: Add ConvergenceDisagreement to ReviewReasonCode**

Find the `ReviewReasonCode` enum in the same file and add a variant:

```rust
    ConvergenceDisagreement,
```

- [ ] **Step 3: Verify compilation**

Run: `cargo check --lib -q`
Expected: compilation may fail if TacticalSetup is constructed without the new field. Fix any construction sites by adding `convergence_detail: None`.

- [ ] **Step 4: Fix all TacticalSetup construction sites**

Search for places that construct TacticalSetup and add `convergence_detail: None`:
- `src/pipeline/reasoning/synthesis.rs` (in `derive_tactical_setups`)
- `src/us/pipeline/reasoning/policy.rs` (in US `derive_tactical_setups`)
- Any test files that construct TacticalSetup directly

- [ ] **Step 5: Verify compilation again**

Run: `cargo check --lib -q`
Expected: clean.

- [ ] **Step 6: Commit**

```bash
git add src/ontology/reasoning.rs src/pipeline/reasoning/synthesis.rs src/us/pipeline/reasoning/policy.rs
git commit -m "feat(ontology): add convergence_detail field to TacticalSetup"
```

---

### Task 4: Wire convergence components into `derive_tactical_setups`

**Files:**
- Modify: `src/pipeline/reasoning/synthesis.rs` (read ConvergenceScore, populate ConvergenceDetail)

- [ ] **Step 1: Write test for convergence detail population**

In `src/pipeline/reasoning/tests.rs`, add:

```rust
#[test]
fn tactical_setup_has_convergence_detail_when_components_available() {
    // This test verifies that derive_tactical_setups populates convergence_detail
    // from the full ConvergenceScore, not just the scalar.
    // Build a minimal DecisionSnapshot with a known ConvergenceScore,
    // then verify the resulting TacticalSetup has a non-None convergence_detail
    // with the expected institutional_alignment value.
    //
    // Implementation: construct test fixtures matching the existing test patterns
    // in this file, then assert setup.convergence_detail.is_some().
}
```

(Fill in the test body using the existing test fixture patterns already present in `src/pipeline/reasoning/tests.rs`.)

- [ ] **Step 2: Modify derive_tactical_setups to accept convergence_components**

In `src/pipeline/reasoning/synthesis.rs`, the `SetupSupportContext` struct (line 25-29) already has `insights`. Add a new field:

```rust
pub(super) struct SetupSupportContext<'a> {
    pub events: &'a EventSnapshot,
    pub insights: &'a GraphInsights,
    pub symbol_dimensions: Option<&'a HashMap<crate::ontology::objects::Symbol, SymbolDimensions>>,
    pub convergence_components: &'a HashMap<crate::ontology::objects::Symbol, crate::graph::convergence::ConvergenceScore>,
}
```

- [ ] **Step 3: Populate convergence_detail in the setup construction**

In `derive_tactical_setups`, around line 804 where `TacticalSetup` is constructed, change `convergence_detail: None` to:

```rust
convergence_detail: support_context
    .convergence_components
    .get(&suggestion.symbol)
    .map(crate::pipeline::reasoning::ConvergenceDetail::from_convergence_score),
```

- [ ] **Step 4: Update all callers of SetupSupportContext**

In `src/pipeline/reasoning.rs`, where `SetupSupportContext` is constructed (around lines 109-113 and 197-201), add the `convergence_components` field:

```rust
synthesis::SetupSupportContext {
    events,
    insights,
    symbol_dimensions,
    convergence_components: &decision.convergence_scores,
},
```

Note: `decision.convergence_scores` is `HashMap<Symbol, ConvergenceScore>` from `DecisionSnapshot`.

- [ ] **Step 5: Verify compilation and run tests**

Run: `cargo check --lib -q && cargo test -p eden --lib -- reasoning -q`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/pipeline/reasoning/synthesis.rs src/pipeline/reasoning.rs
git commit -m "feat(reasoning): populate ConvergenceDetail in tactical setups from graph components"
```

---

### Task 5: Convergence-informed policy rules

**Files:**
- Modify: `src/pipeline/reasoning/policy.rs`
- Modify: `src/pipeline/reasoning/tests.rs` (new tests)

- [ ] **Step 1: Write convergence policy tests**

In `src/pipeline/reasoning/tests.rs`:

```rust
#[test]
fn strong_consensus_promotes_observe_to_review() {
    // Build a TacticalSetup with action="observe" and convergence_detail showing:
    // institutional_alignment=0.62, component_spread=0.20 (strong consensus)
    // After apply_convergence_policy, action should be "review"
    let mut setup = make_observe_setup();
    setup.convergence_detail = Some(ConvergenceDetail {
        institutional_alignment: Decimal::new(62, 2),
        sector_coherence: Some(Decimal::new(50, 2)),
        cross_stock_correlation: Decimal::new(40, 2),
        component_spread: Some(Decimal::new(20, 2)),
        edge_stability: None,
    });
    let result = apply_convergence_policy(setup);
    assert_eq!(result.action, "review");
}

#[test]
fn high_spread_demotes_enter_to_review() {
    let mut setup = make_enter_setup();
    setup.convergence_detail = Some(ConvergenceDetail {
        institutional_alignment: Decimal::new(30, 2),
        sector_coherence: Some(Decimal::new(-20, 2)),
        cross_stock_correlation: Decimal::new(40, 2),
        component_spread: Some(Decimal::new(65, 2)),
        edge_stability: None,
    });
    let result = apply_convergence_policy(setup);
    assert_eq!(result.action, "review");
}

#[test]
fn no_convergence_detail_is_noop() {
    let setup = make_enter_setup(); // convergence_detail = None
    let result = apply_convergence_policy(setup.clone());
    assert_eq!(result.action, setup.action);
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p eden --lib -- reasoning::tests::strong_consensus -q`
Expected: FAIL — `apply_convergence_policy` not defined.

- [ ] **Step 3: Implement apply_convergence_policy in policy.rs**

In `src/pipeline/reasoning/policy.rs`, add a new public function:

```rust
pub(super) fn apply_convergence_policy(mut setups: Vec<TacticalSetup>) -> Vec<TacticalSetup> {
    for setup in &mut setups {
        let Some(detail) = setup.convergence_detail.as_ref() else {
            continue;
        };
        let spread = detail.component_spread.unwrap_or(Decimal::ZERO);

        // Rule 1: Strong consensus promotes observe → review
        let strong_consensus = detail.institutional_alignment.abs() > Decimal::new(5, 1)
            && spread < Decimal::new(3, 1);
        if strong_consensus && setup.action == "observe" {
            setup.action = "review".into();
            setup
                .risk_notes
                .push("promoted: strong institutional consensus".into());
        }

        // Rule 2: High disagreement demotes enter → review
        let high_disagreement = spread > Decimal::new(6, 1);
        if high_disagreement && setup.action == "enter" {
            setup.action = "review".into();
            setup.review_reason_code = Some(ReviewReasonCode::ConvergenceDisagreement);
            setup
                .risk_notes
                .push("demoted: convergence components disagree".into());
        }
    }
    setups
}
```

- [ ] **Step 4: Wire apply_convergence_policy into the pipeline**

In `src/pipeline/reasoning.rs`, in `derive_with_policy` and `derive_with_diffusion`, add after the existing `apply_track_action_policy` call (around line 131/224):

```rust
let tactical_setups = policy::apply_convergence_policy(tactical_setups);
```

- [ ] **Step 5: Run tests**

Run: `cargo test -p eden --lib -- reasoning -q`
Expected: all pass including new tests.

- [ ] **Step 6: Commit**

```bash
git add src/pipeline/reasoning/policy.rs src/pipeline/reasoning.rs src/pipeline/reasoning/tests.rs
git commit -m "feat(reasoning): add convergence-informed policy rules (consensus promotion, disagreement demotion)"
```

---

### Task 6: Family boost in policy layer

**Files:**
- Modify: `src/pipeline/reasoning/policy.rs` (read family_boost in decide_track_action)

- [ ] **Step 1: Write family boost policy test**

In `src/pipeline/reasoning/tests.rs`:

```rust
#[test]
fn family_boost_lowers_min_enter_edge() {
    // Verify that a family with high follow-through (boost=1.2) results in
    // a lower min_enter_edge threshold compared to baseline.
    // This is tested indirectly by checking that a setup that would normally
    // be "observe" gets promoted when family_boost is active.
}
```

- [ ] **Step 2: Add family_boost parameter to decide_track_action**

In `src/pipeline/reasoning/policy.rs`, modify `decide_track_action` signature (line 880) to accept the new parameter:

```rust
fn decide_track_action(
    setup: &TacticalSetup,
    track: &HypothesisTrack,
    previous_track: Option<&HypothesisTrack>,
    timestamp: OffsetDateTime,
    market_regime: &MarketRegimeFilter,
    lineage_priors: &[FamilyContextLineageOutcome],
    multi_horizon_gate: Option<&crate::temporal::lineage::MultiHorizonGate>,
    reviewer_doctrine: Option<&ReviewerDoctrinePressure>,
    family_boost: &crate::pipeline::reasoning::FamilyBoostLedger,
) -> TrackActionDecision {
```

At line 890, after `doctrine_pressure` computation, add:

```rust
    let boost_factor = family_boost.boost_for_family(
        setup_family_key(setup).unwrap_or("unknown"),
    );
```

Then modify `min_enter_edge` (line 901) to incorporate boost:

```rust
    let boost_edge_reduction = (boost_factor - Decimal::ONE) * Decimal::new(2, 2);
    let min_enter_edge = Decimal::new(3, 2) + doctrine_pressure * Decimal::new(2, 2)
        - alpha_boost * Decimal::new(1, 2)
        - boost_edge_reduction;
```

- [ ] **Step 3: Update apply_track_action_policy to pass family_boost**

In `apply_track_action_policy` (line 490), add `family_boost: &crate::pipeline::reasoning::FamilyBoostLedger` parameter, and pass it to `decide_track_action`.

- [ ] **Step 4: Update callers in reasoning.rs**

In `src/pipeline/reasoning.rs`, the calls to `apply_track_action_policy` need the extra parameter. For now, pass `&FamilyBoostLedger::default()` — this will be replaced when ReasoningContext is wired through in Task 8.

- [ ] **Step 5: Verify compilation and tests**

Run: `cargo check --lib -q && cargo test -p eden --lib -- reasoning -q`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/pipeline/reasoning/policy.rs src/pipeline/reasoning.rs
git commit -m "feat(reasoning): integrate FamilyBoostLedger into policy edge thresholds"
```

---

### Task 7: Attribution driver_kind + absence + world state filtering in hypothesis_templates

**Files:**
- Modify: `src/pipeline/reasoning/support.rs`
- Modify: `src/pipeline/reasoning/tests.rs`

- [ ] **Step 1: Write attribution filtering tests**

In `src/pipeline/reasoning/tests.rs`:

```rust
#[test]
fn company_specific_blocks_cross_scope_templates() {
    // Build events where all have EventPropagationScope::Local (company_specific)
    // Call hypothesis_templates with absence_memory=empty, world_state=None
    // Verify: institution_relay, shared_holder_spillover, propagation NOT in result
    // Verify: flow, liquidity, risk ARE in result
}

#[test]
fn no_attribution_allows_all() {
    // Build events with no propagation scope info (cold start)
    // All templates should be allowed
}

#[test]
fn stress_feedback_blocked_in_stabilizing() {
    // Build events that would trigger stress_feedback_loop
    // Set world_state with regime="stabilizing"
    // Verify: stress_feedback_loop NOT in templates
}

#[test]
fn stress_feedback_allowed_in_stress_regime() {
    // Same events but world_state regime="stress"
    // Verify: stress_feedback_loop IS in templates
}
```

- [ ] **Step 2: Add AbsenceMemory and world_state to hypothesis_templates**

In `src/pipeline/reasoning/support.rs`, modify `hypothesis_templates` signature (line 146):

```rust
pub(super) fn hypothesis_templates(
    relevant_events: &[&crate::ontology::Event<crate::pipeline::signals::MarketEventRecord>],
    relevant_signals: &[&crate::ontology::DerivedSignal<
        crate::pipeline::signals::DerivedSignalRecord,
    >],
    relevant_paths: &[&PropagationPath],
    family_gate: Option<&FamilyAlphaGate>,
    absence_memory: &super::context::AbsenceMemory,
    world_state: Option<&crate::ontology::world::WorldStateSnapshot>,
    current_scope: &ReasoningScope,
) -> Vec<HypothesisTemplate> {
```

- [ ] **Step 3: Add absence suppression logic**

After the `family_gate` retain (line 291-292), add:

```rust
    // Suppress propagation/spillover templates for sectors with repeated absence
    if let ReasoningScope::Sector(sector_id) = current_scope {
        templates.retain(|template| {
            let is_propagation = matches!(
                template.key.as_str(),
                "propagation"
                    | "shared_holder_spillover"
                    | "institution_relay"
                    | "sector_rotation_spillover"
                    | "sector_symbol_spillover"
                    | "cross_mechanism_chain"
                    | "stress_feedback_loop"
            );
            if is_propagation {
                !absence_memory.should_suppress(sector_id, &template.family_label)
            } else {
                true
            }
        });
    }
```

- [ ] **Step 4: Add world state regime filtering**

After absence suppression, add:

```rust
    // Block stress_feedback_loop in stabilizing regime
    if let Some(ws) = world_state {
        let is_stabilizing = ws
            .entities
            .iter()
            .any(|e| e.layer == crate::ontology::world::WorldLayer::Market && e.regime == "stabilizing");
        if is_stabilizing {
            templates.retain(|t| t.key != "stress_feedback_loop");
        }
    }
```

- [ ] **Step 5: Update callers of hypothesis_templates**

In `src/pipeline/reasoning/synthesis.rs`, the call to `hypothesis_templates` (line 132) needs the new parameters. Pass `&AbsenceMemory::default()`, `None`, and `&scope` for now:

```rust
let templates = hypothesis_templates(
    &relevant_events,
    &relevant_signals,
    &relevant_paths,
    family_gate,
    &crate::pipeline::reasoning::AbsenceMemory::default(),
    None,
    &scope,
);
```

This will be replaced when the full ReasoningContext is threaded through in Task 8.

- [ ] **Step 6: Run tests**

Run: `cargo check --lib -q && cargo test -p eden --lib -- reasoning -q`
Expected: all pass.

- [ ] **Step 7: Commit**

```bash
git add src/pipeline/reasoning/support.rs src/pipeline/reasoning/synthesis.rs src/pipeline/reasoning/tests.rs
git commit -m "feat(reasoning): add driver_kind, absence memory, and world state regime filtering to hypothesis_templates"
```

---

### Task 8: Thread ReasoningContext through HK pipeline

**Files:**
- Modify: `src/pipeline/reasoning.rs` (change derive_with_policy/derive_with_diffusion signatures)
- Modify: `src/pipeline/reasoning/synthesis.rs` (read from ctx)
- Modify: `src/pipeline/reasoning/policy.rs` (read from ctx)
- Modify: `src/hk/runtime.rs` (assemble ReasoningContext, hold AbsenceMemory)

- [ ] **Step 1: Change derive_with_policy signature**

In `src/pipeline/reasoning.rs`, change `derive_with_policy` (line 78):

```rust
    pub fn derive_with_policy(
        events: &EventSnapshot,
        derived_signals: &DerivedSignalSnapshot,
        insights: &GraphInsights,
        decision: &DecisionSnapshot,
        previous_setups: &[TacticalSetup],
        previous_tracks: &[HypothesisTrack],
        ctx: &ReasoningContext<'_>,
    ) -> Self {
```

Inside the function body, replace references to the old parameters:
- `lineage_priors` → `ctx.lineage_priors`
- `multi_horizon_gate` → `ctx.multi_horizon_gate`
- `symbol_dimensions` → `ctx.symbol_dimensions`
- `reviewer_doctrine` → `ctx.reviewer_doctrine`

Build `FamilyAlphaGate` from `ctx.lineage_priors`:

```rust
let family_gate = (!ctx.lineage_priors.is_empty()).then(|| {
    FamilyAlphaGate::from_lineage_priors(
        ctx.lineage_priors,
        hk_session_label(events.timestamp),
        ctx.market_regime.bias.as_str(),
    )
});
```

Pass `ctx.absence_memory`, `ctx.world_state` to `hypothesis_templates` calls.
Pass `&ctx.family_boost` to `apply_track_action_policy`.
Pass `&decision.convergence_scores` to `SetupSupportContext`.

- [ ] **Step 2: Apply same changes to derive_with_diffusion**

Same pattern as Step 1 for `derive_with_diffusion` (line 163).

- [ ] **Step 3: Update HK runtime to assemble ReasoningContext**

In `src/hk/runtime.rs`, before the tick loop starts (around line 100), add:

```rust
let mut absence_memory = crate::pipeline::reasoning::AbsenceMemory::default();
```

Then at line 557, replace the `ReasoningSnapshot::derive_with_diffusion(...)` call:

```rust
let family_boost = crate::pipeline::reasoning::FamilyBoostLedger::from_lineage_priors(
    &lineage_family_priors,
    crate::pipeline::reasoning::support::hk_session_label(deep_reasoning_event_snapshot.timestamp),
    deep_reasoning_decision.market_regime.bias.as_str(),
);
let reasoning_ctx = crate::pipeline::reasoning::ReasoningContext {
    lineage_priors: &lineage_family_priors,
    multi_horizon_gate: Some(&multi_horizon_gate),
    symbol_dimensions: Some(&dim_snapshot.dimensions),
    reviewer_doctrine: {
        #[cfg(feature = "persistence")]
        { cached_hk_reviewer_doctrine.as_ref() }
        #[cfg(not(feature = "persistence"))]
        { None }
    },
    convergence_components: &deep_reasoning_decision.convergence_scores,
    market_regime: &deep_reasoning_decision.market_regime,
    world_state: None, // Will be populated when world state is available pre-reasoning
    absence_memory: &absence_memory,
    family_boost: &family_boost,
};
let mut reasoning_snapshot = ReasoningSnapshot::derive_with_diffusion(
    &deep_reasoning_event_snapshot,
    &deep_reasoning_derived_signal_snapshot,
    &graph_insights,
    &deep_reasoning_decision,
    previous_setups,
    previous_tracks,
    &reasoning_ctx,
    &brain,
    &reasoning_stock_deltas,
);
```

After the reasoning snapshot is computed, update absence_memory:

```rust
// Update absence memory from this tick's events
let absence_sectors = crate::pipeline::reasoning::propagation_absence_sectors(
    &deep_reasoning_event_snapshot,
);
for sector in &absence_sectors {
    absence_memory.record_absence(sector, "propagation", tick, deep_reasoning_decision.timestamp);
}
// Decay old entries
absence_memory.decay(deep_reasoning_decision.timestamp);
```

Note: `hk_session_label` needs to be made `pub(crate)` if not already.

- [ ] **Step 4: Verify compilation**

Run: `cargo check --lib -q`
Expected: there will likely be some signature mismatches to fix. Work through each one.

- [ ] **Step 5: Run all tests**

Run: `cargo test -p eden --lib -- reasoning -q`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/pipeline/reasoning.rs src/pipeline/reasoning/synthesis.rs src/pipeline/reasoning/policy.rs src/hk/runtime.rs
git commit -m "feat(reasoning): thread ReasoningContext through HK pipeline, replacing scattered parameters"
```

---

### Task 9: US pipeline alignment

**Files:**
- Modify: `src/us/pipeline/reasoning.rs` (change derive_with_policy signature to use ctx or equivalent)
- Modify: `src/us/pipeline/reasoning/support.rs` (import FamilyAlphaGate from shared module)
- Modify: `src/us/runtime.rs`

- [ ] **Step 1: Import FamilyAlphaGate in US support.rs**

The US pipeline has its own separate `HypothesisTemplate` struct (with `&'static str` fields instead of `String`). The FamilyAlphaGate from `family_gate.rs` can still be used — it takes `&str` in `allows()`.

In `src/us/pipeline/reasoning.rs`, add:

```rust
use crate::pipeline::reasoning::family_gate::FamilyAlphaGate;
use crate::pipeline::reasoning::context::{AbsenceMemory, FamilyBoostLedger};
```

- [ ] **Step 2: Add FamilyAlphaGate to US derive_with_policy**

In the US `derive_with_policy` (line 201), add family_gate construction similar to HK:

```rust
let family_gate = lineage_stats.map(|stats| {
    FamilyAlphaGate::from_lineage_priors(
        &stats.family_priors,
        "us_session",
        market_regime.map(|r| r.as_str()).unwrap_or("neutral"),
    )
});
```

Pass `family_gate.as_ref()` to `derive_hypotheses` (requires updating the US `derive_hypotheses` signature to accept it).

- [ ] **Step 3: Update US runtime.rs to hold AbsenceMemory**

Follow the same pattern as HK runtime (Task 8, Step 3), but for `src/us/runtime.rs`.

- [ ] **Step 4: Verify compilation**

Run: `cargo check --lib -q`

- [ ] **Step 5: Run US tests**

Run: `cargo test -p eden --lib -- us::pipeline::reasoning -q`
Expected: all pass.

- [ ] **Step 6: Commit**

```bash
git add src/us/pipeline/reasoning.rs src/us/pipeline/reasoning/support.rs src/us/runtime.rs
git commit -m "feat(us): align US pipeline with shared FamilyAlphaGate and AbsenceMemory"
```

---

### Task 10: Final integration verification

**Files:** (no new changes — verification only)

- [ ] **Step 1: Full cargo check**

Run: `cargo check --lib -q`
Expected: clean.

- [ ] **Step 2: Full test suite**

Run: `cargo test -p eden --lib -q`
Expected: all pass.

- [ ] **Step 3: Verify new tests exist and pass**

Run: `cargo test -p eden --lib -- reasoning::context -q`
Expected: 6 tests (3 absence + 3 family boost).

Run: `cargo test -p eden --lib -- reasoning::tests::strong_consensus -q`
Run: `cargo test -p eden --lib -- reasoning::tests::high_spread -q`
Run: `cargo test -p eden --lib -- reasoning::tests::no_convergence -q`
Run: `cargo test -p eden --lib -- reasoning::tests::company_specific -q`
Run: `cargo test -p eden --lib -- reasoning::tests::stress_feedback_blocked -q`
Expected: all pass.

- [ ] **Step 4: Verify no regressions in existing tests**

Run: `cargo test -p eden --lib -q 2>&1 | tail -5`
Expected: `test result: ok. N passed; 0 failed`

- [ ] **Step 5: Commit verification note**

```bash
git log --oneline -8
```

Expected: 9 commits from this plan, in order:
1. `feat(reasoning): add ReasoningContext, AbsenceMemory, FamilyBoostLedger, ConvergenceDetail`
2. `refactor(reasoning): extract FamilyAlphaGate + helpers into shared family_gate.rs`
3. `feat(ontology): add convergence_detail field to TacticalSetup`
4. `feat(reasoning): populate ConvergenceDetail in tactical setups from graph components`
5. `feat(reasoning): add convergence-informed policy rules`
6. `feat(reasoning): integrate FamilyBoostLedger into policy edge thresholds`
7. `feat(reasoning): add driver_kind, absence memory, and world state regime filtering`
8. `feat(reasoning): thread ReasoningContext through HK pipeline`
9. `feat(us): align US pipeline with shared FamilyAlphaGate and AbsenceMemory`
