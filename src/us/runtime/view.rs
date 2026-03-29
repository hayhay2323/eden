use super::*;

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
    lineage_stats: &UsLineageStats,
    position_tracker: &UsPositionTracker,
    workflows: &[UsActionWorkflow],
    propagation_senses: &[crate::us::graph::insights::UsPropagationSense],
    sorted_events: &[crate::ontology::domain::Event<crate::us::pipeline::signals::UsEventRecord>],
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

    LiveSnapshot {
        tick,
        timestamp: timestamp_str,
        market: LiveMarket::Us,
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
        },
        tactical_cases: reasoning
            .tactical_setups
            .iter()
            .take(10)
            .map(|item| {
                let family_label = reasoning
                    .hypotheses
                    .iter()
                    .find(|hypothesis| hypothesis.hypothesis_id == item.hypothesis_id)
                    .map(|hypothesis| hypothesis.family_label.clone());
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

                LiveTacticalCase {
                    setup_id: item.setup_id.clone(),
                    symbol: match &item.scope {
                        crate::ontology::reasoning::ReasoningScope::Symbol(symbol) => {
                            symbol.0.clone()
                        }
                        _ => String::new(),
                    },
                    title: item.title.clone(),
                    action: item.action.clone(),
                    confidence: item.confidence,
                    confidence_gap: item.confidence_gap,
                    heuristic_edge: item.heuristic_edge,
                    entry_rationale: item.entry_rationale.clone(),
                    family_label,
                    counter_label,
                }
            })
            .collect(),
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
        lineage: if !lineage_stats.is_empty() {
            lineage_stats
                .by_template
                .iter()
                .map(|item| LiveLineageMetric {
                    template: item.template.clone(),
                    total: item.total,
                    resolved: item.resolved,
                    hits: item.hits,
                    hit_rate: item.hit_rate,
                    mean_return: item.mean_return,
                })
                .collect()
        } else {
            Vec::new()
        },
    }
}

pub(super) fn display_us_runtime_summary(
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
    lineage_stats: &UsLineageStats,
    insights: &UsGraphInsights,
    backward: &crate::us::pipeline::world::UsBackwardSnapshot,
    position_tracker: &UsPositionTracker,
    workflows: &[UsActionWorkflow],
) {
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
        for e in sorted_events.iter().take(5) {
            println!(
                "    [{:?}] mag={} {}",
                e.value.kind, e.value.magnitude, e.value.summary
            );
        }
    }

    if !reasoning.tactical_setups.is_empty() {
        println!("  Tactical setups:");
        for setup in reasoning.tactical_setups.iter().take(5) {
            println!(
                "    {} [{}] conf={} gap={} edge={}",
                setup.title,
                setup.action,
                setup.confidence,
                setup.confidence_gap,
                setup.heuristic_edge,
            );
        }
    }

    if !lineage_stats.is_empty() {
        println!("  Lineage:");
        for ls in &lineage_stats.by_template {
            println!(
                "    {} {}/{} resolved, hit_rate={} mean_ret={}",
                ls.template, ls.resolved, ls.total, ls.hit_rate, ls.mean_return,
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
            println!("    {} [{}]", c.conclusion, c.primary_driver);
        }
    }
    if !position_tracker.active_fingerprints().is_empty() {
        println!(
            "  Positions: {} active, {} workflows",
            position_tracker.active_fingerprints().len(),
            workflows.len()
        );
    }
}
