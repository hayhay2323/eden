use super::*;
use crate::agent::builders::shared::us_recent_transitions;
use crate::live_snapshot::{
    apply_raw_disagreement_layer, build_live_raw_microstructure, build_live_raw_sources,
    build_signal_translation_gaps, enforce_freshness_decay, enforce_orphan_action_cap,
    enforce_timing_action_cap, enrich_cross_tick_momentum, mark_directional_conflicts,
    materialize_signal_translation_cases, LiveSuccessPattern,
};
use crate::pipeline::perception::apply_perception_layer;
use crate::temporal::pyramid::build_us_live_temporal_bars;
use crate::temporal::session::{event_half_life_secs, freshness_score_from_age_secs};
use crate::us::temporal::lineage::{
    compute_us_convergence_success_patterns, compute_us_multi_horizon_lineage_metrics,
};

fn diversified_investigation_frontier<'a>(
    selections: &'a [crate::ontology::reasoning::InvestigationSelection],
    limit: usize,
) -> Vec<&'a crate::ontology::reasoning::InvestigationSelection> {
    // V2 Pass 2: family_key removed from InvestigationSelection. The
    // diversification dedups on hypothesis_id (mirrors what
    // diversified_tactical_frontier does for TacticalSetup) — a stable
    // structural identifier instead of a hardcoded categorical bucket
    // list ("structural_diffusion" / "peer_relay" / etc).
    let mut hypothesis_counts = std::collections::HashMap::<&str, usize>::new();
    let mut selected = Vec::new();
    let mut deferred = Vec::new();

    for selection in selections {
        let key = selection.hypothesis_id.as_str();
        let count = hypothesis_counts.get(key).copied().unwrap_or(0);
        if count >= 2 {
            deferred.push(selection);
            continue;
        }
        hypothesis_counts.insert(key, count + 1);
        selected.push(selection);
        if selected.len() == limit {
            return selected;
        }
    }

    if selected.len() < limit {
        for selection in deferred {
            selected.push(selection);
            if selected.len() == limit {
                break;
            }
        }
    }

    selected
}

fn investigation_operator_next_step(attention_hint: &str) -> &'static str {
    match attention_hint {
        "enter" | "review" => "review_desk",
        "observe" => "collect_confirmation",
        _ => "monitor",
    }
}

fn operator_focus_summary(
    selections: &[crate::ontology::reasoning::InvestigationSelection],
    workflows: &[UsActionWorkflow],
) -> Option<String> {
    if !workflows.is_empty() {
        let items = workflows
            .iter()
            .take(3)
            .map(|workflow| format!("{}@{}", workflow.symbol, workflow.stage))
            .collect::<Vec<_>>();
        return Some(format!("workflow_active -> {}", items.join(", ")));
    }

    let queue = diversified_investigation_frontier(selections, 3);
    if queue.is_empty() {
        return None;
    }
    let next_step = investigation_operator_next_step(queue[0].attention_hint.as_str());
    let items = queue
        .iter()
        .map(|item| item.scope.label())
        .collect::<Vec<_>>();
    Some(format!("{next_step} -> {}", items.join(", ")))
}

fn operator_step_rank(step: &str) -> usize {
    match step {
        "execute" => 0,
        "review_gate" => 1,
        "review_desk" => 2,
        "collect_confirmation" => 3,
        "monitor" => 4,
        "review" => 5,
        "complete" => 6,
        _ => 10,
    }
}

fn display_symbol_state_summary(live_snapshot: &LiveSnapshot) {
    if !live_snapshot.world_summary.is_none() {
        if let Some(world) = live_snapshot.world_summary.as_ref() {
            println!(
                "  World:\n    regime={} conf={} dominant={}",
                world.regime,
                world.confidence.round_dp(3),
                if world.dominant_clusters.is_empty() {
                    "-".into()
                } else {
                    world
                        .dominant_clusters
                        .iter()
                        .take(3)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            );
        }
    }

    if !live_snapshot.cluster_states.is_empty() {
        println!("  Clusters:");
        for cluster in live_snapshot
            .cluster_states
            .iter()
            .filter(|cluster| cluster.state != "low_information")
            .take(4)
        {
            println!(
                "    {} [{}] dir={} conf={} leaders={}",
                cluster.label,
                cluster.state,
                cluster.direction,
                cluster.confidence.round_dp(3),
                if cluster.leader_symbols.is_empty() {
                    "-".into()
                } else {
                    cluster
                        .leader_symbols
                        .iter()
                        .take(3)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                }
            );
        }
    }

    if !live_snapshot.symbol_states.is_empty() {
        println!("  Symbol states:");
        for state in live_snapshot.symbol_states.iter().take(5) {
            let top_for = state
                .supporting_evidence
                .first()
                .map(|item| item.code.as_str())
                .unwrap_or("-");
            let top_against = state
                .opposing_evidence
                .first()
                .map(|item| item.code.as_str())
                .unwrap_or("-");
            let top_missing = state
                .missing_evidence
                .first()
                .map(|item| item.code.as_str())
                .unwrap_or("-");
            println!(
                "    {} [{} {}] w_sf={} count_sf={} for={} against={} missing={}",
                state.symbol,
                state.state_kind,
                state.trend,
                state.weighted_support_fraction.round_dp(3),
                state.count_support_fraction.round_dp(3),
                top_for,
                top_against,
                top_missing,
            );
            if !state.expectations.is_empty() {
                let expectation_summary = state
                    .expectations
                    .iter()
                    .take(2)
                    .map(|expectation| {
                        format!(
                            "{}:{}",
                            expectation.kind.as_str(),
                            expectation.status.as_str()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                println!("      expect={}", expectation_summary);
            }
        }
    }
}

fn lifecycle_step_score(thread: &crate::agent::AgentThread) -> i32 {
    match (
        thread.workflow_stage.as_deref(),
        thread.workflow_next_step.as_deref(),
    ) {
        (Some("reviewed"), _) | (_, Some("complete")) => 70,
        (Some("monitoring"), _) | (_, Some("monitor")) => 60,
        (Some("executed"), _) | (_, Some("execute")) => 50,
        (Some("confirmed"), _) => 45,
        (Some("suggested"), _) => 40,
        (_, Some("review_gate")) => 30,
        (_, Some("review_desk")) => 20,
        (_, Some("collect_confirmation")) => 10,
        _ => 0,
    }
}

fn is_blocked_step(step: Option<&str>) -> bool {
    matches!(
        step,
        Some("review_gate" | "review_desk" | "collect_confirmation")
    )
}

fn lifecycle_label(thread: &crate::agent::AgentThread) -> String {
    thread
        .headline
        .clone()
        .or_else(|| thread.title.clone())
        .unwrap_or_else(|| thread.symbol.clone())
}

fn lifecycle_transition_feed(
    session: &crate::agent::AgentSession,
    previous_session: Option<&crate::agent::AgentSession>,
) -> Vec<(String, Vec<String>)> {
    let Some(previous_session) = previous_session else {
        return Vec::new();
    };
    let previous = previous_session
        .active_threads
        .iter()
        .map(|thread| (thread.symbol.to_ascii_lowercase(), thread))
        .collect::<std::collections::HashMap<_, _>>();

    let mut newly_blocked = Vec::new();
    let mut newly_unlocked = Vec::new();
    let mut promoted = Vec::new();
    let mut degraded = Vec::new();

    for thread in &session.active_threads {
        let key = thread.symbol.to_ascii_lowercase();
        let Some(prev) = previous.get(&key).copied() else {
            continue;
        };
        let current_step = thread.workflow_next_step.as_deref();
        let previous_step = prev.workflow_next_step.as_deref();
        let current_blocked = is_blocked_step(current_step);
        let previous_blocked = is_blocked_step(previous_step);
        let label = lifecycle_label(thread);

        if current_blocked && (!previous_blocked || current_step != previous_step) {
            newly_blocked.push(label);
            continue;
        }
        if previous_blocked && !current_blocked {
            newly_unlocked.push(label);
            continue;
        }

        let current_score = lifecycle_step_score(thread);
        let previous_score = lifecycle_step_score(prev);
        if current_score > previous_score {
            promoted.push(label);
        } else if current_score < previous_score {
            degraded.push(label);
        }
    }

    let mut sections = Vec::new();
    if !newly_blocked.is_empty() {
        sections.push(("newly_blocked".into(), newly_blocked));
    }
    if !newly_unlocked.is_empty() {
        sections.push(("newly_unlocked".into(), newly_unlocked));
    }
    if !promoted.is_empty() {
        sections.push(("promoted".into(), promoted));
    }
    if !degraded.is_empty() {
        sections.push(("degraded".into(), degraded));
    }
    sections
}

fn session_focus_summary(session: &crate::agent::AgentSession) -> Option<String> {
    let mut grouped = std::collections::BTreeMap::<(usize, String), Vec<String>>::new();
    for thread in &session.active_threads {
        let Some(step) = thread.workflow_next_step.as_deref() else {
            continue;
        };
        grouped
            .entry((operator_step_rank(step), step.to_string()))
            .or_default()
            .push(thread.symbol.clone());
    }
    if grouped.is_empty() {
        return None;
    }
    let parts = grouped
        .into_iter()
        .take(2)
        .map(|((_, step), symbols)| {
            let labels = symbols.into_iter().take(3).collect::<Vec<_>>().join(", ");
            format!("{step} -> {labels}")
        })
        .collect::<Vec<_>>();
    Some(parts.join(" | "))
}

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

pub(super) fn build_us_bootstrap_snapshot(
    tick: u64,
    timestamp_str: String,
    store: &Arc<ObjectStore>,
    live: &UsLiveState,
    rest: &UsRestSnapshot,
    previous_symbol_states: &[crate::pipeline::state_engine::PersistentSymbolState],
    previous_cluster_states: &[crate::live_snapshot::LiveClusterState],
    previous_world_summary: Option<&crate::live_snapshot::LiveWorldSummary>,
) -> LiveSnapshot {
    let now = time::OffsetDateTime::now_utc();
    let canonical_market_snapshot = live.to_canonical_snapshot(rest, now);
    let dim_snapshot =
        UsDimensionSnapshot::compute_from_canonical(&canonical_market_snapshot, store);

    let mut top_signals = dim_snapshot
        .dimensions
        .iter()
        .map(|(symbol, dims)| LiveSignal {
            symbol: symbol.0.clone(),
            sector: us_sector_name(store, symbol),
            composite: (dims.capital_flow_direction
                + dims.price_momentum
                + dims.volume_profile
                + dims.pre_post_market_anomaly
                + dims.valuation)
                / Decimal::from(5),
            mark_price: None,
            dimension_composite: None,
            capital_flow_direction: dims.capital_flow_direction,
            price_momentum: dims.price_momentum,
            volume_profile: dims.volume_profile,
            pre_post_market_anomaly: dims.pre_post_market_anomaly,
            valuation: dims.valuation,
            cross_stock_correlation: None,
            sector_coherence: None,
            cross_market_propagation: None,
        })
        .collect::<Vec<_>>();
    top_signals.sort_by(|a, b| b.composite.abs().cmp(&a.composite.abs()));
    top_signals.truncate(20);

    let raw_sources = build_live_raw_sources(
        &live.raw_events,
        store,
        &[],
        &top_signals,
        &[],
        time::Duration::minutes(5),
    );
    let bootstrap_translation_gaps =
        build_signal_translation_gaps(&[], &top_signals, &raw_sources, 8);
    let mut tactical_cases = materialize_signal_translation_cases(
        &live.raw_events,
        store,
        &bootstrap_translation_gaps,
        time::Duration::minutes(5),
        5,
        false,
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
    let signal_translation_gaps =
        build_signal_translation_gaps(&tactical_cases, &top_signals, &raw_sources, 8);
    let perception = apply_perception_layer(
        tick,
        LiveMarket::Us,
        &timestamp_str,
        &mut tactical_cases,
        &[],
        &top_signals,
        previous_symbol_states,
        previous_cluster_states,
        previous_world_summary,
    );

    let pre_market_sentiment = if dim_snapshot.dimensions.is_empty() {
        Decimal::ZERO
    } else {
        dim_snapshot
            .dimensions
            .values()
            .map(|dims| dims.pre_post_market_anomaly)
            .sum::<Decimal>()
            / Decimal::from(dim_snapshot.dimensions.len() as i64)
    };

    LiveSnapshot {
        tick,
        timestamp: timestamp_str,
        market: LiveMarket::Us,
        market_phase: us_market_phase(now).into(),
        market_active: us_market_phase(now) != "closed",
        stock_count: store.stocks.len(),
        edge_count: 0,
        hypothesis_count: 0,
        observation_count: dim_snapshot.dimensions.len(),
        active_positions: 0,
        active_position_nodes: vec![],
        market_regime: LiveMarketRegime {
            bias: "pre_market".into(),
            confidence: pre_market_sentiment.abs(),
            breadth_up: Decimal::ZERO,
            breadth_down: Decimal::ZERO,
            average_return: Decimal::ZERO,
            directional_consensus: None,
            pre_market_sentiment: Some(pre_market_sentiment),
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
        tactical_cases,
        hypothesis_tracks: vec![],
        recent_transitions: vec![],
        top_signals: top_signals.clone(),
        convergence_scores: top_signals,
        pressures: vec![],
        backward_chains: vec![],
        causal_leaders: vec![],
        events: vec![],
        cross_market_signals: vec![],
        cross_market_anomalies: vec![],
        structural_deltas: vec![],
        propagation_senses: vec![],
        raw_microstructure: vec![],
        raw_sources,
        signal_translation_gaps,
        cluster_states: perception.cluster_states,
        symbol_states: perception.symbol_states,
        world_summary: perception.world_summary,
        temporal_bars: vec![],
        lineage: vec![],
        success_patterns: vec![],
    }
}

pub(super) fn build_us_live_snapshot(
    tick: u64,
    timestamp_str: String,
    store: &Arc<ObjectStore>,
    graph: &UsGraph,
    dim_snapshot: &UsDimensionSnapshot,
    reasoning: &UsReasoningSnapshot,
    obs_snapshot: &UsObservationSnapshot,
    decision: &UsDecisionSnapshot,
    insights: &UsGraphInsights,
    scorecard: &UsSignalScorecard,
    backward: &crate::us::pipeline::world::UsBackwardSnapshot,
    causal_timelines: &HashMap<Symbol, crate::us::temporal::causality::UsCausalTimeline>,
    cross_market_signals: &[crate::bridges::hk_to_us::CrossMarketSignal],
    dynamics: &HashMap<Symbol, crate::us::temporal::analysis::UsSignalDynamics>,
    tick_history: &UsTickHistory,
    live: &UsLiveState,
    position_tracker: &UsPositionTracker,
    workflows: &[UsActionWorkflow],
    propagation_senses: &[crate::us::graph::insights::UsPropagationSense],
    sorted_events: &[crate::ontology::domain::Event<crate::us::pipeline::signals::UsEventRecord>],
    previous_symbol_states: &[crate::pipeline::state_engine::PersistentSymbolState],
    previous_cluster_states: &[crate::live_snapshot::LiveClusterState],
    previous_world_summary: Option<&crate::live_snapshot::LiveWorldSummary>,
) -> LiveSnapshot {
    let mut top_signals = dim_snapshot
        .dimensions
        .iter()
        .map(|(symbol, dims)| LiveSignal {
            symbol: symbol.0.clone(),
            sector: us_sector_name(store, symbol),
            composite: (dims.capital_flow_direction
                + dims.price_momentum
                + dims.volume_profile
                + dims.pre_post_market_anomaly
                + dims.valuation)
                / Decimal::from(5),
            mark_price: None,
            dimension_composite: None,
            capital_flow_direction: dims.capital_flow_direction,
            price_momentum: dims.price_momentum,
            volume_profile: dims.volume_profile,
            pre_post_market_anomaly: dims.pre_post_market_anomaly,
            valuation: dims.valuation,
            cross_stock_correlation: None,
            sector_coherence: None,
            cross_market_propagation: None,
        })
        .collect::<Vec<_>>();
    top_signals.sort_by(|a, b| b.composite.abs().cmp(&a.composite.abs()));
    top_signals.truncate(120);

    let mut live_events = sorted_events
        .iter()
        .take(8)
        .map(|item| LiveEvent {
            kind: format!("{:?}", item.value.kind),
            symbol: match &item.value.scope {
                crate::us::pipeline::signals::UsSignalScope::Symbol(s) => Some(s.to_string()),
                _ => None,
            },
            magnitude: item.value.magnitude,
            summary: item.value.summary.clone(),
            age_secs: Some(
                (tick_history
                    .latest()
                    .map(|record| record.timestamp)
                    .unwrap_or(item.provenance.observed_at)
                    - item.provenance.observed_at)
                    .whole_seconds()
                    .max(0),
            ),
            freshness: Some(freshness_score_from_age_secs(
                (tick_history
                    .latest()
                    .map(|record| record.timestamp)
                    .unwrap_or(item.provenance.observed_at)
                    - item.provenance.observed_at)
                    .whole_seconds()
                    .max(0),
                event_half_life_secs(&format!("{:?}", item.value.kind)),
            )),
        })
        .collect::<Vec<_>>();
    let mut structural_delta_events = dynamics
        .values()
        .filter(|item| {
            item.composite_delta.abs() >= Decimal::new(4, 2)
                || item.composite_acceleration.abs() >= Decimal::new(2, 2)
        })
        .map(|item| LiveEvent {
            kind: "StructuralDelta".into(),
            symbol: Some(item.symbol.to_string()),
            magnitude: item
                .composite_delta
                .abs()
                .max(item.composite_acceleration.abs())
                .min(Decimal::ONE),
            summary: format!(
                "{} structure delta={} accel={} duration={}",
                item.symbol,
                item.composite_delta.round_dp(4),
                item.composite_acceleration.round_dp(4),
                item.composite_duration
            ),
            age_secs: None,
            freshness: None,
        })
        .collect::<Vec<_>>();
    structural_delta_events.sort_by(|a, b| b.magnitude.cmp(&a.magnitude));
    live_events.extend(structural_delta_events.into_iter().take(8));
    live_events.extend(propagation_senses.iter().take(12).map(|item| LiveEvent {
        kind: "GraphPropagationSense".into(),
        symbol: Some(item.target_symbol.to_string()),
        magnitude: item.propagation_strength,
        summary: format!(
            "{} -> {} via {} strength={} lag_gap={}",
            item.source_symbol,
            item.target_symbol,
            item.channel,
            item.propagation_strength.round_dp(4),
            item.lag_gap.round_dp(4)
        ),
        age_secs: None,
        freshness: None,
    }));
    live_events.sort_by(|a, b| b.magnitude.cmp(&a.magnitude));
    live_events.truncate(24);

    let active_position_nodes = position_tracker
        .active_fingerprints()
        .into_iter()
        .map(|fingerprint| {
            let workflow = workflows
                .iter()
                .find(|workflow| workflow.symbol == fingerprint.symbol);
            let mut node =
                crate::ontology::ActionNode::from_us_position(fingerprint, workflow, tick);
            node.sector = us_sector_name(store, &fingerprint.symbol);
            node
        })
        .collect::<Vec<_>>();
    let temporal_symbols = top_signals
        .iter()
        .map(|item| item.symbol.clone())
        .collect::<Vec<_>>();

    let mut tactical_cases = reasoning
        .tactical_setups
        .iter()
        .map(|item| {
            let hypothesis = reasoning
                .hypotheses
                .iter()
                .find(|hypothesis| hypothesis.hypothesis_id == item.hypothesis_id);
            let family_label = hypothesis.map(|hypothesis| hypothesis.family_label.clone());
            let counter_label = item
                .runner_up_hypothesis_id
                .as_ref()
                .and_then(|id| {
                    reasoning
                        .hypotheses
                        .iter()
                        .find(|hypothesis| hypothesis.hypothesis_id == *id)
                })
                .map(|hypothesis| hypothesis.family_label.clone());

            let mut case = LiveTacticalCase {
                setup_id: item.setup_id.clone(),
                symbol: match &item.scope {
                    crate::ontology::reasoning::ReasoningScope::Symbol(symbol) => symbol.0.clone(),
                    _ => String::new(),
                },
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
                family_label,
                counter_label,
                matched_success_pattern_signature: matched_success_pattern_signature(
                    &item.risk_notes,
                ),
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
                    .map(crate::ontology::Hypothesis::expected_bindings)
                    .unwrap_or_default(),
                expectation_violations: hypothesis
                    .map(crate::ontology::Hypothesis::expectation_violations)
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
            crate::live_snapshot::apply_case_structural_notes(&mut case, &item.risk_notes);
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
    crate::live_snapshot::sort_tactical_cases_for_surface(&mut tactical_cases);
    tactical_cases.truncate(10);

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
    let translation_cases = materialize_signal_translation_cases(
        &live.raw_events,
        store,
        &build_signal_translation_gaps(&tactical_cases, &top_signals, &raw_sources, 8),
        time::Duration::minutes(5),
        3,
        true,
    );
    tactical_cases.extend(translation_cases);
    enforce_orphan_action_cap(&mut tactical_cases);
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
    enforce_timing_action_cap(&mut tactical_cases);
    mark_directional_conflicts(&mut tactical_cases);
    let signal_translation_gaps =
        build_signal_translation_gaps(&tactical_cases, &top_signals, &raw_sources, 8);
    let recent_transitions = us_recent_transitions(tick_history, store, 32);
    // Tick-age freshness decay: downgrade enters that have been at `action=enter`
    // for too many ticks. Runs after sort/truncate so the final top-10 sees
    // freshness-adjusted actions before it hits the perception layer.
    enforce_freshness_decay(&mut tactical_cases, tick, &recent_transitions);
    let perception = apply_perception_layer(
        tick,
        LiveMarket::Us,
        &timestamp_str,
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
                    us_sector_name(store, &symbol).map(|sector| (case.symbol.clone(), sector))
                })
        })
        .collect::<HashMap<_, _>>();
    let history_samples = us_surfaced_case_history_samples(tick_history, tick);
    crate::live_snapshot::enrich_surfaced_case_evidence(
        &mut tactical_cases,
        &sector_by_symbol,
        &history_samples,
    );
    enrich_cross_tick_momentum(LiveMarket::Us, tick, &mut tactical_cases);
    crate::live_snapshot::apply_review_reason_consolidation(&mut tactical_cases);
    crate::live_snapshot::sort_tactical_cases_for_surface(&mut tactical_cases);
    tactical_cases.truncate(10);
    let snapshot_now = tick_history
        .latest()
        .map(|record| record.timestamp)
        .unwrap_or_else(time::OffsetDateTime::now_utc);

    LiveSnapshot {
        tick,
        timestamp: timestamp_str,
        market: LiveMarket::Us,
        market_phase: us_market_phase(snapshot_now).into(),
        market_active: us_market_phase(snapshot_now) != "closed",
        stock_count: graph.stock_nodes.len(),
        edge_count: graph.graph.edge_count(),
        hypothesis_count: reasoning.hypotheses.len(),
        observation_count: obs_snapshot.observations.len(),
        active_positions: active_position_nodes.len(),
        active_position_nodes,
        market_regime: LiveMarketRegime {
            bias: decision.market_regime.bias.as_str().to_string(),
            confidence: decision.market_regime.confidence,
            breadth_up: decision.market_regime.breadth_up,
            breadth_down: decision.market_regime.breadth_down,
            average_return: decision.market_regime.macro_return,
            directional_consensus: None,
            pre_market_sentiment: Some(decision.market_regime.pre_market_sentiment),
        },
        stress: LiveStressSnapshot {
            composite_stress: insights.stress.composite_stress,
            sector_synchrony: None,
            pressure_consensus: None,
            momentum_consensus: Some(insights.stress.momentum_consensus),
            pressure_dispersion: Some(insights.stress.pressure_dispersion),
            volume_anomaly: Some(insights.stress.volume_anomaly),
        },
        scorecard: LiveScorecard {
            total_signals: scorecard.total_signals,
            resolved_signals: scorecard.resolved_signals,
            hits: scorecard.hits,
            misses: scorecard.misses,
            hit_rate: scorecard.hit_rate,
            mean_return: scorecard.mean_return,
            actionable_resolved: scorecard.actionable_resolved,
            actionable_hits: scorecard.actionable_hits,
            actionable_hit_rate: scorecard.actionable_hit_rate,
            actionable_mean_return: scorecard.actionable_mean_return,
            actionable_excess_hit_rate: scorecard.actionable_excess_hit_rate,
        },
        tactical_cases,
        hypothesis_tracks: reasoning
            .hypothesis_tracks
            .iter()
            .map(|t| LiveHypothesisTrack {
                symbol: match &t.scope {
                    crate::ontology::reasoning::ReasoningScope::Symbol(s) => s.0.clone(),
                    _ => String::new(),
                },
                title: t.title.clone(),
                status: t.status.as_str().to_string(),
                age_ticks: t.age_ticks,
                confidence: t.confidence,
            })
            .collect(),
        recent_transitions,
        top_signals,
        convergence_scores: decision
            .convergence_scores
            .iter()
            .map(|(symbol, score)| LiveSignal {
                symbol: symbol.0.clone(),
                sector: us_sector_name(store, symbol),
                composite: score.composite,
                mark_price: None,
                dimension_composite: Some(score.dimension_composite),
                capital_flow_direction: score.capital_flow_direction,
                price_momentum: score.price_momentum,
                volume_profile: score.volume_profile,
                pre_post_market_anomaly: score.pre_post_market_anomaly,
                valuation: score.valuation,
                cross_stock_correlation: Some(score.cross_stock_correlation),
                sector_coherence: score.sector_coherence,
                cross_market_propagation: score.cross_market_propagation,
            })
            .collect(),
        pressures: insights
            .pressures
            .iter()
            .take(120)
            .map(|item| LivePressure {
                symbol: item.symbol.0.clone(),
                sector: us_sector_name(store, &item.symbol),
                capital_flow_pressure: item.capital_flow_pressure,
                momentum: item.momentum,
                pressure_delta: item.pressure_delta,
                pressure_duration: item.pressure_duration,
                accelerating: item.accelerating,
            })
            .collect(),
        backward_chains: backward
            .chains
            .iter()
            .take(160)
            .map(|item| LiveBackwardChain {
                symbol: item.symbol.0.clone(),
                conclusion: item.conclusion.clone(),
                primary_driver: item.primary_driver.clone(),
                confidence: item.confidence,
                freshness: None,
                evidence: item
                    .evidence
                    .iter()
                    .take(5)
                    .map(|e| crate::live_snapshot::LiveEvidence {
                        source: e.source.clone(),
                        description: e.description.clone(),
                        weight: e.weight,
                        direction: e.direction,
                    })
                    .collect(),
            })
            .collect(),
        causal_leaders: causal_timelines
            .iter()
            .take(10)
            .map(|(symbol, item)| LiveCausalLeader {
                symbol: symbol.0.clone(),
                current_leader: item.current_leader.clone(),
                leader_streak: item.leader_streak,
                flips: item.flips.len(),
            })
            .collect(),
        events: live_events,
        cross_market_signals: cross_market_signals
            .iter()
            .map(|item| LiveCrossMarketSignal {
                us_symbol: item.us_symbol.0.clone(),
                hk_symbol: item.hk_symbol.0.clone(),
                propagation_confidence: item.propagation_confidence,
                time_since_hk_close_minutes: Some(item.time_since_hk_close_minutes),
            })
            .collect(),
        cross_market_anomalies: insights
            .cross_market_anomalies
            .iter()
            .map(|item| LiveCrossMarketAnomaly {
                us_symbol: item.us_symbol.0.clone(),
                hk_symbol: item.hk_symbol.0.clone(),
                expected_direction: item.expected_direction,
                actual_direction: item.actual_direction,
                divergence: item.divergence,
            })
            .collect(),
        raw_microstructure,
        raw_sources,
        signal_translation_gaps,
        cluster_states: perception.cluster_states,
        symbol_states: perception.symbol_states,
        world_summary: perception.world_summary,
        structural_deltas: dynamics
            .values()
            .filter(|item| {
                item.composite_delta.abs() >= Decimal::new(2, 2)
                    || item.composite_acceleration.abs() >= Decimal::new(1, 2)
                    || item.pre_market_trend.abs() >= Decimal::new(2, 2)
            })
            .map(|item| {
                let signal = tick_history
                    .latest()
                    .and_then(|record| record.signals.get(&item.symbol));
                LiveStructuralDelta {
                    symbol: item.symbol.to_string(),
                    sector: us_sector_name(store, &item.symbol),
                    composite_delta: item.composite_delta,
                    composite_acceleration: item.composite_acceleration,
                    capital_flow_delta: signal
                        .map(|signal| signal.capital_flow_delta)
                        .unwrap_or(Decimal::ZERO),
                    flow_persistence: signal.map(|signal| signal.flow_persistence).unwrap_or(0),
                    flow_reversal: signal.map(|signal| signal.flow_reversal).unwrap_or(false),
                    pre_market_trend: item.pre_market_trend,
                }
            })
            .collect(),
        propagation_senses: propagation_senses
            .iter()
            .map(|item| LivePropagationSense {
                source_symbol: item.source_symbol.to_string(),
                target_symbol: item.target_symbol.to_string(),
                channel: item.channel.clone(),
                propagation_strength: item.propagation_strength,
                target_momentum: item.target_momentum,
                lag_gap: item.lag_gap,
            })
            .collect(),
        temporal_bars: build_us_live_temporal_bars(tick_history, &temporal_symbols),
        lineage: compute_us_multi_horizon_lineage_metrics(tick_history)
            .into_iter()
            .map(|item| LiveLineageMetric {
                horizon: Some(item.horizon),
                template: item.template,
                total: item.total,
                resolved: item.resolved,
                hits: item.hits,
                hit_rate: item.hit_rate,
                mean_return: item.mean_return,
            })
            .collect(),
        success_patterns: compute_us_convergence_success_patterns(
            tick_history,
            crate::us::common::SIGNAL_RESOLUTION_LAG,
        )
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
            center_kind: None,
            role: None,
        })
        .collect(),
    }
}

fn us_surfaced_case_history_samples(
    history: &crate::us::temporal::buffer::UsTickHistory,
    current_tick: u64,
) -> Vec<crate::live_snapshot::SurfacedCaseHistorySample> {
    history
        .latest_n(8)
        .into_iter()
        .filter(|record| record.tick_number < current_tick)
        .flat_map(|record| {
            record.tactical_setups.iter().filter_map(move |setup| {
                let symbol = match &setup.scope {
                    crate::ontology::reasoning::ReasoningScope::Symbol(symbol) => symbol.0.clone(),
                    _ => return None,
                };
                (setup.confidence >= Decimal::new(7, 1)).then_some(
                    crate::live_snapshot::SurfacedCaseHistorySample {
                        tick: record.tick_number,
                        setup_id: setup.setup_id.clone(),
                        symbol,
                        confidence: setup.confidence,
                        support_fraction: None,
                    },
                )
            })
        })
        .collect()
}

pub(super) fn display_us_runtime_summary(
    live_snapshot: &LiveSnapshot,
    tick: u64,
    timestamp_str: &str,
    graph: &UsGraph,
    decision: &UsDecisionSnapshot,
    event_snapshot: &UsEventSnapshot,
    reasoning: &UsReasoningSnapshot,
    scorecard: &UsSignalScorecard,
    live_push_count: u64,
    sorted_convergence: &[(&Symbol, &crate::us::graph::decision::UsConvergenceScore)],
    cross_market_signals: &[crate::bridges::hk_to_us::CrossMarketSignal],
    sorted_events: &[crate::ontology::domain::Event<crate::us::pipeline::signals::UsEventRecord>],
    insights: &UsGraphInsights,
    backward: &crate::us::pipeline::world::UsBackwardSnapshot,
    position_tracker: &UsPositionTracker,
    workflows: &[UsActionWorkflow],
    briefing: &crate::agent::AgentBriefing,
    session: &crate::agent::AgentSession,
    previous_session: Option<&crate::agent::AgentSession>,
    learned_edge_count: usize,
) {
    let latent_vortex_count = reasoning
        .hypotheses
        .iter()
        .filter(|hypothesis| {
            matches!(
                hypothesis.kind,
                Some(crate::ontology::reasoning::HypothesisKind::LatentVortex)
            )
        })
        .count();
    println!(
        "\n[US tick {}] {} | {} stocks | {} edges | regime={} | {} events | {} hyps | {} setups | scorecard {}/{} ({:.0}%) | {} push",
        tick,
        timestamp_str,
        graph.stock_nodes.len(),
        graph.graph.edge_count(),
        decision.market_regime.bias,
        event_snapshot.events.len(),
        reasoning.hypotheses.len(),
        reasoning.tactical_setups.len(),
        scorecard.hits,
        scorecard.resolved_signals,
        scorecard.hit_rate * Decimal::from(100),
        live_push_count,
    );
    if latent_vortex_count > 0 || learned_edge_count > 0 {
        println!(
            "  Topology: latent_vortex={} learned_edges={}",
            latent_vortex_count, learned_edge_count
        );
    }
    if let Some(focus) = session_focus_summary(session)
        .or_else(|| briefing.headline.as_deref().map(str::to_string))
        .or_else(|| operator_focus_summary(&reasoning.investigation_selections, workflows))
    {
        println!("  Focus: {}", focus);
    }
    let lifecycle_feed = lifecycle_transition_feed(session, previous_session);
    if !lifecycle_feed.is_empty() {
        println!("  Lifecycle:");
        for (label, items) in lifecycle_feed.iter().take(4) {
            println!(
                "    {}: {}",
                label,
                items.iter().take(4).cloned().collect::<Vec<_>>().join(", ")
            );
        }
    }

    display_symbol_state_summary(live_snapshot);

    if !sorted_convergence.is_empty() {
        println!("  Convergence:");
        for (sym, score) in sorted_convergence.iter().take(5) {
            let cm_tag = score
                .cross_market_propagation
                .map(|v| format!(" hk={}", v.round_dp(3)))
                .unwrap_or_default();
            println!(
                "    {} composite={} (dim={} corr={} sec={}){}",
                sym,
                score.composite.round_dp(4),
                score.dimension_composite.round_dp(3),
                score.cross_stock_correlation.round_dp(3),
                score
                    .sector_coherence
                    .map(|v| format!("{}", v.round_dp(3)))
                    .unwrap_or_else(|| "-".into()),
                cm_tag,
            );
        }
    }

    if !cross_market_signals.is_empty() {
        println!("  Cross-market:");
        for sig in cross_market_signals {
            println!(
                "    {} <- {} conf={} (hk_comp={} inst={} {}min ago)",
                sig.us_symbol,
                sig.hk_symbol,
                sig.propagation_confidence,
                sig.hk_composite,
                sig.hk_inst_alignment,
                sig.time_since_hk_close_minutes,
            );
        }
    }

    if !sorted_events.is_empty() {
        println!("  Events:");
        for (event, live_event) in sorted_events
            .iter()
            .zip(live_snapshot.events.iter())
            .take(5)
        {
            println!(
                "    [{:?}] mag={} fresh={} age={}s {}",
                event.value.kind,
                event.value.magnitude,
                live_event
                    .freshness
                    .map(|value| value.round_dp(2).to_string())
                    .unwrap_or_else(|| "-".into()),
                live_event
                    .age_secs
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "-".into()),
                event.value.summary
            );
        }
    }

    if !reasoning.investigation_selections.is_empty() {
        println!("  Investigations:");
        for selection in diversified_investigation_frontier(&reasoning.investigation_selections, 5)
        {
            let gate = multi_horizon_gate_reason(&selection.notes)
                .map(|reason| format!(" gate={reason}"))
                .unwrap_or_default();
            println!(
                "    {} [{}] prio={} conf={} gap={}{}",
                selection.title,
                selection.attention_hint,
                selection.priority_score.round_dp(3),
                selection.confidence.round_dp(3),
                selection.confidence_gap.round_dp(3),
                gate,
            );
        }
    }

    if !reasoning.tactical_setups.is_empty() {
        println!("  Tactical setups:");
        for setup in
            crate::ontology::reasoning::diversified_tactical_frontier(&reasoning.tactical_setups, 5)
        {
            let gate = live_snapshot
                .tactical_cases
                .iter()
                .find(|item| item.setup_id == setup.setup_id)
                .and_then(|item| item.multi_horizon_gate_reason.clone())
                .map(|reason| format!(" gate={reason}"))
                .unwrap_or_default();
            let policy = live_snapshot
                .tactical_cases
                .iter()
                .find(|item| item.setup_id == setup.setup_id)
                .and_then(|item| item.policy_primary.clone())
                .map(|primary| format!(" policy={primary}"))
                .unwrap_or_default();
            println!(
                "    {} [{}] conf={} gap={} edge={}{}{}",
                setup.title,
                setup.action,
                setup.confidence,
                setup.confidence_gap,
                setup.heuristic_edge,
                policy,
                gate,
            );
        }
    }

    if !live_snapshot.lineage.is_empty() {
        println!("  Lineage:");
        for ls in live_snapshot.lineage.iter().take(6) {
            println!(
                "    [{}] {} {}/{} resolved, hit_rate={} mean_ret={}",
                ls.horizon.as_deref().unwrap_or("tick"),
                ls.template,
                ls.resolved,
                ls.total,
                ls.hit_rate.round_dp(3),
                ls.mean_return.round_dp(4),
            );
        }
    }

    if !live_snapshot.temporal_bars.is_empty() {
        println!("  Temporal bars:");
        for bar in live_snapshot.temporal_bars.iter().take(4) {
            println!(
                "    [{}] {} o={} c={} mean={} flow={} events={} persistence={}t",
                bar.horizon,
                bar.symbol,
                bar.open
                    .map(|value| value.round_dp(3).to_string())
                    .unwrap_or_else(|| "-".into()),
                bar.close
                    .map(|value| value.round_dp(3).to_string())
                    .unwrap_or_else(|| "-".into()),
                bar.composite_mean.round_dp(3),
                bar.capital_flow_delta.round_dp(3),
                bar.event_count,
                bar.signal_persistence,
            );
        }
    }

    if !insights.pressures.is_empty() {
        println!("  Pressures:");
        for p in insights.pressures.iter().take(3) {
            println!(
                "    {} flow={} vol={} mom={} {}{}",
                p.symbol,
                p.capital_flow_pressure.round_dp(3),
                p.volume_intensity.round_dp(3),
                p.momentum.round_dp(3),
                if p.accelerating { "↑" } else { "" },
                if p.pressure_duration > 1 {
                    format!(" {}t", p.pressure_duration)
                } else {
                    String::new()
                },
            );
        }
    }
    if !backward.chains.is_empty() {
        println!("  Backward:");
        for c in backward.chains.iter().take(3) {
            let freshness = live_snapshot
                .backward_chains
                .iter()
                .find(|item| item.symbol.eq_ignore_ascii_case(&c.symbol.0))
                .and_then(|item| item.freshness);
            println!(
                "    {} [{}] fresh={}",
                c.conclusion,
                c.primary_driver,
                freshness
                    .map(|value| value.round_dp(2).to_string())
                    .unwrap_or_else(|| "-".into()),
            );
        }
    }

    let operator_queue = session
        .active_threads
        .iter()
        .filter(|thread| {
            matches!(
                thread.workflow_next_step.as_deref(),
                Some("review_gate" | "review_desk" | "collect_confirmation")
            )
        })
        .take(5)
        .collect::<Vec<_>>();
    let operator_workflows = session
        .active_threads
        .iter()
        .filter(|thread| thread.workflow_stage.is_some())
        .take(5)
        .collect::<Vec<_>>();
    if !operator_queue.is_empty()
        || !operator_workflows.is_empty()
        || !workflows.is_empty()
        || !live_snapshot.active_position_nodes.is_empty()
    {
        println!("  Operator:");
        if !operator_queue.is_empty() {
            let mut grouped = std::collections::BTreeMap::<
                (usize, String),
                Vec<&crate::agent::AgentThread>,
            >::new();
            for thread in &operator_queue {
                let step = thread
                    .workflow_next_step
                    .clone()
                    .unwrap_or_else(|| "monitor".into());
                grouped
                    .entry((operator_step_rank(step.as_str()), step))
                    .or_default()
                    .push(*thread);
            }
            for ((_, step), threads) in grouped.into_iter() {
                println!("    {}:", step);
                for thread in threads.into_iter().take(3) {
                    println!(
                        "      {} prio={} {}",
                        thread
                            .title
                            .clone()
                            .unwrap_or_else(|| thread.symbol.clone()),
                        thread.priority.round_dp(3),
                        thread
                            .headline
                            .clone()
                            .unwrap_or_else(|| thread.symbol.clone()),
                    );
                    if let Some(reason) = thread.blocked_reason.as_deref() {
                        println!("        why={}", reason);
                    }
                    if let Some(unlock) = thread.unlock_condition.as_deref() {
                        println!("        unlock={}", unlock);
                    }
                }
            }
        }
        if !operator_workflows.is_empty() {
            println!("    Workflows:");
            for thread in operator_workflows.iter().take(3) {
                let active_node = live_snapshot
                    .active_position_nodes
                    .iter()
                    .find(|item| item.symbol.0.eq_ignore_ascii_case(&thread.symbol));
                let pnl = active_node
                    .and_then(|item| item.pnl)
                    .map(|value| value.round_dp(3).to_string())
                    .unwrap_or_else(|| "-".into());
                let age = active_node.map(|item| item.age_ticks).unwrap_or(0);
                let exit_flag = active_node.map(|item| item.exit_forming).unwrap_or(false);
                println!(
                    "      {} stage={} next={} pnl={} age={}t exit={}",
                    thread.symbol,
                    thread.workflow_stage.as_deref().unwrap_or("-"),
                    thread.workflow_next_step.as_deref().unwrap_or("monitor"),
                    pnl,
                    age,
                    if exit_flag { "yes" } else { "no" },
                );
                if let Some(reason) = thread.blocked_reason.as_deref() {
                    println!("        why={}", reason);
                }
                if let Some(unlock) = thread.unlock_condition.as_deref() {
                    println!("        unlock={}", unlock);
                }
            }
        } else if !position_tracker.active_fingerprints().is_empty() {
            println!(
                "    Positions: {} active",
                position_tracker.active_fingerprints().len()
            );
        }
    } else if !position_tracker.active_fingerprints().is_empty() {
        println!(
            "  Positions: {} active, {} workflows",
            position_tracker.active_fingerprints().len(),
            workflows.len()
        );
    }
}
