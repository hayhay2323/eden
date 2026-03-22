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
