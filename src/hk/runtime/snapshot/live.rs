use super::*;
use crate::live_snapshot::LiveSuccessPattern;
use crate::temporal::lineage::compute_vortex_success_patterns;
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
    tracker: &PositionTracker,
    causal_timelines: &std::collections::HashMap<String, CausalTimeline>,
    dynamics: &std::collections::HashMap<Symbol, eden::temporal::analysis::SignalDynamics>,
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
        .map(|item| LiveTacticalCase {
            setup_id: item.setup_id.clone(),
            symbol: symbol_string_from_scope(&item.scope),
            title: item.title.clone(),
            action: item.action.clone(),
            confidence: item.confidence,
            confidence_gap: item.confidence_gap,
            heuristic_edge: item.heuristic_edge,
            entry_rationale: item.entry_rationale.clone(),
            review_reason_code: item
                .review_reason_code
                .map(|code| code.as_str().to_string()),
            policy_primary: item
                .policy_verdict
                .as_ref()
                .map(|verdict| verdict.primary.as_str().to_string()),
            policy_reason: item
                .policy_verdict
                .as_ref()
                .map(|verdict| verdict.rationale.clone()),
            multi_horizon_gate_reason: multi_horizon_gate_reason(&item.risk_notes),
            family_label: hypothesis_map
                .get(item.hypothesis_id.as_str())
                .map(|hypothesis| hypothesis.family_label.clone()),
            counter_label: item
                .runner_up_hypothesis_id
                .as_ref()
                .and_then(|id| hypothesis_map.get(id.as_str()))
                .map(|hypothesis| hypothesis.family_label.clone()),
            matched_success_pattern_signature: matched_success_pattern_signature(&item.risk_notes),
        })
        .collect::<Vec<_>>();
    tactical_cases.sort_by(|a, b| {
        hk_action_surface_priority(a.action.as_str())
            .cmp(&hk_action_surface_priority(b.action.as_str()))
            .then_with(|| b.heuristic_edge.cmp(&a.heuristic_edge))
            .then_with(|| b.confidence_gap.cmp(&a.confidence_gap))
            .then_with(|| b.confidence.cmp(&a.confidence))
            .then_with(|| a.setup_id.cmp(&b.setup_id))
    });
    tactical_cases.truncate(10);

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

    LiveSnapshot {
        tick,
        timestamp,
        market: LiveMarket::Hk,
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
        temporal_bars: build_hk_live_temporal_bars(history, &temporal_symbols),
        lineage: build_hk_lineage_metrics(history),
        success_patterns: compute_vortex_success_patterns(history, LINEAGE_WINDOW)
            .into_iter()
            .take(6)
            .map(|item| LiveSuccessPattern {
                family: item.top_family,
                signature: item.channel_signature,
                dominant_channels: item.dominant_channels,
                samples: item.samples,
                mean_net_return: item.mean_net_return,
                mean_strength: item.mean_strength,
                mean_coherence: item.mean_coherence,
                mean_channel_diversity: Some(item.mean_channel_diversity),
                center_kind: Some(item.center_kind),
                role: Some(item.role),
            })
            .collect(),
    }
}

/// Debounce window: after receiving a push event, wait this long for more
/// before running the pipeline. Batches rapid-fire events without adding latency.
pub(crate) const LINEAGE_WINDOW: usize = 50;
