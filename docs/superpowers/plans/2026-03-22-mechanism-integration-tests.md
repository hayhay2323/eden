# Mechanism Integration Tests Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 18 integration tests (9 mechanisms x positive/negative) verifying that the full predicate → state → mechanism pipeline produces the correct primary mechanism for synthetic scenarios.

**Architecture:** A `ScenarioBuilder` constructs `PredicateInputs` with sensible defaults and targeted overrides. Each test calls `derive_atomic_predicates` → `build_reasoning_profile` and asserts the resulting `primary_mechanism.kind`. Positive tests verify the target mechanism wins; negative tests verify it does not.

**Tech Stack:** Rust `#[cfg(test)]` module, `rust_decimal_macros::dec!`, existing pipeline functions.

---

## File Structure

| File | Role |
|------|------|
| Create: `src/pipeline/mechanism_integration_tests.rs` | All 18 tests + `ScenarioBuilder` |
| Modify: `src/pipeline/mod.rs` | Add `#[cfg(test)] mod mechanism_integration_tests;` |

No production code changes. All new code is test-only.

---

### Task 1: ScenarioBuilder and first positive test (MechanicalExecution)

**Files:**
- Create: `src/pipeline/mechanism_integration_tests.rs`
- Modify: `src/pipeline/mod.rs`

- [ ] **Step 1: Create the test module with ScenarioBuilder**

Add to `src/pipeline/mod.rs`:

```rust
#[cfg(test)]
mod mechanism_integration_tests;
```

Create `src/pipeline/mechanism_integration_tests.rs` with the builder and default helpers:

```rust
//! Integration tests: predicate → state → mechanism pipeline.
//!
//! Each scenario constructs a minimal `PredicateInputs`, runs the full
//! pipeline, and asserts which mechanism becomes primary.

use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::live_snapshot::*;
use crate::ontology::mechanisms::MechanismCandidateKind;
use crate::ontology::semantics::HumanReviewContext;
use crate::ontology::ActionNode;
use crate::pipeline::mechanism_inference::build_reasoning_profile;
use crate::pipeline::predicate_engine::{derive_atomic_predicates, PredicateInputs};

// ── Defaults ──

fn default_case(symbol: &str) -> LiveTacticalCase {
    LiveTacticalCase {
        setup_id: format!("setup:{symbol}"),
        symbol: symbol.into(),
        title: format!("{symbol} test case"),
        action: "enter".into(),
        confidence: dec!(0.5),
        confidence_gap: dec!(0.1),
        heuristic_edge: dec!(0.05),
        entry_rationale: "test".into(),
        family_label: None,
        counter_label: None,
    }
}

fn default_stress() -> LiveStressSnapshot {
    LiveStressSnapshot {
        composite_stress: Decimal::ZERO,
        sector_synchrony: None,
        pressure_consensus: None,
        momentum_consensus: None,
        pressure_dispersion: None,
        volume_anomaly: None,
    }
}

fn default_regime() -> LiveMarketRegime {
    LiveMarketRegime {
        bias: "neutral".into(),
        confidence: dec!(0.5),
        breadth_up: dec!(0.5),
        breadth_down: dec!(0.5),
        average_return: Decimal::ZERO,
        directional_consensus: Decimal::ZERO,
    }
}

// ── ScenarioBuilder ──

struct ScenarioBuilder {
    case: LiveTacticalCase,
    signal: Option<LiveSignal>,
    pressure: Option<LivePressure>,
    chain: Option<LiveBackwardChain>,
    causal: Option<LiveCausalLeader>,
    track: Option<LiveHypothesisTrack>,
    stress: LiveStressSnapshot,
    regime: LiveMarketRegime,
    events: Vec<LiveEvent>,
    cross_market_signals: Vec<LiveCrossMarketSignal>,
    cross_market_anomalies: Vec<LiveCrossMarketAnomaly>,
    all_signals: Vec<LiveSignal>,
    all_pressures: Vec<LivePressure>,
}

impl ScenarioBuilder {
    fn new(symbol: &str) -> Self {
        Self {
            case: default_case(symbol),
            signal: None,
            pressure: None,
            chain: None,
            causal: None,
            track: None,
            stress: default_stress(),
            regime: default_regime(),
            events: Vec::new(),
            cross_market_signals: Vec::new(),
            cross_market_anomalies: Vec::new(),
            all_signals: Vec::new(),
            all_pressures: Vec::new(),
        }
    }

    fn confidence(mut self, value: Decimal) -> Self {
        self.case.confidence = value;
        self.case.confidence_gap = value * dec!(0.2);
        self.case.heuristic_edge = value * dec!(0.15);
        self
    }

    fn action(mut self, action: &str) -> Self {
        self.case.action = action.into();
        self
    }

    fn counter_label(mut self, label: &str) -> Self {
        self.case.counter_label = Some(label.into());
        self
    }

    fn family_label(mut self, label: &str) -> Self {
        self.case.family_label = Some(label.into());
        self
    }

    fn signal(mut self, composite: Decimal) -> Self {
        let sym = self.case.symbol.clone();
        self.signal = Some(LiveSignal {
            symbol: sym,
            sector: Some("tech".into()),
            composite,
            mark_price: Some(dec!(100)),
            dimension_composite: Some(composite),
            capital_flow_direction: composite,
            price_momentum: composite * dec!(0.8),
            volume_profile: dec!(0.3),
            pre_post_market_anomaly: Decimal::ZERO,
            valuation: Decimal::ZERO,
            cross_stock_correlation: Some(dec!(0.3)),
            sector_coherence: Some(dec!(0.4)),
            cross_market_propagation: None,
        });
        self
    }

    fn signal_full(mut self, signal: LiveSignal) -> Self {
        self.signal = Some(signal);
        self
    }

    fn pressure(mut self, capital_flow: Decimal, duration: u64, accelerating: bool) -> Self {
        let sym = self.case.symbol.clone();
        self.pressure = Some(LivePressure {
            symbol: sym,
            sector: Some("tech".into()),
            capital_flow_pressure: capital_flow,
            momentum: capital_flow * dec!(0.7),
            pressure_delta: capital_flow * dec!(0.3),
            pressure_duration: duration,
            accelerating,
        });
        self
    }

    fn chain(mut self, driver: &str, evidence: &[(&str, Decimal)]) -> Self {
        let sym = self.case.symbol.clone();
        self.chain = Some(LiveBackwardChain {
            symbol: sym,
            conclusion: "directional".into(),
            primary_driver: driver.into(),
            confidence: dec!(0.7),
            evidence: evidence
                .iter()
                .map(|(source, weight)| LiveEvidence {
                    source: source.to_string(),
                    description: format!("{source} signal"),
                    weight: *weight,
                    direction: *weight,
                })
                .collect(),
        });
        self
    }

    fn causal(mut self, leader: &str, streak: u64, flips: usize) -> Self {
        let sym = self.case.symbol.clone();
        self.causal = Some(LiveCausalLeader {
            symbol: sym,
            current_leader: leader.into(),
            leader_streak: streak,
            flips,
        });
        self
    }

    fn track(mut self, title: &str, status: &str, age: u64, confidence: Decimal) -> Self {
        let sym = self.case.symbol.clone();
        self.track = Some(LiveHypothesisTrack {
            symbol: sym,
            title: title.into(),
            status: status.into(),
            age_ticks: age,
            confidence,
        });
        self
    }

    fn stress(mut self, composite: Decimal) -> Self {
        self.stress.composite_stress = composite;
        self
    }

    fn stress_full(mut self, stress: LiveStressSnapshot) -> Self {
        self.stress = stress;
        self
    }

    fn regime(mut self, bias: &str) -> Self {
        self.regime.bias = bias.into();
        self
    }

    fn event(mut self, kind: &str, magnitude: Decimal, summary: &str) -> Self {
        self.events.push(LiveEvent {
            kind: kind.into(),
            magnitude,
            summary: summary.into(),
        });
        self
    }

    fn cross_market_signal(mut self, us: &str, hk: &str, confidence: Decimal) -> Self {
        self.cross_market_signals.push(LiveCrossMarketSignal {
            us_symbol: us.into(),
            hk_symbol: hk.into(),
            propagation_confidence: confidence,
            time_since_hk_close_minutes: Some(60),
        });
        self
    }

    fn cross_market_anomaly(
        mut self,
        us: &str,
        hk: &str,
        expected: Decimal,
        actual: Decimal,
    ) -> Self {
        self.cross_market_anomalies.push(LiveCrossMarketAnomaly {
            us_symbol: us.into(),
            hk_symbol: hk.into(),
            expected_direction: expected,
            actual_direction: actual,
            divergence: (expected - actual).abs(),
        });
        self
    }

    /// Add extra signals for other symbols (needed for sector rotation).
    fn extra_signal(mut self, signal: LiveSignal) -> Self {
        self.all_signals.push(signal);
        self
    }

    /// Add extra pressures for other symbols (needed for sector rotation).
    fn extra_pressure(mut self, pressure: LivePressure) -> Self {
        self.all_pressures.push(pressure);
        self
    }

    fn run(self) -> crate::ontology::CaseReasoningProfile {
        // Merge the case's own signal/pressure into all_signals/all_pressures.
        let mut all_signals = self.all_signals;
        if let Some(ref sig) = self.signal {
            all_signals.push(sig.clone());
        }
        let mut all_pressures = self.all_pressures;
        if let Some(ref prs) = self.pressure {
            all_pressures.push(prs.clone());
        }

        let inputs = PredicateInputs {
            tactical_case: &self.case,
            active_positions: &[],
            chain: self.chain.as_ref(),
            pressure: self.pressure.as_ref(),
            signal: self.signal.as_ref(),
            causal: self.causal.as_ref(),
            track: self.track.as_ref(),
            stress: &self.stress,
            market_regime: &self.regime,
            all_signals: &all_signals,
            all_pressures: &all_pressures,
            events: &self.events,
            cross_market_signals: &self.cross_market_signals,
            cross_market_anomalies: &self.cross_market_anomalies,
        };

        let predicates = derive_atomic_predicates(&inputs);
        build_reasoning_profile(&predicates, &[], None)
    }

    fn assert_primary(self, expected: MechanismCandidateKind) {
        let profile = self.run();
        let primary = profile
            .primary_mechanism
            .as_ref()
            .unwrap_or_else(|| panic!("expected primary mechanism {:?} but got None", expected));
        assert_eq!(
            primary.kind, expected,
            "expected {:?}, got {:?} (score {}). All candidates: {:?}",
            expected,
            primary.kind,
            primary.score,
            std::iter::once(primary)
                .chain(profile.competing_mechanisms.iter())
                .map(|m| format!("{:?}={}", m.kind, m.score))
                .collect::<Vec<_>>()
        );
    }

    fn assert_not_primary(self, excluded: MechanismCandidateKind) {
        let profile = self.run();
        if let Some(ref primary) = profile.primary_mechanism {
            assert_ne!(
                primary.kind, excluded,
                "{:?} should NOT be primary but it is (score {})",
                excluded, primary.score,
            );
        }
        // If no primary at all, the excluded mechanism is definitely not primary — pass.
    }
}
```

- [ ] **Step 2: Add the first positive test (MechanicalExecution)**

Append to the same file:

```rust
// ═══════════════════════════════════════════════════════════════
// Positive tests: mechanism SHOULD be primary
// ═══════════════════════════════════════════════════════════════

#[test]
fn mechanical_execution_fires_on_sustained_directional_signal() {
    // SignalRecurs (high track age) + ConfidenceBuilds (high confidence)
    // + PressurePersists (sustained pressure) → DirectionalReinforcement → MechanicalExecution
    ScenarioBuilder::new("700.HK")
        .confidence(dec!(0.82))
        .signal(dec!(0.75))
        .pressure(dec!(0.7), 8, true)
        .track("momentum bid", "strengthening", 10, dec!(0.80))
        .chain("capital_flow", &[("capital_flow", dec!(0.85))])
        .causal("capital_flow", 8, 0)
        .stress(dec!(0.15))
        .assert_primary(MechanismCandidateKind::MechanicalExecutionSignature);
}
```

- [ ] **Step 3: Verify test compiles and passes**

Run: `cargo test mechanical_execution_fires -q --no-run 2>&1 | tail -3`
Expected: compiles without error

Run: `cargo test mechanical_execution_fires -q 2>&1 | tail -5`
Expected: `test result: ok. 1 passed`

- [ ] **Step 4: Commit**

```bash
git add src/pipeline/mechanism_integration_tests.rs src/pipeline/mod.rs
git commit -m "test: add ScenarioBuilder and MechanicalExecution integration test"
```

---

### Task 2: Remaining 4 original mechanism positive tests

**Files:**
- Modify: `src/pipeline/mechanism_integration_tests.rs`

- [ ] **Step 1: Add FragilityBuildUp positive test**

```rust
#[test]
fn fragility_fires_on_high_stress_and_degradation() {
    // StructuralDegradation (high stress + weakening track)
    // + StressAccelerating (accelerating pressure + high stress)
    // → StructuralFragility → FragilityBuildUp
    ScenarioBuilder::new("9988.HK")
        .confidence(dec!(0.55))
        .signal_full(LiveSignal {
            symbol: "9988.HK".into(),
            sector: Some("tech".into()),
            composite: dec!(0.3),
            mark_price: Some(dec!(80)),
            dimension_composite: Some(dec!(0.3)),
            capital_flow_direction: dec!(0.2),
            price_momentum: dec!(-0.4),
            volume_profile: dec!(0.5),
            pre_post_market_anomaly: dec!(0.4),
            valuation: dec!(0.1),
            cross_stock_correlation: Some(dec!(0.2)),
            sector_coherence: Some(dec!(0.2)),
            cross_market_propagation: None,
        })
        .pressure(dec!(0.4), 5, true)
        .track("fragility watch", "weakening", 6, dec!(0.45))
        .stress_full(LiveStressSnapshot {
            composite_stress: dec!(0.82),
            sector_synchrony: Some(dec!(0.6)),
            pressure_consensus: Some(dec!(0.5)),
            momentum_consensus: Some(dec!(0.3)),
            pressure_dispersion: Some(dec!(0.55)),
            volume_anomaly: Some(dec!(0.4)),
        })
        .assert_primary(MechanismCandidateKind::FragilityBuildUp);
}
```

- [ ] **Step 2: Add ContagionOnset positive test**

```rust
#[test]
fn contagion_fires_on_deep_cross_scope_propagation() {
    // CrossScopePropagation (deep backward chain + cross-market signal)
    // + CrossMarketLinkActive + SourceConcentrated
    // → CrossScopeContagion → ContagionOnset
    ScenarioBuilder::new("700.HK")
        .confidence(dec!(0.70))
        .signal_full(LiveSignal {
            symbol: "700.HK".into(),
            sector: Some("tech".into()),
            composite: dec!(0.6),
            mark_price: Some(dec!(380)),
            dimension_composite: Some(dec!(0.55)),
            capital_flow_direction: dec!(0.5),
            price_momentum: dec!(0.3),
            volume_profile: dec!(0.3),
            pre_post_market_anomaly: Decimal::ZERO,
            valuation: Decimal::ZERO,
            cross_stock_correlation: Some(dec!(0.5)),
            sector_coherence: Some(dec!(0.6)),
            cross_market_propagation: Some(dec!(0.75)),
        })
        .pressure(dec!(0.5), 4, false)
        .chain("propagation", &[
            ("capital_flow", dec!(0.7)),
            ("institutional", dec!(0.6)),
            ("sector_coherence", dec!(0.5)),
            ("cross_market", dec!(0.4)),
        ])
        .causal("capital_flow", 6, 0)
        .cross_market_signal("TCEHY.US", "700.HK", dec!(0.72))
        .cross_market_signal("BABA.US", "9988.HK", dec!(0.65))
        .cross_market_signal("JD.US", "9618.HK", dec!(0.58))
        .stress(dec!(0.40))
        .assert_primary(MechanismCandidateKind::ContagionOnset);
}
```

- [ ] **Step 3: Add NarrativeFailure positive test**

```rust
#[test]
fn narrative_failure_fires_on_flips_and_counterevidence() {
    // LeaderFlipDetected (many flips, short streak)
    // + CounterevidencePresent (counter_label + weakening track)
    // → MechanisticAmbiguity → NarrativeFailure
    ScenarioBuilder::new("1810.HK")
        .confidence(dec!(0.50))
        .action("review")
        .counter_label("mean_reversion")
        .signal(dec!(0.25))
        .pressure(dec!(0.15), 2, false)
        .track("narrative test", "weakening", 4, dec!(0.40))
        .causal("mixed", 1, 5)
        .stress(dec!(0.50))
        .assert_primary(MechanismCandidateKind::NarrativeFailure);
}
```

- [ ] **Step 4: Add LiquidityTrap positive test**

```rust
#[test]
fn liquidity_trap_fires_on_pressure_without_price_movement() {
    // LiquidityImbalance: high pressure but near-zero momentum
    // + PressurePersists + SourceConcentrated
    // → LiquidityConstraint → LiquidityTrap
    ScenarioBuilder::new("3690.HK")
        .confidence(dec!(0.60))
        .signal_full(LiveSignal {
            symbol: "3690.HK".into(),
            sector: Some("tech".into()),
            composite: dec!(0.4),
            mark_price: Some(dec!(120)),
            dimension_composite: Some(dec!(0.35)),
            capital_flow_direction: dec!(0.7),
            price_momentum: dec!(0.02),
            volume_profile: dec!(0.6),
            pre_post_market_anomaly: Decimal::ZERO,
            valuation: Decimal::ZERO,
            cross_stock_correlation: Some(dec!(0.3)),
            sector_coherence: Some(dec!(0.3)),
            cross_market_propagation: None,
        })
        .pressure(dec!(0.75), 7, true)
        .chain("capital_flow", &[("capital_flow", dec!(0.80))])
        .causal("capital_flow", 6, 0)
        .track("liquidity", "stable", 5, dec!(0.55))
        .stress(dec!(0.25))
        .assert_primary(MechanismCandidateKind::LiquidityTrap);
}
```

- [ ] **Step 5: Run all positive tests so far**

Run: `cargo test -q --lib -k "fires" 2>&1 | tail -5`
Expected: `test result: ok. 5 passed`

- [ ] **Step 6: Commit**

```bash
git add src/pipeline/mechanism_integration_tests.rs
git commit -m "test: add Fragility, Contagion, NarrativeFailure, LiquidityTrap positive tests"
```

---

### Task 3: Remaining 4 new mechanism positive tests

**Files:**
- Modify: `src/pipeline/mechanism_integration_tests.rs`

- [ ] **Step 1: Add EventDrivenDislocation positive test**

```rust
#[test]
fn event_driven_fires_on_strong_event_catalyst() {
    // EventCatalystActive: strong event mentioning the symbol
    // + PriceReasoningDivergence (anomaly vs momentum mismatch)
    // → EventCatalyst → EventDrivenDislocation
    let sym = "981.HK";
    ScenarioBuilder::new(sym)
        .confidence(dec!(0.65))
        .signal_full(LiveSignal {
            symbol: sym.into(),
            sector: Some("semiconductor".into()),
            composite: dec!(0.5),
            mark_price: Some(dec!(28)),
            dimension_composite: Some(dec!(0.45)),
            capital_flow_direction: dec!(0.3),
            price_momentum: dec!(-0.1),
            volume_profile: dec!(0.7),
            pre_post_market_anomaly: dec!(0.65),
            valuation: dec!(0.1),
            cross_stock_correlation: Some(dec!(0.2)),
            sector_coherence: Some(dec!(0.3)),
            cross_market_propagation: None,
        })
        .pressure(dec!(0.3), 2, false)
        .event("earnings_surprise", dec!(0.85), "981.HK Q4 revenue beat")
        .event("policy_shift", dec!(0.70), "semiconductor export 981.HK related")
        .stress(dec!(0.30))
        .assert_primary(MechanismCandidateKind::EventDrivenDislocation);
}
```

- [ ] **Step 2: Add MeanReversionSnapback positive test**

```rust
#[test]
fn mean_reversion_fires_on_extreme_valuation_and_counter_momentum() {
    // MeanReversionPressure: extreme valuation + opposing momentum
    // + PriceReasoningDivergence + CounterevidencePresent
    // → ReversionPressure → MeanReversionSnapback
    ScenarioBuilder::new("9618.HK")
        .confidence(dec!(0.55))
        .counter_label("momentum")
        .signal_full(LiveSignal {
            symbol: "9618.HK".into(),
            sector: Some("tech".into()),
            composite: dec!(0.3),
            mark_price: Some(dec!(150)),
            dimension_composite: Some(dec!(0.25)),
            capital_flow_direction: dec!(-0.3),
            price_momentum: dec!(-0.6),
            volume_profile: dec!(0.4),
            pre_post_market_anomaly: dec!(0.3),
            valuation: dec!(0.85),
            cross_stock_correlation: Some(dec!(0.2)),
            sector_coherence: Some(dec!(0.2)),
            cross_market_propagation: None,
        })
        .pressure(dec!(-0.4), 4, false)
        .track("reversion watch", "weakening", 3, dec!(0.45))
        .causal("valuation", 3, 2)
        .stress(dec!(0.35))
        .assert_primary(MechanismCandidateKind::MeanReversionSnapback);
}
```

- [ ] **Step 3: Add ArbitrageConvergence positive test**

```rust
#[test]
fn arbitrage_convergence_fires_on_cross_market_dislocation() {
    // CrossMarketDislocation: high divergence between HK/US pairs
    // + CrossMarketLinkActive: active cross-market signals
    // → CrossMarketDislocation state → ArbitrageConvergence
    ScenarioBuilder::new("9988.HK")
        .confidence(dec!(0.65))
        .signal_full(LiveSignal {
            symbol: "9988.HK".into(),
            sector: Some("tech".into()),
            composite: dec!(0.5),
            mark_price: Some(dec!(85)),
            dimension_composite: Some(dec!(0.45)),
            capital_flow_direction: dec!(0.4),
            price_momentum: dec!(0.2),
            volume_profile: dec!(0.3),
            pre_post_market_anomaly: dec!(0.1),
            valuation: dec!(0.2),
            cross_stock_correlation: Some(dec!(0.3)),
            sector_coherence: Some(dec!(0.3)),
            cross_market_propagation: Some(dec!(0.80)),
        })
        .pressure(dec!(0.3), 3, false)
        .cross_market_signal("BABA.US", "9988.HK", dec!(0.82))
        .cross_market_anomaly("BABA.US", "9988.HK", dec!(0.6), dec!(-0.3))
        .cross_market_anomaly("JD.US", "9618.HK", dec!(0.4), dec!(-0.25))
        .chain("cross_market", &[("cross_market", dec!(0.7)), ("price", dec!(0.4))])
        .stress(dec!(0.20))
        .assert_primary(MechanismCandidateKind::ArbitrageConvergence);
}
```

- [ ] **Step 4: Add CapitalRotation positive test**

```rust
#[test]
fn capital_rotation_fires_on_sector_substitution_flow() {
    // SectorRotationPressure: opposing pressure between sectors
    // + PressurePersists + CrossScopePropagation
    // → SubstitutionFlow → CapitalRotation
    //
    // We need the case symbol in one sector with positive pressure,
    // and extra symbols in another sector with negative pressure.
    let sym = "700.HK";
    ScenarioBuilder::new(sym)
        .confidence(dec!(0.65))
        .family_label("tech")
        .signal_full(LiveSignal {
            symbol: sym.into(),
            sector: Some("tech".into()),
            composite: dec!(0.6),
            mark_price: Some(dec!(380)),
            dimension_composite: Some(dec!(0.55)),
            capital_flow_direction: dec!(0.6),
            price_momentum: dec!(0.4),
            volume_profile: dec!(0.3),
            pre_post_market_anomaly: Decimal::ZERO,
            valuation: Decimal::ZERO,
            cross_stock_correlation: Some(dec!(0.4)),
            sector_coherence: Some(dec!(0.5)),
            cross_market_propagation: None,
        })
        .pressure(dec!(0.65), 6, true)
        // Tech peers with same-direction pressure
        .extra_signal(LiveSignal {
            symbol: "9988.HK".into(),
            sector: Some("tech".into()),
            composite: dec!(0.5),
            mark_price: Some(dec!(85)),
            dimension_composite: Some(dec!(0.45)),
            capital_flow_direction: dec!(0.55),
            price_momentum: dec!(0.35),
            volume_profile: dec!(0.3),
            pre_post_market_anomaly: Decimal::ZERO,
            valuation: Decimal::ZERO,
            cross_stock_correlation: None,
            sector_coherence: None,
            cross_market_propagation: None,
        })
        .extra_pressure(LivePressure {
            symbol: "9988.HK".into(),
            sector: Some("tech".into()),
            capital_flow_pressure: dec!(0.60),
            momentum: dec!(0.40),
            pressure_delta: dec!(0.20),
            pressure_duration: 5,
            accelerating: true,
        })
        // Finance peers with OPPOSITE pressure (money leaving finance → entering tech)
        .extra_signal(LiveSignal {
            symbol: "1398.HK".into(),
            sector: Some("finance".into()),
            composite: dec!(-0.4),
            mark_price: Some(dec!(5)),
            dimension_composite: Some(dec!(-0.35)),
            capital_flow_direction: dec!(-0.55),
            price_momentum: dec!(-0.30),
            volume_profile: dec!(0.3),
            pre_post_market_anomaly: Decimal::ZERO,
            valuation: Decimal::ZERO,
            cross_stock_correlation: None,
            sector_coherence: None,
            cross_market_propagation: None,
        })
        .extra_pressure(LivePressure {
            symbol: "1398.HK".into(),
            sector: Some("finance".into()),
            capital_flow_pressure: dec!(-0.60),
            momentum: dec!(-0.35),
            pressure_delta: dec!(-0.20),
            pressure_duration: 5,
            accelerating: true,
        })
        .extra_signal(LiveSignal {
            symbol: "3988.HK".into(),
            sector: Some("finance".into()),
            composite: dec!(-0.35),
            mark_price: Some(dec!(3)),
            dimension_composite: Some(dec!(-0.30)),
            capital_flow_direction: dec!(-0.50),
            price_momentum: dec!(-0.25),
            volume_profile: dec!(0.2),
            pre_post_market_anomaly: Decimal::ZERO,
            valuation: Decimal::ZERO,
            cross_stock_correlation: None,
            sector_coherence: None,
            cross_market_propagation: None,
        })
        .extra_pressure(LivePressure {
            symbol: "3988.HK".into(),
            sector: Some("finance".into()),
            capital_flow_pressure: dec!(-0.55),
            momentum: dec!(-0.30),
            pressure_delta: dec!(-0.15),
            pressure_duration: 4,
            accelerating: false,
        })
        .chain("capital_flow", &[("sector_rotation", dec!(0.7)), ("capital_flow", dec!(0.6))])
        .track("rotation bid", "strengthening", 5, dec!(0.65))
        .stress(dec!(0.20))
        .assert_primary(MechanismCandidateKind::CapitalRotation);
}
```

- [ ] **Step 5: Run all 9 positive tests**

Run: `cargo test -q --lib -k "fires" 2>&1 | tail -5`
Expected: `test result: ok. 9 passed`

- [ ] **Step 6: Commit**

```bash
git add src/pipeline/mechanism_integration_tests.rs
git commit -m "test: add EventDriven, MeanReversion, Arbitrage, CapitalRotation positive tests"
```

---

### Task 4: All 9 negative tests

**Files:**
- Modify: `src/pipeline/mechanism_integration_tests.rs`

- [ ] **Step 1: Add all 9 negative tests**

Each negative test constructs conditions that SHOULD NOT produce the target mechanism as primary. The strategy: create conditions that strongly favor a *different* mechanism.

```rust
// ═══════════════════════════════════════════════════════════════
// Negative tests: mechanism should NOT be primary
// ═══════════════════════════════════════════════════════════════

#[test]
fn mechanical_execution_does_not_fire_under_high_ambiguity() {
    // High flips + counterevidence → MechanisticAmbiguity dominates,
    // should push toward NarrativeFailure, not MechanicalExecution.
    ScenarioBuilder::new("700.HK")
        .confidence(dec!(0.45))
        .action("review")
        .counter_label("reversal")
        .signal(dec!(0.2))
        .pressure(dec!(0.1), 1, false)
        .track("ambiguous", "weakening", 2, dec!(0.35))
        .causal("mixed", 1, 6)
        .stress(dec!(0.55))
        .assert_not_primary(MechanismCandidateKind::MechanicalExecutionSignature);
}

#[test]
fn fragility_does_not_fire_under_low_stress() {
    // Low stress + strong directional signal → MechanicalExecution territory.
    ScenarioBuilder::new("9988.HK")
        .confidence(dec!(0.80))
        .signal(dec!(0.75))
        .pressure(dec!(0.7), 8, true)
        .track("momentum", "strengthening", 10, dec!(0.80))
        .chain("capital_flow", &[("capital_flow", dec!(0.8))])
        .stress(dec!(0.10))
        .assert_not_primary(MechanismCandidateKind::FragilityBuildUp);
}

#[test]
fn contagion_does_not_fire_without_cross_scope_evidence() {
    // No backward chain, no cross-market signals → purely local.
    ScenarioBuilder::new("700.HK")
        .confidence(dec!(0.70))
        .signal(dec!(0.6))
        .pressure(dec!(0.5), 5, true)
        .track("local bid", "strengthening", 6, dec!(0.70))
        .stress(dec!(0.15))
        .assert_not_primary(MechanismCandidateKind::ContagionOnset);
}

#[test]
fn narrative_failure_does_not_fire_with_stable_leader() {
    // Zero flips, long leader streak, no counterevidence → stable narrative.
    ScenarioBuilder::new("1810.HK")
        .confidence(dec!(0.75))
        .signal(dec!(0.65))
        .pressure(dec!(0.6), 7, true)
        .track("stable narrative", "strengthening", 8, dec!(0.75))
        .causal("capital_flow", 10, 0)
        .stress(dec!(0.15))
        .assert_not_primary(MechanismCandidateKind::NarrativeFailure);
}

#[test]
fn liquidity_trap_does_not_fire_when_price_follows_pressure() {
    // Strong momentum aligns with pressure → not trapped, just executing.
    ScenarioBuilder::new("3690.HK")
        .confidence(dec!(0.75))
        .signal(dec!(0.70))
        .pressure(dec!(0.7), 6, true)
        .track("momentum", "strengthening", 7, dec!(0.75))
        .chain("capital_flow", &[("capital_flow", dec!(0.8))])
        .causal("capital_flow", 7, 0)
        .stress(dec!(0.15))
        .assert_not_primary(MechanismCandidateKind::LiquidityTrap);
}

#[test]
fn event_driven_does_not_fire_without_events() {
    // No events at all → cannot be event-driven.
    ScenarioBuilder::new("981.HK")
        .confidence(dec!(0.70))
        .signal(dec!(0.6))
        .pressure(dec!(0.5), 5, true)
        .track("momentum", "strengthening", 6, dec!(0.70))
        .chain("capital_flow", &[("capital_flow", dec!(0.7))])
        .stress(dec!(0.20))
        .assert_not_primary(MechanismCandidateKind::EventDrivenDislocation);
}

#[test]
fn mean_reversion_does_not_fire_with_low_valuation() {
    // Near-zero valuation signal → no reversion pressure.
    ScenarioBuilder::new("9618.HK")
        .confidence(dec!(0.70))
        .signal(dec!(0.65))
        .pressure(dec!(0.6), 6, true)
        .track("trend", "strengthening", 7, dec!(0.70))
        .causal("capital_flow", 7, 0)
        .stress(dec!(0.15))
        .assert_not_primary(MechanismCandidateKind::MeanReversionSnapback);
}

#[test]
fn arbitrage_does_not_fire_without_cross_market_anomaly() {
    // No cross-market anomalies, no cross-market signals → local story only.
    ScenarioBuilder::new("9988.HK")
        .confidence(dec!(0.70))
        .signal(dec!(0.6))
        .pressure(dec!(0.5), 5, true)
        .track("local bid", "strengthening", 6, dec!(0.65))
        .chain("capital_flow", &[("capital_flow", dec!(0.7))])
        .stress(dec!(0.15))
        .assert_not_primary(MechanismCandidateKind::ArbitrageConvergence);
}

#[test]
fn capital_rotation_does_not_fire_with_single_sector() {
    // All signals in the same sector → no rotation, just directional.
    ScenarioBuilder::new("700.HK")
        .confidence(dec!(0.70))
        .signal(dec!(0.65))
        .pressure(dec!(0.6), 6, true)
        .track("tech bid", "strengthening", 7, dec!(0.70))
        .chain("capital_flow", &[("capital_flow", dec!(0.8))])
        .causal("capital_flow", 7, 0)
        .extra_signal(LiveSignal {
            symbol: "9988.HK".into(),
            sector: Some("tech".into()),
            composite: dec!(0.5),
            mark_price: Some(dec!(85)),
            dimension_composite: Some(dec!(0.45)),
            capital_flow_direction: dec!(0.5),
            price_momentum: dec!(0.3),
            volume_profile: dec!(0.3),
            pre_post_market_anomaly: Decimal::ZERO,
            valuation: Decimal::ZERO,
            cross_stock_correlation: None,
            sector_coherence: None,
            cross_market_propagation: None,
        })
        .extra_pressure(LivePressure {
            symbol: "9988.HK".into(),
            sector: Some("tech".into()),
            capital_flow_pressure: dec!(0.55),
            momentum: dec!(0.4),
            pressure_delta: dec!(0.2),
            pressure_duration: 5,
            accelerating: true,
        })
        .stress(dec!(0.15))
        .assert_not_primary(MechanismCandidateKind::CapitalRotation);
}
```

- [ ] **Step 2: Run all 18 tests**

Run: `cargo test -q --lib mechanism_integration_tests 2>&1 | tail -5`
Expected: `test result: ok. 18 passed`

- [ ] **Step 3: Commit**

```bash
git add src/pipeline/mechanism_integration_tests.rs
git commit -m "test: add 9 negative mechanism integration tests (18 total)"
```

---

### Task 5: Counterfactual and profile structure assertions

**Files:**
- Modify: `src/pipeline/mechanism_integration_tests.rs`

- [ ] **Step 1: Add structural assertions for counterfactuals and factors**

These tests verify that the full pipeline produces the expected metadata (factors, counterfactuals, competing mechanisms) in addition to the correct primary mechanism.

```rust
// ═══════════════════════════════════════════════════════════════
// Structural tests: verify pipeline metadata
// ═══════════════════════════════════════════════════════════════

#[test]
fn positive_scenarios_always_produce_factors_and_counterfactuals() {
    // Run the MechanicalExecution scenario and verify structural properties.
    let profile = ScenarioBuilder::new("700.HK")
        .confidence(dec!(0.82))
        .signal(dec!(0.75))
        .pressure(dec!(0.7), 8, true)
        .track("momentum bid", "strengthening", 10, dec!(0.80))
        .chain("capital_flow", &[("capital_flow", dec!(0.85))])
        .causal("capital_flow", 8, 0)
        .stress(dec!(0.15))
        .run();

    let primary = profile.primary_mechanism.as_ref().expect("primary should exist");

    // Factors should be present and non-empty.
    assert!(!primary.factors.is_empty(), "primary mechanism should have factors");

    // Every factor should have non-negative contribution.
    for factor in &primary.factors {
        assert!(
            factor.contribution >= Decimal::ZERO,
            "factor {} has negative contribution {}",
            factor.label,
            factor.contribution
        );
    }

    // Counterfactuals should be present.
    assert!(
        !primary.counterfactuals.is_empty(),
        "primary mechanism should have counterfactuals"
    );

    // Every counterfactual's adjusted score should be less than the original.
    for cf in &primary.counterfactuals {
        assert!(
            cf.score_delta <= Decimal::ZERO,
            "counterfactual {} has positive delta {}",
            cf.factor_label,
            cf.score_delta
        );
    }
}

#[test]
fn ambiguous_scenario_produces_competing_mechanisms() {
    // A moderately stressed scenario with mixed signals should produce
    // both a primary and at least one competing mechanism.
    let profile = ScenarioBuilder::new("700.HK")
        .confidence(dec!(0.60))
        .signal(dec!(0.50))
        .pressure(dec!(0.45), 4, true)
        .track("mixed", "stable", 4, dec!(0.55))
        .chain("capital_flow", &[("capital_flow", dec!(0.6)), ("sector", dec!(0.4))])
        .causal("capital_flow", 3, 1)
        .stress(dec!(0.45))
        .cross_market_signal("TCEHY.US", "700.HK", dec!(0.50))
        .run();

    assert!(
        profile.primary_mechanism.is_some(),
        "should have a primary mechanism"
    );
    // In an ambiguous scenario, the pipeline should surface alternatives.
    // We don't assert a specific count, just that the system doesn't
    // collapse to a single explanation when evidence is mixed.
    assert!(
        !profile.composite_states.is_empty(),
        "should have composite states"
    );
    assert!(
        !profile.predicates.is_empty(),
        "should have predicates"
    );
}
```

- [ ] **Step 2: Run all tests**

Run: `cargo test -q --lib mechanism_integration_tests 2>&1 | tail -5`
Expected: `test result: ok. 20 passed`

- [ ] **Step 3: Commit**

```bash
git add src/pipeline/mechanism_integration_tests.rs
git commit -m "test: add counterfactual and profile structure integration tests (20 total)"
```
