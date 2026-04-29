use super::*;
use crate::agent::builders::shared::hk_recent_transitions;
use crate::live_snapshot::{
    apply_case_structural_notes, apply_raw_disagreement_layer, build_live_raw_microstructure,
    build_live_raw_sources, build_signal_translation_gaps, enforce_freshness_decay,
    enforce_orphan_action_cap, enforce_timing_action_cap, enrich_cross_tick_momentum,
    enrich_surfaced_case_evidence, mark_directional_conflicts, sort_tactical_cases_for_surface,
    SurfacedCaseHistorySample,
};
use crate::pipeline::perception::apply_perception_layer;
use crate::temporal::pyramid::build_hk_live_temporal_bars;
use crate::temporal::session::{event_half_life_secs, freshness_score_from_age_secs};

fn multi_horizon_gate_reason(notes: &[String]) -> Option<String> {
    notes
        .iter()
        .find_map(|note| note.strip_prefix("multi_horizon_gate=blocked: "))
        .map(|value| value.to_string())
}

fn matched_success_pattern_signature(notes: &[String]) -> Option<String> {
    notes
        .iter()
        .find_map(|note| note.strip_prefix("matched_success_pattern="))
        .map(|value| value.to_string())
        .filter(|value| !value.is_empty())
}

pub(crate) fn build_hk_live_snapshot(
    tick: u64,
    timestamp: String,
    store: &std::sync::Arc<eden::ontology::store::ObjectStore>,
    brain: &BrainGraph,
    decision: &DecisionSnapshot,
    graph_insights: &GraphInsights,
    reasoning_snapshot: &ReasoningSnapshot,
    event_snapshot: &EventSnapshot,
    observation_snapshot: &ObservationSnapshot,
    scorecard: &SignalScorecard,
    dim_snapshot: &DimensionSnapshot,
    history: &TickHistory,
    latest: &TickRecord,
    live: &LiveState,
    tracker: &PositionTracker,
    causal_timelines: &std::collections::HashMap<String, CausalTimeline>,
    dynamics: &std::collections::HashMap<Symbol, eden::temporal::analysis::SignalDynamics>,
    previous_symbol_states: &[eden::pipeline::state_engine::PersistentSymbolState],
    previous_cluster_states: &[eden::live_snapshot::LiveClusterState],
    previous_world_summary: Option<&eden::live_snapshot::LiveWorldSummary>,
) -> LiveSnapshot {
    let hypothesis_map: HashMap<&str, &eden::Hypothesis> = reasoning_snapshot
        .hypotheses
        .iter()
        .map(|item| (item.hypothesis_id.as_str(), item))
        .collect();

    let mut top_signals = latest
        .signals
        .iter()
        .map(|(symbol, signal)| {
            let dims = dim_snapshot.dimensions.get(symbol);
            LiveSignal {
                symbol: symbol.0.clone(),
                sector: sector_name_for_symbol(store, symbol),
                composite: signal.composite,
                mark_price: signal.mark_price,
                dimension_composite: None,
                capital_flow_direction: signal.capital_flow_direction,
                price_momentum: dims
                    .map(|item| item.activity_momentum)
                    .unwrap_or(Decimal::ZERO),
                volume_profile: dims
                    .map(|item| item.candlestick_conviction)
                    .unwrap_or(Decimal::ZERO),
                pre_post_market_anomaly: Decimal::ZERO,
                valuation: dims
                    .map(|item| item.valuation_support)
                    .unwrap_or(Decimal::ZERO),
                cross_stock_correlation: Some(signal.cross_stock_correlation),
                sector_coherence: signal.sector_coherence,
                cross_market_propagation: None,
            }
        })
        .filter(|signal| signal.composite.abs() > Decimal::new(3, 2))
        .collect::<Vec<_>>();
    top_signals.sort_by(|a, b| b.composite.abs().cmp(&a.composite.abs()));
    top_signals.truncate(120);
    let temporal_symbols = top_signals
        .iter()
        .map(|item| item.symbol.clone())
        .collect::<Vec<_>>();

    let mut tactical_cases = reasoning_snapshot
        .tactical_setups
        .iter()
        .map(|item| {
            let hypothesis = hypothesis_map.get(item.hypothesis_id.as_str()).copied();
            let mut case = LiveTacticalCase {
                setup_id: item.setup_id.clone(),
                symbol: symbol_string_from_scope(&item.scope),
                title: item.title.clone(),
                action: item.action.to_string(),
                confidence: item.confidence,
                confidence_gap: item.confidence_gap,
                heuristic_edge: item.heuristic_edge,
                entry_rationale: item.entry_rationale.clone(),
                causal_narrative: item.causal_narrative.clone(),
                review_reason_code: item
                    .review_reason_code
                    .map(|code| code.as_str().to_string()),
                review_reason_family: None,
                review_reason_subreasons: vec![],
                policy_primary: item
                    .policy_verdict
                    .as_ref()
                    .map(|verdict| verdict.primary.as_str().to_string()),
                policy_reason: item
                    .policy_verdict
                    .as_ref()
                    .map(|verdict| verdict.rationale.clone()),
                multi_horizon_gate_reason: multi_horizon_gate_reason(&item.risk_notes),
                family_label: hypothesis.map(|hypothesis| hypothesis.family_label.clone()),
                counter_label: item
                    .runner_up_hypothesis_id
                    .as_ref()
                    .and_then(|id| hypothesis_map.get(id.as_str()))
                    .map(|hypothesis| hypothesis.family_label.clone()),
                matched_success_pattern_signature: matched_success_pattern_signature(
                    &item.risk_notes,
                ),
                lifecycle_phase: item
                    .risk_notes
                    .iter()
                    .find(|n| n.starts_with("phase="))
                    .map(|n| n.trim_start_matches("phase=").to_string()),
                tension_driver: item
                    .risk_notes
                    .iter()
                    .find(|n| n.starts_with("driver="))
                    .map(|n| n.trim_start_matches("driver=").to_string()),
                driver_class: item
                    .risk_notes
                    .iter()
                    .find(|n| n.starts_with("driver_class="))
                    .map(|n| n.trim_start_matches("driver_class=").to_string()),
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
                horizon_bucket: Some(crate::live_snapshot::horizon_bucket_label(
                    item.horizon.primary,
                )),
                horizon_urgency: Some(crate::live_snapshot::horizon_urgency_label(
                    item.horizon.urgency,
                )),
                horizon_secondary: item
                    .horizon
                    .secondary
                    .iter()
                    .map(|s| crate::live_snapshot::horizon_bucket_label(s.bucket))
                    .collect(),
                case_signature: Some(item.case_signature(hypothesis)),
                archetype_projections: item.archetype_projections(hypothesis),
                expectation_bindings: hypothesis
                    .map(eden::Hypothesis::expected_bindings)
                    .unwrap_or_default(),
                expectation_violations: hypothesis
                    .map(eden::Hypothesis::expectation_violations)
                    .unwrap_or_default(),
                inferred_intent: Some(item.intent_hypothesis(hypothesis)),
                freshness_state: crate::live_snapshot::live_case_freshness_state(&item.risk_notes),
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
            apply_case_structural_notes(&mut case, &item.risk_notes);
            case
        })
        .collect::<Vec<_>>();
    apply_raw_disagreement_layer(
        &live.raw_events,
        store,
        &mut tactical_cases,
        time::Duration::minutes(5),
    );
    for case in &mut tactical_cases {
        if let Some(symbol) = (!case.symbol.is_empty()).then(|| Symbol(case.symbol.clone())) {
            case.timing_state = crate::live_snapshot::live_case_timing_state(
                &live.raw_events,
                &symbol,
                case,
                time::Duration::minutes(5),
            );
            case.timing_position_in_range = crate::live_snapshot::live_case_position_in_range(
                &live.raw_events,
                &symbol,
                time::Duration::minutes(5),
            );
        }
    }
    enforce_orphan_action_cap(&mut tactical_cases);
    enforce_timing_action_cap(&mut tactical_cases);
    mark_directional_conflicts(&mut tactical_cases);

    let hypothesis_tracks = reasoning_snapshot
        .hypothesis_tracks
        .iter()
        .filter(|item| item.status.as_str() != "stable")
        .take(10)
        .map(|item| LiveHypothesisTrack {
            symbol: symbol_string_from_scope(&item.scope),
            title: item.title.clone(),
            status: item.status.as_str().to_string(),
            age_ticks: item.age_ticks,
            confidence: item.confidence,
        })
        .collect::<Vec<_>>();
    let recent_transitions = hk_recent_transitions(history, store, 32);

    let pressures = graph_insights
        .pressures
        .iter()
        .take(10)
        .map(|item| LivePressure {
            symbol: item.symbol.0.clone(),
            sector: sector_name_for_symbol(store, &item.symbol),
            capital_flow_pressure: item.net_pressure,
            momentum: Decimal::ZERO,
            pressure_delta: item.pressure_delta,
            pressure_duration: item.pressure_duration,
            accelerating: item.accelerating,
        })
        .collect::<Vec<_>>();

    let events = event_snapshot
        .events
        .iter()
        .take(8)
        .map(|item| LiveEvent {
            kind: format!("{:?}", item.value.kind),
            symbol: None,
            magnitude: item.value.magnitude,
            summary: item.value.summary.clone(),
            age_secs: Some(
                (latest.timestamp - item.provenance.observed_at)
                    .whole_seconds()
                    .max(0),
            ),
            freshness: Some(freshness_score_from_age_secs(
                (latest.timestamp - item.provenance.observed_at)
                    .whole_seconds()
                    .max(0),
                event_half_life_secs(&format!("{:?}", item.value.kind)),
            )),
        })
        .collect::<Vec<_>>();
    let structural_deltas = build_hk_structural_deltas(store, dynamics);
    let propagation_senses = build_hk_propagation_senses(reasoning_snapshot, dynamics);
    let active_position_nodes = tracker
        .active_fingerprints()
        .iter()
        .map(|fingerprint| {
            let mut node =
                eden::ontology::ActionNode::from_hk_fingerprint(&fingerprint.symbol, fingerprint);
            node.sector = store
                .sector_name_for_symbol(&fingerprint.symbol)
                .map(str::to_string);
            node
        })
        .collect::<Vec<_>>();
    let raw_microstructure = build_live_raw_microstructure(
        &live.raw_events,
        store,
        &tactical_cases,
        &top_signals,
        &active_position_nodes,
        time::Duration::minutes(5),
    );
    let raw_sources = build_live_raw_sources(
        &live.raw_events,
        store,
        &tactical_cases,
        &top_signals,
        &active_position_nodes,
        time::Duration::minutes(5),
    );
    let signal_translation_gaps =
        build_signal_translation_gaps(&tactical_cases, &top_signals, &raw_sources, 8);
    // Tick-age freshness decay on HK runtime too (parallel to US).
    enforce_freshness_decay(&mut tactical_cases, tick, &recent_transitions);
    let perception = apply_perception_layer(
        tick,
        LiveMarket::Hk,
        &timestamp,
        &mut tactical_cases,
        &recent_transitions,
        &top_signals,
        previous_symbol_states,
        previous_cluster_states,
        previous_world_summary,
    );
    let sector_by_symbol = tactical_cases
        .iter()
        .filter_map(|case| {
            (!case.symbol.is_empty())
                .then(|| Symbol(case.symbol.clone()))
                .and_then(|symbol| {
                    sector_name_for_symbol(store, &symbol)
                        .map(|sector| (case.symbol.clone(), sector))
                })
        })
        .collect::<HashMap<_, _>>();
    let history_samples = hk_surfaced_case_history_samples(history, tick);
    enrich_surfaced_case_evidence(&mut tactical_cases, &sector_by_symbol, &history_samples);
    enrich_cross_tick_momentum(LiveMarket::Hk, tick, &mut tactical_cases);
    crate::live_snapshot::apply_review_reason_consolidation(&mut tactical_cases);
    sort_tactical_cases_for_surface(&mut tactical_cases);
    tactical_cases.truncate(10);

    LiveSnapshot {
        tick,
        timestamp,
        market: LiveMarket::Hk,
        market_phase: hk_market_phase(latest.timestamp).into(),
        market_active: hk_market_active(latest.timestamp),
        stock_count: store.stocks.len(),
        edge_count: brain.graph.edge_count(),
        hypothesis_count: reasoning_snapshot.hypotheses.len(),
        observation_count: observation_snapshot.observations.len(),
        active_positions: active_position_nodes.len(),
        active_position_nodes,
        market_regime: LiveMarketRegime {
            bias: decision.market_regime.bias.as_str().to_string(),
            confidence: decision.market_regime.confidence,
            breadth_up: decision.market_regime.breadth_up,
            breadth_down: decision.market_regime.breadth_down,
            average_return: decision.market_regime.average_return,
            directional_consensus: Some(decision.market_regime.directional_consensus),
            pre_market_sentiment: None,
        },
        stress: LiveStressSnapshot {
            composite_stress: graph_insights.stress.composite_stress,
            sector_synchrony: Some(graph_insights.stress.sector_synchrony),
            pressure_consensus: Some(graph_insights.stress.pressure_consensus),
            momentum_consensus: None,
            pressure_dispersion: None,
            volume_anomaly: None,
        },
        scorecard: summarize_hk_scorecard(scorecard),
        tactical_cases,
        hypothesis_tracks,
        recent_transitions,
        top_signals: top_signals.clone(),
        convergence_scores: top_signals,
        pressures,
        backward_chains: build_hk_backward_chains(&latest.backward_reasoning),
        causal_leaders: build_hk_causal_leaders(causal_timelines),
        events,
        cross_market_signals: Vec::new(),
        cross_market_anomalies: Vec::new(),
        structural_deltas,
        propagation_senses,
        raw_microstructure,
        raw_sources,
        signal_translation_gaps,
        cluster_states: perception.cluster_states,
        symbol_states: perception.symbol_states,
        world_summary: perception.world_summary,
        temporal_bars: build_hk_live_temporal_bars(history, &temporal_symbols),
        lineage: build_hk_lineage_metrics(history),
        success_patterns: Vec::new(),
    }
}

fn hk_surfaced_case_history_samples(
    history: &TickHistory,
    current_tick: u64,
) -> Vec<SurfacedCaseHistorySample> {
    history
        .latest_n(8)
        .into_iter()
        .filter(|record| record.tick_number < current_tick)
        .flat_map(|record| {
            record.tactical_setups.iter().filter_map(move |setup| {
                let symbol = match &setup.scope {
                    eden::ReasoningScope::Symbol(symbol) => symbol.0.clone(),
                    _ => return None,
                };
                (setup.confidence >= Decimal::new(7, 1)).then_some(SurfacedCaseHistorySample {
                    tick: record.tick_number,
                    setup_id: setup.setup_id.clone(),
                    symbol,
                    confidence: setup.confidence,
                    support_fraction: None,
                })
            })
        })
        .collect()
}

/// Debounce window: after receiving a push event, wait this long for more
/// before running the pipeline. Batches rapid-fire events without adding latency.
pub(crate) const LINEAGE_WINDOW: usize = 50;
