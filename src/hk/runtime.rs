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
use serde_json::json;
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
use crate::core::artifact_repository::resolve_artifact_path;
use crate::core::market::{ArtifactKind, MarketId};
use crate::core::projection::{project_hk, HkProjectionInputs};
#[cfg(feature = "persistence")]
use crate::core::runtime::PreparedRuntimeContext;
use crate::core::runtime::{prepare_runtime_artifact_path, prepare_runtime_context_or_exit};
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
use crate::pipeline::tension::TensionSnapshot;
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
use crate::pipeline::world::{derive_with_backward_confirmation, WorldSnapshots};
use crate::core::runtime_loop::TickState;
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
use persistence::PERSISTENCE_MAX_IN_FLIGHT;
#[cfg(feature = "persistence")]
use persistence::{run_hk_persistence_stage, run_hk_projection_stage};
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
#[path = "runtime/integration.rs"]
pub mod integration;

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
    let mut last_idle_log_at = Instant::now();
    let mut integration = integration::RuntimeIntegration::new(WATCHLIST.len());
    #[cfg(feature = "persistence")]
    if let Some(ref store) = runtime.store {
        eden::persistence::case_reasoning_assessment::backfill_doctrine_assessments(store, "hk").await;
    }

    let mut hidden_force_state = eden::pipeline::residual::HiddenForceVerificationState::default();
    let mut edge_ledger = eden::graph::edge_learning::EdgeLearningLedger::default();
    let mut seen_hk_edge_learning_setups = HashSet::new();
    let mut energy_momentum = eden::graph::energy::EnergyMomentum::default();
    let mut pressure_field = eden::pipeline::pressure::PressureField::new(time::OffsetDateTime::now_utc());
    let mut lifecycle_tracker = eden::pipeline::pressure::reasoning::LifecycleTracker::default();
    let mut sector_members: HashMap<eden::ontology::objects::SectorId, Vec<Symbol>> = store
        .sectors
        .keys()
        .cloned()
        .map(|sector_id| (sector_id, Vec::new()))
        .collect();
    let mut symbol_sector = HashMap::new();
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
        let mut rest_updated = false;
        let Some(tick_advance) = ({
            let mut tick_state = HkTickState {
                live: &mut live,
                rest: &mut rest,
                rest_updated: &mut rest_updated,
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
            if last_idle_log_at.elapsed() >= std::time::Duration::from_secs(30) {
                eprintln!(
                    "[HK idle] tick={} push_count={} dirty={} quotes={} depths={} brokers={} calc_indexes={} capital_flows={}",
                    tick,
                    live.push_count,
                    live.dirty,
                    live.quotes.len(),
                    live.depths.len(),
                    live.brokers.len(),
                    rest.calc_indexes.len(),
                    rest.capital_flows.len(),
                );
                last_idle_log_at = Instant::now();
            }
            continue;
        };
        last_idle_log_at = Instant::now();
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

        let intraday_obs: Vec<eden::ontology::links::IntradayObservation> = rest
            .intraday_lines
            .iter()
            .filter_map(|(sym, lines)| {
                let last = lines.last()?;
                if last.avg_price <= Decimal::ZERO {
                    return None;
                }
                let deviation = (last.price - last.avg_price) / last.avg_price;
                Some(eden::ontology::links::IntradayObservation {
                    symbol: sym.clone(),
                    avg_price: last.avg_price,
                    last_price: last.price,
                    vwap_deviation: deviation,
                    point_count: lines.len(),
                })
            })
            .collect();
        let links = LinkSnapshot::compute(&raw, &store).with_intraday(intraday_obs);
        let readiness = compute_readiness(&links);
        let dim_snapshot = DimensionSnapshot::compute(&links, &store);
        let tension_snapshot = TensionSnapshot::compute(&dim_snapshot);
        let narrative_snapshot = NarrativeSnapshot::compute(&tension_snapshot, &dim_snapshot);
        let brain = BrainGraph::compute(&narrative_snapshot, &dim_snapshot, &links, &store);

        let graph_temporal_delta = edge_registry.update(&brain, tick);
        let graph_node_delta = node_registry.update(&brain, tick);
        let broker_delta =
            broker_registry.update(&links.broker_queues, &links.order_books, &store, tick);
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
                // Skip rolling stats for symbols the attention budget marks as inactive
                if !integration.should_compute_rolling_stats(&symbol.0) {
                    continue;
                }
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
        if edge_ledger.is_empty() && !history.is_empty() {
            let credited = eden::graph::edge_learning::ingest_hk_topology_outcomes(
                &mut edge_ledger,
                &mut seen_hk_edge_learning_setups,
                &history,
                &brain,
                LINEAGE_WINDOW as u64,
                now,
            );
            if credited > 0 {
                eprintln!(
                    "[hk] seeded edge ledger from restored history (credited_setups={}, learned_edges={})",
                    credited,
                    edge_ledger.len()
                );
            }
        }
        let mut decision =
            DecisionSnapshot::compute(&brain, &links, &active_fps, &store, Some(&temporal_ctx), Some(&edge_ledger));

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
        if let Some(previous_agent_snapshot) =
            runtime.projection_state.previous_agent_snapshot.as_ref()
        {
            let catalyst_events = crate::pipeline::signals::catalyst_events_from_macro_events(
                &previous_agent_snapshot.macro_events,
                links.timestamp,
            );
            event_snapshot.events.extend(catalyst_events);
            crate::pipeline::signals::enrich_attribution_with_evidence(
                &mut event_snapshot,
                &links.cross_stock_presences,
                &previous_agent_snapshot.macro_events,
            );
        }
        crate::pipeline::signals::detect_propagation_absences(
            &mut event_snapshot,
            &dim_snapshot,
            &symbol_sector,
        );
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
        let attention_plan = attention_reasoning_plan(
            brain.stock_nodes.keys().cloned(),
            &integration,
            previous_setups,
            previous_tracks,
        );
        let reasoning_active_symbols = attention_plan.active_symbols();
        let _deep_reasoning_event_snapshot =
            filter_event_snapshot_for_reasoning(&event_snapshot, &attention_plan.deep_symbols);
        let reasoning_event_snapshot =
            filter_event_snapshot_for_reasoning(&event_snapshot, &reasoning_active_symbols);
        let _deep_reasoning_derived_signal_snapshot = filter_derived_signal_snapshot_for_reasoning(
            &derived_signal_snapshot,
            &attention_plan.deep_symbols,
        );
        let reasoning_derived_signal_snapshot = filter_derived_signal_snapshot_for_reasoning(
            &derived_signal_snapshot,
            &reasoning_active_symbols,
        );
        let deep_reasoning_decision =
            filter_decision_for_reasoning(&decision, &attention_plan.deep_symbols);
        let reasoning_decision =
            filter_decision_for_reasoning(&decision, &reasoning_active_symbols);
        let lineage_family_priors = compute_family_context_outcomes(&history, LINEAGE_WINDOW);
        let _multi_horizon_lineage = eden::temporal::lineage::compute_multi_horizon_lineage_metrics(
            &history,
            LINEAGE_WINDOW,
            330,
        );
        let _multi_horizon_gate =
            eden::temporal::lineage::MultiHorizonGate::from_metrics(&_multi_horizon_lineage);
        let reasoning_stock_deltas = compute_reasoning_stock_deltas(
            &deep_reasoning_decision.convergence_scores,
            history.latest(),
        );
        let residual_field = eden::pipeline::residual::compute_residual_field(
            &deep_reasoning_decision.convergence_scores,
            &dim_snapshot.dimensions,
            &reasoning_stock_deltas,
            &brain,
        );
        if !residual_field.residuals.is_empty() {
            eprintln!(
                "[hk] residual field: {} symbols with residual, {} sector clusters, {} divergent pairs",
                residual_field.residuals.len(),
                residual_field.clustered_sectors.len(),
                residual_field.divergent_pairs.len(),
            );
            for cluster in &residual_field.clustered_sectors {
                eprintln!(
                    "[hk]   sector {} residual={:.4} coherence={:.2} dimension={} ({} symbols)",
                    cluster.sector.0,
                    cluster.mean_residual,
                    cluster.coherence,
                    cluster.dominant_dimension.label(),
                    cluster.symbol_count,
                );
            }
            for pair in residual_field.divergent_pairs.iter().take(3) {
                eprintln!(
                    "[hk]   divergence: {} ({:+.4}) vs {} ({:+.4}) strength={:.4}",
                    pair.symbol_a.0, pair.residual_a,
                    pair.symbol_b.0, pair.residual_b,
                    pair.divergence_strength,
                );
            }
        }
        // Energy propagation: build energy map from diffusion paths, blend into momentum,
        // then apply momentum-based energy to convergence scores (mutated in place,
        // avoiding a full DecisionSnapshot clone).
        let diffusion_paths = eden::pipeline::reasoning::derive_diffusion_propagation_paths(
            &brain,
            &reasoning_stock_deltas,
            deep_reasoning_decision.timestamp,
        );
        let tick_energy =
            eden::graph::energy::NodeEnergyMap::from_propagation_paths(&diffusion_paths);
        energy_momentum.update(&tick_energy, Decimal::new(7, 1));
        let mut deep_reasoning_decision = deep_reasoning_decision;
        if !energy_momentum.is_empty() {
            eden::graph::energy::apply_energy_to_convergence(
                &mut deep_reasoning_decision.convergence_scores,
                &energy_momentum,
            );
        }
        // Pressure field: inject local pressure, propagate along graph edges, detect vortices.
        pressure_field.tick(
            deep_reasoning_decision.timestamp,
            &dim_snapshot.dimensions,
            &brain,
            &edge_ledger,
        );
        for vortex in &pressure_field.vortices {
            lifecycle_tracker.record(&vortex.symbol, tick, vortex.tension);
        }
        lifecycle_tracker.decay(tick);
        if !pressure_field.vortices.is_empty() {
            eprintln!(
                "[hk] pressure field: {} vortices detected (top: {} strength={} channels={} dir={})",
                pressure_field.vortices.len(),
                pressure_field.vortices[0].symbol.0,
                pressure_field.vortices[0].tension,
                pressure_field.vortices[0].tense_channel_count,
                pressure_field.vortices[0].temporal_divergence,
            );
        }
        for vortex in pressure_field.vortices.iter().take(5) {
            if let Some(insight) = eden::pipeline::pressure::reasoning::reason_about_vortex(
                vortex,
                &pressure_field,
                &lifecycle_tracker,
                &sector_members,
                &symbol_sector,
            ) {
                eprintln!("[hk] {}", insight.summary);
            }
        }
        // Vortex outcome learning: record pending vortices, resolve old ones, update edges
        {
            let prices: std::collections::HashMap<eden::ontology::objects::Symbol, rust_decimal::Decimal> =
                raw.quotes.iter().filter_map(|(sym, q)| {
                    if q.last_done > rust_decimal::Decimal::ZERO {
                        Some((sym.clone(), q.last_done))
                    } else {
                        None
                    }
                }).collect();
            pressure_field.record_pending_vortices(tick, &prices);
            if !pressure_field.recent_outcomes.is_empty() {
                let correct = pressure_field.recent_outcomes.iter().filter(|o| o.correct).count();
                let total = pressure_field.recent_outcomes.len();
                eprintln!(
                    "[hk] vortex outcomes: {}/{} correct ({:.0}%)",
                    correct, total, correct as f64 / total as f64 * 100.0,
                );
                pressure_field.apply_outcomes_to_edges(&mut edge_ledger, now);
            }
        }

        let mut reasoning_snapshot = ReasoningSnapshot::empty(deep_reasoning_decision.timestamp);

        // Inject vortex-derived tactical setups from pressure field.
        let vortex_setups = eden::pipeline::pressure::bridge::vortices_to_tactical_setups(
            &pressure_field.vortices,
            deep_reasoning_decision.timestamp,
            tick,
            10,
        );
        if !vortex_setups.is_empty() {
            eprintln!(
                "[hk] pressure→action: {} vortex setups (top: {} action={} conf={})",
                vortex_setups.len(),
                vortex_setups[0].scope.label(),
                vortex_setups[0].action,
                vortex_setups[0].confidence,
            );
            reasoning_snapshot.tactical_setups.extend(vortex_setups);
        }

        // Inject hidden force hypotheses from residual field
        let hidden_force_hypotheses =
            eden::pipeline::residual::infer_hidden_forces(&residual_field, decision.timestamp);
        if !hidden_force_hypotheses.is_empty() {
            eprintln!(
                "[hk] injected {} hidden force hypotheses ({} isolated, {} sector, {} connection)",
                hidden_force_hypotheses.len(),
                hidden_force_hypotheses.iter().filter(|h| h.family_key == "hidden_force" && h.hypothesis_id.contains("isolated")).count(),
                hidden_force_hypotheses.iter().filter(|h| h.hypothesis_id.contains("sector")).count(),
                hidden_force_hypotheses.iter().filter(|h| h.family_key == "hidden_connection").count(),
            );
            reasoning_snapshot
                .hypotheses
                .extend(hidden_force_hypotheses);
        }

        // Verify hidden forces against current residuals (tick-level outcome)
        let verification_result = hidden_force_state.tick(
            &residual_field,
            &reasoning_snapshot.hypotheses,
            tick,
        );
        if !verification_result.confirmed.is_empty()
            || !verification_result.invalidated.is_empty()
            || !verification_result.dissipating.is_empty()
        {
            eprintln!(
                "[hk] hidden force verification: {} confirmed, {} dissipating, {} invalidated, {} resolved, {} new",
                verification_result.confirmed.len(),
                verification_result.dissipating.len(),
                verification_result.invalidated.len(),
                verification_result.resolved.len(),
                verification_result.new_trackers,
            );
        }
        // Apply confidence adjustments from verification
        for (hyp_id, adjustment) in hidden_force_state.confidence_adjustments() {
            if let Some(hyp) = reasoning_snapshot
                .hypotheses
                .iter_mut()
                .find(|h| h.hypothesis_id == hyp_id)
            {
                hyp.confidence = (hyp.confidence + adjustment)
                    .clamp(Decimal::ZERO, Decimal::ONE)
                    .round_dp(4);
            }
        }

        edge_ledger.decay(deep_reasoning_decision.timestamp);

        // Crystallize confirmed forces → attention boosts + emergent paths + graph edges
        let crystallization =
            eden::pipeline::residual::crystallize_confirmed_forces(&hidden_force_state);
        if !crystallization.attention_boosts.is_empty()
            || !crystallization.emergent_paths.is_empty()
            || !crystallization.emergent_edges.is_empty()
        {
            eprintln!(
                "[hk] crystallization: {} attention boosts, {} emergent paths, {} emergent edges",
                crystallization.attention_boosts.len(),
                crystallization.emergent_paths.len(),
                crystallization.emergent_edges.len(),
            );
            // Log attention boosts (integration feeds these into next tick's attention plan)
            for boost in &crystallization.attention_boosts {
                eprintln!(
                    "[hk]   attention boost: {} — {}",
                    boost.symbol.0, boost.boost_reason,
                );
            }
            // Inject emergent propagation paths into reasoning
            let emergent_prop_paths = eden::pipeline::residual::emergent_paths_to_propagation_paths(
                &crystallization.emergent_paths,
                decision.timestamp,
            );
            reasoning_snapshot
                .propagation_paths
                .extend(emergent_prop_paths);
            // Log emergent edges (graph integration deferred to next tick's BrainGraph::compute)
            for edge in &crystallization.emergent_edges {
                eprintln!(
                    "[hk]   emergent edge: {} ↔ {} type={:?} strength={:.2} ({})",
                    edge.symbol_a.0, edge.symbol_b.0, edge.edge_type,
                    edge.strength, edge.evidence_summary,
                );
            }
        }

        merge_standard_attention_maintenance(
            &mut reasoning_snapshot,
            history.latest(),
            &attention_plan.standard_symbols,
            previous_setups,
            previous_tracks,
            decision.timestamp,
        );
        let world_snapshots = derive_with_backward_confirmation(
            &reasoning_event_snapshot,
            &reasoning_derived_signal_snapshot,
            &graph_insights,
            &reasoning_decision,
            &mut reasoning_snapshot,
            previous_setups,
            previous_tracks,
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
        let _ = (
            &action_stage.workflow_records,
            &action_stage.workflow_events,
        );

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
        eden::graph::edge_learning::ingest_hk_topology_outcomes(
            &mut edge_ledger,
            &mut seen_hk_edge_learning_setups,
            &history,
            &brain,
            LINEAGE_WINDOW as u64,
            now,
        );
        integration.refresh_vortex_attention(&world_snapshots.world_state);

        store
            .knowledge_write()
            .accumulate_institutional_memory(tick, &brain);

        // Update new infrastructure modules with tick results
        {
            let captured_at_str = now
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default();
            let signal_symbols: Vec<String> = derived_signal_snapshot
                .signals
                .iter()
                .filter_map(|s| match &s.value.scope {
                    SignalScope::Symbol(sym) => Some(sym.0.clone()),
                    _ => None,
                })
                .collect();
            integration.after_tick(
                tick,
                &captured_at_str,
                signal_symbols.clone(),
                None, // regime — wired later when regime detection matures
                None, // stress — wired later
                derived_signal_snapshot.signals.len(),
                reasoning_snapshot.hypothesis_tracks.len(),
                vec![], // decisions placeholder
                tick_started_at.elapsed().as_millis() as u64,
            );

            // Update per-symbol attention activity for next tick's budget allocation
            let signal_set: std::collections::HashSet<&str> =
                signal_symbols.iter().map(|s| s.as_str()).collect();
            for symbol in brain.stock_nodes.keys() {
                let sym = &symbol.0;
                let has_signal = signal_set.contains(sym.as_str());
                let has_convergence = decision.convergence_scores.contains_key(symbol);
                let hyp_count = reasoning_snapshot
                    .hypothesis_tracks
                    .iter()
                    .filter(|h| matches!(&h.scope, eden::ontology::reasoning::ReasoningScope::Symbol(s) if s.0 == *sym))
                    .count() as u32;
                let change_pct: f64 = links
                    .quotes
                    .iter()
                    .find(|q| q.symbol.0 == *sym)
                    .map(|q| {
                        use rust_decimal::prelude::ToPrimitive;
                        let last = q.last_done.to_f64().unwrap_or(0.0);
                        let prev = q.prev_close.to_f64().unwrap_or(0.0);
                        if prev.abs() > 0.0001 {
                            ((last - prev) / prev) * 100.0
                        } else {
                            0.0
                        }
                    })
                    .unwrap_or(0.0);
                integration.update_symbol_activity(
                    sym,
                    has_signal,
                    change_pct.abs() > 0.5,
                    change_pct,
                    hyp_count,
                    has_convergence,
                );
            }
        }

        // ── Persist to SurrealDB (non-blocking, fire-and-forget) ──
        #[cfg(feature = "persistence")]
        if let Some(latest_record) = history.latest() {
            run_hk_persistence_stage(
                &runtime,
                tick,
                now,
                &raw,
                &links,
                latest_record,
                &action_stage.workflow_records,
                &action_stage.workflow_events,
                &reasoning_snapshot,
            )
            .await;
        }

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
                &history,
                latest,
                &tracker,
                &causal_timelines,
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
                previous_agent_scoreboard: runtime
                    .projection_state
                    .previous_agent_scoreboard
                    .as_ref(),
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
                vec![(
                    bridge_snapshot_path.clone(),
                    json_payload(&hk_bridge_snapshot),
                )],
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

        runtime.runtime_task_heartbeat(
            format!(
                "hk runtime tick {} · pushes={} · ready={}",
                tick,
                live.push_count,
                readiness.ready_symbols.len()
            ),
            json!({
                "market": "hk",
                "tick": tick,
                "push_count": live.push_count,
                "received_push": tick_advance.received_push,
                "received_update": tick_advance.received_update,
                "tick_ms": tick_started_at.elapsed().as_millis(),
                "history_len": history.len(),
                "learned_edges": edge_ledger.len(),
                "ready_symbols": readiness.ready_symbols.len(),
                "quote_symbols": readiness.quote_symbols,
                "order_book_symbols": readiness.order_book_symbols,
                "context_symbols": readiness.context_symbols,
                "quotes": live.quotes.len(),
                "depths": live.depths.len(),
                "brokers": live.brokers.len(),
                "calc_indexes": rest.calc_indexes.len(),
                "capital_flows": rest.capital_flows.len(),
            }),
        );

        prev_insights = Some(graph_insights);
    }

    runtime.complete_runtime_task(
        "hk runtime stopped",
        json!({
            "market": "hk",
            "final_tick": tick,
            "push_count": live.push_count,
            "history_len": history.len(),
        }),
    );
}
