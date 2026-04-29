use super::*;

pub(crate) fn display_hk_live_summary(
    live_snapshot: &crate::live_snapshot::LiveSnapshot,
    briefing: &crate::agent::AgentBriefing,
    session: &crate::agent::AgentSession,
) {
    if let Some(headline) = briefing
        .headline
        .as_deref()
        .or_else(|| briefing.summary.first().map(String::as_str))
    {
        println!("  Focus: {}", headline);
    }

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
            println!(
                "    {} [{} {}] w_sf={} count_sf={} for={} against={} missing={}",
                state.symbol,
                state.state_kind,
                state.trend,
                state.weighted_support_fraction.round_dp(3),
                state.count_support_fraction.round_dp(3),
                state
                    .supporting_evidence
                    .first()
                    .map(|item| item.code.as_str())
                    .unwrap_or("-"),
                state
                    .opposing_evidence
                    .first()
                    .map(|item| item.code.as_str())
                    .unwrap_or("-"),
                state
                    .missing_evidence
                    .first()
                    .map(|item| item.code.as_str())
                    .unwrap_or("-"),
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

    if !live_snapshot.tactical_cases.is_empty() {
        println!("  Cases:");
        for case in live_snapshot.tactical_cases.iter().take(5) {
            println!(
                "    {} [{}] state={} weighted_sf={} count_sf={}",
                case.title,
                case.action,
                case.local_state.as_deref().unwrap_or("unknown"),
                case.raw_disagreement
                    .as_ref()
                    .map(|item| item.support_fraction.round_dp(3).to_string())
                    .unwrap_or_else(|| "-".into()),
                case.raw_disagreement
                    .as_ref()
                    .map(|item| item.count_support_fraction.round_dp(3).to_string())
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
    if !operator_queue.is_empty() {
        println!("  Operator:");
        for thread in operator_queue {
            println!(
                "    {} -> {}",
                thread.symbol,
                thread.workflow_next_step.as_deref().unwrap_or("monitor")
            );
            if let Some(reason) = thread.blocked_reason.as_deref() {
                println!("      why={}", reason);
            }
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn display_hk_reasoning_console(
    pct: Decimal,
    store: &std::sync::Arc<eden::ontology::store::ObjectStore>,
    graph_insights: &GraphInsights,
    observation_snapshot: &ObservationSnapshot,
    event_snapshot: &EventSnapshot,
    derived_signal_snapshot: &DerivedSignalSnapshot,
    workflow_snapshots: &[ActionWorkflowSnapshot],
    reasoning_snapshot: &ReasoningSnapshot,
    world_snapshots: &WorldSnapshots,
    decision: &DecisionSnapshot,
    ready_convergence_scores: &HashMap<Symbol, crate::graph::convergence::ConvergenceScore>,
    actionable_order_suggestions: &[crate::graph::decision::OrderSuggestion],
    lineage_stats: &crate::temporal::lineage::LineageStats,
    causal_timelines: &HashMap<String, CausalTimeline>,
) {
    println!("\n── Convergence Scores ──");
    let mut conv_syms: Vec<_> = ready_convergence_scores.iter().collect();
    conv_syms.sort_by(|a, b| b.1.composite.abs().cmp(&a.1.composite.abs()));
    for (sym, c) in &conv_syms {
        let dir = if c.composite > Decimal::ZERO {
            "▲"
        } else if c.composite < Decimal::ZERO {
            "▼"
        } else {
            "—"
        };
        println!(
            "  {:>8}  composite={}{:>+7}%  inst={:>+7}%  sector={:>+7}%  corr={:>+7}%",
            sym,
            dir,
            (c.composite * pct).round_dp(1),
            (c.institutional_alignment * pct).round_dp(1),
            c.sector_coherence
                .map(|s| format!("{:>+7}", (s * pct).round_dp(1)))
                .unwrap_or_else(|| "    n/a".into()),
            (c.cross_stock_correlation * pct).round_dp(1),
        );
    }

    graph_insights.display(store);

    println!(
        "\n── Semantic Layers ──\n  observations={}  events={}  derived_signals={}  workflows={}  hypotheses={}  paths={}  setups={}  tracks={}  clusters={}  world_entities={}  backward_cases={}",
        observation_snapshot.observations.len(),
        event_snapshot.events.len(),
        derived_signal_snapshot.signals.len(),
        workflow_snapshots.len(),
        reasoning_snapshot.hypotheses.len(),
        reasoning_snapshot.propagation_paths.len(),
        reasoning_snapshot.tactical_setups.len(),
        reasoning_snapshot.hypothesis_tracks.len(),
        reasoning_snapshot.case_clusters.len(),
        world_snapshots.world_state.entities.len(),
        world_snapshots.backward_reasoning.investigations.len(),
    );
    for event in event_snapshot.events.iter().take(5) {
        println!(
            "  Event:        {:?}  {:?}  mag={:+}  {}",
            event.value.scope,
            event.value.kind,
            event.value.magnitude.round_dp(2),
            event.value.summary,
        );
    }
    for signal in derived_signal_snapshot.signals.iter().take(5) {
        println!(
            "  Signal:       {:?}  {:?}  strength={:+}  {}",
            signal.value.scope,
            signal.value.kind,
            signal.value.strength.round_dp(2),
            signal.value.summary,
        );
    }
    for path in select_propagation_preview(&reasoning_snapshot.propagation_paths, 5) {
        println!(
            "  Path:         hops={}  conf={:+}  {}",
            path.steps.len(),
            path.confidence.round_dp(3),
            path.summary,
        );
    }
    if let Some(path) = best_multi_hop_by_len(&reasoning_snapshot.propagation_paths, 2) {
        println!(
            "  best_2hop:    conf={:+}  {}",
            path.confidence.round_dp(3),
            path.summary,
        );
    }
    if let Some(path) = best_multi_hop_by_len(&reasoning_snapshot.propagation_paths, 3) {
        println!(
            "  best_3hop:    conf={:+}  {}",
            path.confidence.round_dp(3),
            path.summary,
        );
    }
    for workflow in workflow_snapshots.iter().take(5) {
        println!(
            "  Workflow:     {}  stage={}  {}",
            workflow.workflow_id, workflow.stage, workflow.title,
        );
        if let Some(note) = &workflow.note {
            println!("                why={}", note);
        }
    }
    let hypothesis_map: HashMap<_, _> = reasoning_snapshot
        .hypotheses
        .iter()
        .map(|hypothesis| (hypothesis.hypothesis_id.as_str(), hypothesis))
        .collect();
    let track_map: HashMap<_, _> = reasoning_snapshot
        .hypothesis_tracks
        .iter()
        .filter(|track| track.invalidated_at.is_none())
        .map(|track| (track.setup_id.as_str(), track))
        .collect();
    if !reasoning_snapshot.case_clusters.is_empty() {
        println!("\n── Top Tactical Clusters ──");
        for cluster in reasoning_snapshot.case_clusters.iter().take(5) {
            println!(
                "  {}  trend={}  members={}  avg_gap={:+}  avg_edge={:+}",
                cluster.title,
                cluster.trend,
                cluster.member_count,
                cluster.average_gap.round_dp(3),
                cluster.average_edge.round_dp(3),
            );
            println!(
                "                lead={}  strongest={}  weakest={}",
                cluster.lead_statement, cluster.strongest_title, cluster.weakest_title,
            );
        }
    }
    if !world_snapshots.world_state.entities.is_empty() {
        println!("\n── World State ──");
        for entity in world_snapshots.world_state.entities.iter().take(6) {
            println!(
                "  {}  layer={}  conf={:+}  local={:+}  propagated={:+}  regime={}",
                entity.label,
                entity.layer,
                entity.confidence.round_dp(3),
                entity.local_support.round_dp(3),
                entity.propagated_support.round_dp(3),
                entity.regime,
            );
            if let Some(driver) = entity.drivers.first() {
                println!("                driver={}", driver);
            }
            println!(
                "                provenance={:?}  trace={}  inputs={}",
                entity.provenance.source,
                entity.provenance.trace_id.as_deref().unwrap_or("-"),
                entity
                    .provenance
                    .inputs
                    .iter()
                    .take(3)
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
    }
    println!(
        "\n── Market Gate ──\n  bias={}  conf={}  breadth_up={:.0}%  breadth_down={:.0}%  avg_return={:+.2}%  consensus={:+.2}",
        decision.market_regime.bias,
        (decision.market_regime.confidence * pct).round_dp(0),
        (decision.market_regime.breadth_up * pct).round_dp(0),
        (decision.market_regime.breadth_down * pct).round_dp(0),
        (decision.market_regime.average_return * pct).round_dp(2),
        decision.market_regime.directional_consensus.round_dp(2),
    );
    if let Some(leader_return) = decision.market_regime.leader_return {
        println!(
            "                leader_return={:+.2}%",
            (leader_return * pct).round_dp(2),
        );
    }
    if let Some(best) = actionable_order_suggestions
        .iter()
        .max_by(|a, b| a.convergence_score.cmp(&b.convergence_score))
    {
        let direction = match best.direction {
            OrderDirection::Buy => "long",
            OrderDirection::Sell => "short",
        };
        println!(
            "                best_convergence={}  {}={:.0}%",
            best.symbol,
            direction,
            (best.convergence_score * pct).round_dp(0),
        );
    }
    let mut top_cases = reasoning_snapshot
        .tactical_setups
        .iter()
        .collect::<Vec<_>>();
    top_cases.sort_by(|a, b| {
        setup_action_priority(&a.action)
            .cmp(&setup_action_priority(&b.action))
            .then_with(|| b.confidence_gap.cmp(&a.confidence_gap))
            .then_with(|| b.heuristic_edge.cmp(&a.heuristic_edge))
            .then_with(|| b.confidence.cmp(&a.confidence))
    });
    if !top_cases.is_empty() {
        println!("\n── Top Tactical Cases ──");
        for setup in top_cases.iter().take(5) {
            let primary = hypothesis_map.get(setup.hypothesis_id.as_str()).copied();
            let runner_up = setup
                .runner_up_hypothesis_id
                .as_ref()
                .and_then(|hypothesis_id| hypothesis_map.get(hypothesis_id.as_str()).copied())
                .map(|hypothesis| hypothesis.statement.as_str())
                .unwrap_or("none");
            let track = track_map.get(setup.setup_id.as_str()).copied();
            let status = track.map(|track| track.status.as_str()).unwrap_or("new");
            let conf_delta = track
                .map(|track| track.confidence_change.round_dp(3))
                .unwrap_or(Decimal::ZERO);
            println!(
                "  {}  action={}  status={}  d_conf={:+}  gap={:+}  edge={:+}  family={}  winner={}  runner_up={}",
                setup.title,
                setup.action,
                status,
                conf_delta,
                setup.confidence_gap.round_dp(3),
                setup.heuristic_edge.round_dp(3),
                primary
                    .map(|hypothesis| hypothesis.family_label.as_str())
                    .unwrap_or("unknown"),
                primary
                    .map(|hypothesis| hypothesis.statement.as_str())
                    .unwrap_or("unknown"),
                runner_up,
            );
            if let Some(hypothesis) = primary {
                println!(
                    "                evidence local={:+}/{:+}  propagated={:+}/{:+}",
                    hypothesis.local_support_weight.round_dp(3),
                    hypothesis.local_contradict_weight.round_dp(3),
                    hypothesis.propagated_support_weight.round_dp(3),
                    hypothesis.propagated_contradict_weight.round_dp(3),
                );
                if let Some(invalidation) = hypothesis.invalidation_conditions.first() {
                    println!(
                        "                invalidates_on={}",
                        invalidation.description
                    );
                }
                println!(
                    "                provenance={:?}  trace={}  inputs={}",
                    hypothesis.provenance.source,
                    hypothesis.provenance.trace_id.as_deref().unwrap_or("-"),
                    hypothesis
                        .provenance
                        .inputs
                        .iter()
                        .take(3)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                );
            }
            if let Some(track) = track {
                println!("                why={}", track.policy_reason);
                if let Some(transition_reason) = &track.transition_reason {
                    println!("                transition={}", transition_reason);
                }
            }
            if !setup.lineage.based_on.is_empty()
                || !setup.lineage.blocked_by.is_empty()
                || !setup.lineage.promoted_by.is_empty()
                || !setup.lineage.falsified_by.is_empty()
            {
                println!(
                    "                lineage based_on=[{}] blocked_by=[{}] promoted_by=[{}] falsified_by=[{}]",
                    setup.lineage.based_on.join(", "),
                    setup.lineage.blocked_by.join(", "),
                    setup.lineage.promoted_by.join(", "),
                    setup.lineage.falsified_by.join(", "),
                );
            }
        }
    }
    let invalidated_cases = reasoning_snapshot
        .hypothesis_tracks
        .iter()
        .filter(|track| track.status.as_str() == "invalidated")
        .collect::<Vec<_>>();
    if !invalidated_cases.is_empty() {
        println!("\n── Recently Invalidated Cases ──");
        for track in invalidated_cases.iter().take(5) {
            println!(
                "  {}  action={}  last_conf={:+}  last_gap={:+}",
                track.title,
                track.action,
                track.confidence.round_dp(3),
                track.confidence_gap.round_dp(3),
            );
        }
    }
    if !lineage_stats.based_on.is_empty()
        || !lineage_stats.blocked_by.is_empty()
        || !lineage_stats.promoted_by.is_empty()
        || !lineage_stats.falsified_by.is_empty()
    {
        println!("\n── Lineage Stats ──");
        if let Some((label, count)) = lineage_stats.based_on.first() {
            println!("  top_based_on     {}  x{}", label, count);
        }
        if let Some((label, count)) = lineage_stats.blocked_by.first() {
            println!("  top_blocked_by   {}  x{}", label, count);
        }
        if let Some((label, count)) = lineage_stats.promoted_by.first() {
            println!("  top_promoted_by  {}  x{}", label, count);
        }
        if let Some((label, count)) = lineage_stats.falsified_by.first() {
            println!("  top_falsified_by {}  x{}", label, count);
        }
    }
    if !world_snapshots.backward_reasoning.investigations.is_empty() {
        println!("\n── Backward Reasoning ──");
        for investigation in world_snapshots
            .backward_reasoning
            .investigations
            .iter()
            .take(5)
        {
            println!(
                "  {}  regime={}  contest={}  streak={}  prev_lead={}",
                investigation.leaf_label,
                investigation.leaf_regime,
                investigation.contest_state,
                investigation.leading_cause_streak,
                investigation
                    .previous_leading_cause_id
                    .as_deref()
                    .unwrap_or("none"),
            );
        }
    }
    if !causal_timelines.is_empty() {
        println!("\n── Causal Memory ──");
        let mut timelines = causal_timelines.values().collect::<Vec<_>>();
        timelines.sort_by(|a, b| {
            let a_flips = a.flip_events.len();
            let b_flips = b.flip_events.len();
            b_flips
                .cmp(&a_flips)
                .then_with(|| a.leaf_label.cmp(&b.leaf_label))
        });
        for timeline in timelines.iter().take(5) {
            let sequence = timeline.recent_leader_sequence(4);
            println!(
                "  {}  scope={}  flips={}  latest_style={}",
                timeline.leaf_label,
                timeline.leaf_scope_key,
                timeline.flip_events.len(),
                timeline
                    .latest_flip_style()
                    .map(|style| style.to_string())
                    .unwrap_or_else(|| "none".into()),
            );
            if !sequence.is_empty() {
                println!("                leaders={}", sequence.join(" -> "));
            }
        }
    }
}
