use super::*;
use crate::ontology::{
    BackwardCause, CausalContestState, ProvenanceMetadata, ProvenanceSource, Symbol, WorldLayer,
};
use rust_decimal_macros::dec;
use time::OffsetDateTime;

fn base_snapshot(symbols: Vec<AgentSymbolState>) -> AgentSnapshot {
    AgentSnapshot {
        tick: 10,
        timestamp: "2026-03-23T00:00:00Z".into(),
        market: LiveMarket::Hk,
        market_regime: LiveMarketRegime {
            bias: "neutral".into(),
            confidence: dec!(0.7),
            breadth_up: dec!(0.60),
            breadth_down: dec!(0.20),
            average_return: dec!(0.02),
            directional_consensus: Some(dec!(0.15)),
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
        wake: AgentWakeState {
            should_speak: true,
            priority: dec!(0.8),
            headline: None,
            summary: vec![],
            focus_symbols: symbols.iter().map(|item| item.symbol.clone()).collect(),
            reasons: vec![],
            suggested_tools: vec![],
        },
        world_state: None,
        backward_reasoning: None,
        notices: vec![],
        active_structures: vec![],
        recent_transitions: vec![],
        investigation_selections: vec![],
        sector_flows: vec![],
        symbols,
        events: vec![],
        cross_market_signals: vec![],
        context_priors: vec![],
        macro_event_candidates: vec![],
        macro_events: vec![],
        knowledge_links: vec![],
    }
}

fn symbol(symbol: &str) -> AgentSymbolState {
    AgentSymbolState {
        symbol: symbol.into(),
        sector: Some("Technology".into()),
        structure: None,
        signal: None,
        depth: None,
        brokers: None,
        invalidation: None,
        pressure: None,
        active_position: None,
        latest_events: vec![],
    }
}

fn context<'a>(snapshot: &'a AgentSnapshot, symbol: &'a AgentSymbolState) -> LensContext<'a> {
    LensContext {
        snapshot,
        symbol,
        current_transition: None,
        current_notice: None,
        backward: None,
        bias: "long",
        confidence: dec!(0.7),
        best_action: "follow",
        severity: "high",
        expected_net_alpha: Some(dec!(0.02)),
    }
}

#[test]
fn iceberg_lens_skips_symbols_without_iceberg_signal() {
    let state = symbol("700.HK");
    let snapshot = base_snapshot(vec![state.clone()]);
    let observations = super::iceberg::IcebergLens.observe(&context(&snapshot, &state));
    assert!(observations.is_empty());
}

#[test]
fn iceberg_lens_emits_from_iceberg_events() {
    let mut state = symbol("700.HK");
    state.latest_events = vec![LiveEvent {
        kind: "IcebergDetected".into(),
        symbol: Some("700.HK".into()),
        magnitude: dec!(0.72),
        summary: "iceberg".into(),
        age_secs: None,
        freshness: None,
    }];
    state.brokers = Some(AgentBrokerState {
        current: vec![],
        entered: vec!["6998".into()],
        exited: vec![],
        switched_to_bid: vec![],
        switched_to_ask: vec![],
    });
    let snapshot = base_snapshot(vec![state.clone()]);
    let observations = super::iceberg::IcebergLens.observe(&context(&snapshot, &state));
    assert_eq!(observations.len(), 1);
    assert!(observations[0].why_fragment.contains("冰山回補"));
    assert!(observations[0]
        .invalidation_fragments
        .contains(&"冰山回補停止".into()));
}

#[test]
fn structural_lens_emits_status_and_invalidation() {
    let mut state = symbol("700.HK");
    state.structure = Some(AgentStructureState {
        symbol: "700.HK".into(),
        sector: Some("Technology".into()),
        setup_id: Some("setup:700.HK".into()),
        title: "Long 700.HK".into(),
        action: "enter".into(),
        status: Some("strengthening".into()),
        age_ticks: Some(5),
        status_streak: Some(3),
        confidence: dec!(0.82),
        confidence_change: None,
        confidence_gap: Some(dec!(0.12)),
        transition_reason: None,
        contest_state: None,
        current_leader: None,
        leader_streak: None,
        leader_transition_summary: Some("leader remains flow".into()),
        thesis_family: Some("Directed Flow".into()),
        action_expectancies: AgentActionExpectancies::default(),
        expected_net_alpha: Some(dec!(0.02)),
        alpha_horizon: Some("intraday:10t".into()),
        invalidation_rule: Some("institutional alignment 翻負".into()),
    });
    state.invalidation = Some(AgentInvalidationState {
        status: "armed".into(),
        invalidated: false,
        transition_reason: None,
        leading_falsifier: None,
        rules: vec!["sector coherence < 0.3".into()],
    });
    let snapshot = base_snapshot(vec![state.clone()]);
    let observations = super::structural::StructuralLens.observe(&context(&snapshot, &state));
    assert!(observations
        .iter()
        .any(|item| item.why_fragment.contains("結構 strengthening")));
    assert!(observations
        .iter()
        .flat_map(|item| item.invalidation_fragments.iter())
        .any(|item| item.contains("institutional alignment")));
}

#[test]
fn causal_lens_emits_leading_cause_and_falsifier() {
    let state = symbol("700.HK");
    let mut snapshot = base_snapshot(vec![state.clone()]);
    snapshot.backward_reasoning = Some(BackwardReasoningSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        investigations: vec![BackwardInvestigation {
            investigation_id: "backward:700.HK".into(),
            leaf_scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
            leaf_label: "Long 700.HK".into(),
            leaf_regime: "flow-led".into(),
            contest_state: CausalContestState::Stable,
            leading_cause_streak: 4,
            previous_leading_cause_id: None,
            leading_cause: Some(BackwardCause {
                cause_id: "cause:market:700.HK".into(),
                scope: ReasoningScope::market(),
                layer: WorldLayer::Forest,
                depth: 1,
                provenance: ProvenanceMetadata::new(
                    ProvenanceSource::Computed,
                    OffsetDateTime::UNIX_EPOCH,
                ),
                explanation: "institutional flow dominates".into(),
                chain_summary: None,
                confidence: dec!(0.6),
                support_weight: dec!(0.5),
                contradict_weight: dec!(0.1),
                net_conviction: dec!(0.4),
                competitive_score: dec!(0.7),
                falsifier: Some("alignment flips negative".into()),
                supporting_evidence: vec![],
                contradicting_evidence: vec![],
                references: vec![],
            }),
            runner_up_cause: None,
            cause_gap: None,
            leading_support_delta: None,
            leading_contradict_delta: None,
            leader_transition_summary: Some("leader remains market".into()),
            leading_falsifier: Some("alignment flips negative".into()),
            candidate_causes: vec![],
        }],
    });
    let ctx = LensContext {
        backward: snapshot.backward_investigation("700.HK"),
        ..context(&snapshot, &state)
    };
    let observations = super::causal::CausalAttributionLens.observe(&ctx);
    assert!(observations[0]
        .why_fragment
        .contains("主因: institutional flow dominates"));
    assert!(observations[0]
        .invalidation_fragments
        .iter()
        .any(|item| item.contains("alignment flips negative")));
}

#[test]
fn lineage_lens_emits_prior_summary() {
    let mut state = symbol("700.HK");
    state.structure = Some(AgentStructureState {
        symbol: "700.HK".into(),
        sector: Some("Technology".into()),
        setup_id: Some("setup:700.HK".into()),
        title: "Long 700.HK".into(),
        action: "enter".into(),
        status: Some("strengthening".into()),
        age_ticks: Some(5),
        status_streak: Some(3),
        confidence: dec!(0.82),
        confidence_change: None,
        confidence_gap: Some(dec!(0.12)),
        transition_reason: None,
        contest_state: None,
        current_leader: None,
        leader_streak: None,
        leader_transition_summary: None,
        thesis_family: Some("Directed Flow".into()),
        action_expectancies: AgentActionExpectancies {
            follow_expectancy: Some(dec!(0.012)),
            fade_expectancy: Some(dec!(-0.003)),
            wait_expectancy: Some(Decimal::ZERO),
        },
        expected_net_alpha: Some(dec!(0.012)),
        alpha_horizon: Some("intraday:20t".into()),
        invalidation_rule: None,
    });
    let snapshot = base_snapshot(vec![state.clone()]);
    let observations = super::lineage::LineagePriorLens.observe(&context(&snapshot, &state));
    assert_eq!(observations.len(), 1);
    assert!(observations[0].why_fragment.contains("歷史先驗"));
    assert!(observations[0].why_fragment.contains("Directed Flow"));
}

#[test]
fn lens_engine_orders_and_dedupes_fragments() {
    let mut state = symbol("700.HK");
    state.latest_events = vec![LiveEvent {
        kind: "IcebergDetected".into(),
        symbol: Some("700.HK".into()),
        magnitude: dec!(0.72),
        summary: "iceberg".into(),
        age_secs: None,
        freshness: None,
    }];
    state.structure = Some(AgentStructureState {
        symbol: "700.HK".into(),
        sector: Some("Technology".into()),
        setup_id: Some("setup:700.HK".into()),
        title: "Long 700.HK".into(),
        action: "enter".into(),
        status: Some("strengthening".into()),
        age_ticks: Some(5),
        status_streak: Some(3),
        confidence: dec!(0.82),
        confidence_change: None,
        confidence_gap: Some(dec!(0.12)),
        transition_reason: None,
        contest_state: None,
        current_leader: None,
        leader_streak: None,
        leader_transition_summary: None,
        thesis_family: Some("Directed Flow".into()),
        action_expectancies: AgentActionExpectancies::default(),
        expected_net_alpha: Some(dec!(0.012)),
        alpha_horizon: Some("intraday:20t".into()),
        invalidation_rule: Some("冰山回補停止".into()),
    });
    let snapshot = base_snapshot(vec![state.clone()]);
    let bundle = default_lens_engine().observe(&context(&snapshot, &state));
    assert!(!bundle.why_fragments.is_empty());
    assert!(!bundle.invalidation_fragments.is_empty());
    assert_eq!(bundle.invalidation_fragments[0], "冰山回補停止");
}
