use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use super::*;
use crate::action::workflow::ActionExecutionPolicy;
#[cfg(feature = "persistence")]
use crate::cases::reasoning_story::describe_mechanism_transition;
use crate::cases::review_analytics::build_case_review_analytics;
#[cfg(feature = "persistence")]
use crate::cases::review_analytics::build_case_review_analytics_with_assessments;
use crate::live_snapshot::{
    LiveMarket, LiveMarketRegime, LiveScorecard, LiveSnapshot, LiveStressSnapshot, LiveTacticalCase,
};
#[cfg(feature = "persistence")]
use crate::persistence::case_reasoning_assessment::CaseReasoningAssessmentRecord;
use crate::CaseReasoningProfile;
#[cfg(feature = "persistence")]
use time::OffsetDateTime;

fn empty_context() -> CaseMarketContext {
    CaseMarketContext {
        market: LiveMarket::Hk,
        tick: 1,
        timestamp: "2026-03-24T00:00:00Z".into(),
        stock_count: 0,
        edge_count: 0,
        hypothesis_count: 0,
        observation_count: 0,
        active_positions: 0,
        market_regime: LiveMarketRegime {
            bias: "neutral".into(),
            confidence: Decimal::ZERO,
            breadth_up: Decimal::ZERO,
            breadth_down: Decimal::ZERO,
            average_return: Decimal::ZERO,
            directional_consensus: None,
            pre_market_sentiment: None,
        },
        stress: LiveStressSnapshot {
            composite_stress: Decimal::ZERO,
            sector_synchrony: None,
            pressure_consensus: None,
            momentum_consensus: None,
            pressure_dispersion: None,
            volume_anomaly: None,
        },
        scorecard: LiveScorecard::default(),
        events: vec![],
        cross_market_signals: vec![],
        cross_market_anomalies: vec![],
        lineage: vec![],
    }
}

fn case_summary_with_mechanisms(primary: &str, competing: &[&str]) -> CaseSummary {
    CaseSummary {
        case_id: format!("case:{primary}"),
        setup_id: format!("setup:{primary}"),
        workflow_id: None,
        execution_policy: None,
        owner: None,
        reviewer: None,
        queue_pin: None,
        workflow_actor: None,
        workflow_note: None,
        symbol: "700.HK".into(),
        title: "Test Case".into(),
        sector: Some("Technology".into()),
        market: LiveMarket::Hk,
        recommended_action: "enter".into(),
        workflow_state: "suggest".into(),
        governance: None,
        governance_bucket: "review_required".into(),
        governance_reason_code: None,
        governance_reason: None,
        market_regime_bias: "neutral".into(),
        market_regime_confidence: Decimal::ZERO,
        market_breadth_delta: Decimal::ZERO,
        market_average_return: Decimal::ZERO,
        market_directional_consensus: None,
        confidence: dec!(0.6),
        confidence_gap: dec!(0.2),
        heuristic_edge: dec!(0.1),
        review_reason_code: None,
        review_reason_family: None,
        review_reason_subreasons: vec![],
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
        driver_confidence: None,
        absence_summary: None,
        competition_summary: None,
        competition_winner: None,
        competition_runner_up: None,
        priority_rank: None,
        state_persistence_ticks: None,
        direction_stability_rounds: None,
        state_reason_codes: vec![],
        why_now: "why".into(),
        primary_lens: None,
        primary_driver: None,
        family_label: None,
        counter_label: None,
        hypothesis_status: None,
        current_leader: None,
        flip_count: 0,
        leader_streak: None,
        case_signature: None,
        archetype_projections: vec![],
        inferred_intent: None,
        intent_opportunities: vec![],
        expectation_binding_count: 0,
        expectation_violation_count: 0,
        key_evidence: vec![],
        invalidation_rules: vec![],
        reasoning_profile: CaseReasoningProfile {
            laws: vec![],
            predicates: vec![],
            composite_states: vec![],
            human_review: None,
            primary_mechanism: Some(crate::ontology::MechanismCandidate {
                kind: crate::ontology::MechanismCandidateKind::LiquidityTrap,
                label: primary.into(),
                score: dec!(0.8),
                summary: "summary".into(),
                supporting_states: vec![],
                invalidation: vec![],
                human_checks: vec![],
                factors: vec![],
                counterfactuals: vec![],
            }),
            competing_mechanisms: competing
                .iter()
                .map(|label| crate::ontology::MechanismCandidate {
                    kind: crate::ontology::MechanismCandidateKind::CapitalRotation,
                    label: (*label).into(),
                    score: dec!(0.4),
                    summary: "summary".into(),
                    supporting_states: vec![],
                    invalidation: vec![],
                    human_checks: vec![],
                    factors: vec![],
                    counterfactuals: vec![],
                })
                .collect(),
            automated_invalidations: vec![],
        },
        updated_at: "2026-03-24T00:00:00Z".into(),
        case_resolution: None,
        horizon_breakdown: None,
    }
}

#[test]
fn actionable_cases_sort_first() {
    let snapshot = LiveSnapshot {
        tick: 1,
        timestamp: "2026-03-22T09:30:00Z".into(),
        market: LiveMarket::Us,
        market_phase: "cash_session".into(),
        market_active: true,
        stock_count: 2,
        edge_count: 1,
        hypothesis_count: 1,
        observation_count: 1,
        active_positions: 0,
        active_position_nodes: vec![],
        market_regime: LiveMarketRegime {
            bias: "risk_on".into(),
            confidence: dec!(0.6),
            breadth_up: dec!(0.5),
            breadth_down: dec!(0.4),
            average_return: dec!(0.02),
            directional_consensus: None,
            pre_market_sentiment: None,
        },
        stress: LiveStressSnapshot {
            composite_stress: dec!(0.1),
            sector_synchrony: None,
            pressure_consensus: None,
            momentum_consensus: None,
            pressure_dispersion: None,
            volume_anomaly: None,
        },
        scorecard: LiveScorecard {
            total_signals: 1,
            resolved_signals: 1,
            hits: 1,
            misses: 0,
            hit_rate: dec!(1),
            mean_return: dec!(0.03),
            ..LiveScorecard::default()
        },
        tactical_cases: vec![
            LiveTacticalCase {
                setup_id: "setup:b".into(),
                symbol: "B.US".into(),
                title: "Watch B".into(),
                action: "review".into(),
                confidence: dec!(0.4),
                confidence_gap: dec!(0.1),
                heuristic_edge: dec!(0.02),
                entry_rationale: "watch".into(),
                causal_narrative: None,
                review_reason_code: None,
                review_reason_family: None,
                review_reason_subreasons: vec![],
                policy_primary: None,
                policy_reason: None,
                multi_horizon_gate_reason: None,
                family_label: None,
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
            },
            LiveTacticalCase {
                setup_id: "setup:a".into(),
                symbol: "A.US".into(),
                title: "Long A".into(),
                action: "enter".into(),
                confidence: dec!(0.7),
                confidence_gap: dec!(0.2),
                heuristic_edge: dec!(0.05),
                entry_rationale: "go".into(),
                causal_narrative: None,
                review_reason_code: None,
                review_reason_family: None,
                review_reason_subreasons: vec![],
                policy_primary: None,
                policy_reason: None,
                multi_horizon_gate_reason: None,
                family_label: None,
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
            },
        ],
        hypothesis_tracks: vec![],
        recent_transitions: vec![],
        top_signals: vec![],
        convergence_scores: vec![],
        pressures: vec![],
        backward_chains: vec![],
        causal_leaders: vec![],
        events: vec![],
        cross_market_signals: vec![],
        cross_market_anomalies: vec![],
        structural_deltas: vec![],
        propagation_senses: vec![],
        raw_microstructure: vec![],
        raw_sources: vec![],
        signal_translation_gaps: vec![],
        cluster_states: vec![],
        symbol_states: vec![],
        world_summary: None,
        temporal_bars: vec![],
        lineage: vec![],
        success_patterns: vec![],
    };

    let cases = build_case_summaries(&snapshot);
    assert_eq!(cases[0].setup_id, "setup:a");
    assert!(!cases[0].reasoning_profile.predicates.is_empty());
}

#[test]
fn case_list_can_filter_by_opportunity_horizon_and_bias() {
    let snapshot = LiveSnapshot {
        tick: 1,
        timestamp: "2026-03-22T09:30:00Z".into(),
        market: LiveMarket::Us,
        market_phase: "cash_session".into(),
        market_active: true,
        stock_count: 1,
        edge_count: 1,
        hypothesis_count: 1,
        observation_count: 1,
        active_positions: 0,
        active_position_nodes: vec![],
        market_regime: LiveMarketRegime {
            bias: "neutral".into(),
            confidence: dec!(0.4),
            breadth_up: dec!(0.3),
            breadth_down: dec!(0.2),
            average_return: dec!(0.01),
            directional_consensus: None,
            pre_market_sentiment: None,
        },
        stress: LiveStressSnapshot {
            composite_stress: dec!(0.1),
            sector_synchrony: None,
            pressure_consensus: None,
            momentum_consensus: None,
            pressure_dispersion: None,
            volume_anomaly: None,
        },
        scorecard: LiveScorecard {
            total_signals: 1,
            ..LiveScorecard::default()
        },
        tactical_cases: vec![LiveTacticalCase {
            setup_id: "setup:intent".into(),
            symbol: "FICO.US".into(),
            title: "Long FICO".into(),
            action: "enter".into(),
            confidence: dec!(0.9),
            confidence_gap: dec!(0.1),
            heuristic_edge: dec!(0.2),
            entry_rationale: "volume expansion with peer silence".into(),
            causal_narrative: None,
            review_reason_code: None,
            review_reason_family: None,
            review_reason_subreasons: vec![],
            policy_primary: None,
            policy_reason: None,
            multi_horizon_gate_reason: None,
            family_label: Some("Directed Flow".into()),
            counter_label: None,
            matched_success_pattern_signature: None,
            lifecycle_phase: Some("Growing".into()),
            tension_driver: Some("trade_flow".into()),
            driver_class: None,
            is_isolated: Some(true),
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
            inferred_intent: Some(crate::ontology::IntentHypothesis {
                intent_id: "intent:setup:intent".into(),
                kind: crate::ontology::IntentKind::FailedPropagation,
                scope: crate::ontology::ReasoningScope::Symbol(crate::ontology::Symbol(
                    "FICO.US".into(),
                )),
                direction: crate::ontology::IntentDirection::Buy,
                state: crate::ontology::IntentState::Active,
                confidence: dec!(0.9),
                urgency: dec!(0.7),
                persistence: dec!(0.6),
                conflict_score: dec!(0.7),
                strength: crate::ontology::IntentStrength {
                    flow_strength: dec!(0.8),
                    impact_strength: dec!(0.7),
                    persistence_strength: dec!(0.6),
                    propagation_strength: dec!(0.5),
                    resistance_strength: dec!(0.3),
                    composite: dec!(0.6),
                },
                propagation_targets: vec![],
                supporting_archetypes: vec!["emergent".into()],
                supporting_case_signature: None,
                expectation_bindings: vec![],
                expectation_violations: vec![],
                exit_signals: vec![],
                opportunities: vec![],
                falsifiers: vec![],
                rationale: "failed propagation buy intent".into(),
            }),
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
        }],
        hypothesis_tracks: vec![],
        recent_transitions: vec![],
        top_signals: vec![],
        convergence_scores: vec![],
        pressures: vec![],
        backward_chains: vec![],
        causal_leaders: vec![],
        events: vec![],
        cross_market_signals: vec![],
        cross_market_anomalies: vec![],
        structural_deltas: vec![],
        propagation_senses: vec![],
        raw_microstructure: vec![],
        raw_sources: vec![],
        signal_translation_gaps: vec![],
        temporal_bars: vec![
            crate::live_snapshot::LiveTemporalBar {
                horizon: "5m".into(),
                symbol: "FICO.US".into(),
                bucket_started_at: "2026-03-22T09:25:00Z".into(),
                open: Some(dec!(100)),
                high: Some(dec!(102)),
                low: Some(dec!(99)),
                close: Some(dec!(102)),
                composite_open: dec!(0.3),
                composite_high: dec!(0.8),
                composite_low: dec!(0.2),
                composite_close: dec!(0.7),
                composite_mean: dec!(0.5),
                capital_flow_sum: dec!(0.8),
                capital_flow_delta: dec!(0.6),
                volume_total: 1000,
                event_count: 2,
                signal_persistence: 4,
            },
            crate::live_snapshot::LiveTemporalBar {
                horizon: "30m".into(),
                symbol: "FICO.US".into(),
                bucket_started_at: "2026-03-22T09:00:00Z".into(),
                open: Some(dec!(98)),
                high: Some(dec!(102)),
                low: Some(dec!(97)),
                close: Some(dec!(102)),
                composite_open: dec!(0.2),
                composite_high: dec!(0.7),
                composite_low: dec!(0.1),
                composite_close: dec!(0.5),
                composite_mean: dec!(0.4),
                capital_flow_sum: dec!(0.7),
                capital_flow_delta: dec!(0.4),
                volume_total: 5000,
                event_count: 5,
                signal_persistence: 5,
            },
        ],
        cluster_states: vec![],
        symbol_states: vec![],
        world_summary: None,
        lineage: vec![],
        success_patterns: vec![],
    };

    let mut list = build_case_list(&snapshot);
    assert_eq!(list.cases.len(), 1);
    assert!(!list.cases[0].intent_opportunities.is_empty());

    filter_case_list_by_opportunity(&mut list, Some("5m"), Some("enter"));
    assert_eq!(list.cases.len(), 1);

    filter_case_list_by_opportunity(&mut list, Some("30m"), Some("exit"));
    assert!(list.cases.is_empty());
}

#[test]
fn build_case_summaries_assigns_primary_lens_and_buckets() {
    let snapshot = LiveSnapshot {
        tick: 1,
        timestamp: "2026-03-22T09:30:00Z".into(),
        market: LiveMarket::Hk,
        market_phase: "cash_session".into(),
        market_active: true,
        stock_count: 3,
        edge_count: 0,
        hypothesis_count: 0,
        observation_count: 0,
        active_positions: 0,
        active_position_nodes: vec![],
        market_regime: LiveMarketRegime {
            bias: "neutral".into(),
            confidence: dec!(0.5),
            breadth_up: dec!(0.4),
            breadth_down: dec!(0.3),
            average_return: dec!(0.01),
            directional_consensus: None,
            pre_market_sentiment: None,
        },
        stress: LiveStressSnapshot {
            composite_stress: dec!(0.1),
            sector_synchrony: None,
            pressure_consensus: None,
            momentum_consensus: None,
            pressure_dispersion: None,
            volume_anomaly: None,
        },
        scorecard: LiveScorecard::default(),
        tactical_cases: vec![
            LiveTacticalCase {
                setup_id: "setup:ice".into(),
                symbol: "ICE.HK".into(),
                title: "Iceberg Case".into(),
                action: "enter".into(),
                confidence: dec!(0.7),
                confidence_gap: dec!(0.2),
                heuristic_edge: dec!(0.06),
                entry_rationale: "ice".into(),
                causal_narrative: None,
                review_reason_code: None,
                review_reason_family: None,
                review_reason_subreasons: vec![],
                policy_primary: None,
                policy_reason: None,
                multi_horizon_gate_reason: None,
                family_label: None,
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
            },
            LiveTacticalCase {
                setup_id: "setup:cause".into(),
                symbol: "CAUSE.HK".into(),
                title: "Causal Case".into(),
                action: "review".into(),
                confidence: dec!(0.6),
                confidence_gap: dec!(0.1),
                heuristic_edge: dec!(0.04),
                entry_rationale: "cause".into(),
                causal_narrative: None,
                review_reason_code: None,
                review_reason_family: None,
                review_reason_subreasons: vec![],
                policy_primary: None,
                policy_reason: None,
                multi_horizon_gate_reason: None,
                family_label: None,
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
            },
            LiveTacticalCase {
                setup_id: "setup:lineage".into(),
                symbol: "LINE.HK".into(),
                title: "Lineage Case".into(),
                action: "review".into(),
                confidence: dec!(0.5),
                confidence_gap: dec!(0.1),
                heuristic_edge: dec!(0.03),
                entry_rationale: "lineage".into(),
                causal_narrative: None,
                review_reason_code: None,
                review_reason_family: None,
                review_reason_subreasons: vec![],
                policy_primary: None,
                policy_reason: None,
                multi_horizon_gate_reason: None,
                family_label: Some("Directed Flow".into()),
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
            },
        ],
        hypothesis_tracks: vec![],
        recent_transitions: vec![],
        top_signals: vec![],
        convergence_scores: vec![],
        pressures: vec![],
        backward_chains: vec![crate::live_snapshot::LiveBackwardChain {
            symbol: "CAUSE.HK".into(),
            conclusion: "causal".into(),
            primary_driver: "driver".into(),
            confidence: dec!(0.7),
            freshness: None,
            evidence: vec![],
        }],
        causal_leaders: vec![],
        events: vec![crate::live_snapshot::LiveEvent {
            kind: "IcebergDetected".into(),
            symbol: Some("ICE.HK".into()),
            magnitude: dec!(0.8),
            summary: "iceberg".into(),
            age_secs: None,
            freshness: None,
        }],
        cross_market_signals: vec![],
        cross_market_anomalies: vec![],
        structural_deltas: vec![],
        propagation_senses: vec![],
        raw_microstructure: vec![],
        raw_sources: vec![],
        signal_translation_gaps: vec![],
        cluster_states: vec![],
        symbol_states: vec![],
        world_summary: None,
        temporal_bars: vec![],
        success_patterns: vec![],
        lineage: vec![crate::live_snapshot::LiveLineageMetric {
            horizon: None,
            template: "Directed Flow".into(),
            total: 12,
            resolved: 10,
            hits: 7,
            hit_rate: dec!(0.7),
            mean_return: dec!(0.02),
        }],
    };

    let list = build_case_list(&snapshot);
    let cases_by_symbol = list
        .cases
        .iter()
        .map(|case| (case.symbol.as_str(), case.primary_lens.as_deref()))
        .collect::<std::collections::HashMap<_, _>>();

    assert_eq!(
        cases_by_symbol.get("ICE.HK").copied().flatten(),
        Some("iceberg")
    );
    assert_eq!(
        cases_by_symbol.get("CAUSE.HK").copied().flatten(),
        Some("causal")
    );
    assert_eq!(
        cases_by_symbol.get("LINE.HK").copied().flatten(),
        Some("lineage_prior")
    );
    assert_eq!(
        list.primary_lens_buckets
            .buckets
            .get("iceberg")
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        list.primary_lens_buckets
            .buckets
            .get("causal")
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        list.primary_lens_buckets
            .buckets
            .get("lineage_prior")
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn filter_case_list_by_mechanism_matches_primary_and_competing_labels() {
    let mut response = CaseListResponse {
        context: empty_context(),
        cases: vec![
            case_summary_with_mechanisms("Liquidity Trap", &[]),
            case_summary_with_mechanisms("Capital Rotation", &["Liquidity Trap"]),
            case_summary_with_mechanisms("Narrative Failure", &[]),
        ],
        governance_buckets: CaseGovernanceBuckets::default(),
        governance_reason_buckets: CaseGovernanceReasonBuckets::default(),
        primary_lens_buckets: CasePrimaryLensBuckets::default(),
        queue_pin_buckets: CaseQueuePinBuckets::default(),
    };

    filter_case_list_by_mechanism(&mut response, Some("liquidity_trap"));

    assert_eq!(response.cases.len(), 2);
}

#[test]
fn filter_case_list_by_governance_reason_code_matches_inferred_reason() {
    let mut response = CaseListResponse {
        context: empty_context(),
        cases: vec![
            case_summary_with_mechanisms("Liquidity Trap", &[]),
            case_summary_with_mechanisms("Capital Rotation", &[]),
        ],
        governance_buckets: CaseGovernanceBuckets::default(),
        governance_reason_buckets: CaseGovernanceReasonBuckets::default(),
        primary_lens_buckets: CasePrimaryLensBuckets::default(),
        queue_pin_buckets: CaseQueuePinBuckets::default(),
    };
    response.cases[0].governance_reason_code =
        Some(crate::action::workflow::ActionGovernanceReasonCode::AdvisoryAction);
    response.cases[1].governance_reason_code =
        Some(crate::action::workflow::ActionGovernanceReasonCode::SeverityRequiresReview);

    filter_case_list_by_governance_reason_code(
        &mut response,
        Some(crate::action::workflow::ActionGovernanceReasonCode::SeverityRequiresReview),
    );
    refresh_case_list_governance(&mut response);

    assert_eq!(response.cases.len(), 1);
    assert_eq!(response.cases[0].setup_id, "setup:Capital Rotation");
    assert_eq!(response.governance_reason_buckets.buckets.len(), 1);
    assert_eq!(
        response
            .governance_reason_buckets
            .buckets
            .get(&crate::action::workflow::ActionGovernanceReasonCode::SeverityRequiresReview)
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn filter_case_list_by_queue_pin_supports_exact_and_any() {
    let mut response = CaseListResponse {
        context: empty_context(),
        cases: vec![
            case_summary_with_mechanisms("Liquidity Trap", &[]),
            case_summary_with_mechanisms("Capital Rotation", &[]),
            case_summary_with_mechanisms("Event Catalyst", &[]),
        ],
        governance_buckets: CaseGovernanceBuckets::default(),
        governance_reason_buckets: CaseGovernanceReasonBuckets::default(),
        primary_lens_buckets: CasePrimaryLensBuckets::default(),
        queue_pin_buckets: CaseQueuePinBuckets::default(),
    };
    response.cases[0].queue_pin = Some("frontend-review-list".into());
    response.cases[1].queue_pin = Some("pm-desk".into());

    let mut exact = response.clone();
    filter_case_list_by_queue_pin(&mut exact, Some("frontend-review-list"));
    refresh_case_list_governance(&mut exact);
    assert_eq!(exact.cases.len(), 1);
    assert_eq!(exact.cases[0].setup_id, "setup:Liquidity Trap");
    assert_eq!(exact.queue_pin_buckets.pinned.len(), 1);

    let mut any = response.clone();
    filter_case_list_by_queue_pin(&mut any, Some("any"));
    refresh_case_list_governance(&mut any);
    assert_eq!(any.cases.len(), 2);
    assert_eq!(any.queue_pin_buckets.pinned.len(), 2);

    let mut none = response;
    filter_case_list_by_queue_pin(&mut none, Some("none"));
    refresh_case_list_governance(&mut none);
    assert_eq!(none.cases.len(), 1);
    assert_eq!(none.cases[0].setup_id, "setup:Event Catalyst");
    assert_eq!(none.queue_pin_buckets.unpinned.len(), 1);
}

#[test]
fn filter_case_list_by_primary_lens_supports_exact_and_any() {
    let mut response = CaseListResponse {
        context: empty_context(),
        cases: vec![
            case_summary_with_mechanisms("Liquidity Trap", &[]),
            case_summary_with_mechanisms("Capital Rotation", &[]),
            case_summary_with_mechanisms("Event Catalyst", &[]),
        ],
        governance_buckets: CaseGovernanceBuckets::default(),
        governance_reason_buckets: CaseGovernanceReasonBuckets::default(),
        primary_lens_buckets: CasePrimaryLensBuckets::default(),
        queue_pin_buckets: CaseQueuePinBuckets::default(),
    };
    response.cases[0].primary_lens = Some("iceberg".into());
    response.cases[1].primary_lens = Some("causal".into());

    let mut exact = response.clone();
    filter_case_list_by_primary_lens(&mut exact, Some("iceberg"));
    refresh_case_list_governance(&mut exact);
    assert_eq!(exact.cases.len(), 1);
    assert_eq!(exact.cases[0].setup_id, "setup:Liquidity Trap");
    assert_eq!(
        exact
            .primary_lens_buckets
            .buckets
            .get("iceberg")
            .map(Vec::len),
        Some(1)
    );

    let mut any = response.clone();
    filter_case_list_by_primary_lens(&mut any, Some("any"));
    refresh_case_list_governance(&mut any);
    assert_eq!(any.cases.len(), 2);
    assert_eq!(any.primary_lens_buckets.buckets.len(), 2);

    let mut none = response;
    filter_case_list_by_primary_lens(&mut none, Some("unknown"));
    refresh_case_list_governance(&mut none);
    assert_eq!(none.cases.len(), 1);
    assert_eq!(none.cases[0].setup_id, "setup:Event Catalyst");
    assert_eq!(
        none.primary_lens_buckets
            .buckets
            .get("unknown")
            .map(Vec::len),
        Some(1)
    );
}

#[test]
fn case_briefing_and_review_include_governance_policy_counts() {
    let mut actionable = case_summary_with_mechanisms("Liquidity Trap", &[]);
    actionable.recommended_action = "enter".into();
    actionable.workflow_state = "suggest".into();
    actionable.execution_policy = Some(ActionExecutionPolicy::ReviewRequired);

    let mut watch = case_summary_with_mechanisms("Capital Rotation", &[]);
    watch.recommended_action = "watch".into();
    watch.workflow_state = "review".into();
    watch.execution_policy = Some(ActionExecutionPolicy::ManualOnly);

    let mut auto = case_summary_with_mechanisms("Event Catalyst", &[]);
    auto.recommended_action = "enter".into();
    auto.workflow_state = "confirm".into();
    auto.execution_policy = Some(ActionExecutionPolicy::AutoEligible);

    let list = CaseListResponse {
        context: empty_context(),
        cases: vec![actionable, watch, auto],
        governance_buckets: CaseGovernanceBuckets::default(),
        governance_reason_buckets: CaseGovernanceReasonBuckets::default(),
        primary_lens_buckets: CasePrimaryLensBuckets::default(),
        queue_pin_buckets: CaseQueuePinBuckets::default(),
    };

    let briefing = build_case_briefing(&list);
    assert_eq!(briefing.metrics.manual_only, 1);
    assert_eq!(briefing.metrics.review_required, 1);
    assert_eq!(briefing.metrics.auto_eligible, 1);
    assert_eq!(briefing.metrics.queue_pinned, 0);
    assert_eq!(briefing.governance_buckets.manual_only.len(), 1);
    assert_eq!(briefing.governance_buckets.review_required.len(), 1);
    assert_eq!(briefing.governance_buckets.auto_eligible.len(), 1);
    assert_eq!(briefing.primary_lens_buckets.buckets.len(), 1);
    assert_eq!(briefing.queue_pin_buckets.pinned.len(), 0);

    let review = build_case_review(&list);
    assert_eq!(review.metrics.manual_only, 1);
    assert_eq!(review.metrics.review_required, 1);
    assert_eq!(review.metrics.auto_eligible, 1);
    assert_eq!(review.metrics.queue_pinned, 0);
    assert_eq!(review.governance_buckets.manual_only.len(), 1);
    assert_eq!(review.governance_buckets.review_required.len(), 1);
    assert_eq!(review.governance_buckets.auto_eligible.len(), 1);
    assert_eq!(review.primary_lens_buckets.buckets.len(), 1);
    assert_eq!(review.queue_pin_buckets.pinned.len(), 0);
}

#[test]
fn governance_reason_buckets_group_and_sort_review_cases() {
    let mut severity = case_summary_with_mechanisms("Liquidity Trap", &[]);
    severity.workflow_state = "review".into();
    severity.execution_policy = Some(ActionExecutionPolicy::ReviewRequired);
    severity.governance_reason_code =
        Some(crate::action::workflow::ActionGovernanceReasonCode::SeverityRequiresReview);
    severity.governance_reason =
        Some("severity=`high` forces human review before `enter` can execute".into());
    severity.symbol = "A.US".into();
    severity.confidence = dec!(0.80);

    let mut invalidation = case_summary_with_mechanisms("Capital Rotation", &[]);
    invalidation.workflow_state = "review".into();
    invalidation.execution_policy = Some(ActionExecutionPolicy::ReviewRequired);
    invalidation.governance_reason_code =
        Some(crate::action::workflow::ActionGovernanceReasonCode::InvalidationRuleMissing);
    invalidation.governance_reason =
        Some("missing invalidation rule keeps this recommendation in review-required mode".into());
    invalidation.symbol = "B.US".into();
    invalidation.confidence = dec!(0.70);

    let mut alpha = case_summary_with_mechanisms("Event Catalyst", &[]);
    alpha.workflow_state = "review".into();
    alpha.execution_policy = Some(ActionExecutionPolicy::ReviewRequired);
    alpha.governance_reason_code =
        Some(crate::action::workflow::ActionGovernanceReasonCode::NonPositiveExpectedAlpha);
    alpha.governance_reason = Some(
        "non-positive expected alpha keeps this recommendation in review-required mode".into(),
    );
    alpha.symbol = "C.US".into();
    alpha.confidence = dec!(0.60);

    let list = CaseListResponse {
        context: empty_context(),
        cases: vec![alpha.clone(), invalidation.clone(), severity.clone()],
        governance_buckets: CaseGovernanceBuckets::default(),
        governance_reason_buckets: CaseGovernanceReasonBuckets::default(),
        primary_lens_buckets: CasePrimaryLensBuckets::default(),
        queue_pin_buckets: CaseQueuePinBuckets::default(),
    };

    let briefing = build_case_briefing(&list);
    let review = build_case_review(&list);

    assert_eq!(
        briefing
            .governance_reason_buckets
            .buckets
            .get(&crate::action::workflow::ActionGovernanceReasonCode::SeverityRequiresReview)
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        briefing
            .governance_reason_buckets
            .buckets
            .get(&crate::action::workflow::ActionGovernanceReasonCode::InvalidationRuleMissing)
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(
        briefing
            .governance_reason_buckets
            .buckets
            .get(&crate::action::workflow::ActionGovernanceReasonCode::NonPositiveExpectedAlpha)
            .map(Vec::len),
        Some(1)
    );
    assert_eq!(briefing.review_cases[0].symbol, "A.US");
    assert_eq!(briefing.review_cases[1].symbol, "B.US");
    assert_eq!(briefing.review_cases[2].symbol, "C.US");
    assert_eq!(review.buckets.under_review[0].symbol, "A.US");
    assert_eq!(review.buckets.under_review[1].symbol, "B.US");
    assert_eq!(review.buckets.under_review[2].symbol, "C.US");
}

#[test]
fn queue_pin_metrics_and_buckets_surface_pinned_cases() {
    let mut pinned = case_summary_with_mechanisms("Liquidity Trap", &[]);
    pinned.queue_pin = Some("frontend-review-list".into());
    pinned.workflow_state = "review".into();
    pinned.execution_policy = Some(ActionExecutionPolicy::ReviewRequired);

    let mut unpinned = case_summary_with_mechanisms("Capital Rotation", &[]);
    unpinned.workflow_state = "review".into();
    unpinned.execution_policy = Some(ActionExecutionPolicy::ReviewRequired);

    let list = CaseListResponse {
        context: empty_context(),
        cases: vec![pinned.clone(), unpinned.clone()],
        governance_buckets: CaseGovernanceBuckets::default(),
        governance_reason_buckets: CaseGovernanceReasonBuckets::default(),
        primary_lens_buckets: CasePrimaryLensBuckets::default(),
        queue_pin_buckets: CaseQueuePinBuckets::default(),
    };

    let briefing = build_case_briefing(&list);
    let review = build_case_review(&list);

    assert_eq!(briefing.metrics.queue_pinned, 1);
    assert_eq!(briefing.queue_pin_buckets.pinned.len(), 1);
    assert_eq!(
        briefing.queue_pin_buckets.pinned[0].setup_id,
        pinned.setup_id
    );
    assert_eq!(review.metrics.queue_pinned, 1);
    assert_eq!(review.queue_pin_buckets.pinned.len(), 1);
    assert_eq!(review.queue_pin_buckets.pinned[0].setup_id, pinned.setup_id);
}

#[test]
fn review_analytics_groups_review_required_cases_by_lens() {
    let mut iceberg = case_summary_with_mechanisms("Liquidity Trap", &[]);
    iceberg.workflow_state = "review".into();
    iceberg.primary_lens = Some("iceberg".into());

    let mut causal = case_summary_with_mechanisms("Capital Rotation", &[]);
    causal.workflow_state = "review".into();
    causal.primary_lens = Some("causal".into());

    let mut another_iceberg = case_summary_with_mechanisms("Event Catalyst", &[]);
    another_iceberg.workflow_state = "review".into();
    another_iceberg.primary_lens = Some("iceberg".into());

    let analytics = build_case_review_analytics(&[iceberg, causal, another_iceberg]);
    assert_eq!(analytics.review_required_by_lens.len(), 2);
    assert_eq!(analytics.review_required_by_lens[0].lens, "iceberg");
    assert_eq!(analytics.review_required_by_lens[0].cases, 2);
    assert_eq!(analytics.review_required_by_lens[1].lens, "causal");
    assert_eq!(analytics.review_required_by_lens[1].cases, 1);
}

#[cfg(feature = "persistence")]
#[test]
fn review_analytics_capture_drift_and_invalidation_patterns() {
    let base_case = CaseSummary {
        case_id: "setup:a".into(),
        setup_id: "setup:a".into(),
        workflow_id: Some("wf:a".into()),
        execution_policy: None,
        owner: Some("owner".into()),
        reviewer: Some("reviewer".into()),
        queue_pin: None,
        workflow_actor: Some("actor".into()),
        workflow_note: Some("reject narrative".into()),
        symbol: "A.US".into(),
        title: "Case A".into(),
        sector: Some("Technology".into()),
        market: LiveMarket::Us,
        recommended_action: "enter".into(),
        workflow_state: "review".into(),
        governance: None,
        governance_bucket: "review_required".into(),
        governance_reason_code: None,
        governance_reason: None,
        market_regime_bias: "risk_off".into(),
        market_regime_confidence: dec!(0.75),
        market_breadth_delta: dec!(-0.35),
        market_average_return: dec!(-0.04),
        market_directional_consensus: Some(dec!(-0.20)),
        confidence: dec!(0.7),
        confidence_gap: dec!(0.2),
        heuristic_edge: dec!(0.1),
        review_reason_code: None,
        review_reason_family: None,
        review_reason_subreasons: vec![],
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
        driver_confidence: None,
        absence_summary: None,
        competition_summary: None,
        competition_winner: None,
        competition_runner_up: None,
        priority_rank: None,
        state_persistence_ticks: None,
        direction_stability_rounds: None,
        state_reason_codes: vec![],
        why_now: "why".into(),
        primary_lens: None,
        primary_driver: None,
        family_label: None,
        counter_label: None,
        hypothesis_status: Some("weakening".into()),
        current_leader: None,
        flip_count: 0,
        leader_streak: None,
        case_signature: None,
        archetype_projections: vec![],
        inferred_intent: None,
        intent_opportunities: vec![],
        expectation_binding_count: 0,
        expectation_violation_count: 0,
        key_evidence: vec![],
        invalidation_rules: vec!["若反向假說重新主導則撤回".into()],
        reasoning_profile: CaseReasoningProfile {
            laws: vec![],
            predicates: vec![],
            composite_states: vec![],
            human_review: Some(crate::ontology::HumanReviewContext {
                verdict: crate::ontology::HumanReviewVerdict::Rejected,
                verdict_label: "Rejected".into(),
                confidence: dec!(0.8),
                reasons: vec![crate::ontology::HumanReviewReason {
                    kind: crate::ontology::HumanReviewReasonKind::MechanismMismatch,
                    label: "Mechanism Mismatch".into(),
                    confidence: dec!(0.8),
                }],
                note: Some("reject narrative".into()),
            }),
            primary_mechanism: Some(crate::ontology::MechanismCandidate {
                kind: crate::ontology::MechanismCandidateKind::NarrativeFailure,
                label: "Narrative Failure".into(),
                score: dec!(0.71),
                summary: "s".into(),
                supporting_states: vec![],
                invalidation: vec![],
                human_checks: vec![],
                factors: vec![],
                counterfactuals: vec![],
            }),
            competing_mechanisms: vec![],
            automated_invalidations: vec![],
        },
        updated_at: "2026-03-22T00:00:00Z".into(),
        case_resolution: None,
        horizon_breakdown: None,
    };

    let runtime_1 = CaseReasoningAssessmentRecord::from_case_summary(
        &base_case,
        OffsetDateTime::from_unix_timestamp(1_711_102_000).unwrap(),
        "runtime",
    );
    let mut runtime_2_case = base_case.clone();
    runtime_2_case.reasoning_profile.primary_mechanism =
        Some(crate::ontology::MechanismCandidate {
            kind: crate::ontology::MechanismCandidateKind::FragilityBuildUp,
            label: "Fragility Build-up".into(),
            score: dec!(0.66),
            summary: "s".into(),
            supporting_states: vec![],
            invalidation: vec![],
            human_checks: vec![],
            factors: vec![],
            counterfactuals: vec![],
        });
    runtime_2_case.invalidation_rules = vec!["若 stress 回落則撤回".into()];
    let mut runtime_2 = CaseReasoningAssessmentRecord::from_case_summary(
        &runtime_2_case,
        OffsetDateTime::from_unix_timestamp(1_711_105_600).unwrap(),
        "runtime",
    );
    runtime_2.expectation_violations = vec![crate::ontology::ExpectationViolation {
        kind: crate::ontology::ExpectationViolationKind::MissingPropagation,
        expectation_id: Some("exp:peer-follow".into()),
        description: "peer failed to confirm".into(),
        magnitude: dec!(0.6),
        falsifier: Some("peer_silence".into()),
    }];
    let workflow_update = CaseReasoningAssessmentRecord::from_case_summary(
        &base_case,
        OffsetDateTime::from_unix_timestamp(1_711_105_900).unwrap(),
        "workflow_update",
    );

    let analytics = build_case_review_analytics_with_assessments(
        &[base_case],
        &[runtime_1, runtime_2, workflow_update],
        &[
            crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord {
                setup_id: "setup:a".into(),
                workflow_id: Some("wf:a".into()),
                market: "us".into(),
                symbol: Some("A.US".into()),
                primary_lens: Some("iceberg".into()),
                family: "Directed Flow".into(),
                session: "opening".into(),
                market_regime: "risk_off".into(),
                entry_tick: 1,
                entry_timestamp: OffsetDateTime::from_unix_timestamp(1_711_105_000).unwrap(),
                resolved_tick: 6,
                resolved_at: OffsetDateTime::from_unix_timestamp(1_711_105_600).unwrap(),
                direction: 1,
                return_pct: dec!(0.03),
                net_return: dec!(0.02),
                max_favorable_excursion: dec!(0.05),
                max_adverse_excursion: dec!(-0.01),
                followed_through: true,
                invalidated: false,
                structure_retained: true,
                convergence_score: dec!(0.5),
            },
        ],
        &[
            crate::persistence::discovered_archetype::DiscoveredArchetypeRecord {
                archetype_id: "archetype:us:directed_flow".into(),
                market: "us".into(),
                archetype_key: "directed_flow".into(),
                label: "Directed Flow".into(),
                topology: Some("isolated".into()),
                temporal_shape: Some("burst".into()),
                conflict_shape: Some("contradictory".into()),
                dominant_channels: vec!["volume".into(), "propagation".into()],
                expectation_violation_kinds: vec!["missingpropagation".into()],
                family_label: Some("Directed Flow".into()),
                bucket: crate::ontology::horizon::HorizonBucket::Session,
                samples: 4,
                hits: 3,
                hit_rate: dec!(0.75),
                mean_net_return: dec!(0.02),
                mean_affinity: dec!(0.8),
                updated_at: "2026-03-22T00:00:00Z".into(),
                confirmed_count: 0,
                invalidated_count: 0,
                profitable_but_late_count: 0,
                partially_confirmed_count: 0,
                exhausted_count: 0,
                early_exited_count: 0,
                structurally_right_count: 0,
            },
        ],
        crate::pipeline::learning_loop::OutcomeLearningContext::default(),
    );

    assert_eq!(analytics.review_required_by_lens.len(), 1);
    assert_eq!(analytics.review_required_by_lens[0].lens, "unknown");
    assert_eq!(analytics.human_override_by_lens.len(), 1);
    assert_eq!(analytics.human_override_by_lens[0].lens, "unknown");
    assert_eq!(analytics.lens_regime_hit_rates.len(), 1);
    assert_eq!(analytics.lens_regime_hit_rates[0].lens, "iceberg");
    assert_eq!(analytics.lens_regime_hit_rates[0].market_regime, "risk_off");
    assert_eq!(analytics.lens_regime_hit_rates[0].hits, 1);
    assert!(!analytics.mechanism_drift.is_empty());
    assert!(!analytics.mechanism_transition_breakdown.is_empty());
    assert!(!analytics.transition_by_sector.is_empty());
    assert!(!analytics.transition_by_regime.is_empty());
    assert!(!analytics.transition_by_reviewer.is_empty());
    assert!(!analytics.recent_mechanism_transitions.is_empty());
    assert!(!analytics.reviewer_doctrine.is_empty());
    assert!(!analytics.human_review_reasons.is_empty());
    assert!(!analytics.invalidation_patterns.is_empty());
    assert!(analytics.intelligence_signals.stable_archetypes >= 1);
    assert!(!analytics.violation_predictiveness.is_empty());
}

#[cfg(feature = "persistence")]
#[test]
fn mechanism_transition_story_classifies_regime_shift() {
    let old_case = CaseSummary {
        case_id: "setup:rot".into(),
        setup_id: "setup:rot".into(),
        workflow_id: Some("wf:rot".into()),
        execution_policy: None,
        owner: None,
        reviewer: None,
        queue_pin: None,
        workflow_actor: None,
        workflow_note: None,
        symbol: "9901.HK".into(),
        title: "Rotation".into(),
        sector: Some("Technology".into()),
        market: LiveMarket::Hk,
        recommended_action: "enter".into(),
        workflow_state: "suggest".into(),
        governance: None,
        governance_bucket: "review_required".into(),
        governance_reason_code: None,
        governance_reason: None,
        market_regime_bias: "neutral".into(),
        market_regime_confidence: dec!(0.30),
        market_breadth_delta: dec!(-0.05),
        market_average_return: dec!(0.00),
        market_directional_consensus: Some(dec!(0.01)),
        confidence: dec!(0.55),
        confidence_gap: dec!(0.10),
        heuristic_edge: dec!(0.04),
        review_reason_code: None,
        review_reason_family: None,
        review_reason_subreasons: vec![],
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
        driver_confidence: None,
        absence_summary: None,
        competition_summary: None,
        competition_winner: None,
        competition_runner_up: None,
        priority_rank: None,
        state_persistence_ticks: None,
        direction_stability_rounds: None,
        state_reason_codes: vec![],
        why_now: "why".into(),
        primary_lens: None,
        primary_driver: None,
        family_label: None,
        counter_label: None,
        hypothesis_status: None,
        current_leader: None,
        flip_count: 0,
        leader_streak: None,
        case_signature: None,
        archetype_projections: vec![],
        inferred_intent: None,
        intent_opportunities: vec![],
        expectation_binding_count: 0,
        expectation_violation_count: 0,
        key_evidence: vec![],
        invalidation_rules: vec![],
        reasoning_profile: CaseReasoningProfile {
            laws: vec![],
            predicates: vec![],
            composite_states: vec![crate::ontology::CompositeState {
                kind: crate::ontology::CompositeStateKind::DirectionalReinforcement,
                label: "Directional Reinforcement".into(),
                score: dec!(0.20),
                summary: "s".into(),
                predicates: vec![],
            }],
            human_review: None,
            primary_mechanism: Some(crate::ontology::MechanismCandidate {
                kind: crate::ontology::MechanismCandidateKind::MechanicalExecutionSignature,
                label: "Mechanical Execution Signature".into(),
                score: dec!(0.35),
                summary: "s".into(),
                supporting_states: vec![],
                invalidation: vec![],
                human_checks: vec![],
                factors: vec![crate::ontology::MechanismFactor {
                    key: "state:directional_reinforcement".into(),
                    label: "Directional Reinforcement".into(),
                    source: crate::ontology::MechanismFactorSource::State,
                    activation: dec!(0.20),
                    base_weight: dec!(0.45),
                    learned_weight_delta: Decimal::ZERO,
                    effective_weight: dec!(0.50),
                    contribution: dec!(0.10),
                }],
                counterfactuals: vec![],
            }),
            competing_mechanisms: vec![],
            automated_invalidations: vec![],
        },
        updated_at: "2026-03-22T00:00:00Z".into(),
        case_resolution: None,
        horizon_breakdown: None,
    };

    let mut new_case = old_case.clone();
    new_case.market_regime_bias = "risk_off".into();
    new_case.reasoning_profile.composite_states = vec![
        crate::ontology::CompositeState {
            kind: crate::ontology::CompositeStateKind::SubstitutionFlow,
            label: "Substitution Flow".into(),
            score: dec!(0.72),
            summary: "s".into(),
            predicates: vec![],
        },
        crate::ontology::CompositeState {
            kind: crate::ontology::CompositeStateKind::CrossScopeContagion,
            label: "Cross-scope Contagion".into(),
            score: dec!(0.24),
            summary: "s".into(),
            predicates: vec![],
        },
    ];
    new_case.reasoning_profile.primary_mechanism = Some(crate::ontology::MechanismCandidate {
        kind: crate::ontology::MechanismCandidateKind::CapitalRotation,
        label: "Capital Rotation".into(),
        score: dec!(0.68),
        summary: "s".into(),
        supporting_states: vec![],
        invalidation: vec![],
        human_checks: vec![],
        factors: vec![crate::ontology::MechanismFactor {
            key: "state:substitution_flow".into(),
            label: "Substitution Flow".into(),
            source: crate::ontology::MechanismFactorSource::State,
            activation: dec!(0.72),
            base_weight: dec!(0.60),
            learned_weight_delta: Decimal::ZERO,
            effective_weight: dec!(0.60),
            contribution: dec!(0.43),
        }],
        counterfactuals: vec![],
    });

    let old_snapshot = CaseReasoningAssessmentSnapshot::from_record(
        CaseReasoningAssessmentRecord::from_case_summary(
            &old_case,
            OffsetDateTime::from_unix_timestamp(1_711_102_000).unwrap(),
            "runtime",
        ),
    );
    let new_snapshot = CaseReasoningAssessmentSnapshot::from_record(
        CaseReasoningAssessmentRecord::from_case_summary(
            &new_case,
            OffsetDateTime::from_unix_timestamp(1_711_105_600).unwrap(),
            "runtime",
        ),
    );

    let transition = describe_mechanism_transition(&old_snapshot, &new_snapshot);
    assert_eq!(transition.classification, "regime_shift");
    assert!(transition.regime_change.is_some());
    assert!(!transition.regime_evidence.is_empty());
}

#[cfg(feature = "persistence")]
#[test]
fn review_reason_feedback_aggregates_blocked_outcomes() {
    let mut base_case = case_summary_with_mechanisms("Liquidity Trap", &[]);
    base_case.market = LiveMarket::Us;
    base_case.workflow_state = "review".into();
    base_case.review_reason_code = Some("directional_conflict".into());
    base_case.workflow_id = Some("wf:a".into());
    base_case.symbol = "A.US".into();

    let mut case_b = base_case.clone();
    case_b.case_id = "setup:b".into();
    case_b.setup_id = "setup:b".into();
    case_b.workflow_id = Some("wf:b".into());
    case_b.symbol = "B.US".into();

    let runtime_a_1 = CaseReasoningAssessmentRecord::from_case_summary(
        &base_case,
        OffsetDateTime::from_unix_timestamp(1_711_102_000).unwrap(),
        "runtime",
    );
    let runtime_a_2 = CaseReasoningAssessmentRecord::from_case_summary(
        &base_case,
        OffsetDateTime::from_unix_timestamp(1_711_102_060).unwrap(),
        "runtime",
    );
    let runtime_b = CaseReasoningAssessmentRecord::from_case_summary(
        &case_b,
        OffsetDateTime::from_unix_timestamp(1_711_102_120).unwrap(),
        "runtime",
    );

    let outcomes = vec![
        crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord {
            setup_id: "setup:Liquidity Trap".into(),
            workflow_id: Some("wf:a".into()),
            market: "us".into(),
            symbol: Some("A.US".into()),
            primary_lens: None,
            family: "Flow".into(),
            session: "opening".into(),
            market_regime: "risk_on".into(),
            entry_tick: 1,
            entry_timestamp: OffsetDateTime::UNIX_EPOCH,
            resolved_tick: 5,
            resolved_at: OffsetDateTime::UNIX_EPOCH,
            direction: 1,
            return_pct: dec!(0.02),
            net_return: dec!(0.02),
            max_favorable_excursion: dec!(0.03),
            max_adverse_excursion: dec!(-0.01),
            followed_through: true,
            invalidated: false,
            structure_retained: true,
            convergence_score: dec!(0.6),
        },
        crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord {
            setup_id: "setup:b".into(),
            workflow_id: Some("wf:b".into()),
            market: "us".into(),
            symbol: Some("B.US".into()),
            primary_lens: None,
            family: "Flow".into(),
            session: "opening".into(),
            market_regime: "risk_on".into(),
            entry_tick: 2,
            entry_timestamp: OffsetDateTime::UNIX_EPOCH,
            resolved_tick: 6,
            resolved_at: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(1),
            direction: 1,
            return_pct: dec!(-0.01),
            net_return: dec!(-0.01),
            max_favorable_excursion: dec!(0.01),
            max_adverse_excursion: dec!(-0.03),
            followed_through: false,
            invalidated: true,
            structure_retained: false,
            convergence_score: dec!(0.3),
        },
    ];

    let analytics = build_case_review_analytics_with_assessments(
        &[base_case, case_b],
        &[runtime_a_1, runtime_a_2, runtime_b],
        &outcomes,
        &[],
        crate::pipeline::learning_loop::OutcomeLearningContext::default(),
    );

    let stat = analytics
        .review_reason_feedback
        .iter()
        .find(|item| item.review_reason_code == "directional_conflict")
        .expect("directional_conflict stat");
    assert_eq!(stat.blocked_count, 2);
    assert_eq!(stat.resolved_count, 2);
    assert_eq!(stat.post_block_hits, 1);
    assert_eq!(stat.post_block_hit_rate, dec!(0.5));
    assert_eq!(stat.invalidation_rate, dec!(0.5));
    assert_eq!(stat.mean_net_return, dec!(0.005));
}
