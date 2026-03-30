use super::watchlist::WATCHLIST;
use crate as eden;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use longport::quote::{
    CalcIndex, MarketTemperature, Period, PushEvent, PushEventDetail, QuoteContext,
    SecurityBrokers, SecurityCalcIndex, SecurityDepth, SecurityQuote, SubFlags, Trade,
    TradeSessions,
};
use longport::{Config, Market};
use rust_decimal::prelude::Signed;
use rust_decimal::Decimal;
#[cfg(feature = "persistence")]
use tokio::sync::Semaphore;

use crate::action::narrative::NarrativeSnapshot;
use crate::action::workflow::{ActionDescriptor, ActionWorkflowSnapshot, SuggestedAction};
use crate::bridges::hk_to_us::{HkSignalEntry, HkSnapshot};
use crate::bridges::pairs::CROSS_MARKET_PAIRS;
use crate::bridges::service::FileSystemBridgeService;
#[cfg(feature = "persistence")]
use crate::cases::build_case_list_with_feedback;
#[cfg(feature = "persistence")]
use crate::core::analyst_service::AnalystService;
use crate::core::analyst_service::DefaultAnalystService;
use crate::core::market::{ArtifactKind, MarketId};
use crate::core::artifact_repository::resolve_artifact_path;
use crate::core::projection::{project_hk, HkProjectionInputs};
use crate::core::runtime::{prepare_runtime_artifact_path, prepare_runtime_context_or_exit};
#[cfg(feature = "persistence")]
use crate::core::runtime::PreparedRuntimeContext;
use crate::external::polymarket::{
    fetch_polymarket_snapshot, load_polymarket_configs, PolymarketMarketConfig, PolymarketSnapshot,
};
use crate::graph::decision::{DecisionSnapshot, OrderDirection, StructuralFingerprint};
use crate::graph::graph::BrainGraph;
use crate::graph::insights::{ConflictHistory, GraphInsights};
use crate::graph::temporal::{TemporalBrokerRegistry, TemporalEdgeRegistry, TemporalNodeRegistry};
use crate::graph::tracker::PositionTracker;
use crate::graph::validation::{SignalScorecard, SignalType};
use crate::live_snapshot::{
    json_payload, LiveBackwardChain, LiveCausalLeader, LiveEvent, LiveEvidence,
    LiveHypothesisTrack, LiveLineageMetric, LiveMarket, LiveMarketRegime, LivePressure,
    LiveScorecard, LiveSignal, LiveSnapshot, LiveStressSnapshot, LiveTacticalCase,
};
use crate::logic::tension::TensionSnapshot;
use crate::ontology::links::LinkSnapshot;
use crate::ontology::objects::{BrokerId, Symbol};
use crate::ontology::reasoning::HypothesisTrack;
use crate::ontology::snapshot::{self, RawSnapshot};
use crate::ontology::store;
use crate::ontology::TacticalSetup;
#[cfg(feature = "persistence")]
use crate::ontology::{merged_knowledge_events, merged_knowledge_links};
use crate::persistence::action_workflow::{ActionWorkflowEventRecord, ActionWorkflowRecord};
#[cfg(feature = "persistence")]
use crate::persistence::agent_graph::{
    build_knowledge_node_records, build_runtime_knowledge_events, build_runtime_knowledge_links,
    reasoning_knowledge_events, reasoning_knowledge_links, KnowledgeEventHistoryRecord,
    KnowledgeEventStateRecord, KnowledgeLinkHistoryRecord, KnowledgeLinkStateRecord,
    MacroEventHistoryRecord, MacroEventStateRecord,
};
#[cfg(feature = "persistence")]
use crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord;
use crate::pipeline::dimensions::DimensionSnapshot;
#[cfg(feature = "persistence")]
use crate::pipeline::learning_loop::{
    derive_learning_feedback, derive_outcome_learning_context_from_hk_rows,
    ReasoningLearningFeedback,
};
use crate::pipeline::reasoning::{path_has_family, path_is_mixed_multi_hop, ReasoningSnapshot};
use crate::pipeline::signals::{
    DerivedSignalSnapshot, EventSnapshot, MarketEventKind, ObservationSnapshot, SignalScope,
};
use crate::pipeline::world::WorldSnapshots;
use crate::runtime_loop::TickState;
use crate::temporal::analysis::{compute_dynamics, compute_polymarket_dynamics};
use crate::temporal::buffer::TickHistory;
use crate::temporal::causality::{compute_causal_timelines, CausalTimeline};
#[cfg(feature = "persistence")]
use crate::temporal::causality::{CausalFlipEvent, CausalTimelinePoint};
#[cfg(feature = "persistence")]
use crate::temporal::lineage::compute_case_realized_outcomes;
#[cfg(feature = "persistence")]
use crate::temporal::lineage::compute_case_realized_outcomes_adaptive;
use crate::temporal::lineage::{compute_family_context_outcomes, compute_lineage_stats};
use crate::temporal::record::TickRecord;

#[cfg(feature = "persistence")]
use crate::persistence::hypothesis_track::HypothesisTrackRecord;
#[cfg(feature = "persistence")]
use crate::persistence::lineage_metric_row::{row_matches_filters, snapshot_records_from_rows};
#[cfg(feature = "persistence")]
use crate::persistence::store::EdenStore;
#[cfg(feature = "persistence")]
use crate::persistence::tactical_setup::TacticalSetupRecord;
#[path = "runtime/support.rs"]
mod support;
use support::*;
#[path = "runtime/display.rs"]
mod display;
use display::*;
#[path = "runtime/persistence.rs"]
mod persistence;
#[cfg(feature = "persistence")]
use persistence::{run_hk_persistence_stage, run_hk_projection_stage};
#[cfg(feature = "persistence")]
use persistence::{CASE_OUTCOME_RESOLUTION_LAG, PERSISTENCE_MAX_IN_FLIGHT};
#[path = "runtime/state.rs"]
mod state;
use state::*;
#[path = "runtime/snapshot.rs"]
mod snapshot_support;
use snapshot_support::*;
#[path = "runtime/actions.rs"]
mod actions;
use actions::*;
#[path = "runtime/startup.rs"]
mod startup;
use startup::*;


pub async fn merge_external_priors(
    base: &PolymarketSnapshot,
    now: time::OffsetDateTime,
    bridge_service: &FileSystemBridgeService,
) -> PolymarketSnapshot {
    let mut merged = base.clone();
    let bridge_snapshot = crate::bridges::us_to_hk::to_polymarket_snapshot(
        now,
        &bridge_service.load_us_to_hk(now).await.signals,
    );
    if merged.fetched_at < bridge_snapshot.fetched_at {
        merged.fetched_at = bridge_snapshot.fetched_at;
    }
    merged.priors.extend(bridge_snapshot.priors);
    merged
}

pub async fn run() {
    let HkRuntimeBootstrap {
        store,
        mut live,
        mut rest,
        mut tracker,
        mut history,
        mut prev_insights,
        mut conflict_history,
        mut edge_registry,
        mut node_registry,
        mut broker_registry,
        mut scorecard,
        bridge_service,
        analyst_service,
        bridge_snapshot_path,
        mut runtime,
        mut push_rx,
        mut rest_rx,
        mut tick,
        debounce,
        mut bootstrap_pending,
    } = initialize_hk_runtime().await;
    let pct = Decimal::new(100, 0);

    loop {
        let mut rest_updated = false;
        let Some(tick_advance) = ({
            let mut tick_state = HkTickState {
                live: &mut live,
                rest: &mut rest,
                rest_updated: &mut rest_updated,
            };
            match runtime.begin_tick(
                &mut bootstrap_pending,
                &mut push_rx,
                &mut rest_rx,
                debounce,
                &mut tick_state,
                &mut tick,
            )
            .await
            {
                Some(result) => Some(result),
                None => {
                    break;
                }
            }
        }) else {
            continue;
        };
        let tick_started_at = tick_advance.started_at;
        let tick_advance = tick_advance.advance;
        let now = tick_advance.now;
        let previous_polymarket = history
            .latest()
            .map(|tick| tick.polymarket_priors.clone())
            .unwrap_or_default();

        if tick_advance.received_update {
            for idx in rest.calc_indexes.values() {
                if let (Some(vr), Some(tr)) = (idx.volume_ratio, idx.turnover_rate) {
                    if vr > Decimal::TWO {
                        println!(
                            "  [VOLUME] {}  volume_ratio={:.1}  turnover_rate={:.2}%  5min_chg={:+.2}%",
                            idx.symbol,
                            vr,
                            tr * pct,
                            idx.five_minutes_change_rate.unwrap_or(Decimal::ZERO) * pct,
                        );
                    }
                }
            }

            if let Some(temp) = &rest.market_temperature {
                println!(
                    "  [MARKET] HK temperature={} valuation={} sentiment={} ({})",
                    temp.temperature, temp.valuation, temp.sentiment, temp.description,
                );
            }

            if !rest.polymarket.priors.is_empty() {
                for prior in rest.polymarket.priors.iter().take(3) {
                    let delta = previous_polymarket
                        .iter()
                        .find(|previous| previous.slug == prior.slug)
                        .map(|previous| prior.probability - previous.probability)
                        .unwrap_or(Decimal::ZERO);
                    println!(
                        "  [POLY] {}  outcome={}  prob={:.0}%  d_prob={:+.0}%  bias={}",
                        prior.label,
                        prior.selected_outcome,
                        (prior.probability * pct).round_dp(0),
                        (delta * pct).round_dp(0),
                        prior.bias.as_str(),
                    );
                }
            }
        }

        println!("══════════════════════════════════════════════════════════");
        println!(
            "  #{:<4}  {}  │  {} total pushes",
            tick,
            now.format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| now.to_string()),
            live.push_count,
        );
        println!("══════════════════════════════════════════════════════════");

        // ── Build snapshot and run full pipeline ──
        let raw = live.to_raw_snapshot(&rest);

        // Show trade activity if any
        let trade_symbols: Vec<_> = raw
            .trades
            .iter()
            .filter(|(_, t)| !t.is_empty())
            .map(|(s, t)| (s.clone(), t.len(), t.iter().map(|t| t.volume).sum::<i64>()))
            .collect();

        let links = LinkSnapshot::compute(&raw, &store);
        let readiness = compute_readiness(&links);
        let dim_snapshot = DimensionSnapshot::compute(&links, &store);
        let tension_snapshot = TensionSnapshot::compute(&dim_snapshot);
        let narrative_snapshot = NarrativeSnapshot::compute(&tension_snapshot, &dim_snapshot);
        let brain = BrainGraph::compute(&narrative_snapshot, &dim_snapshot, &links, &store);

        let graph_temporal_delta = edge_registry.update(&brain, tick);
        let graph_node_delta = node_registry.update(&brain, tick);
        let broker_delta = broker_registry.update(&links.broker_queues, &links.order_books, &store, tick);
        let graph_insights = GraphInsights::compute(
            &brain,
            &store,
            prev_insights.as_ref(),
            &mut conflict_history,
            tick,
        );
        let external_priors = merge_external_priors(&rest.polymarket, now, &bridge_service).await;

        let active_fps = tracker.active_fingerprints();

        // Build temporal convergence context from edge registry + recent history
        // Build rolling stats from recent tick history
        let rolling_composites = {
            let mut map = std::collections::HashMap::new();
            for symbol in brain.stock_nodes.keys() {
                let series = history.signal_series(symbol, |s| s.composite);
                if series.len() >= 5 {
                    let n = series.len() as f64;
                    let mean_f: f64 = series
                        .iter()
                        .map(|d| {
                            let f: f64 = (*d).try_into().unwrap_or(0.0);
                            f
                        })
                        .sum::<f64>()
                        / n;
                    let variance: f64 = series
                        .iter()
                        .map(|d| {
                            let f: f64 = (*d).try_into().unwrap_or(0.0);
                            (f - mean_f).powi(2)
                        })
                        .sum::<f64>()
                        / n;
                    let stddev_f = variance.sqrt();
                    // Simple trend: last value - first value / count
                    let first: f64 = series
                        .first()
                        .map(|d| (*d).try_into().unwrap_or(0.0))
                        .unwrap_or(0.0);
                    let last: f64 = series
                        .last()
                        .map(|d| (*d).try_into().unwrap_or(0.0))
                        .unwrap_or(0.0);
                    let trend_f = (last - first) / n;
                    map.insert(
                        symbol.clone(),
                        crate::graph::convergence::RollingStats {
                            mean: rust_decimal::Decimal::try_from(mean_f).unwrap_or_default(),
                            stddev: rust_decimal::Decimal::try_from(stddev_f).unwrap_or_default(),
                            trend: rust_decimal::Decimal::try_from(trend_f).unwrap_or_default(),
                            sample_count: series.len(),
                        },
                    );
                }
            }
            map
        };
        let temporal_ctx = crate::graph::decision::TemporalConvergenceContext {
            edge_registry: &edge_registry,
            tick,
            microstructure_deltas: history
                .latest()
                .and_then(|r| r.microstructure_deltas.as_ref()),
            rolling_composites,
        };
        let mut decision =
            DecisionSnapshot::compute(&brain, &links, &active_fps, &store, Some(&temporal_ctx));

        display_hk_temporal_debug(tick, &decision, &graph_node_delta, &broker_delta);

        if !external_priors.is_empty() {
            decision.apply_polymarket_snapshot(&external_priors, &store);
        }
        let ready_convergence_scores =
            filter_convergence_scores(&decision.convergence_scores, &readiness.ready_symbols);
        let ready_order_suggestions =
            filter_order_suggestions(&decision.order_suggestions, &readiness.ready_symbols);
        let aged_degradations = filter_degradations(
            &decision.degradations,
            &active_fps,
            now,
            &readiness.ready_symbols,
        );
        let observation_snapshot = ObservationSnapshot::from_links(&links);
        let mut event_snapshot = EventSnapshot::detect(
            &history,
            tick,
            &links,
            &dim_snapshot,
            &graph_insights,
            &decision,
        );
        // Inject broker-level perception events
        let broker_events =
            crate::pipeline::signals::broker_events_from_delta(&broker_delta, links.timestamp);
        event_snapshot.events.extend(broker_events);
        let derived_signal_snapshot = DerivedSignalSnapshot::compute(
            &dim_snapshot,
            &graph_insights,
            &decision,
            &event_snapshot,
        );
        let previous_setups = history
            .latest()
            .map(|tick| tick.tactical_setups.as_slice())
            .unwrap_or(&[]);
        let previous_tracks = history
            .latest()
            .map(|tick| tick.hypothesis_tracks.as_slice())
            .unwrap_or(&[]);
        let lineage_family_priors = compute_family_context_outcomes(&history, LINEAGE_WINDOW);
        let reasoning_stock_deltas =
            compute_reasoning_stock_deltas(&decision.convergence_scores, history.latest());
        let reasoning_snapshot = ReasoningSnapshot::derive_with_diffusion(
            &event_snapshot,
            &derived_signal_snapshot,
            &graph_insights,
            &decision,
            previous_setups,
            previous_tracks,
            &lineage_family_priors,
            &brain,
            &reasoning_stock_deltas,
        );
        let world_snapshots = WorldSnapshots::derive(
            &event_snapshot,
            &derived_signal_snapshot,
            &graph_insights,
            &decision,
            &reasoning_snapshot,
            (!external_priors.is_empty()).then_some(&external_priors),
            history.latest().map(|tick| &tick.backward_reasoning),
        );
        let action_stage = build_hk_action_stage(
            now,
            &brain,
            &mut tracker,
            &readiness,
            &decision,
            &ready_convergence_scores,
            &ready_order_suggestions,
            &aged_degradations,
            &event_snapshot,
            &reasoning_snapshot,
        );
        let new_set: HashSet<&Symbol> = action_stage.newly_entered.iter().collect();
        #[cfg(not(feature = "persistence"))]
        let _ = (&action_stage.workflow_records, &action_stage.workflow_events);

        // Refresh fingerprints every 30 ticks to prevent stale degradation baselines
        if tick % 30 == 0 && tracker.active_count() > 0 {
            tracker.refresh_all(&brain);
        }

        // ── Capture tick record into history ──
        let tick_record = TickRecord::capture(
            tick,
            now,
            &decision.convergence_scores,
            &dim_snapshot.dimensions,
            &links.order_books,
            &links.quotes,
            &links.trade_activities,
            &aged_degradations,
            &observation_snapshot,
            &event_snapshot,
            &derived_signal_snapshot,
            &action_stage.workflow_snapshots,
            &external_priors.priors,
            &reasoning_snapshot,
            &world_snapshots.world_state,
            &world_snapshots.backward_reasoning,
            &graph_temporal_delta.transitions,
            &graph_node_delta.transitions,
        );
        history.push(tick_record);
        store.knowledge.write().unwrap().accumulate_institutional_memory(tick, &brain);

        // ── Persist to SurrealDB (non-blocking, fire-and-forget) ──
        #[cfg(feature = "persistence")]
        run_hk_persistence_stage(
            &runtime,
            tick,
            now,
            &raw,
            &links,
            history.latest().expect("tick history contains latest record after push"),
            &action_stage.workflow_records,
            &action_stage.workflow_events,
            &reasoning_snapshot,
        )
        .await;

        // ── Compute temporal dynamics ──
        let dynamics = compute_dynamics(&history);
        let polymarket_dynamics = compute_polymarket_dynamics(&history);
        let causal_timelines = compute_causal_timelines(&history);
        let lineage_stats = compute_lineage_stats(&history, LINEAGE_WINDOW);

        if let Some(latest) = history.latest() {
            let captured_at = now
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default();
            let live_snapshot = build_hk_live_snapshot(
                tick,
                captured_at.clone(),
                &store,
                &brain,
                &decision,
                &graph_insights,
                &reasoning_snapshot,
                &event_snapshot,
                &observation_snapshot,
                &scorecard,
                &dim_snapshot,
                latest,
                &tracker,
                &causal_timelines,
                &lineage_stats,
                &dynamics,
            );
            let hk_bridge_snapshot = build_hk_bridge_snapshot(
                captured_at,
                &decision.convergence_scores,
                &dim_snapshot,
                &links,
            );
            let artifact_projection = project_hk(HkProjectionInputs {
                live_snapshot,
                history: &history,
                links: &links,
                store: &store,
                lineage_priors: &lineage_family_priors,
                previous_agent_snapshot: runtime.projection_state.previous_agent_snapshot.as_ref(),
                previous_agent_session: runtime.projection_state.previous_agent_session.as_ref(),
                previous_agent_scoreboard: runtime.projection_state.previous_agent_scoreboard.as_ref(),
            });
            #[cfg(feature = "persistence")]
            run_hk_projection_stage(
                &mut runtime,
                &analyst_service,
                tick,
                now,
                tick_started_at,
                tick_advance.received_push,
                tick_advance.received_update,
                live.push_count,
                &history,
                &reasoning_snapshot,
                &world_snapshots,
                &lineage_stats,
                &bridge_snapshot_path,
                &hk_bridge_snapshot,
                &artifact_projection,
            )
            .await;
            #[cfg(not(feature = "persistence"))]
            runtime.publish_projection(
                MarketId::Hk,
                crate::cases::CaseMarket::Hk,
                &artifact_projection,
                vec![(bridge_snapshot_path.clone(), json_payload(&hk_bridge_snapshot))],
                &analyst_service,
                tick,
                live.push_count,
                tick_started_at,
                tick_advance.received_push,
                tick_advance.received_update,
            );
        }

        let bootstrap_mode = readiness.bootstrap_mode(tick);
        if bootstrap_mode {
            display_hk_bootstrap_preview(
                &readiness,
                &action_stage.workflow_snapshots,
                &reasoning_snapshot.propagation_paths,
            );
        } else {
            display_hk_reasoning_console(
                pct,
                &store,
                &graph_insights,
                &observation_snapshot,
                &event_snapshot,
                &derived_signal_snapshot,
                &action_stage.workflow_snapshots,
                &reasoning_snapshot,
                &world_snapshots,
                &decision,
                &ready_convergence_scores,
                &action_stage.actionable_order_suggestions,
                &lineage_stats,
                &causal_timelines,
            );
        }

        display_hk_market_microstructure(
            pct,
            tick,
            bootstrap_mode,
            history.len(),
            &dynamics,
            &polymarket_dynamics,
            &action_stage.actionable_order_suggestions,
            &new_set,
            &mut scorecard,
            &links,
            &readiness,
            &graph_insights,
            &aged_degradations,
            trade_symbols,
            &live,
            &tracker,
            &action_stage.newly_entered,
        );

        prev_insights = Some(graph_insights);
    }
}
