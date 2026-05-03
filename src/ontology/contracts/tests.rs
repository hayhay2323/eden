use super::*;
use crate::agent::AgentWakeState;
use crate::live_snapshot::{LiveRawSource, LiveScorecard};
use crate::ontology::{
    world::{
        AttentionAllocation, PerceptualEvidence, PerceptualEvidencePolarity, PerceptualExpectation,
        PerceptualExpectationKind, PerceptualExpectationStatus, PerceptualState,
        PerceptualUncertainty, WorldStateSnapshot,
    },
    ExpectationKind, ExpectationViolationKind, IntentDirection, IntentHypothesis, IntentKind,
    IntentOpportunityWindow, IntentStrength, ReasoningScope, Symbol,
};
use rust_decimal_macros::dec;
use time::OffsetDateTime;

fn fixture_market_regime() -> LiveMarketRegime {
    LiveMarketRegime {
        bias: "neutral".into(),
        confidence: dec!(0.4),
        breadth_up: dec!(0.3),
        breadth_down: dec!(0.4),
        average_return: dec!(0.01),
        directional_consensus: None,
        pre_market_sentiment: None,
    }
}

fn fixture_stress() -> LiveStressSnapshot {
    LiveStressSnapshot {
        composite_stress: dec!(0.2),
        sector_synchrony: None,
        pressure_consensus: None,
        momentum_consensus: None,
        pressure_dispersion: None,
        volume_anomaly: None,
    }
}

fn fixture_case_signature() -> CaseSignature {
    CaseSignature {
        active_channels: vec![CaseChannel::Volume, CaseChannel::Propagation],
        topology: CaseTopology::Isolated,
        temporal_shape: CaseTemporalShape::Burst,
        conflict_shape: ConflictShape::Contradictory,
        expectation_support: 1,
        expectation_violations: 1,
        novelty_score: dec!(0.8),
        notes: vec!["phase=Growing".into(), "driver=volume".into()],
    }
}

fn fixture_expectation_bindings() -> Vec<ExpectationBinding> {
    vec![ExpectationBinding {
        expectation_id: "exp:fico:peer-follow".into(),
        kind: ExpectationKind::Propagation,
        scope: ReasoningScope::Symbol(Symbol("FICO.US".into())),
        target_scope: Some(ReasoningScope::Symbol(Symbol("MSCI.US".into()))),
        horizon: "intraday:10t".into(),
        strength: dec!(0.7),
        rationale: "historically propagates to peers".into(),
    }]
}

fn fixture_expectation_violations() -> Vec<ExpectationViolation> {
    vec![ExpectationViolation {
        kind: ExpectationViolationKind::MissingPropagation,
        expectation_id: Some("exp:fico:peer-follow".into()),
        description: "peer failed to confirm".into(),
        magnitude: dec!(0.6),
        falsifier: Some("peer_silence".into()),
    }]
}

fn fixture_live_case() -> LiveTacticalCase {
    LiveTacticalCase {
        setup_id: "pf:FICO.US:42".into(),
        symbol: "FICO.US".into(),
        title: "Long FICO.US (enter vortex)".into(),
        action: "enter".into(),
        confidence: dec!(0.91),
        confidence_gap: dec!(0.12),
        heuristic_edge: dec!(0.15),
        entry_rationale: "volume expansion with peer silence".into(),
        causal_narrative: Some(
            "peer silence plus volume expansion implies failed propagation".into(),
        ),
        review_reason_code: None,
        review_reason_family: None,
        review_reason_subreasons: vec![],
        policy_primary: Some("review_required".into()),
        policy_reason: Some("needs human review".into()),
        multi_horizon_gate_reason: None,
        family_label: Some("Directed Flow".into()),
        counter_label: Some("mean_reversion".into()),
        matched_success_pattern_signature: Some("flow+absence".into()),
        lifecycle_phase: Some("Growing".into()),
        tension_driver: Some("trade_flow".into()),
        driver_class: Some("company_specific".into()),
        is_isolated: Some(true),
        peer_active_count: Some(0),
        peer_silent_count: Some(3),
        peer_confirmation_ratio: Some(dec!(0.0)),
        isolation_score: Some(dec!(1.0)),
        competition_margin: Some(dec!(0.7)),
        driver_confidence: Some(dec!(0.8)),
        absence_summary: Some("FICO.US is isolated while peers stay silent".into()),
        competition_summary: Some(
            "best explanation is CompanySpecific over TradeFlowDriven".into(),
        ),
        competition_winner: Some("CompanySpecific".into()),
        competition_runner_up: Some("TradeFlowDriven".into()),
        lifecycle_velocity: Some(dec!(0.1)),
        lifecycle_acceleration: Some(dec!(0.0)),
        horizon_bucket: None,
        horizon_urgency: None,
        horizon_secondary: vec![],
        case_signature: Some(fixture_case_signature()),
        archetype_projections: vec![ArchetypeProjection {
            archetype_key: "emergent".into(),
            label: "emergent pattern".into(),
            affinity: dec!(0.85),
            rationale: "isolated propagation break".into(),
        }],
        expectation_bindings: fixture_expectation_bindings(),
        expectation_violations: fixture_expectation_violations(),
        inferred_intent: Some(IntentHypothesis {
            intent_id: "intent:pf:FICO.US:42".into(),
            kind: IntentKind::FailedPropagation,
            scope: ReasoningScope::Symbol(Symbol("FICO.US".into())),
            direction: IntentDirection::Buy,
            state: crate::ontology::IntentState::AtRisk,
            confidence: dec!(0.91),
            urgency: dec!(0.8),
            persistence: dec!(0.5),
            conflict_score: dec!(0.7),
            strength: IntentStrength {
                flow_strength: dec!(0.8),
                impact_strength: dec!(0.7),
                persistence_strength: dec!(0.5),
                propagation_strength: dec!(0.6),
                resistance_strength: dec!(0.4),
                composite: dec!(0.55),
            },
            propagation_targets: vec![ReasoningScope::Symbol(Symbol("MSCI.US".into()))],
            supporting_archetypes: vec!["emergent".into()],
            supporting_case_signature: Some(fixture_case_signature()),
            expectation_bindings: fixture_expectation_bindings(),
            expectation_violations: fixture_expectation_violations(),
            exit_signals: vec![crate::ontology::IntentExitSignal {
                kind: crate::ontology::IntentExitKind::Absorbed,
                confidence: dec!(0.6),
                rationale: "expected propagation was absorbed or blocked".into(),
                trigger: "peer failed to confirm".into(),
            }],
            opportunities: vec![IntentOpportunityWindow::new(
                crate::ontology::horizon::HorizonBucket::Fast5m,
                crate::ontology::horizon::Urgency::Immediate,
                crate::ontology::IntentOpportunityBias::Enter,
                dec!(0.85),
                dec!(0.8),
                "failed propagation window".into(),
            )],
            falsifiers: vec!["peer_silence".into()],
            rationale: "failed propagation inferred from isolated peer silence".into(),
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
    }
}

fn fixture_live_snapshot() -> LiveSnapshot {
    LiveSnapshot {
        tick: 42,
        timestamp: "2026-04-11T14:12:38Z".into(),
        market: LiveMarket::Us,
        market_phase: "cash_session".into(),
        market_active: true,
        stock_count: 1,
        edge_count: 0,
        hypothesis_count: 1,
        observation_count: 1,
        active_positions: 0,
        active_position_nodes: vec![],
        market_regime: fixture_market_regime(),
        stress: fixture_stress(),
        scorecard: LiveScorecard {
            total_signals: 1,
            ..LiveScorecard::default()
        },
        tactical_cases: vec![fixture_live_case()],
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
        raw_sources: vec![LiveRawSource {
            source: "trade".into(),
            symbol: Some("FICO.US".into()),
            scope: "symbol".into(),
            summary: "buy imbalance across recent prints".into(),
            window_start: None,
            window_end: None,
            payload: serde_json::Value::Null,
        }],
        signal_translation_gaps: vec![],
        cluster_states: vec![],
        symbol_states: vec![],
        world_summary: None,
        temporal_bars: vec![],
        lineage: vec![],
        success_patterns: vec![],
    }
}

fn fixture_agent_snapshot() -> AgentSnapshot {
    AgentSnapshot {
        tick: 42,
        timestamp: "2026-04-11T14:12:38Z".into(),
        market: LiveMarket::Us,
        market_regime: fixture_market_regime(),
        stress: fixture_stress(),
        wake: AgentWakeState {
            should_speak: false,
            priority: dec!(0.4),
            headline: Some("FICO.US active".into()),
            summary: vec!["FICO.US active".into()],
            focus_symbols: vec!["FICO.US".into()],
            reasons: vec![],
            suggested_tools: vec![],
        },
        world_state: Some(WorldStateSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            entities: vec![],
            world_intents: vec![],
            perceptual_states: vec![PerceptualState {
                state_id: "ps:fico".into(),
                scope: ReasoningScope::Symbol(Symbol("FICO.US".into())),
                label: "FICO.US perceptual state".into(),
                state_kind: "continuation".into(),
                trend: "strengthening".into(),
                direction: Some("buy".into()),
                age_ticks: 4,
                persistence_ticks: 3,
                direction_continuity_ticks: 3,
                confidence: dec!(0.81),
                strength: dec!(0.76),
                support_count: 4,
                contradict_count: 1,
                count_support_fraction: dec!(0.80),
                weighted_support_fraction: dec!(0.88),
                support_weight: dec!(1.70),
                contradict_weight: dec!(0.22),
                supporting_evidence: vec![PerceptualEvidence {
                    evidence_id: "ev:support".into(),
                    target_scope: ReasoningScope::Symbol(Symbol("FICO.US".into())),
                    source_scope: None,
                    channel: "raw".into(),
                    polarity: PerceptualEvidencePolarity::Supports,
                    weight: dec!(0.28),
                    rationale: "weighted raw support remains strong".into(),
                }],
                opposing_evidence: vec![PerceptualEvidence {
                    evidence_id: "ev:oppose".into(),
                    target_scope: ReasoningScope::Symbol(Symbol("FICO.US".into())),
                    source_scope: None,
                    channel: "timing".into(),
                    polarity: PerceptualEvidencePolarity::Contradicts,
                    weight: dec!(0.10),
                    rationale: "timing is not ideal".into(),
                }],
                missing_evidence: vec![PerceptualEvidence {
                    evidence_id: "ev:missing".into(),
                    target_scope: ReasoningScope::Symbol(Symbol("FICO.US".into())),
                    source_scope: None,
                    channel: "peer".into(),
                    polarity: PerceptualEvidencePolarity::Missing,
                    weight: dec!(0.12),
                    rationale: "peer follow-through is still missing".into(),
                }],
                conflict_age_ticks: 0,
                expectations: vec![PerceptualExpectation {
                    expectation_id: "exp:fico".into(),
                    target_scope: ReasoningScope::Symbol(Symbol("FICO.US".into())),
                    kind: PerceptualExpectationKind::PeerFollowThrough,
                    status: PerceptualExpectationStatus::StillPending,
                    rationale: "waiting for peers to confirm".into(),
                    pending_ticks: 1,
                }],
                attention_allocations: vec![AttentionAllocation {
                    allocation_id: "att:fico".into(),
                    target_scope: ReasoningScope::Symbol(Symbol("FICO.US".into())),
                    channel: "raw".into(),
                    weight: dec!(0.90),
                    rationale: "raw channel dominates current attention".into(),
                }],
                uncertainties: vec![PerceptualUncertainty {
                    uncertainty_id: "unc:fico".into(),
                    target_scope: ReasoningScope::Symbol(Symbol("FICO.US".into())),
                    level: dec!(0.24),
                    rationale: "peer confirmation remains incomplete".into(),
                    degraded_channels: vec!["peer".into()],
                }],
                active_setup_ids: vec!["pf:FICO.US:42".into()],
                dominant_intent_kind: Some("failed_propagation".into()),
                dominant_intent_state: Some("at_risk".into()),
                cluster_key: "symbol:FICO.US".into(),
                cluster_label: "FICO.US".into(),
                last_transition_summary: Some("FICO.US latent -> continuation".into()),
            }],
            vortices: vec![],
        }),
        backward_reasoning: None,
        perception: None,
        notices: vec![],
        active_structures: vec![],
        recent_transitions: vec![],
        investigation_selections: vec![],
        sector_flows: vec![],
        symbols: vec![crate::agent::AgentSymbolState {
            symbol: "FICO.US".into(),
            sector: Some("Technology".into()),
            structure: None,
            signal: None,
            depth: None,
            brokers: None,
            invalidation: None,
            pressure: None,
            active_position: None,
            latest_events: vec![],
        }],
        events: vec![],
        cross_market_signals: vec![],
        raw_sources: vec![LiveRawSource {
            source: "trade".into(),
            symbol: Some("FICO.US".into()),
            scope: "symbol".into(),
            summary: "agent observed buy imbalance".into(),
            window_start: None,
            window_end: None,
            payload: serde_json::Value::Null,
        }],
        context_priors: vec![],
        macro_event_candidates: vec![],
        macro_events: vec![],
        perception_states: vec![],
        knowledge_links: vec![],
    }
}

fn fixture_agent_session() -> AgentSession {
    AgentSession {
        tick: 42,
        timestamp: "2026-04-11T14:12:38Z".into(),
        market: LiveMarket::Us,
        should_speak: false,
        active_thread_count: 0,
        focus_symbols: vec!["FICO.US".into()],
        active_threads: vec![],
        current_investigations: vec![],
        current_judgments: vec![],
        recent_turns: vec![],
    }
}

fn fixture_recommendation() -> AgentRecommendation {
    AgentRecommendation {
        recommendation_id: "rec:42:fico".into(),
        tick: 42,
        symbol: "FICO.US".into(),
        sector: Some("Technology".into()),
        title: Some("Long FICO.US".into()),
        action: "enter".into(),
        action_label: Some("Enter".into()),
        bias: "long".into(),
        severity: "high".into(),
        confidence: dec!(0.91),
        score: dec!(0.15),
        horizon_ticks: 10,
        regime_bias: "neutral".into(),
        status: Some("active".into()),
        why: "volume expansion with peer silence".into(),
        why_components: vec![],
        primary_lens: Some("pressure_flow".into()),
        supporting_lenses: vec!["absence".into()],
        review_lens: Some("pressure_flow".into()),
        watch_next: vec![],
        do_not: vec![],
        fragility: vec![],
        transition: Some("review -> enter".into()),
        thesis_family: Some("Directed Flow".into()),
        matched_success_pattern_signature: Some("flow+absence".into()),
        state_transition: Some("growing".into()),
        best_action: "follow".into(),
        action_expectancies: AgentActionExpectancies::default(),
        decision_attribution: AgentDecisionAttribution::default(),
        expected_net_alpha: Some(dec!(0.02)),
        alpha_horizon: "intraday:10t".into(),
        price_at_decision: None,
        resolution: None,
        invalidation_rule: Some("peer confirms downside".into()),
        invalidation_components: vec![],
        execution_policy: ActionExecutionPolicy::ReviewRequired,
        governance: ActionGovernanceContract::for_recommendation(
            ActionExecutionPolicy::ReviewRequired,
        ),
        governance_reason_code: ActionGovernanceReasonCode::SeverityRequiresReview,
        governance_reason: "severity requires human review".into(),
    }
}

fn fixture_recommendations() -> AgentRecommendations {
    AgentRecommendations {
        tick: 42,
        timestamp: "2026-04-11T14:12:38Z".into(),
        market: LiveMarket::Us,
        regime_bias: "neutral".into(),
        total: 1,
        market_recommendation: None,
        decisions: vec![],
        items: vec![fixture_recommendation()],
        knowledge_links: vec![],
    }
}

#[test]
fn operational_snapshot_preserves_case_projection_fields() {
    let live_snapshot = fixture_live_snapshot();
    let snapshot = build_operational_snapshot(
        &live_snapshot,
        &fixture_agent_snapshot(),
        &fixture_agent_session(),
        &fixture_recommendations(),
        None,
    )
    .expect("operational snapshot should build");

    let case = snapshot.cases.first().expect("case should exist");
    assert_eq!(case.case_signature, Some(fixture_case_signature()));
    assert_eq!(case.archetype_projections.len(), 1);
    assert_eq!(case.expectation_bindings, fixture_expectation_bindings());
    assert_eq!(
        case.expectation_violations,
        fixture_expectation_violations()
    );
    assert_eq!(
        case.inferred_intent.as_ref().map(|intent| intent.kind),
        Some(IntentKind::FailedPropagation)
    );
    assert!(!case
        .inferred_intent
        .as_ref()
        .expect("intent should exist")
        .opportunities
        .is_empty());

    let recommendation = snapshot
        .recommendations
        .first()
        .expect("recommendation should exist");
    assert_eq!(
        recommendation.case_signature,
        Some(fixture_case_signature())
    );
    assert_eq!(recommendation.archetype_projections.len(), 1);
    assert_eq!(
        recommendation.expectation_bindings,
        fixture_expectation_bindings()
    );
    assert_eq!(
        recommendation.expectation_violations,
        fixture_expectation_violations()
    );
    assert_eq!(
        recommendation
            .inferred_intent
            .as_ref()
            .map(|intent| intent.kind),
        Some(IntentKind::FailedPropagation)
    );
    assert!(!recommendation
        .inferred_intent
        .as_ref()
        .expect("intent should exist")
        .opportunities
        .is_empty());

    let json = serde_json::to_value(&snapshot).expect("snapshot should serialize");
    let case_json = &json["cases"][0];
    assert!(case_json.get("case_signature").is_some());
    assert!(case_json.get("archetype_projections").is_some());
    assert!(case_json.get("expectation_bindings").is_some());
    assert!(case_json.get("expectation_violations").is_some());
    assert!(case_json.get("inferred_intent").is_some());

    let recommendation_json = &json["recommendations"][0];
    assert!(recommendation_json.get("case_signature").is_some());
    assert!(recommendation_json.get("archetype_projections").is_some());
    assert!(recommendation_json.get("expectation_bindings").is_some());
    assert!(recommendation_json.get("expectation_violations").is_some());
    assert!(recommendation_json.get("inferred_intent").is_some());
    assert!(!snapshot.sidecars.raw_sources.is_empty());
    assert!(json["sidecars"]["raw_sources"].is_array());
}

#[test]
fn operational_snapshot_materializes_perceptual_contracts() {
    let live_snapshot = fixture_live_snapshot();
    let snapshot = build_operational_snapshot(
        &live_snapshot,
        &fixture_agent_snapshot(),
        &fixture_agent_session(),
        &fixture_recommendations(),
        None,
    )
    .expect("operational snapshot should build");

    assert_eq!(snapshot.perceptual_states.len(), 1);
    assert_eq!(snapshot.perceptual_evidence.len(), 3);
    assert_eq!(snapshot.perceptual_expectations.len(), 1);
    assert_eq!(snapshot.attention_allocations.len(), 1);
    assert_eq!(snapshot.perceptual_uncertainties.len(), 1);

    let symbol = snapshot.symbol("FICO.US").expect("symbol should exist");
    assert!(symbol.perceptual_state.is_some());
    assert!(symbol.relationships.perceptual_state.is_some());
    assert_eq!(symbol.relationships.supporting_evidence.len(), 1);
    assert_eq!(symbol.relationships.opposing_evidence.len(), 1);
    assert_eq!(symbol.relationships.missing_evidence.len(), 1);
    assert_eq!(symbol.relationships.expectations.len(), 1);
    assert_eq!(symbol.relationships.attention_allocations.len(), 1);
    assert_eq!(symbol.relationships.uncertainties.len(), 1);
}

#[test]
fn perceptual_contracts_expose_navigation_and_neighborhood() {
    let live_snapshot = fixture_live_snapshot();
    let snapshot = build_operational_snapshot(
        &live_snapshot,
        &fixture_agent_snapshot(),
        &fixture_agent_session(),
        &fixture_recommendations(),
        None,
    )
    .expect("operational snapshot should build");

    let perceptual_state = snapshot
        .perceptual_states
        .first()
        .expect("perceptual state should exist");
    assert!(perceptual_state.navigation.self_ref.is_some());
    assert!(perceptual_state.navigation.neighborhood_endpoint.is_some());

    let neighborhood = snapshot
        .neighborhood(OperationalObjectKind::PerceptualState, &perceptual_state.id)
        .expect("perceptual state neighborhood should resolve");
    assert_eq!(neighborhood.root.id, perceptual_state.id);
    assert!(neighborhood
        .relationships
        .iter()
        .any(|group| group.name == "supporting_evidence"));

    let evidence = snapshot
        .perceptual_evidence
        .first()
        .expect("perceptual evidence should exist");
    assert!(evidence.navigation.self_ref.is_some());
    assert!(snapshot
        .navigation(OperationalObjectKind::PerceptualEvidence, &evidence.id)
        .is_some());

    let expectation = snapshot
        .perceptual_expectations
        .first()
        .expect("perceptual expectation should exist");
    assert!(snapshot
        .navigation(
            OperationalObjectKind::PerceptualExpectation,
            &expectation.id,
        )
        .is_some());

    let allocation = snapshot
        .attention_allocations
        .first()
        .expect("attention allocation should exist");
    assert!(snapshot
        .navigation(OperationalObjectKind::AttentionAllocation, &allocation.id,)
        .is_some());

    let uncertainty = snapshot
        .perceptual_uncertainties
        .first()
        .expect("perceptual uncertainty should exist");
    assert!(snapshot
        .navigation(
            OperationalObjectKind::PerceptualUncertainty,
            &uncertainty.id,
        )
        .is_some());
}

#[test]
fn operational_snapshot_exposes_organ_first_overview() {
    let live_snapshot = fixture_live_snapshot();
    let snapshot = build_operational_snapshot(
        &live_snapshot,
        &fixture_agent_snapshot(),
        &fixture_agent_session(),
        &fixture_recommendations(),
        None,
    )
    .expect("operational snapshot should build");

    let organ = snapshot.organ_overview();
    assert_eq!(organ.role, "sensory_organ");
    assert_eq!(organ.market, LiveMarket::Us);
    assert_eq!(organ.perceptual_states.len(), 1);
    assert_eq!(organ.perceptual_evidence.len(), 3);
    assert_eq!(organ.perceptual_expectations.len(), 1);
    assert_eq!(organ.attention_allocations.len(), 1);
    assert_eq!(organ.perceptual_uncertainties.len(), 1);
    assert_eq!(organ.projected_cases.len(), 1);
    assert!(!organ.raw_sources.is_empty());

    let json = serde_json::to_value(&organ).expect("organ overview should serialize");
    assert_eq!(json["role"], "sensory_organ");
    assert!(json.get("world_state").is_some());
    assert!(json.get("perceptual_states").is_some());
    assert!(json.get("projected_cases").is_some());
    assert!(json.get("projected_recommendations").is_none());
}

#[test]
fn derived_agent_briefing_includes_dominant_intents() {
    let live_snapshot = fixture_live_snapshot();
    let snapshot = build_operational_snapshot(
        &live_snapshot,
        &fixture_agent_snapshot(),
        &fixture_agent_session(),
        &fixture_recommendations(),
        None,
    )
    .expect("operational snapshot should build");

    let briefing = derive_agent_briefing(&snapshot);
    assert!(!briefing.dominant_intents.is_empty());
    assert!(briefing
        .dominant_intents
        .iter()
        .any(|item| item.contains("failed propagation")));
    assert!(briefing
        .summary
        .iter()
        .any(|item| item.contains("dominant opportunity:")));
    assert!(briefing
        .reasons
        .iter()
        .any(|item| item.contains("dominant opportunity:")));
}
