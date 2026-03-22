use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::live_snapshot::{
    LiveBackwardChain, LiveCausalLeader, LiveCrossMarketAnomaly, LiveCrossMarketSignal, LiveEvent,
    LiveEvidence, LiveHypothesisTrack, LiveMarketRegime, LivePressure, LiveSignal,
    LiveStressSnapshot, LiveTacticalCase,
};
use crate::ontology::mechanisms::MechanismCandidateKind;
use crate::ontology::{ActionNode, CaseReasoningProfile};
use crate::pipeline::mechanism_inference::build_reasoning_profile;
use crate::pipeline::predicate_engine::{derive_atomic_predicates, PredicateInputs};

// ---------------------------------------------------------------------------
// Default constructors
// ---------------------------------------------------------------------------

fn default_case(symbol: &str) -> LiveTacticalCase {
    LiveTacticalCase {
        setup_id: format!("setup:{}", symbol),
        symbol: symbol.to_string(),
        title: format!("Test case for {}", symbol),
        action: "review".to_string(),
        confidence: dec!(0.60),
        confidence_gap: dec!(0.10),
        heuristic_edge: dec!(0.10),
        entry_rationale: "synthetic test scenario".to_string(),
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
        bias: "neutral".to_string(),
        confidence: Decimal::ZERO,
        breadth_up: Decimal::ZERO,
        breadth_down: Decimal::ZERO,
        average_return: Decimal::ZERO,
        directional_consensus: None,
        pre_market_sentiment: None,
    }
}

// ---------------------------------------------------------------------------
// ScenarioBuilder
// ---------------------------------------------------------------------------

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
    extra_signals: Vec<LiveSignal>,
    extra_pressures: Vec<LivePressure>,
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
            extra_signals: Vec::new(),
            extra_pressures: Vec::new(),
        }
    }

    /// Set confidence; auto-derive gap and edge as 50% of (1.0 - confidence).
    fn confidence(mut self, value: Decimal) -> Self {
        self.case.confidence = value;
        let remainder = Decimal::ONE - value;
        let half = remainder / dec!(2);
        self.case.confidence_gap = half;
        self.case.heuristic_edge = half;
        self
    }

    fn action(mut self, action: &str) -> Self {
        self.case.action = action.to_string();
        self
    }

    fn counter_label(mut self, label: &str) -> Self {
        self.case.counter_label = Some(label.to_string());
        self
    }

    fn family_label(mut self, label: &str) -> Self {
        self.case.family_label = Some(label.to_string());
        self
    }

    /// Create a LiveSignal with the given composite value; derive sub-fields proportionally.
    fn signal(mut self, composite: Decimal) -> Self {
        self.signal = Some(LiveSignal {
            symbol: self.case.symbol.clone(),
            sector: None,
            composite,
            mark_price: None,
            dimension_composite: Some(composite),
            capital_flow_direction: composite,
            price_momentum: composite,
            volume_profile: composite,
            pre_post_market_anomaly: Decimal::ZERO,
            valuation: Decimal::ZERO,
            cross_stock_correlation: None,
            sector_coherence: None,
            cross_market_propagation: None,
        });
        self
    }

    fn signal_full(mut self, signal: LiveSignal) -> Self {
        self.signal = Some(signal);
        self
    }

    /// Create a LivePressure.
    fn pressure(mut self, capital_flow: Decimal, duration: u64, accelerating: bool) -> Self {
        self.pressure = Some(LivePressure {
            symbol: self.case.symbol.clone(),
            sector: None,
            capital_flow_pressure: capital_flow,
            momentum: capital_flow,
            pressure_delta: capital_flow,
            pressure_duration: duration,
            accelerating,
        });
        self
    }

    /// Create a LiveBackwardChain with the given driver and evidence pairs (source, weight).
    fn chain(mut self, driver: &str, evidence_pairs: &[(&str, Decimal)]) -> Self {
        let evidence = evidence_pairs
            .iter()
            .map(|(src, weight)| LiveEvidence {
                source: src.to_string(),
                description: format!("evidence from {}", src),
                weight: *weight,
                direction: Decimal::ONE,
            })
            .collect();

        self.chain = Some(LiveBackwardChain {
            symbol: self.case.symbol.clone(),
            conclusion: "directional signal confirmed".to_string(),
            primary_driver: driver.to_string(),
            confidence: evidence_pairs
                .first()
                .map(|(_, w)| *w)
                .unwrap_or(dec!(0.70)),
            evidence,
        });
        self
    }

    /// Create a LiveCausalLeader.
    fn causal(mut self, leader: &str, streak: u64, flips: usize) -> Self {
        self.causal = Some(LiveCausalLeader {
            symbol: self.case.symbol.clone(),
            current_leader: leader.to_string(),
            leader_streak: streak,
            flips,
        });
        self
    }

    /// Create a LiveHypothesisTrack.
    fn track(mut self, title: &str, status: &str, age: u64, confidence: Decimal) -> Self {
        self.track = Some(LiveHypothesisTrack {
            symbol: self.case.symbol.clone(),
            title: title.to_string(),
            status: status.to_string(),
            age_ticks: age,
            confidence,
        });
        self
    }

    /// Set composite stress.
    fn stress(mut self, composite: Decimal) -> Self {
        self.stress.composite_stress = composite;
        self
    }

    fn stress_full(mut self, stress: LiveStressSnapshot) -> Self {
        self.stress = stress;
        self
    }

    /// Set market regime bias.
    fn regime(mut self, bias: &str) -> Self {
        self.regime.bias = bias.to_string();
        self
    }

    /// Add a LiveEvent.
    fn event(mut self, kind: &str, magnitude: Decimal, summary: &str) -> Self {
        self.events.push(LiveEvent {
            kind: kind.to_string(),
            magnitude,
            summary: summary.to_string(),
        });
        self
    }

    /// Add a LiveCrossMarketSignal.
    fn cross_market_signal(mut self, us: &str, hk: &str, confidence: Decimal) -> Self {
        self.cross_market_signals.push(LiveCrossMarketSignal {
            us_symbol: us.to_string(),
            hk_symbol: hk.to_string(),
            propagation_confidence: confidence,
            time_since_hk_close_minutes: None,
        });
        self
    }

    /// Add a LiveCrossMarketAnomaly.
    fn cross_market_anomaly(
        mut self,
        us: &str,
        hk: &str,
        expected: Decimal,
        actual: Decimal,
    ) -> Self {
        let divergence = (actual - expected).abs();
        self.cross_market_anomalies.push(LiveCrossMarketAnomaly {
            us_symbol: us.to_string(),
            hk_symbol: hk.to_string(),
            expected_direction: expected,
            actual_direction: actual,
            divergence,
        });
        self
    }

    /// Add an extra signal (for multi-symbol scenarios).
    fn extra_signal(mut self, signal: LiveSignal) -> Self {
        self.extra_signals.push(signal);
        self
    }

    /// Add an extra pressure (for multi-symbol scenarios).
    fn extra_pressure(mut self, pressure: LivePressure) -> Self {
        self.extra_pressures.push(pressure);
        self
    }

    // ---------------------------------------------------------------------------
    // Pipeline execution
    // ---------------------------------------------------------------------------

    fn run(self) -> CaseReasoningProfile {
        // Merge the case's own signal/pressure into the all_* vecs.
        let mut all_signals: Vec<LiveSignal> = Vec::new();
        if let Some(ref s) = self.signal {
            all_signals.push(s.clone());
        }
        all_signals.extend(self.extra_signals.iter().cloned());

        let mut all_pressures: Vec<LivePressure> = Vec::new();
        if let Some(ref p) = self.pressure {
            all_pressures.push(p.clone());
        }
        all_pressures.extend(self.extra_pressures.iter().cloned());

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

    fn assert_primary(self, kind: MechanismCandidateKind) {
        let profile = self.run();
        let primary = profile
            .primary_mechanism
            .as_ref()
            .unwrap_or_else(|| panic!("expected a primary mechanism but got None"));
        assert_eq!(
            primary.kind, kind,
            "expected primary mechanism {:?} but got {:?}",
            kind, primary.kind
        );
    }

    fn assert_not_primary(self, kind: MechanismCandidateKind) {
        let profile = self.run();
        if let Some(primary) = profile.primary_mechanism.as_ref() {
            assert_ne!(
                primary.kind, kind,
                "expected primary mechanism to NOT be {:?} but it was",
                kind
            );
        }
        // None primary also satisfies "not primary == kind"
    }
}

// ---------------------------------------------------------------------------
// Integration tests
// ---------------------------------------------------------------------------

#[test]
fn mechanical_execution_fires_on_sustained_directional_signal() {
    // High track age + high confidence + sustained pressure
    // → DirectionalReinforcement → MechanicalExecutionSignature
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

#[test]
fn fragility_fires_on_high_stress_and_degradation() {
    // High composite_stress + pressure_dispersion → StructuralDegradation (score ~0.58)
    // Tiny accelerating pressure → StressAccelerating fires (~0.64)
    // → StructuralFragility ~0.45.
    // Adding 3 causal flips + counter_label → MechanisticAmbiguity ~0.47, clarity ~0.53.
    // This suppresses MechanicalExecution's clarity bonus while raising FragilityBuildUp score:
    //   FragilityBuildUp = 0.45*0.60 + 0.47*0.25 = 0.27 + 0.12 = 0.39
    //   NarrativeFailure = 0.47*0.50 + 0.45*0.30 = 0.24 + 0.14 = 0.37  (less than FragilityBuildUp)
    //   MechanicalExecution = D*0.45 + 0.53*0.20 ≈ 0.12 + 0.11 = 0.22  (lowest)
    ScenarioBuilder::new("2318.HK")
        .confidence(dec!(0.45))
        .action("review")
        .counter_label("stress_driven_not_directional")
        .signal_full(LiveSignal {
            symbol: "2318.HK".to_string(),
            sector: None,
            composite: dec!(-0.15),
            mark_price: None,
            dimension_composite: Some(dec!(-0.15)),
            capital_flow_direction: dec!(-0.15),
            price_momentum: dec!(-0.14),
            volume_profile: dec!(0.20),
            pre_post_market_anomaly: Decimal::ZERO,
            valuation: Decimal::ZERO,
            cross_stock_correlation: None,
            sector_coherence: None,
            cross_market_propagation: None,
        })
        // Tiny pressure magnitude ≈ price_momentum → absorption_gap ≈ 0 (suppresses LiquidityTrap).
        // Accelerating = true → StressAccelerating fires.
        // Short duration → PressurePersists stays weak.
        .pressure(dec!(-0.16), 2, true)
        // 3 flips, short streak → LeaderFlipDetected high (~0.77)
        // Combined with counter_label → MechanisticAmbiguity ~0.47, reducing clarity to ~0.53
        .causal("macro_stress", 2, 3)
        .stress_full(LiveStressSnapshot {
            composite_stress: dec!(0.92),
            sector_synchrony: Some(dec!(0.75)),
            pressure_consensus: Some(dec!(0.82)),
            momentum_consensus: Some(dec!(0.78)),
            pressure_dispersion: Some(dec!(0.85)),
            volume_anomaly: Some(dec!(0.70)),
        })
        // No track → SignalRecurs = 0, no track_score in StructuralDegradation.
        // Stress + dispersion alone drive StructuralDegradation to ~0.58.
        .regime("neutral")
        .assert_primary(MechanismCandidateKind::FragilityBuildUp);
}

#[test]
fn contagion_fires_on_deep_cross_scope_propagation() {
    // 4-item evidence chain + multiple cross-market signals + high cross_market_propagation
    // → CrossScopePropagation (≈1.0) + CrossMarketLinkActive → CrossScopeContagion (high)
    // High stress + dispersion → StructuralFragility (ContagionOnset weight 0.40)
    // No track → SignalRecurs = 0. Very low confidence → ConfidenceBuilds low.
    // No pressure → PressurePersists = 0. Result: DirectionalReinforcement near 0.
    // ContagionOnset score = CrossScopeContagion*0.50 + StructuralFragility*0.40 beats
    // MechanicalExecution = CrossScopeContagion*0.35 + clarity*0.20 (≈0.55 + 0.20 = 0.75 vs
    // ContagionOnset ≈ C*0.50 + F*0.40 which at C≈0.80, F≈0.50 gives 0.60).
    // Actually MechanicalExecution clarity factor (0.20) and no DirectionalReinforcement
    // means: Mech = C*0.35 + 0.20 ≈ 0.48; Contagion = C*0.50 + F*0.40 ≈ 0.60.
    ScenarioBuilder::new("941.HK")
        .confidence(dec!(0.42))
        .action("review")
        .signal_full(LiveSignal {
            symbol: "941.HK".to_string(),
            sector: None,
            composite: dec!(0.20),
            mark_price: None,
            dimension_composite: Some(dec!(0.20)),
            capital_flow_direction: dec!(0.18),
            price_momentum: dec!(0.16),
            volume_profile: dec!(0.20),
            pre_post_market_anomaly: Decimal::ZERO,
            valuation: Decimal::ZERO,
            cross_stock_correlation: None,
            sector_coherence: None,
            cross_market_propagation: Some(dec!(0.95)),
        })
        // No pressure → PressurePersists = 0, no StressAccelerating acceleration bonus
        .chain(
            "cross_market_flow",
            &[
                ("us_tech_selloff", dec!(0.82)),
                ("hk_index_drag", dec!(0.78)),
                ("sector_contagion", dec!(0.74)),
                ("option_chain_signal", dec!(0.70)),
            ],
        )
        .cross_market_signal("BABA.US", "941.HK", dec!(0.90))
        .cross_market_signal("TCOM.US", "941.HK", dec!(0.85))
        .stress_full(LiveStressSnapshot {
            composite_stress: dec!(0.88),
            sector_synchrony: Some(dec!(0.82)),
            pressure_consensus: Some(dec!(0.78)),
            momentum_consensus: Some(dec!(0.72)),
            pressure_dispersion: Some(dec!(0.80)),
            volume_anomaly: Some(dec!(0.65)),
        })
        // No track → SignalRecurs = 0, StructuralDegradation uses only stress + dispersion
        .regime("neutral")
        .assert_primary(MechanismCandidateKind::ContagionOnset);
}

#[test]
fn narrative_failure_fires_on_flips_and_counterevidence() {
    // Many causal flips + short streak + counter_label + weakening track
    // → LeaderFlipDetected + CounterevidencePresent → MechanisticAmbiguity → NarrativeFailure
    ScenarioBuilder::new("1299.HK")
        .confidence(dec!(0.55))
        .action("review")
        .counter_label("macro_headwind_invalidates_thesis")
        .signal_full(LiveSignal {
            symbol: "1299.HK".to_string(),
            sector: None,
            composite: dec!(-0.20),
            mark_price: None,
            dimension_composite: Some(dec!(-0.20)),
            capital_flow_direction: dec!(-0.25),
            price_momentum: dec!(-0.15),
            volume_profile: dec!(0.30),
            pre_post_market_anomaly: Decimal::ZERO,
            valuation: Decimal::ZERO,
            cross_stock_correlation: None,
            sector_coherence: None,
            cross_market_propagation: None,
        })
        .pressure(dec!(-0.30), 5, false)
        .causal("macro_risk", 1, 6)
        .track("momentum thesis", "weakening", 4, dec!(0.45))
        .stress(dec!(0.40))
        .regime("neutral")
        .assert_primary(MechanismCandidateKind::NarrativeFailure);
}

#[test]
fn liquidity_trap_fires_on_pressure_without_price_movement() {
    // High capital_flow_pressure but near-zero price_momentum → high absorption_gap
    // → LiquidityImbalance → LiquidityConstraint → LiquidityTrap
    ScenarioBuilder::new("3988.HK")
        .confidence(dec!(0.65))
        .action("watch")
        .signal_full(LiveSignal {
            symbol: "3988.HK".to_string(),
            sector: None,
            composite: dec!(0.30),
            mark_price: None,
            dimension_composite: Some(dec!(0.30)),
            capital_flow_direction: dec!(0.70),
            price_momentum: dec!(0.02),
            volume_profile: dec!(0.45),
            pre_post_market_anomaly: Decimal::ZERO,
            valuation: Decimal::ZERO,
            cross_stock_correlation: None,
            sector_coherence: None,
            cross_market_propagation: None,
        })
        .pressure(dec!(0.75), 14, true)
        .chain(
            "capital_flow",
            &[
                ("large_order_flow", dec!(0.80)),
                ("dark_pool_activity", dec!(0.75)),
            ],
        )
        .stress(dec!(0.30))
        .regime("neutral")
        .assert_primary(MechanismCandidateKind::LiquidityTrap);
}

#[test]
fn event_driven_fires_on_strong_event_catalyst() {
    // 2+ events mentioning the symbol + high pre_post_market_anomaly
    // → EventCatalystActive → EventCatalyst → EventDrivenDislocation
    ScenarioBuilder::new("9988.HK")
        .confidence(dec!(0.75))
        .action("enter")
        .signal_full(LiveSignal {
            symbol: "9988.HK".to_string(),
            sector: None,
            composite: dec!(0.70),
            mark_price: None,
            dimension_composite: Some(dec!(0.70)),
            capital_flow_direction: dec!(0.65),
            price_momentum: dec!(0.55),
            volume_profile: dec!(0.60),
            pre_post_market_anomaly: dec!(0.82),
            valuation: Decimal::ZERO,
            cross_stock_correlation: None,
            sector_coherence: None,
            cross_market_propagation: None,
        })
        .pressure(dec!(0.60), 6, true)
        .event("earnings_beat", dec!(0.90), "9988.HK quarterly earnings significantly exceeded analyst estimates")
        .event("analyst_upgrade", dec!(0.85), "9988.HK receives major analyst upgrade with raised price target")
        .regime("neutral")
        .assert_primary(MechanismCandidateKind::EventDrivenDislocation);
}

#[test]
fn mean_reversion_fires_on_extreme_valuation_and_counter_momentum() {
    // High valuation + large |price_momentum| (stretch) + no flow support → high MeanReversionPressure
    // counter_label + weakening track → CounterevidencePresent → ReversionPressure
    // Keep confidence low, no causal leader → suppress DirectionalReinforcement / ConfidenceBuilds
    // No pressure provided → weak_flow_support = stretch itself (high)
    ScenarioBuilder::new("2331.HK")
        .confidence(dec!(0.50))
        .action("review")
        .counter_label("overextended_mean_reversion_imminent")
        .signal_full(LiveSignal {
            symbol: "2331.HK".to_string(),
            sector: None,
            composite: dec!(-0.15),
            mark_price: None,
            dimension_composite: Some(dec!(-0.15)),
            capital_flow_direction: dec!(-0.10),
            price_momentum: dec!(-0.78),
            volume_profile: dec!(0.30),
            pre_post_market_anomaly: dec!(0.10),
            valuation: dec!(0.90),
            cross_stock_correlation: None,
            sector_coherence: None,
            cross_market_propagation: None,
        })
        // No pressure → weak_flow_support = |price_momentum| = 0.78 (high)
        .track("uptrend thesis", "weakening", 3, dec!(0.38))
        .stress(dec!(0.25))
        .regime("neutral")
        .assert_primary(MechanismCandidateKind::MeanReversionSnapback);
}

#[test]
fn arbitrage_convergence_fires_on_cross_market_dislocation() {
    // High-divergence cross-market anomalies (direction mismatch) + strong cross-market signals
    // → CrossMarketDislocation + CrossMarketLinkActive → CrossMarketDislocation state → ArbitrageConvergence
    ScenarioBuilder::new("5.HK")
        .confidence(dec!(0.72))
        .action("enter")
        .signal_full(LiveSignal {
            symbol: "5.HK".to_string(),
            sector: None,
            composite: dec!(0.50),
            mark_price: None,
            dimension_composite: Some(dec!(0.50)),
            capital_flow_direction: dec!(0.55),
            price_momentum: dec!(0.40),
            volume_profile: dec!(0.45),
            pre_post_market_anomaly: Decimal::ZERO,
            valuation: Decimal::ZERO,
            cross_stock_correlation: None,
            sector_coherence: None,
            cross_market_propagation: Some(dec!(0.78)),
        })
        .pressure(dec!(0.55), 7, false)
        .cross_market_signal("HSBC.US", "5.HK", dec!(0.85))
        .cross_market_signal("JPM.US", "5.HK", dec!(0.75))
        // expected positive but actual strongly negative → high divergence + direction mismatch
        .cross_market_anomaly("HSBC.US", "5.HK", dec!(0.60), dec!(-0.55))
        .cross_market_anomaly("C.US", "5.HK", dec!(0.50), dec!(-0.45))
        .regime("neutral")
        .assert_primary(MechanismCandidateKind::ArbitrageConvergence);
}

#[test]
fn capital_rotation_fires_on_sector_substitution_flow() {
    // "tech" sector: 2 symbols with positive capital_flow_pressure
    // "finance" sector: 2 symbols with NEGATIVE capital_flow_pressure (opposite direction)
    // → SectorRotationPressure → SubstitutionFlow → CapitalRotation
    ScenarioBuilder::new("700.HK")
        .confidence(dec!(0.68))
        .action("enter")
        .signal_full(LiveSignal {
            symbol: "700.HK".to_string(),
            sector: Some("tech".to_string()),
            composite: dec!(0.60),
            mark_price: None,
            dimension_composite: Some(dec!(0.60)),
            capital_flow_direction: dec!(0.65),
            price_momentum: dec!(0.50),
            volume_profile: dec!(0.55),
            pre_post_market_anomaly: Decimal::ZERO,
            valuation: Decimal::ZERO,
            cross_stock_correlation: None,
            sector_coherence: None,
            cross_market_propagation: None,
        })
        .pressure(dec!(0.70), 10, true)
        // Second tech symbol (positive pressure — same direction as case)
        .extra_pressure(LivePressure {
            symbol: "9999.HK".to_string(),
            sector: Some("tech".to_string()),
            capital_flow_pressure: dec!(0.65),
            momentum: dec!(0.55),
            pressure_delta: dec!(0.60),
            pressure_duration: 9,
            accelerating: true,
        })
        // Finance sector — two symbols with negative pressure (opposite direction)
        .extra_pressure(LivePressure {
            symbol: "939.HK".to_string(),
            sector: Some("finance".to_string()),
            capital_flow_pressure: dec!(-0.68),
            momentum: dec!(-0.55),
            pressure_delta: dec!(-0.60),
            pressure_duration: 10,
            accelerating: true,
        })
        .extra_pressure(LivePressure {
            symbol: "1398.HK".to_string(),
            sector: Some("finance".to_string()),
            capital_flow_pressure: dec!(-0.62),
            momentum: dec!(-0.50),
            pressure_delta: dec!(-0.55),
            pressure_duration: 9,
            accelerating: false,
        })
        .regime("neutral")
        .assert_primary(MechanismCandidateKind::CapitalRotation);
}
