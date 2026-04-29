use rust_decimal_macros::dec;

use super::*;
use crate::live_snapshot::{
    LiveBackwardChain, LiveCrossMarketAnomaly, LiveCrossMarketSignal, LiveEvent, LiveEvidence,
    LiveHypothesisTrack, LiveMarketRegime, LivePressure, LiveSignal, LiveStressSnapshot,
    LiveTacticalCase,
};
use crate::ontology::semantics::{HumanReviewReasonKind, HumanReviewVerdict};
use crate::ontology::{ActionDirection, ActionNode, ActionNodeStage, Market, Symbol};

#[test]
fn workflow_rejection_predicate_can_be_injected() {
    let predicates = augment_predicates_with_workflow(&[], "review", Some("reviewer reject"));
    assert!(predicates
        .iter()
        .any(|predicate| predicate.kind == AtomicPredicateKind::HumanRejected));
}

#[test]
fn derive_predicates_returns_structural_signals() {
    let tactical_case = LiveTacticalCase {
        setup_id: "setup:test".into(),
        symbol: "700.HK".into(),
        title: "Test".into(),
        action: "enter".into(),
        confidence: dec!(0.74),
        confidence_gap: dec!(0.12),
        heuristic_edge: dec!(0.08),
        entry_rationale: "test".into(),
        causal_narrative: None,
        review_reason_code: None,
        review_reason_family: None,
        review_reason_subreasons: vec![],
        policy_primary: None,
        policy_reason: None,
        multi_horizon_gate_reason: None,
        family_label: Some("momentum".into()),
        counter_label: Some("mean_reversion".into()),
        matched_success_pattern_signature: None,
        lifecycle_phase: None,
        tension_driver: None,
        driver_class: None,
        is_isolated: None,
        peer_active_count: None,
        peer_silent_count: None,
        peer_confirmation_ratio: None,
        isolation_score: None,
        competition_margin: None,
        driver_confidence: None,
        absence_summary: None,
        competition_summary: None,
        competition_winner: None,
        competition_runner_up: None,
        lifecycle_velocity: None,
        lifecycle_acceleration: None,
        horizon_bucket: None,
        horizon_urgency: None,
        horizon_secondary: vec![],
        case_signature: None,
        archetype_projections: vec![],
        expectation_bindings: vec![],
        expectation_violations: vec![],
        inferred_intent: None,
        freshness_state: None,
        first_enter_tick: None,
        ticks_since_first_enter: None,
        ticks_since_first_seen: None,
        timing_state: None,
        timing_position_in_range: None,
        local_state: None,
        local_state_confidence: None,
        actionability_score: None,
        actionability_state: None,
        confidence_velocity_5t: None,
        support_fraction_velocity_5t: None,
        priority_rank: None,
        state_persistence_ticks: None,
        direction_stability_rounds: None,
        state_reason_codes: vec![],
        raw_disagreement: None,
    };
    let chain = LiveBackwardChain {
        symbol: tactical_case.symbol.clone(),
        conclusion: "up".into(),
        primary_driver: "propagation".into(),
        confidence: dec!(0.7),
        freshness: None,
        evidence: vec![
            LiveEvidence {
                source: "a".into(),
                description: "a".into(),
                weight: dec!(0.8),
                direction: dec!(0.8),
            },
            LiveEvidence {
                source: "b".into(),
                description: "b".into(),
                weight: dec!(0.2),
                direction: dec!(0.1),
            },
        ],
    };
    let pressure = LivePressure {
        symbol: tactical_case.symbol.clone(),
        sector: Some("tech".into()),
        capital_flow_pressure: dec!(0.7),
        momentum: dec!(0.5),
        pressure_delta: dec!(0.4),
        pressure_duration: 6,
        accelerating: true,
    };
    let signal = LiveSignal {
        symbol: tactical_case.symbol.clone(),
        sector: Some("tech".into()),
        composite: dec!(0.65),
        mark_price: Some(dec!(380)),
        dimension_composite: Some(dec!(0.55)),
        capital_flow_direction: dec!(0.6),
        price_momentum: dec!(-0.2),
        volume_profile: dec!(0.3),
        pre_post_market_anomaly: dec!(0.6),
        valuation: dec!(0.1),
        cross_stock_correlation: Some(dec!(0.4)),
        sector_coherence: Some(dec!(0.45)),
        cross_market_propagation: Some(dec!(0.7)),
    };
    let track = LiveHypothesisTrack {
        symbol: tactical_case.symbol.clone(),
        title: "Tech bid".into(),
        status: "weakening".into(),
        age_ticks: 5,
        confidence: dec!(0.69),
    };
    let inputs = PredicateInputs {
        tactical_case: &tactical_case,
        active_positions: &[],
        chain: Some(&chain),
        pressure: Some(&pressure),
        signal: Some(&signal),
        causal: None,
        track: Some(&track),
        stress: &LiveStressSnapshot {
            composite_stress: dec!(0.72),
            sector_synchrony: Some(dec!(0.5)),
            pressure_consensus: Some(dec!(0.6)),
            momentum_consensus: Some(dec!(0.4)),
            pressure_dispersion: Some(dec!(0.45)),
            volume_anomaly: Some(dec!(0.2)),
        },
        market_regime: &LiveMarketRegime {
            bias: "neutral".into(),
            confidence: dec!(0.2),
            breadth_up: dec!(0.4),
            breadth_down: dec!(0.5),
            average_return: dec!(0.01),
            directional_consensus: None,
            pre_market_sentiment: None,
        },
        all_signals: &[
            signal.clone(),
            LiveSignal {
                symbol: "9988.HK".into(),
                sector: Some("Internet".into()),
                composite: dec!(-0.55),
                mark_price: Some(dec!(82)),
                dimension_composite: Some(dec!(-0.45)),
                capital_flow_direction: dec!(-0.6),
                price_momentum: dec!(-0.4),
                volume_profile: dec!(0.1),
                pre_post_market_anomaly: dec!(0),
                valuation: dec!(0.2),
                cross_stock_correlation: None,
                sector_coherence: Some(dec!(-0.5)),
                cross_market_propagation: None,
            },
        ],
        all_pressures: &[
            pressure.clone(),
            LivePressure {
                symbol: "9988.HK".into(),
                sector: Some("Internet".into()),
                capital_flow_pressure: dec!(-0.75),
                momentum: dec!(-0.4),
                pressure_delta: dec!(0.3),
                pressure_duration: 9,
                accelerating: false,
            },
        ],
        events: &[LiveEvent {
            kind: "PreMarketDislocation".into(),
            symbol: Some("700.HK".into()),
            magnitude: dec!(0.8),
            summary: "700.HK pre-market dislocation".into(),
            age_secs: None,
            freshness: None,
        }],
        cross_market_signals: &[LiveCrossMarketSignal {
            us_symbol: "TCEHY.US".into(),
            hk_symbol: tactical_case.symbol.clone(),
            propagation_confidence: dec!(0.66),
            time_since_hk_close_minutes: Some(120),
        }],
        cross_market_anomalies: &[LiveCrossMarketAnomaly {
            us_symbol: "TCEHY.US".into(),
            hk_symbol: tactical_case.symbol.clone(),
            expected_direction: dec!(0.5),
            actual_direction: dec!(-0.2),
            divergence: dec!(0.7),
        }],
    };

    let predicates = derive_atomic_predicates(&inputs);
    assert!(!predicates.is_empty());
    assert!(predicates
        .iter()
        .any(|item| item.kind == AtomicPredicateKind::PressurePersists));
    assert!(predicates
        .iter()
        .any(|item| item.kind == AtomicPredicateKind::CrossMarketLinkActive));
    assert!(predicates
        .iter()
        .any(|item| item.kind == AtomicPredicateKind::EventCatalystActive));
    assert!(predicates
        .iter()
        .any(|item| item.kind == AtomicPredicateKind::SectorRotationPressure));
}

fn base_case(symbol: &str, title: &str) -> LiveTacticalCase {
    LiveTacticalCase {
        setup_id: format!("setup:{symbol}:enter"),
        symbol: symbol.into(),
        title: title.into(),
        action: "enter".into(),
        confidence: dec!(0.8),
        confidence_gap: dec!(0.2),
        heuristic_edge: dec!(0.1),
        entry_rationale: "test".into(),
        causal_narrative: None,
        review_reason_code: None,
        review_reason_family: None,
        review_reason_subreasons: vec![],
        policy_primary: None,
        policy_reason: None,
        multi_horizon_gate_reason: None,
        family_label: Some("Momentum".into()),
        counter_label: None,
        matched_success_pattern_signature: None,
        lifecycle_phase: None,
        tension_driver: None,
        driver_class: None,
        is_isolated: None,
        peer_active_count: None,
        peer_silent_count: None,
        peer_confirmation_ratio: None,
        isolation_score: None,
        competition_margin: None,
        driver_confidence: None,
        absence_summary: None,
        competition_summary: None,
        competition_winner: None,
        competition_runner_up: None,
        lifecycle_velocity: None,
        lifecycle_acceleration: None,
        horizon_bucket: None,
        horizon_urgency: None,
        horizon_secondary: vec![],
        case_signature: None,
        archetype_projections: vec![],
        expectation_bindings: vec![],
        expectation_violations: vec![],
        inferred_intent: None,
        freshness_state: None,
        first_enter_tick: None,
        ticks_since_first_enter: None,
        ticks_since_first_seen: None,
        timing_state: None,
        timing_position_in_range: None,
        local_state: None,
        local_state_confidence: None,
        actionability_score: None,
        actionability_state: None,
        confidence_velocity_5t: None,
        support_fraction_velocity_5t: None,
        priority_rank: None,
        state_persistence_ticks: None,
        direction_stability_rounds: None,
        state_reason_codes: vec![],
        raw_disagreement: None,
    }
}

fn base_signal(symbol: &str, sector: &str, composite: Decimal) -> LiveSignal {
    LiveSignal {
        symbol: symbol.into(),
        sector: Some(sector.into()),
        composite,
        mark_price: Some(dec!(100)),
        dimension_composite: Some(composite),
        capital_flow_direction: composite,
        price_momentum: composite / Decimal::TWO,
        volume_profile: dec!(0.2),
        pre_post_market_anomaly: dec!(0.1),
        valuation: Decimal::ZERO,
        cross_stock_correlation: None,
        sector_coherence: None,
        cross_market_propagation: None,
    }
}

fn active_position(
    symbol: &str,
    sector: &str,
    direction: ActionDirection,
    exit_forming: bool,
) -> ActionNode {
    ActionNode {
        workflow_id: format!("wf:{symbol}"),
        symbol: Symbol(symbol.into()),
        market: Market::Hk,
        sector: Some(sector.into()),
        stage: ActionNodeStage::Monitoring,
        direction,
        entry_confidence: dec!(0.7),
        current_confidence: dec!(0.8),
        entry_price: Some(dec!(95)),
        pnl: Some(dec!(0.05)),
        age_ticks: 12,
        degradation_score: Some(if exit_forming { dec!(0.9) } else { dec!(0.2) }),
        exit_forming,
    }
}

#[test]
fn counterevidence_flips_keep_full_configured_weight() {
    let tactical_case = base_case("700.HK", "700.HK tactical case");
    let causal = crate::live_snapshot::LiveCausalLeader {
        symbol: "700.HK".into(),
        current_leader: "leader".into(),
        leader_streak: 1,
        flips: 4,
    };
    let predicate = counterevidence_present(&PredicateInputs {
        tactical_case: &tactical_case,
        active_positions: &[],
        chain: None,
        pressure: None,
        signal: None,
        causal: Some(&causal),
        track: None,
        stress: &LiveStressSnapshot {
            composite_stress: Decimal::ZERO,
            sector_synchrony: None,
            pressure_consensus: None,
            momentum_consensus: None,
            pressure_dispersion: None,
            volume_anomaly: None,
        },
        market_regime: &LiveMarketRegime {
            bias: "neutral".into(),
            confidence: Decimal::ZERO,
            breadth_up: Decimal::ZERO,
            breadth_down: Decimal::ZERO,
            average_return: Decimal::ZERO,
            directional_consensus: None,
            pre_market_sentiment: None,
        },
        all_signals: &[],
        all_pressures: &[],
        events: &[],
        cross_market_signals: &[],
        cross_market_anomalies: &[],
    });

    assert_eq!(predicate.score, dec!(0.30));
}

#[test]
fn position_conflict_predicate_triggers_on_opposing_active_position() {
    let tactical_case = base_case("700.HK", "Long 700.HK");
    let signal = base_signal("700.HK", "tech", dec!(0.7));
    let active_positions = [active_position(
        "700.HK",
        "tech",
        ActionDirection::Short,
        false,
    )];

    let predicates = derive_atomic_predicates(&PredicateInputs {
        tactical_case: &tactical_case,
        active_positions: &active_positions,
        chain: None,
        pressure: None,
        signal: Some(&signal),
        causal: None,
        track: None,
        stress: &LiveStressSnapshot {
            composite_stress: Decimal::ZERO,
            sector_synchrony: None,
            pressure_consensus: None,
            momentum_consensus: None,
            pressure_dispersion: None,
            volume_anomaly: None,
        },
        market_regime: &LiveMarketRegime {
            bias: "neutral".into(),
            confidence: Decimal::ZERO,
            breadth_up: Decimal::ZERO,
            breadth_down: Decimal::ZERO,
            average_return: Decimal::ZERO,
            directional_consensus: None,
            pre_market_sentiment: None,
        },
        all_signals: std::slice::from_ref(&signal),
        all_pressures: &[],
        events: &[],
        cross_market_signals: &[],
        cross_market_anomalies: &[],
    });

    assert!(predicates
        .iter()
        .any(|predicate| predicate.kind == AtomicPredicateKind::PositionConflict));
}

#[test]
fn reinforcement_and_exit_predicates_use_active_position_overlay() {
    let tactical_case = base_case("700.HK", "Long 700.HK");
    let signal = base_signal("700.HK", "tech", dec!(0.7));
    let active_positions = [active_position(
        "700.HK",
        "tech",
        ActionDirection::Long,
        true,
    )];

    let predicates = derive_atomic_predicates(&PredicateInputs {
        tactical_case: &tactical_case,
        active_positions: &active_positions,
        chain: None,
        pressure: None,
        signal: Some(&signal),
        causal: None,
        track: None,
        stress: &LiveStressSnapshot {
            composite_stress: Decimal::ZERO,
            sector_synchrony: None,
            pressure_consensus: None,
            momentum_consensus: None,
            pressure_dispersion: None,
            volume_anomaly: None,
        },
        market_regime: &LiveMarketRegime {
            bias: "neutral".into(),
            confidence: Decimal::ZERO,
            breadth_up: Decimal::ZERO,
            breadth_down: Decimal::ZERO,
            average_return: Decimal::ZERO,
            directional_consensus: None,
            pre_market_sentiment: None,
        },
        all_signals: std::slice::from_ref(&signal),
        all_pressures: &[],
        events: &[],
        cross_market_signals: &[],
        cross_market_anomalies: &[],
    });

    assert!(predicates
        .iter()
        .any(|predicate| predicate.kind == AtomicPredicateKind::PositionReinforcement));
    assert!(predicates
        .iter()
        .any(|predicate| predicate.kind == AtomicPredicateKind::ExitConditionForming));
}

#[test]
fn concentration_risk_triggers_for_same_sector_stack() {
    let tactical_case = base_case("700.HK", "Long 700.HK");
    let signal = base_signal("700.HK", "tech", dec!(0.7));
    let active_positions = [
        active_position("700.HK", "tech", ActionDirection::Long, false),
        active_position("9988.HK", "tech", ActionDirection::Long, false),
        active_position("3690.HK", "tech", ActionDirection::Long, false),
    ];

    let predicates = derive_atomic_predicates(&PredicateInputs {
        tactical_case: &tactical_case,
        active_positions: &active_positions,
        chain: None,
        pressure: None,
        signal: Some(&signal),
        causal: None,
        track: None,
        stress: &LiveStressSnapshot {
            composite_stress: Decimal::ZERO,
            sector_synchrony: None,
            pressure_consensus: None,
            momentum_consensus: None,
            pressure_dispersion: None,
            volume_anomaly: None,
        },
        market_regime: &LiveMarketRegime {
            bias: "neutral".into(),
            confidence: Decimal::ZERO,
            breadth_up: Decimal::ZERO,
            breadth_down: Decimal::ZERO,
            average_return: Decimal::ZERO,
            directional_consensus: None,
            pre_market_sentiment: None,
        },
        all_signals: std::slice::from_ref(&signal),
        all_pressures: &[],
        events: &[],
        cross_market_signals: &[],
        cross_market_anomalies: &[],
    });

    assert!(predicates
        .iter()
        .any(|predicate| predicate.kind == AtomicPredicateKind::ConcentrationRisk));
}

#[test]
fn human_review_context_is_structured() {
    let review = derive_human_review_context(
        "review",
        Some("reject thesis, timing too early, risk too high"),
    )
    .expect("human review");
    assert_eq!(review.verdict, HumanReviewVerdict::Rejected);
    assert!(review
        .reasons
        .iter()
        .any(|item| item.kind == HumanReviewReasonKind::MechanismMismatch));
    assert!(review
        .reasons
        .iter()
        .any(|item| item.kind == HumanReviewReasonKind::TimingMismatch));
}
