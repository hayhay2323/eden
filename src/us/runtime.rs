use std::collections::HashMap;
use std::sync::Arc;

use crate::bridges::service::FileSystemBridgeService;
#[cfg(feature = "persistence")]
use crate::cases::build_case_list_with_feedback;
#[cfg(feature = "persistence")]
use crate::core::analyst_service::AnalystService;
use crate::core::analyst_service::DefaultAnalystService;
use crate::core::market::MarketId;
use crate::core::projection::{project_us, UsProjectionInputs};
#[cfg(feature = "persistence")]
use crate::core::runtime::PreparedRuntimeContext;
use crate::core::runtime::prepare_runtime_context_or_exit;
use crate::live_snapshot::{
    LiveBackwardChain, LiveCausalLeader, LiveCrossMarketAnomaly, LiveCrossMarketSignal,
    LiveEvent, LiveHypothesisTrack, LiveLineageMetric, LiveMarket, LiveMarketRegime,
    LivePressure, LivePropagationSense, LiveScorecard, LiveSignal, LiveSnapshot,
    LiveStressSnapshot, LiveStructuralDelta, LiveTacticalCase,
};
use crate::math::clamp_signed_unit_interval;
use crate::ontology::links::{
    CalcIndexObservation, CandlestickObservation, CapitalFlow, MarketStatus, QuoteObservation,
    YuanAmount,
};
use crate::ontology::objects::{SectorId, Stock, Symbol};
use crate::ontology::reasoning::TacticalSetup;
#[cfg(feature = "persistence")]
use crate::ontology::snapshot::RawSnapshot;
use crate::ontology::store::{us_sector_names, us_symbol_sector, ObjectStore};
#[cfg(feature = "persistence")]
use crate::ontology::{merged_knowledge_events, merged_knowledge_links};
#[cfg(feature = "persistence")]
use crate::persistence::agent_graph::{
    build_knowledge_node_records, build_runtime_knowledge_events, build_runtime_knowledge_links,
    reasoning_knowledge_events, reasoning_knowledge_links, KnowledgeEventHistoryRecord,
    KnowledgeEventStateRecord, KnowledgeLinkHistoryRecord, KnowledgeLinkStateRecord,
    MacroEventHistoryRecord, MacroEventStateRecord,
};
#[cfg(feature = "persistence")]
use crate::persistence::store::EdenStore;
#[cfg(feature = "persistence")]
use crate::pipeline::learning_loop::{
    derive_learning_feedback, derive_outcome_learning_context_from_us_rows,
    ReasoningLearningFeedback,
};
use crate::runtime_loop::TickState;
use crate::us::action::tracker::{UsPositionTracker, UsStructuralFingerprint};
use crate::us::action::workflow::{UsActionStage, UsActionWorkflow};
use crate::us::common::SIGNAL_RESOLUTION_LAG;
use crate::us::graph::decision::{UsDecisionSnapshot, UsSignalRecord, UsSignalScorecard};
use crate::us::graph::graph::UsGraph;
use crate::us::graph::insights::{compute_propagation_senses, UsGraphInsights};
use crate::us::pipeline::dimensions::UsDimensionSnapshot;
use crate::us::pipeline::reasoning::{UsReasoningSnapshot, UsStructuralRankMetrics};
use crate::us::pipeline::signals::{
    PreviousFlows, UsDerivedSignalSnapshot, UsEventSnapshot, UsObservationSnapshot,
};
use crate::us::pipeline::world::derive_backward_snapshot;
use crate::us::temporal::analysis::compute_us_dynamics;
use crate::us::temporal::buffer::UsTickHistory;
use crate::us::temporal::causality::compute_causal_timelines;
use crate::us::temporal::lineage::{compute_us_lineage_stats, UsLineageStats};
use crate::us::temporal::record::{UsSymbolSignals, UsTickRecord};
use crate::us::watchlist::US_WATCHLIST;
use chrono::{Datelike, NaiveDate, TimeZone, Utc, Weekday};
use futures::stream::{self, StreamExt};
use longport::quote::{
    CalcIndex, Period, PushEvent, PushEventDetail, QuoteContext, SecurityCalcIndex, SecurityQuote,
    SubFlags, Trade, TradeSessions, TradeStatus,
};
use longport::Config;
use rust_decimal::prelude::Signed;
use rust_decimal::Decimal;
#[cfg(feature = "persistence")]
use tokio::sync::Semaphore;
#[path = "runtime/support.rs"]
mod support;
use support::*;
#[path = "runtime/startup.rs"]
mod startup;
use startup::*;
#[path = "runtime/view.rs"]
mod view;
use view::*;

const US_SIGNAL_RECORD_CAP: usize = 4_000;
const US_SIGNAL_RECORD_RETENTION_TICKS: u64 = 240;
const US_WORKFLOW_CAP: usize = 512;
const TRADE_BUFFER_CAP_PER_SYMBOL: usize = 2_000;
const US_PUSH_BATCH_SIZE: usize = 256;
const US_PUSH_BATCH_CHANNEL_CAP: usize = 1024;
#[cfg(feature = "persistence")]
const US_PERSISTENCE_MAX_IN_FLIGHT: usize = 16;
#[cfg(feature = "persistence")]
const US_LEARNING_FEEDBACK_REFRESH_INTERVAL: u64 = 30;

// ── Runtime entry ──

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    let UsRuntimeBootstrap {
        store,
        mut live,
        mut rest,
        mut tick_history,
        mut signal_records,
        mut previous_setups,
        mut previous_tracks,
        mut previous_flows,
        mut lineage_stats,
        mut prev_insights,
        mut position_tracker,
        mut workflows,
        bridge_service,
        analyst_service,
        mut runtime,
        mut push_rx,
        mut rest_rx,
        mut tick,
        debounce,
        mut bootstrap_pending,
        #[cfg(feature = "persistence")]
        mut cached_us_learning_feedback,
    } = initialize_us_runtime().await?;
    #[cfg(feature = "persistence")]
    const US_LEARNING_FEEDBACK_REFRESH_INTERVAL: u64 = 30;

    loop {
        let Some(tick_advance) = ({
            let mut tick_state = UsTickState {
                live: &mut live,
                rest: &mut rest,
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
        let trades_this_tick = std::mem::take(&mut live.trades);
        let _ = trades_this_tick; // trades available for future use

        let market_open = is_us_regular_market_hours(now);
        if !market_open {
            // Still write snapshot but mark as after-hours, skip reasoning
            if tick % 100 == 0 {
                let (open_hour, open_minute, close_hour, close_minute) = us_market_hours_utc(now);
                println!(
                    "[US tick {}] after-hours (UTC {:02}:{:02}, session {:02}:{:02}-{:02}:{:02}), skipping reasoning",
                    tick,
                    now.hour(),
                    now.minute(),
                    open_hour,
                    open_minute,
                    close_hour,
                    close_minute,
                );
            }
            continue;
        }

        // Build link-level observations
        let quotes = build_quotes(&live.quotes);
        let capital_flows = build_capital_flows(&rest.capital_flows);
        let calc_indexes = build_calc_indexes(&rest.calc_indexes);
        let candlesticks = build_candlesticks(&live.candlesticks);

        // Build US dimensions
        let dim_snapshot = UsDimensionSnapshot::compute(
            &quotes,
            &capital_flows,
            &calc_indexes,
            &candlesticks,
            &store,
            now,
        );

        // Build US graph
        let sector_map: HashMap<Symbol, SectorId> = store
            .stocks
            .iter()
            .filter_map(|(sym, s)| s.sector_id.clone().map(|sid| (sym.clone(), sid)))
            .collect();
        let sector_names: HashMap<SectorId, String> = store
            .sectors
            .iter()
            .map(|(id, s)| (id.clone(), s.name.clone()))
            .collect();
        let graph = UsGraph::compute(&dim_snapshot, &sector_map, &sector_names);
        let prev_record = tick_history.latest();
        let prev_prev_record = tick_history.latest_n(2).into_iter().next();
        let structural_metrics = dim_snapshot
            .dimensions
            .iter()
            .map(|(sym, dims)| {
                let composite = (dims.capital_flow_direction
                    + dims.price_momentum
                    + dims.volume_profile
                    + dims.pre_post_market_anomaly
                    + dims.valuation)
                    / Decimal::from(5);
                let prev_signal = prev_record.and_then(|record| record.signals.get(sym));
                let prev_prev_signal = prev_prev_record.and_then(|record| record.signals.get(sym));
                let composite_delta = prev_signal
                    .map(|signal| composite - signal.composite)
                    .unwrap_or(Decimal::ZERO);
                let composite_acceleration = match (prev_signal, prev_prev_signal) {
                    (Some(prev_signal), Some(prev_prev_signal)) => {
                        let prev_delta = prev_signal.composite - prev_prev_signal.composite;
                        composite_delta - prev_delta
                    }
                    _ => Decimal::ZERO,
                };
                let capital_flow_delta = prev_signal
                    .map(|signal| dims.capital_flow_direction - signal.capital_flow_direction)
                    .unwrap_or(Decimal::ZERO);
                let flow_reversal = prev_signal
                    .map(|signal| {
                        signal.capital_flow_direction.signum() != Decimal::ZERO
                            && dims.capital_flow_direction.signum() != Decimal::ZERO
                            && signal.capital_flow_direction.signum()
                                != dims.capital_flow_direction.signum()
                    })
                    .unwrap_or(false);
                let flow_persistence = if dims.capital_flow_direction == Decimal::ZERO {
                    0
                } else if let Some(prev_signal) = prev_signal {
                    if prev_signal.capital_flow_direction != Decimal::ZERO
                        && prev_signal.capital_flow_direction.signum()
                            == dims.capital_flow_direction.signum()
                    {
                        prev_signal.flow_persistence + 1
                    } else {
                        1
                    }
                } else {
                    1
                };

                (
                    sym.clone(),
                    UsStructuralRankMetrics {
                        composite_delta,
                        composite_acceleration,
                        capital_flow_delta,
                        flow_persistence,
                        flow_reversal,
                    },
                )
            })
            .collect::<HashMap<_, _>>();

        // Cross-market propagation: read HK snapshot if available
        let cross_market_data = bridge_service.load_hk_to_us(now).await;
        let hk_counterpart_moves = cross_market_data.hk_counterpart_moves;
        let cross_market_signals =
            stabilize_cross_market_signals(cross_market_data.signals, &dim_snapshot);

        // ── Reasoning stack ──

        // 1. Observation snapshot
        let obs_snapshot = UsObservationSnapshot::from_raw(
            &quotes,
            &capital_flows,
            &calc_indexes,
            &candlesticks,
            now,
        );

        // 2. Event detection
        let event_snapshot = UsEventSnapshot::detect(
            &quotes,
            &calc_indexes,
            &capital_flows,
            &previous_flows,
            &hk_counterpart_moves,
            now,
        );

        // 3. Derived signals
        let hk_signal_map: HashMap<Symbol, Decimal> = cross_market_signals
            .iter()
            .map(|s| (s.us_symbol.clone(), s.propagation_confidence))
            .collect();
        let derived_snapshot = UsDerivedSignalSnapshot::compute(&dim_snapshot, &hk_signal_map, now);

        // 4. Decision: convergence + regime + suggestions
        let decision = UsDecisionSnapshot::compute(&graph, &cross_market_signals, tick);

        // 5. Reasoning: hypotheses + tactical setups
        let lineage_prior = compute_us_lineage_stats(&tick_history, SIGNAL_RESOLUTION_LAG);
        let reasoning = UsReasoningSnapshot::derive_with_diffusion(
            &event_snapshot,
            &derived_snapshot,
            &previous_setups,
            &previous_tracks,
            Some(decision.market_regime.bias),
            Some(&lineage_prior),
            Some(&structural_metrics),
            &graph,
            &cross_market_signals,
        );

        // 6. Build UsTickRecord
        let mut per_symbol_signals: HashMap<Symbol, UsSymbolSignals> = HashMap::new();
        for (sym, dims) in &dim_snapshot.dimensions {
            let composite = (dims.capital_flow_direction
                + dims.price_momentum
                + dims.volume_profile
                + dims.pre_post_market_anomaly
                + dims.valuation)
                / Decimal::from(5);
            let prev_signal = prev_record.and_then(|record| record.signals.get(sym));
            let prev_pre_post = prev_signal
                .map(|signal| signal.pre_post_market_anomaly)
                .unwrap_or(Decimal::ZERO);
            let mark_price = quotes
                .iter()
                .find(|q| &q.symbol == sym)
                .map(|q| q.last_done);
            let metrics = structural_metrics.get(sym).copied().unwrap_or_default();
            per_symbol_signals.insert(
                sym.clone(),
                UsSymbolSignals {
                    mark_price,
                    composite,
                    composite_delta: metrics.composite_delta,
                    composite_acceleration: metrics.composite_acceleration,
                    capital_flow_direction: dims.capital_flow_direction,
                    capital_flow_delta: metrics.capital_flow_delta,
                    flow_persistence: metrics.flow_persistence,
                    flow_reversal: metrics.flow_reversal,
                    price_momentum: dims.price_momentum,
                    volume_profile: dims.volume_profile,
                    pre_post_market_anomaly: dims.pre_post_market_anomaly,
                    valuation: dims.valuation,
                    pre_market_delta: dims.pre_post_market_anomaly - prev_pre_post,
                },
            );
        }

        let tick_record = UsTickRecord {
            tick_number: tick,
            timestamp: now,
            signals: per_symbol_signals,
            cross_market_signals: cross_market_signals.clone(),
            events: event_snapshot.events.clone(),
            derived_signals: derived_snapshot.signals.clone(),
            hypotheses: reasoning.hypotheses.clone(),
            tactical_setups: reasoning.tactical_setups.clone(),
            market_regime: decision.market_regime.bias,
        };
        #[cfg(feature = "persistence")]
        run_us_persistence_stage(
            &runtime,
            tick,
            now,
            &live,
            &rest,
            &tick_record,
        )
        .await;
        tick_history.push(tick_record);
        let dynamics = compute_us_dynamics(&tick_history);

        // 7. Signal scorecard: record new suggestions, resolve old ones
        for suggestion in &decision.order_suggestions {
            signal_records.push(UsSignalRecord {
                symbol: suggestion.symbol.clone(),
                tick_emitted: tick,
                direction: suggestion.direction,
                composite_at_emission: suggestion.convergence.composite,
                price_at_emission: quotes
                    .iter()
                    .find(|q| q.symbol == suggestion.symbol)
                    .map(|q| q.last_done),
                resolved: false,
                price_at_resolution: None,
                hit: None,
                realized_return: None,
            });
        }
        for record in &mut signal_records {
            let current_price = quotes
                .iter()
                .find(|q| q.symbol == record.symbol)
                .map(|q| q.last_done);
            UsSignalScorecard::try_resolve(record, tick, current_price);
        }
        prune_us_signal_records(&mut signal_records, tick);
        let scorecard = UsSignalScorecard::compute(&signal_records);

        // Update state for next tick
        previous_setups = reasoning.tactical_setups.clone();
        previous_tracks = reasoning.hypothesis_tracks.clone();
        previous_flows = capital_flows
            .iter()
            .map(|cf| (cf.symbol.clone(), cf.net_inflow.as_yuan()))
            .collect();

        // 8. Lineage stats every 30 ticks
        if tick % 30 == 0 && tick_history.len() > 1 {
            lineage_stats = compute_us_lineage_stats(&tick_history, SIGNAL_RESOLUTION_LAG);
            #[cfg(feature = "persistence")]
            maybe_persist_us_lineage_stage(
                &runtime,
                tick,
                now,
                tick_history.len(),
                &lineage_stats,
            )
            .await;
        }

        // 9. Graph insights (pressure, rotation, clusters, stress, cross-market anomalies)
        let insights = UsGraphInsights::compute(
            &graph,
            &dim_snapshot,
            &cross_market_signals,
            prev_insights.as_ref(),
            tick,
        );
        let propagation_senses = compute_propagation_senses(&graph, &dim_snapshot, &dynamics);

        // 10. Backward reasoning chains
        let sector_name_strings: HashMap<String, String> = sector_names
            .iter()
            .map(|(id, name)| (id.0.clone(), name.clone()))
            .collect();
        let backward = derive_backward_snapshot(
            &decision,
            &graph,
            &cross_market_signals,
            &reasoning.investigation_selections,
            &sector_name_strings,
        );

        // 11. Causal timelines (every 10 ticks to avoid overhead)
        let causal_timelines = if tick % 10 == 0 && tick_history.len() > 2 {
            compute_causal_timelines(&tick_history)
        } else {
            HashMap::new()
        };

        // 12. Position tracker: auto-enter high-confidence setups, monitor exits
        for setup in &reasoning.tactical_setups {
            if setup.action == "enter" && setup.confidence >= Decimal::new(7, 1) {
                if let crate::ontology::reasoning::ReasoningScope::Symbol(sym) = &setup.scope {
                    if !position_tracker.is_active(sym) {
                        let price = quotes
                            .iter()
                            .find(|q| &q.symbol == sym)
                            .map(|q| q.last_done);
                        if let Some(dims) = dim_snapshot.dimensions.get(sym) {
                            let fp =
                                UsStructuralFingerprint::capture(sym.clone(), tick, price, dims);
                            position_tracker.enter(fp);
                            let mut wf = UsActionWorkflow::from_setup(setup, tick, price);
                            // Auto-system: immediately confirm + execute → Monitoring
                            if let Err(error) = wf.confirm(tick) {
                                eprintln!(
                                    "Warning: failed to confirm workflow {} for {}: {}",
                                    wf.workflow_id, wf.symbol, error
                                );
                            }
                            if let Some(p) = price {
                                if let Err(error) = wf.execute(p, tick) {
                                    eprintln!(
                                        "Warning: failed to execute workflow {} for {}: {}",
                                        wf.workflow_id, wf.symbol, error
                                    );
                                }
                            }
                            workflows.push(wf);
                        }
                    }
                }
            }
        }
        let exit_candidates = position_tracker.auto_exit_candidates(&dim_snapshot);
        for deg in &exit_candidates {
            if deg.should_exit {
                position_tracker.exit(&deg.symbol);
                if let Some(wf) = workflows.iter_mut().find(|w| {
                    w.symbol == deg.symbol && matches!(w.stage, UsActionStage::Monitoring)
                }) {
                    if let Err(error) = wf.review("auto-exit: structural degradation", tick) {
                        eprintln!(
                            "Warning: failed to review workflow {} for {}: {}",
                            wf.workflow_id, wf.symbol, error
                        );
                    }
                }
            }
        }
        // Update monitoring for active workflows
        for wf in &mut workflows {
            if matches!(wf.stage, UsActionStage::Monitoring) {
                let price = quotes
                    .iter()
                    .find(|q| q.symbol == wf.symbol)
                    .map(|q| q.last_done);
                if let Some(deg) = exit_candidates.iter().find(|d| d.symbol == wf.symbol) {
                    if let Err(error) = wf.update_monitoring(price, deg.clone()) {
                        eprintln!(
                            "Warning: failed to update workflow {} for {}: {}",
                            wf.workflow_id, wf.symbol, error
                        );
                    }
                }
            }
        }
        // Prune stale workflows
        workflows.retain(|w| !w.is_stale(tick));
        prune_us_workflows(&mut workflows);

        prev_insights = Some(insights.clone());

        // ── Build live snapshot JSON ──
        let timestamp_str = now
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();

        let mut sorted_events = event_snapshot.events.clone();
        sorted_events.sort_by(|a, b| b.value.magnitude.cmp(&a.value.magnitude));

        let live_snapshot = build_us_live_snapshot(
            tick,
            timestamp_str.clone(),
            &store,
            &graph,
            &dim_snapshot,
            &reasoning,
            &obs_snapshot,
            &decision,
            &insights,
            &scorecard,
            &backward,
            &causal_timelines,
            &cross_market_signals,
            &dynamics,
            &tick_history,
            &lineage_stats,
            &position_tracker,
            &workflows,
            &propagation_senses,
            &sorted_events,
        );
        let mut sorted_convergence: Vec<_> = decision.convergence_scores.iter().collect();
        sorted_convergence.sort_by(|a, b| b.1.composite.abs().cmp(&a.1.composite.abs()));

        display_us_runtime_summary(
            tick,
            &timestamp_str,
            &graph,
            &decision,
            &event_snapshot,
            &reasoning,
            &scorecard,
            live.push_count,
            &sorted_convergence,
            &cross_market_signals,
            &sorted_events,
            &lineage_stats,
            &insights,
            &backward,
            &position_tracker,
            &workflows,
        );

        // Build projection bundle
        let artifact_projection = project_us(UsProjectionInputs {
            live_snapshot,
            history: &tick_history,
            reasoning: &reasoning,
            backward: &backward,
            store: &store,
            lineage_stats: &lineage_stats,
            previous_agent_snapshot: runtime.projection_state.previous_agent_snapshot.as_ref(),
            previous_agent_session: runtime.projection_state.previous_agent_session.as_ref(),
            previous_agent_scoreboard: runtime.projection_state.previous_agent_scoreboard.as_ref(),
        });
        #[cfg(feature = "persistence")]
        run_us_projection_stage(
            &mut runtime,
            &analyst_service,
            tick,
            now,
            tick_started_at,
            tick_advance.received_push,
            tick_advance.received_update,
            live.push_count,
            &mut cached_us_learning_feedback,
            &reasoning,
            &artifact_projection,
            &store,
        )
        .await;
        #[cfg(not(feature = "persistence"))]
        runtime.publish_projection(
            MarketId::Us,
            crate::cases::CaseMarket::Us,
            &artifact_projection,
            Vec::new(),
            &analyst_service,
            tick,
            live.push_count,
            tick_started_at,
            tick_advance.received_push,
            tick_advance.received_update,
        );
    }

    Ok(())
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
