use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use crate::bridges::service::FileSystemBridgeService;
#[cfg(feature = "persistence")]
use crate::cases::build_case_list_with_feedback;
#[cfg(feature = "persistence")]
use crate::core::analyst_service::AnalystService;
use crate::core::analyst_service::DefaultAnalystService;
use crate::core::market::MarketId;
use crate::core::projection::{project_us, UsProjectionInputs};
use crate::core::runtime::prepare_runtime_context_or_exit;
#[cfg(feature = "persistence")]
use crate::core::runtime::PreparedRuntimeContext;
use crate::core::runtime_loop::TickState;
use crate::live_snapshot::{
    LiveBackwardChain, LiveCausalLeader, LiveCrossMarketAnomaly, LiveCrossMarketSignal, LiveEvent,
    LiveHypothesisTrack, LiveLineageMetric, LiveMarket, LiveMarketRegime, LivePressure,
    LivePropagationSense, LiveScorecard, LiveSignal, LiveSnapshot, LiveStressSnapshot,
    LiveStructuralDelta, LiveTacticalCase,
};
use crate::math::clamp_signed_unit_interval;
use crate::ontology::links::{
    CalcIndexObservation, CandlestickObservation, CapitalFlow, IntradayObservation, MarketStatus,
    OptionSurfaceObservation, QuoteObservation, YuanAmount,
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
use crate::pipeline::attention_budget::AttentionBudgetAllocator;
#[cfg(feature = "persistence")]
use crate::pipeline::learning_loop::{
    derive_learning_feedback, derive_outcome_learning_context_from_us_rows,
    ReasoningLearningFeedback,
};
use crate::us::action::tracker::{UsPositionTracker, UsStructuralFingerprint};
use crate::us::action::workflow::{UsActionStage, UsActionWorkflow};
use crate::us::common::SIGNAL_RESOLUTION_LAG;
use crate::us::graph::decision::{
    UsDecisionSnapshot, UsSignalRecord, UsSignalScorecard, UsSignalScorecardAccumulator,
};
use crate::us::graph::graph::UsGraph;
use crate::us::graph::insights::{compute_propagation_senses, UsGraphInsights};
use crate::us::pipeline::dimensions::UsDimensionSnapshot;
use crate::us::pipeline::reasoning::{UsReasoningSnapshot, UsStructuralRankMetrics};
use crate::us::pipeline::signals::{
    PreviousFlows, UsDerivedSignalSnapshot, UsEventKind, UsEventSnapshot, UsObservationSnapshot,
    UsSignalScope,
};
use crate::us::pipeline::world::derive_backward_snapshot;
use crate::us::temporal::analysis::compute_us_dynamics;
use crate::us::temporal::buffer::UsTickHistory;
use crate::us::temporal::causality::compute_causal_timelines;
use crate::us::temporal::lineage::{
    compute_us_convergence_success_patterns, compute_us_lineage_stats,
    evaluate_us_candidate_mechanisms, UsLineageStats,
};
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
use serde_json::json;
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
const US_PUSH_BATCH_SIZE: usize = 2_048;
const US_PUSH_BATCH_CHANNEL_CAP: usize = 8_192;
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
        mut scorecard_accumulator,
        mut signal_momentum,
        mut previous_setups,
        mut previous_tracks,
        mut previous_flows,
        mut lineage_stats,
        mut lineage_accumulator,
        mut lineage_prev_resolved,
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
        mut energy_momentum,
        #[cfg(feature = "persistence")]
        mut cached_us_learning_feedback,
    } = initialize_us_runtime().await?;
    #[cfg(feature = "persistence")]
    const US_LEARNING_FEEDBACK_REFRESH_INTERVAL: u64 = 30;
    let mut attention = AttentionBudgetAllocator::from_universe_size(US_WATCHLIST.len());
    let mut vortex_attention = UsVortexAttention::default();
    let mut cached_us_candidate_mechanisms: Vec<
        crate::persistence::candidate_mechanism::CandidateMechanismRecord,
    > = Vec::new();
    #[cfg(feature = "persistence")]
    if let Some(ref store) = runtime.store {
        if let Ok(mechs) = store.load_candidate_mechanisms("us").await {
            eprintln!(
                "[us] loaded {} candidate mechanisms from store",
                mechs.len()
            );
            cached_us_candidate_mechanisms = mechs;
        }
    }
    #[cfg(feature = "persistence")]
    if let Some(ref store) = runtime.store {
        crate::persistence::case_reasoning_assessment::backfill_doctrine_assessments(store, "us")
            .await;
    }

    #[cfg(feature = "persistence")]
    let _cached_us_causal_schemas: Vec<crate::persistence::causal_schema::CausalSchemaRecord> = {
        let mut schemas_vec = Vec::new();
        if let Some(ref store) = runtime.store {
            if let Ok(schemas) = store.load_causal_schemas("us").await {
                eprintln!("[us] loaded {} causal schemas from store", schemas.len());
                schemas_vec = schemas;
            }
        }
        schemas_vec
    };

    let mut us_hidden_force_state =
        crate::pipeline::residual::HiddenForceVerificationState::default();
    let mut edge_ledger = crate::graph::edge_learning::EdgeLearningLedger::default();
    let mut seen_us_edge_learning_setups = HashSet::new();
    let mut pressure_field = crate::pipeline::pressure::PressureField::new(time::OffsetDateTime::now_utc());
    let mut lifecycle_tracker =
        crate::pipeline::pressure::reasoning::LifecycleTracker::default();
    let mut sector_members: HashMap<SectorId, Vec<Symbol>> = store
        .sectors
        .keys()
        .cloned()
        .map(|sector_id| (sector_id, Vec::new()))
        .collect();
    let mut symbol_sector: HashMap<Symbol, SectorId> = HashMap::new();
    for (symbol, stock) in &store.stocks {
        if let Some(sector_id) = stock.sector_id.clone() {
            sector_members
                .entry(sector_id.clone())
                .or_default()
                .push(symbol.clone());
            symbol_sector.insert(symbol.clone(), sector_id);
        }
    }

    loop {
        let Some(tick_advance) = ({
            let mut tick_state = UsTickState {
                live: &mut live,
                rest: &mut rest,
            };
            match runtime
                .begin_tick(
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
            lifecycle_tracker.decay(tick);
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
            runtime.runtime_task_heartbeat(
                "us runtime waiting for regular market hours",
                json!({
                    "market": "us",
                    "tick": tick,
                    "market_open": false,
                    "quotes": live.quotes.len(),
                    "candlesticks": live.candlesticks.len(),
                    "workflows": workflows.len(),
                }),
            );
            continue;
        }

        // Build link-level observations
        let quotes = build_quotes(&live.quotes);
        let capital_flows = build_capital_flows(&rest.capital_flows);
        let calc_indexes = build_calc_indexes(&rest.calc_indexes);
        let candlesticks = build_candlesticks(&live.candlesticks);
        let intraday = build_intraday(&rest.intraday_lines);

        // Build US dimensions (with VWAP from intraday)
        let dim_snapshot = UsDimensionSnapshot::compute_with_intraday(
            &quotes,
            &capital_flows,
            &calc_indexes,
            &candlesticks,
            &store,
            now,
            &intraday,
        );

        // Build US graph
        let sector_names: HashMap<SectorId, String> = store
            .sectors
            .iter()
            .map(|(id, s)| (id.clone(), s.name.clone()))
            .collect();
        let graph = UsGraph::compute(&dim_snapshot, &symbol_sector, &sector_names);
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
        let mut event_snapshot = UsEventSnapshot::detect(
            &quotes,
            &calc_indexes,
            &capital_flows,
            &previous_flows,
            &hk_counterpart_moves,
            now,
        );
        if let Some(previous_agent_snapshot) =
            runtime.projection_state.previous_agent_snapshot.as_ref()
        {
            let catalyst_events = crate::us::pipeline::signals::catalyst_events_from_macro_events(
                &previous_agent_snapshot.macro_events,
                now,
            );
            event_snapshot.events.extend(catalyst_events);

            crate::us::pipeline::signals::enrich_us_attribution_with_evidence(
                &mut event_snapshot,
                &previous_agent_snapshot.macro_events,
            );
            crate::us::pipeline::signals::detect_us_propagation_absences(&mut event_snapshot, now);
        }

        // 3. Derived signals
        let hk_signal_map: HashMap<Symbol, Decimal> = cross_market_signals
            .iter()
            .map(|s| (s.us_symbol.clone(), s.propagation_confidence))
            .collect();
        let derived_snapshot = UsDerivedSignalSnapshot::compute(&dim_snapshot, &hk_signal_map, now);

        if edge_ledger.is_empty() && !tick_history.is_empty() {
            let credited = crate::graph::edge_learning::ingest_us_topology_outcomes(
                &mut edge_ledger,
                &mut seen_us_edge_learning_setups,
                &tick_history,
                &graph,
                SIGNAL_RESOLUTION_LAG,
                now,
            );
            if credited > 0 {
                eprintln!(
                    "[us] seeded edge ledger from restored history (credited_setups={}, learned_edges={})",
                    credited,
                    edge_ledger.len()
                );
            }
        }

        // 4. Decision: convergence + regime + suggestions
        let decision =
            UsDecisionSnapshot::compute(&graph, &cross_market_signals, tick, Some(&edge_ledger));

        // 5. Reasoning: hypotheses + tactical setups
        let _lineage_prior = compute_us_lineage_stats(&tick_history, SIGNAL_RESOLUTION_LAG);
        let attention_plan = attention_reasoning_plan(
            graph.stock_nodes.keys().cloned(),
            &attention,
            &previous_setups,
            &previous_tracks,
            &vortex_attention,
        );
        let reasoning_active_symbols = attention_plan.active_symbols();
        let reasoning_event_snapshot =
            filter_us_event_snapshot_for_reasoning(&event_snapshot, &reasoning_active_symbols);
        let reasoning_derived_snapshot = filter_us_derived_signal_snapshot_for_reasoning(
            &derived_snapshot,
            &reasoning_active_symbols,
        );
        let reasoning_decision =
            filter_us_decision_for_reasoning(&decision, &reasoning_active_symbols);
        // Pressure field: inject local pressure, propagate along US graph edges, detect vortices.
        pressure_field.tick_us(now, &dim_snapshot.dimensions, &graph);
        for vortex in &pressure_field.vortices {
            lifecycle_tracker.record(&vortex.symbol, tick, vortex.tension);
        }
        lifecycle_tracker.decay(tick);
        if !pressure_field.vortices.is_empty() {
            eprintln!(
                "[us] pressure field: {} vortices (top: {} strength={} ch={} dir={})",
                pressure_field.vortices.len(),
                pressure_field.vortices[0].symbol.0,
                pressure_field.vortices[0].tension,
                pressure_field.vortices[0].tense_channel_count,
                pressure_field.vortices[0].temporal_divergence,
            );
        }
        for vortex in pressure_field.vortices.iter().take(5) {
            if let Some(insight) = crate::pipeline::pressure::reasoning::reason_about_vortex(
                vortex,
                &pressure_field,
                &lifecycle_tracker,
                &sector_members,
                &symbol_sector,
            ) {
                eprintln!("[us] {}", insight.summary);
            }
        }
        // Vortex outcome learning
        {
            let prices: std::collections::HashMap<crate::ontology::objects::Symbol, Decimal> =
                quotes.iter().filter_map(|q| {
                    if q.last_done > Decimal::ZERO {
                        Some((q.symbol.clone(), q.last_done))
                    } else {
                        None
                    }
                }).collect();
            pressure_field.record_pending_vortices(tick, &prices);
            if !pressure_field.recent_outcomes.is_empty() {
                let correct = pressure_field.recent_outcomes.iter().filter(|o| o.correct).count();
                let total = pressure_field.recent_outcomes.len();
                eprintln!(
                    "[us] vortex outcomes: {}/{} correct ({:.0}%)",
                    correct, total, correct as f64 / total as f64 * 100.0,
                );
                pressure_field.apply_outcomes_to_edges(&mut edge_ledger, now);
            }
        }

        let mut reasoning = UsReasoningSnapshot::empty(now);

        // Pressure field → tactical setups: surface vortices as actionable items.
        let vortex_setups = crate::pipeline::pressure::bridge::vortices_to_tactical_setups(
            &pressure_field.vortices,
            now,
            tick,
            10,
        );
        if !vortex_setups.is_empty() {
            eprintln!(
                "[us] pressure→action: {} vortex setups (top: {} action={} conf={})",
                vortex_setups.len(),
                vortex_setups[0].scope.label(),
                vortex_setups[0].action,
                vortex_setups[0].confidence,
            );
            reasoning.tactical_setups.extend(vortex_setups);
        }

        merge_us_standard_attention_maintenance(
            &mut reasoning,
            tick_history.latest(),
            &attention_plan.standard_symbols,
            &previous_setups,
            &previous_tracks,
            now,
        );

        // 5a. Energy momentum: accumulate diffusion energy across ticks
        {
            let tick_energy = crate::graph::energy::NodeEnergyMap::from_propagation_paths(
                &reasoning.propagation_paths,
            );
            energy_momentum.update(&tick_energy, Decimal::new(7, 1));
        }

        // 5b. Residual Field: compute, infer hidden forces, verify, cross-validate with options
        let us_residual_field = crate::us::pipeline::residual::compute_us_residual_field(
            &decision.convergence_scores,
            &quotes,
        );
        if !us_residual_field.residuals.is_empty() {
            eprintln!(
                "[us] residual field: {} symbols, {} clusters, {} divergent pairs",
                us_residual_field.residuals.len(),
                us_residual_field.clustered_sectors.len(),
                us_residual_field.divergent_pairs.len(),
            );
        }
        // Infer hidden forces
        let us_hidden_forces =
            crate::pipeline::residual::infer_hidden_forces(&us_residual_field, now);
        if !us_hidden_forces.is_empty() {
            eprintln!(
                "[us] injected {} hidden force hypotheses",
                us_hidden_forces.len()
            );
            reasoning.hypotheses.extend(us_hidden_forces);
        }
        // Verify hidden forces (tick-level)
        let us_verify = us_hidden_force_state.tick(&us_residual_field, &reasoning.hypotheses, tick);
        if !us_verify.confirmed.is_empty() || !us_verify.invalidated.is_empty() {
            eprintln!(
                "[us] hidden force verification: {}c/{}d/{}i/{}r",
                us_verify.confirmed.len(),
                us_verify.dissipating.len(),
                us_verify.invalidated.len(),
                us_verify.resolved.len(),
            );
        }
        // Apply residual-based confidence adjustments
        for (hyp_id, adj) in us_hidden_force_state.confidence_adjustments() {
            if let Some(hyp) = reasoning
                .hypotheses
                .iter_mut()
                .find(|h| h.hypothesis_id == hyp_id)
            {
                hyp.confidence = (hyp.confidence + adj)
                    .clamp(Decimal::ZERO, Decimal::ONE)
                    .round_dp(4);
            }
        }
        // Option cross-validation (US has option surfaces!)
        let option_validations = crate::pipeline::residual::cross_validate_with_options(
            &us_hidden_force_state,
            &rest.option_surfaces,
        );
        if !option_validations.is_empty() {
            for v in &option_validations {
                eprintln!(
                    "[us] option cross-validation: {} {:?} (confidence={:.2}) — {}",
                    v.symbol.0, v.verdict, v.confidence, v.explanation,
                );
            }
            // Apply option-based confidence adjustments
            for (hyp_id, adj) in
                crate::pipeline::residual::option_confidence_adjustments(&option_validations)
            {
                if let Some(hyp) = reasoning
                    .hypotheses
                    .iter_mut()
                    .find(|h| h.hypothesis_id == hyp_id)
                {
                    hyp.confidence = (hyp.confidence + adj)
                        .clamp(Decimal::ZERO, Decimal::ONE)
                        .round_dp(4);
                }
            }
        }
        // Crystallize confirmed forces
        let us_crystallization =
            crate::pipeline::residual::crystallize_confirmed_forces(&us_hidden_force_state);
        if !us_crystallization.emergent_paths.is_empty() {
            let emergent = crate::pipeline::residual::emergent_paths_to_propagation_paths(
                &us_crystallization.emergent_paths,
                now,
            );
            reasoning.propagation_paths.extend(emergent);
        }

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
        run_us_persistence_stage(&runtime, tick, now, &live, &rest, &tick_record).await;
        tick_history.push(tick_record);
        crate::graph::edge_learning::ingest_us_topology_outcomes(
            &mut edge_ledger,
            &mut seen_us_edge_learning_setups,
            &tick_history,
            &graph,
            SIGNAL_RESOLUTION_LAG,
            now,
        );
        edge_ledger.decay(now);

        // Accumulate institutional memory from US graph
        store
            .knowledge_write()
            .accumulate_from_us_graph(tick, &graph);

        // Evaluate and persist US candidate mechanisms
        {
            let next_patterns =
                compute_us_convergence_success_patterns(&tick_history, SIGNAL_RESOLUTION_LAG);
            let now_str = now
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default();
            cached_us_candidate_mechanisms = evaluate_us_candidate_mechanisms(
                &next_patterns,
                &cached_us_candidate_mechanisms,
                tick,
                &now_str,
            );
            let live_count = cached_us_candidate_mechanisms
                .iter()
                .filter(|m| m.mode == "live")
                .count();
            if !cached_us_candidate_mechanisms.is_empty() {
                eprintln!(
                    "[us] candidate mechanisms: {} total, {} live",
                    cached_us_candidate_mechanisms.len(),
                    live_count,
                );
            }
            #[cfg(feature = "persistence")]
            if let Some(ref store) = runtime.store {
                if let Err(err) = store
                    .write_candidate_mechanisms(&cached_us_candidate_mechanisms)
                    .await
                {
                    eprintln!("[us] failed to persist candidate mechanisms: {err}");
                }
            }
        }

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
            UsSignalScorecard::try_resolve_with_accumulator(
                record,
                tick,
                current_price,
                &mut scorecard_accumulator,
            );
        }
        prune_us_signal_records(&mut signal_records, tick);
        let active_signal_count = signal_records
            .iter()
            .filter(|record| !record.resolved)
            .count();
        let scorecard = scorecard_accumulator.to_scorecard(active_signal_count);
        vortex_attention =
            derive_us_vortex_attention(&reasoning.hypotheses, &reasoning.propagation_paths);
        for symbol in graph.stock_nodes.keys() {
            let signal_fired = reasoning_event_snapshot.events.iter().any(|event| {
                matches!(&event.value.scope, UsSignalScope::Symbol(candidate) if candidate == symbol)
            }) || reasoning_derived_snapshot.signals.iter().any(|signal| {
                matches!(&signal.value.scope, UsSignalScope::Symbol(candidate) if candidate == symbol)
            });
            let has_recommendation = reasoning_decision
                .order_suggestions
                .iter()
                .any(|suggestion| suggestion.symbol == *symbol);
            let active_hypotheses = reasoning
                .hypothesis_tracks
                .iter()
                .filter(|track| {
                    matches!(
                        &track.scope,
                        crate::ontology::reasoning::ReasoningScope::Symbol(candidate)
                            if candidate == symbol
                    )
                })
                .count() as u32;
            let change_pct = quotes
                .iter()
                .find(|quote| quote.symbol == *symbol)
                .map(|quote| {
                    use rust_decimal::prelude::ToPrimitive;
                    let last = quote.last_done.to_f64().unwrap_or(0.0);
                    let prev = quote.prev_close.to_f64().unwrap_or(0.0);
                    if prev.abs() > 0.0001 {
                        ((last - prev) / prev) * 100.0
                    } else {
                        0.0
                    }
                })
                .unwrap_or(0.0);
            attention.update_activity(
                &symbol.0,
                signal_fired,
                change_pct.abs() > 0.5,
                change_pct,
                active_hypotheses,
                has_recommendation,
            );
        }

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
            // Feed cumulative accumulator so gate decisions have stable long-term stats
            lineage_accumulator.ingest(&lineage_stats, &lineage_prev_resolved);
            lineage_prev_resolved = lineage_stats
                .by_template
                .iter()
                .map(|s| (s.template.clone(), s.resolved))
                .collect();
            #[cfg(feature = "persistence")]
            maybe_persist_us_lineage_stage(&runtime, tick, now, tick_history.len(), &lineage_stats)
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
        //     Signal momentum check: skip enter if signal is peaking/collapsing (Palantir 2nd derivative)
        for setup in &reasoning.tactical_setups {
            if setup.action == "enter" && setup.confidence >= Decimal::new(7, 1) {
                if let crate::ontology::reasoning::ReasoningScope::Symbol(sym) = &setup.scope {
                    let health = signal_momentum.signal_health(sym);
                    if matches!(
                        health,
                        crate::us::temporal::lineage::SignalHealth::Peaking
                            | crate::us::temporal::lineage::SignalHealth::Collapsing
                    ) {
                        continue; // skip enter when signal is peaking or collapsing
                    }
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
            &position_tracker,
            &workflows,
            &propagation_senses,
            &sorted_events,
        );
        let mut sorted_convergence: Vec<_> = decision.convergence_scores.iter().collect();
        sorted_convergence.sort_by(|a, b| b.1.composite.abs().cmp(&a.1.composite.abs()));

        // Feed signal momentum tracker (Palantir-style second derivative detection)
        for (sym, score) in &decision.convergence_scores {
            signal_momentum.record_convergence(sym.clone(), score.composite);
        }
        for event in &event_snapshot.events {
            if matches!(event.value.kind, UsEventKind::VolumeSpike) {
                if let UsSignalScope::Symbol(symbol) = &event.value.scope {
                    signal_momentum.record_volume_spike(symbol.clone(), event.value.magnitude);
                }
            }
        }

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

        display_us_runtime_summary(
            &artifact_projection.live_snapshot,
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
            &insights,
            &backward,
            &position_tracker,
            &workflows,
            &artifact_projection.agent_briefing,
            &artifact_projection.agent_session,
            runtime.projection_state.previous_agent_session.as_ref(),
            edge_ledger.len(),
        );
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
            &tick_history,
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

        runtime.runtime_task_heartbeat(
            format!(
                "us runtime tick {} · pushes={} · workflows={}",
                tick,
                live.push_count,
                workflows.len()
            ),
            json!({
                "market": "us",
                "tick": tick,
                "push_count": live.push_count,
                "received_push": tick_advance.received_push,
                "received_update": tick_advance.received_update,
                "tick_ms": tick_started_at.elapsed().as_millis(),
                "quotes": live.quotes.len(),
                "candlesticks": live.candlesticks.len(),
                "tick_history_len": tick_history.len(),
                "signal_records": signal_records.len(),
                "workflows": workflows.len(),
                "learned_edges": edge_ledger.len(),
                "latent_vortex_hypotheses": reasoning
                    .hypotheses
                    .iter()
                    .filter(|hypothesis| hypothesis.family_key == "latent_vortex")
                    .count(),
                "market_open": true,
            }),
        );
    }

    runtime.complete_runtime_task(
        "us runtime stopped",
        json!({
            "market": "us",
            "final_tick": tick,
            "push_count": live.push_count,
            "tick_history_len": tick_history.len(),
            "workflows": workflows.len(),
        }),
    );

    Ok(())
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
