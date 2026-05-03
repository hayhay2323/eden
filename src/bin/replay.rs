//! Replay historical tick archives through Eden's own reasoning stack.
//!
//! Supports two data sources:
//! - SurrealDB (--db, requires --features persistence)
//! - Parquet JSON archives (--parquet <dir>, no persistence needed)
//!
//! The Parquet path reads TickArchive JSON files produced by `replay_parquet.py`.
//!
//! Usage:
//!   # From Parquet (no persistence feature needed):
//!   cargo run --bin replay --release -- --parquet data/parquet_replay
//!   cargo run --bin replay --release -- --parquet data/parquet_replay --limit 50 --chains 8
//!
//!   # From SurrealDB (requires persistence):
//!   cargo run --features persistence --bin replay --release
//!   cargo run --features persistence --bin replay --release -- --limit 300 --chains 8
//!   cargo run --features persistence --bin replay --release -- --db data/eden.db

use std::collections::{HashMap, HashSet};
use std::io::Write;

use eden::action::narrative::NarrativeSnapshot;
use eden::core::projection::{project_hk, HkProjectionInputs, ProjectionBundle};
use eden::graph::decision::ConvergenceScore;
use eden::graph::decision::{DecisionSnapshot, StructuralFingerprint};
use eden::graph::graph::BrainGraph;
use eden::graph::insights::{ConflictHistory, GraphInsights, StockPressure};
use eden::graph::temporal::{TemporalBrokerRegistry, TemporalEdgeRegistry, TemporalNodeRegistry};
use eden::graph::tracker::PositionTracker;
use eden::live_snapshot::{
    action_surface_priority, apply_raw_disagreement_layer, build_live_raw_microstructure,
    build_live_raw_sources, build_signal_translation_gaps, enforce_orphan_action_cap,
    enforce_timing_action_cap, LiveBackwardChain, LiveCausalLeader, LiveEvent, LiveHypothesisTrack,
    LiveLineageMetric, LiveMarket, LiveMarketRegime, LivePressure, LiveScorecard, LiveSignal,
    LiveSnapshot, LiveStressSnapshot, LiveTacticalCase, LiveTemporalBar,
};
use eden::ontology::build_operational_snapshot;
use eden::ontology::links::LinkSnapshot;
use eden::ontology::microstructure::TickArchive;
use eden::ontology::objects::{Market, Symbol};
use eden::ontology::store::ObjectStore;
use eden::ontology::{
    direction_from_setup, BackwardInvestigation, CaseReasoningProfile, ReasoningScope,
    TacticalAction, TacticalDirection, TacticalSetup,
};
#[cfg(feature = "persistence")]
use eden::persistence::store::EdenStore;
use eden::pipeline::dimensions::{DimensionSnapshot, SymbolDimensions};
use eden::pipeline::mechanism_inference::build_reasoning_profile;
use eden::pipeline::perception::apply_perception_layer;
use eden::pipeline::predicate_engine::{derive_atomic_predicates, PredicateInputs};
use eden::pipeline::raw_events::{RawEventSource, RawEventStore, RawSourceExport};
use eden::pipeline::reasoning::ReasoningSnapshot;
use eden::pipeline::signals::{
    broker_events_from_delta, DerivedSignalSnapshot, EventSnapshot, ObservationSnapshot,
};
use eden::pipeline::tension::TensionSnapshot;
use eden::temporal::buffer::TickHistory;
use eden::temporal::record::TickRecord;
use eden::HypothesisTrackStatus;
use rust_decimal::Decimal;
use sha2::{Digest, Sha256};

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    if let Err(error) = run().await {
        eprintln!("replay failed: {error}");
        std::process::exit(1);
    }
}

async fn run() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let args: Vec<String> = std::env::args().collect();
    let requested_limit: usize = parse_flag(&args, "--limit").unwrap_or(300);
    let replay_limit = if args.iter().any(|arg| arg == "--all") || requested_limit == 0 {
        None
    } else {
        Some(requested_limit)
    };
    let chains: usize = parse_flag(&args, "--chains").unwrap_or(8);
    let parquet_dir = parse_flag_str(&args, "--parquet");
    let market_filter = parse_flag_str(&args, "--market");
    let raw_sources_out = parse_flag_str(&args, "--raw-sources-out")
        .unwrap_or_else(|| "data/replay_raw_sources.jsonl".into());

    let archives = load_archives(
        &args,
        parquet_dir.as_deref(),
        replay_limit,
        market_filter.as_deref(),
    )
    .await?;

    if archives.is_empty() {
        println!("No archives found.");
        return Ok(());
    }

    println!("Loaded {} archives.", archives.len());
    println!("chains:  {chains}");
    println!();

    let history_capacity = archives.len().clamp(512, 4096);
    let mut history = TickHistory::new(history_capacity);
    let mut conflict_history = ConflictHistory::new();
    let mut edge_registry = TemporalEdgeRegistry::new();
    let mut node_registry = TemporalNodeRegistry::new();
    let mut broker_registry = TemporalBrokerRegistry::new();
    let mut tracker = PositionTracker::new();
    let mut prev_insights: Option<GraphInsights> = None;

    let mut positions: HashMap<Symbol, ReplayPosition> = HashMap::new();
    let mut closed_positions = Vec::new();
    let mut printed_chains = 0usize;
    let mut printed_candidate_symbols = HashSet::new();
    let mut printed_candidates = Vec::new();
    let mut seen_symbols = HashSet::new();
    let mut regression_report = ReplayRegressionReport::new();
    let mut latest_replay_snapshot: Option<(LiveSnapshot, ProjectionBundle)> = None;
    let total_archives = archives.len();
    let first_tick = archives.first().map(|a| a.tick_number);
    let last_tick = archives.last().map(|a| a.tick_number);

    for archive in &archives {
        collect_symbols_from_archive(&mut seen_symbols, archive);
    }
    let current_store = object_store_from_symbols(&seen_symbols);
    println!(
        "Archives tick {}..{}, known stocks={}",
        first_tick.unwrap_or_default(),
        last_tick.unwrap_or_default(),
        current_store.stocks.len(),
    );

    {
        for archive in &archives {
            let tick = archive.tick_number;
            let links = LinkSnapshot::from_archive(archive, &current_store);
            let price_map = current_prices(&links);
            update_candidate_outcomes(&mut printed_candidates, tick, &price_map);

            let dim_snapshot = DimensionSnapshot::compute(&links, &current_store);
            let tension_snapshot = TensionSnapshot::compute(&dim_snapshot);
            let narrative_snapshot = NarrativeSnapshot::compute(&tension_snapshot, &dim_snapshot);
            let brain =
                BrainGraph::compute(&narrative_snapshot, &dim_snapshot, &links, &current_store);

            let graph_temporal_delta = edge_registry.update(&brain, tick);
            let graph_node_delta = node_registry.update(&brain, tick);
            let broker_delta = broker_registry.update(
                &links.broker_queues,
                &links.order_books,
                &current_store,
                tick,
            );
            let graph_insights = GraphInsights::compute(
                &brain,
                &current_store,
                prev_insights.as_ref(),
                &mut conflict_history,
                tick,
            );
            let active_fps = tracker.active_fingerprints();
            let decision =
                DecisionSnapshot::compute(&brain, &links, &active_fps, &current_store, None, None);
            let observation_snapshot = ObservationSnapshot::from_links(&links);
            let mut event_snapshot = EventSnapshot::detect(
                &history,
                tick,
                &links,
                &dim_snapshot,
                &graph_insights,
                &decision,
            );
            event_snapshot
                .events
                .extend(broker_events_from_delta(&broker_delta, links.timestamp));
            let derived_signal_snapshot = DerivedSignalSnapshot::compute(
                &dim_snapshot,
                &graph_insights,
                &decision,
                &event_snapshot,
            );
            let previous_setups = history
                .latest()
                .map(|record| record.tactical_setups.as_slice())
                .unwrap_or(&[]);
            let previous_tracks = history
                .latest()
                .map(|record| record.hypothesis_tracks.as_slice())
                .unwrap_or(&[]);
            let mut reasoning_snapshot = ReasoningSnapshot::empty(decision.timestamp);
            let previous_backward = history.latest().map(|record| &record.backward_reasoning);
            let world_snapshots = eden::pipeline::world::derive_with_backward_confirmation(
                &event_snapshot,
                &derived_signal_snapshot,
                &graph_insights,
                &decision,
                &mut reasoning_snapshot,
                previous_setups,
                previous_tracks,
                previous_backward,
            );

            let symbol_states = build_symbol_states(
                &reasoning_snapshot,
                &world_snapshots.backward_reasoning.investigations,
                &graph_insights.pressures,
                &decision,
                &dim_snapshot,
                &event_snapshot,
                &current_store,
            );

            let mut enter_symbols = HashSet::new();
            let mut tick_printed = false;

            for (symbol, state) in &symbol_states {
                if let Some(position) = positions.get(symbol) {
                    if let Some(reason) = exit_reason(state) {
                        let position = position.clone();
                        let exit_price = price_map
                            .get(symbol)
                            .copied()
                            .unwrap_or(position.entry_price);
                        let return_pct = directional_return(
                            position.entry_price,
                            exit_price,
                            position.direction,
                        );
                        closed_positions.push(ClosedReplayPosition {
                            symbol: symbol.0.clone(),
                            setup_id: position.setup_id.clone(),
                            entry_tick: position.entry_tick,
                            exit_tick: tick,
                            direction: direction_label(position.direction).into(),
                            entry_price: position.entry_price,
                            exit_price,
                            return_pct,
                            primary_mechanism: position.primary_mechanism.clone(),
                            primary_driver: position.primary_driver.clone(),
                            exit_reason: reason.clone(),
                        });
                        tracker.exit(symbol);
                        positions.remove(symbol);
                        if printed_chains < chains {
                            let mut raw_events = RawEventStore::default();
                            raw_events.ingest_tick_archive(archive, RawEventSource::Push);
                            append_replay_raw_sources(
                                &raw_sources_out,
                                "exit",
                                tick,
                                &state.symbol,
                                &reason,
                                &raw_events,
                                &current_store,
                            )?;
                            print_exit_chain(
                                tick,
                                state,
                                &position,
                                exit_price,
                                return_pct,
                                &reason,
                                &raw_events,
                                &current_store,
                            );
                            printed_chains += 1;
                            tick_printed = true;
                        }
                    }
                    continue;
                }

                if let Some(reason) = enter_reason(state) {
                    let Some(entry_price) = price_map.get(symbol).copied() else {
                        continue;
                    };
                    if let Some(mut fingerprint) =
                        StructuralFingerprint::capture(symbol, &brain, tick, Some(entry_price))
                    {
                        fingerprint.entry_composite = state.signal.composite;
                        tracker.enter(fingerprint);
                    }
                    positions.insert(
                        symbol.clone(),
                        ReplayPosition {
                            setup_id: state.setup.setup_id.clone(),
                            entry_tick: tick,
                            entry_price,
                            direction: setup_direction(&state.setup, state.signal.composite),
                            primary_mechanism: state
                                .reasoning_profile
                                .primary_mechanism
                                .as_ref()
                                .map(|item| item.label.clone()),
                            primary_driver: state
                                .backward
                                .as_ref()
                                .and_then(|item| {
                                    item.leading_cause
                                        .as_ref()
                                        .map(|cause| cause.explanation.clone())
                                })
                                .or_else(|| Some(state.setup.entry_rationale.clone())),
                        },
                    );
                    enter_symbols.insert(symbol.clone());
                    if printed_chains < chains {
                        let mut raw_events = RawEventStore::default();
                        raw_events.ingest_tick_archive(archive, RawEventSource::Push);
                        append_replay_raw_sources(
                            &raw_sources_out,
                            "entry",
                            tick,
                            &state.symbol,
                            &reason,
                            &raw_events,
                            &current_store,
                        )?;
                        print_entry_chain(
                            tick,
                            state,
                            entry_price,
                            &reason,
                            &raw_events,
                            &current_store,
                        );
                        printed_chains += 1;
                        tick_printed = true;
                    }
                }
            }

            if !tick_printed && printed_chains < chains {
                if let Some(candidate) =
                    strongest_candidate(&symbol_states, &positions, &printed_candidate_symbols)
                {
                    let mut raw_events = RawEventStore::default();
                    raw_events.ingest_tick_archive(archive, RawEventSource::Push);
                    append_replay_raw_sources(
                        &raw_sources_out,
                        "candidate",
                        tick,
                        &candidate.symbol,
                        &candidate_reason(candidate),
                        &raw_events,
                        &current_store,
                    )?;
                    print_candidate_chain(tick, candidate, &raw_events, &current_store);
                    printed_candidates.push(CandidateReplayObservation {
                        symbol: candidate.symbol.clone(),
                        tick,
                        entry_price: price_map
                            .get(&candidate.symbol)
                            .copied()
                            .unwrap_or(Decimal::ZERO),
                        direction: setup_direction(&candidate.setup, candidate.signal.composite),
                        action: candidate.setup.action.to_string(),
                        track_status: candidate
                            .track
                            .as_ref()
                            .map(|track| track.status.to_string()),
                        policy_reason: candidate
                            .track
                            .as_ref()
                            .map(|track| track.policy_reason.clone()),
                        primary_mechanism: candidate
                            .reasoning_profile
                            .primary_mechanism
                            .as_ref()
                            .map(|item| item.label.clone()),
                        horizon_returns: HashMap::new(),
                    });
                    printed_candidate_symbols.insert(candidate.symbol.clone());
                    printed_chains += 1;
                }
            }

            let tick_record = TickRecord::capture(
                tick,
                links.timestamp,
                &decision.convergence_scores,
                &dim_snapshot.dimensions,
                &links.order_books,
                &links.quotes,
                &links.trade_activities,
                &decision.degradations,
                &observation_snapshot,
                &event_snapshot,
                &derived_signal_snapshot,
                &[],
                &reasoning_snapshot,
                &world_snapshots.world_state,
                &world_snapshots.backward_reasoning,
                &graph_temporal_delta.transitions,
                &graph_node_delta.transitions,
            );
            history.push(tick_record);
            current_store
                .knowledge
                .write()
                .unwrap()
                .accumulate_institutional_memory(tick, &brain);

            let regression_feed = build_replay_regression_feed(
                tick,
                &decision.convergence_scores,
                &event_snapshot,
                &derived_signal_snapshot,
                &reasoning_snapshot,
                &symbol_states,
                &positions,
                &closed_positions,
            );
            regression_report.record_feed(&regression_feed);

            let mut raw_events = RawEventStore::default();
            raw_events.ingest_tick_archive(archive, RawEventSource::Push);
            let live_snapshot = build_replay_live_snapshot(
                tick,
                &links,
                &decision,
                &graph_insights,
                &reasoning_snapshot,
                &world_snapshots.backward_reasoning.investigations,
                &symbol_states,
                &tracker,
                &current_store,
                &raw_events,
            );
            let projection_bundle = project_hk(HkProjectionInputs {
                live_snapshot: live_snapshot.clone(),
                history: &history,
                links: &links,
                store: &current_store,
                lineage_priors: &[],
                hk_momentum: None,
                previous_agent_snapshot: None,
                previous_agent_session: None,
                previous_agent_scoreboard: None,
            });
            latest_replay_snapshot = Some((live_snapshot, projection_bundle));

            if tick % 30 == 0 && tracker.active_count() > 0 {
                tracker.refresh_all(&brain);
            }

            prev_insights = Some(graph_insights);
        }
    }

    println!("Archive stream complete.");
    println!(
        "Replay store: {} stocks, {} sectors",
        current_store.stocks.len(),
        current_store.sectors.len(),
    );
    println!();

    println!();
    println!("=== Replay Results ===");
    println!("Total archives:    {}", total_archives);
    println!("Chains printed:    {}", printed_chains);
    println!(
        "Entries opened:    {}",
        positions.len() + closed_positions.len()
    );
    println!("Entries closed:    {}", closed_positions.len());
    println!("Entries still open:{}", positions.len());
    println!();

    if !printed_candidates.is_empty() {
        print_candidate_outcomes(&printed_candidates);
    }

    println!("Regression fingerprint: {}", regression_report.finish());

    if closed_positions.is_empty() {
        println!("No closed replay positions yet.");
        return Ok(());
    }

    let total = closed_positions.len() as f64;
    let wins = closed_positions
        .iter()
        .filter(|item| item.return_pct > Decimal::ZERO)
        .count();
    let losses = closed_positions
        .iter()
        .filter(|item| item.return_pct < Decimal::ZERO)
        .count();
    let flats = closed_positions.len() - wins - losses;
    let avg_return = closed_positions
        .iter()
        .map(|item| item.return_pct)
        .sum::<Decimal>()
        / Decimal::from(closed_positions.len() as i64);

    println!("Win rate:          {:.1}%", wins as f64 / total * 100.0);
    println!("Loss rate:         {:.1}%", losses as f64 / total * 100.0);
    println!("Flat rate:         {:.1}%", flats as f64 / total * 100.0);
    println!(
        "Avg directional:   {:+.4}%",
        avg_return * Decimal::new(100, 0)
    );
    println!();

    let mut by_symbol: HashMap<String, Vec<&ClosedReplayPosition>> = HashMap::new();
    for item in &closed_positions {
        by_symbol.entry(item.symbol.clone()).or_default().push(item);
    }
    let mut symbol_rows = by_symbol
        .into_iter()
        .map(|(symbol, items)| {
            let count = items.len();
            let avg = items.iter().map(|item| item.return_pct).sum::<Decimal>()
                / Decimal::from(count as i64);
            let wins = items
                .iter()
                .filter(|item| item.return_pct > Decimal::ZERO)
                .count();
            (symbol, count, wins, avg)
        })
        .collect::<Vec<_>>();
    symbol_rows.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));

    println!("--- Top Symbols ---");
    println!(
        "{:12} {:>6} {:>8} {:>12}",
        "Symbol", "Count", "WinRate", "AvgRet"
    );
    for (symbol, count, wins, avg) in symbol_rows.iter().take(12) {
        println!(
            "{:12} {:>6} {:>7.1}% {:>+11.4}%",
            symbol,
            count,
            *wins as f64 / *count as f64 * 100.0,
            *avg * Decimal::new(100, 0),
        );
    }

    println!();
    println!("Done.");

    if let Some((live_snapshot, projection_bundle)) = latest_replay_snapshot {
        std::fs::write(
            "data/replay_live_snapshot.json",
            serde_json::to_string_pretty(&live_snapshot)?,
        )?;
        let operational = build_operational_snapshot(
            &live_snapshot,
            &projection_bundle.agent_snapshot,
            &projection_bundle.agent_session,
            &projection_bundle.agent_recommendations,
            Some(&projection_bundle.agent_narration),
        )?;
        std::fs::write(
            "data/replay_operational_snapshot.json",
            serde_json::to_string_pretty(&operational)?,
        )?;
        println!("Wrote data/replay_live_snapshot.json");
        println!("Wrote data/replay_operational_snapshot.json");
    }

    Ok(())
}

#[derive(Clone)]
struct ReplaySymbolState {
    symbol: Symbol,
    setup: TacticalSetup,
    track: Option<eden::HypothesisTrack>,
    signal: LiveSignal,
    pressure: Option<LivePressure>,
    backward: Option<BackwardInvestigation>,
    reasoning_profile: CaseReasoningProfile,
}

#[derive(Clone)]
struct ReplayPosition {
    setup_id: String,
    entry_tick: u64,
    entry_price: Decimal,
    direction: i8,
    primary_mechanism: Option<String>,
    primary_driver: Option<String>,
}

struct ClosedReplayPosition {
    symbol: String,
    setup_id: String,
    entry_tick: u64,
    exit_tick: u64,
    direction: String,
    entry_price: Decimal,
    exit_price: Decimal,
    return_pct: Decimal,
    primary_mechanism: Option<String>,
    primary_driver: Option<String>,
    exit_reason: String,
}

struct CandidateReplayObservation {
    symbol: Symbol,
    tick: u64,
    entry_price: Decimal,
    direction: i8,
    action: String,
    track_status: Option<String>,
    policy_reason: Option<String>,
    primary_mechanism: Option<String>,
    horizon_returns: HashMap<u64, Decimal>,
}

fn build_replay_live_snapshot(
    tick: u64,
    links: &LinkSnapshot,
    decision: &DecisionSnapshot,
    graph_insights: &GraphInsights,
    reasoning_snapshot: &ReasoningSnapshot,
    backward_investigations: &[BackwardInvestigation],
    symbol_states: &HashMap<Symbol, ReplaySymbolState>,
    tracker: &PositionTracker,
    store: &ObjectStore,
    raw_events: &RawEventStore,
) -> LiveSnapshot {
    let mut top_signals = symbol_states
        .values()
        .map(|state| state.signal.clone())
        .collect::<Vec<_>>();
    top_signals.sort_by(|a, b| b.composite.abs().cmp(&a.composite.abs()));
    top_signals.truncate(120);

    let mut tactical_cases = symbol_states
        .values()
        .map(|state| state.setup.clone())
        .map(|setup| {
            let symbol = match &setup.scope {
                ReasoningScope::Symbol(symbol) => symbol.clone(),
                _ => Symbol(String::new()),
            };
            build_live_tactical_case(&setup, &symbol)
        })
        .collect::<Vec<_>>();
    apply_raw_disagreement_layer(
        raw_events,
        store,
        &mut tactical_cases,
        time::Duration::minutes(5),
    );
    for case in &mut tactical_cases {
        if let Some(symbol) = (!case.symbol.is_empty()).then(|| Symbol(case.symbol.clone())) {
            case.timing_state = eden::live_snapshot::live_case_timing_state(
                raw_events,
                &symbol,
                case,
                time::Duration::minutes(5),
            );
        }
    }
    enforce_orphan_action_cap(&mut tactical_cases);
    enforce_timing_action_cap(&mut tactical_cases);
    tactical_cases.sort_by(|a, b| {
        action_surface_priority(a.action.as_str())
            .cmp(&action_surface_priority(b.action.as_str()))
            .then_with(|| b.heuristic_edge.cmp(&a.heuristic_edge))
            .then_with(|| b.confidence_gap.cmp(&a.confidence_gap))
            .then_with(|| b.confidence.cmp(&a.confidence))
            .then_with(|| a.setup_id.cmp(&b.setup_id))
    });
    tactical_cases.truncate(10);

    let active_position_nodes = tracker
        .active_fingerprints()
        .iter()
        .map(|fingerprint| {
            eden::ontology::ActionNode::from_hk_fingerprint(&fingerprint.symbol, fingerprint)
        })
        .collect::<Vec<_>>();

    let raw_microstructure = build_live_raw_microstructure(
        raw_events,
        store,
        &tactical_cases,
        &top_signals,
        &active_position_nodes,
        time::Duration::minutes(5),
    );
    let raw_sources = build_live_raw_sources(
        raw_events,
        store,
        &tactical_cases,
        &top_signals,
        &active_position_nodes,
        time::Duration::minutes(5),
    );
    let signal_translation_gaps =
        build_signal_translation_gaps(&tactical_cases, &top_signals, &raw_sources, 8);
    let replay_timestamp = links
        .timestamp
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    let perception = apply_perception_layer(
        tick,
        LiveMarket::Hk,
        &replay_timestamp,
        &mut tactical_cases,
        &[],
        &top_signals,
        &[],
        &[],
        None,
    );

    let hypothesis_tracks = symbol_states
        .values()
        .filter_map(|state| state.track.as_ref())
        .map(build_live_track)
        .collect::<Vec<_>>();

    let pressures = symbol_states
        .values()
        .filter_map(|state| state.pressure.clone())
        .collect::<Vec<_>>();

    let events = links
        .trade_activities
        .iter()
        .take(8)
        .map(|activity| LiveEvent {
            kind: "ReplayTradeActivity".into(),
            symbol: Some(activity.symbol.to_string()),
            magnitude: if activity.total_volume > 0 {
                Decimal::from(activity.total_volume).min(Decimal::new(100, 0))
                    / Decimal::new(100, 0)
            } else {
                Decimal::ZERO
            },
            summary: format!(
                "{} trades={} buy_vol={} sell_vol={}",
                activity.symbol, activity.trade_count, activity.buy_volume, activity.sell_volume
            ),
            age_secs: None,
            freshness: None,
        })
        .collect::<Vec<_>>();

    let backward_chains = backward_investigations
        .iter()
        .filter_map(|item| {
            let symbol = match &item.leaf_scope {
                ReasoningScope::Symbol(symbol) => symbol.clone(),
                _ => return None,
            };
            Some(build_live_backward_chain(&symbol, item))
        })
        .collect::<Vec<_>>();

    let causal_leaders = Vec::<LiveCausalLeader>::new();
    let temporal_bars = Vec::<LiveTemporalBar>::new();
    let lineage = Vec::<LiveLineageMetric>::new();

    LiveSnapshot {
        tick,
        timestamp: replay_timestamp,
        market: LiveMarket::Hk,
        market_phase: "replay".into(),
        market_active: true,
        stock_count: store.stocks.len(),
        edge_count: 0,
        hypothesis_count: reasoning_snapshot.hypotheses.len(),
        observation_count: symbol_states.len(),
        active_positions: active_position_nodes.len(),
        active_position_nodes,
        market_regime: build_live_market_regime(&decision.market_regime),
        stress: LiveStressSnapshot {
            composite_stress: graph_insights.stress.composite_stress,
            sector_synchrony: Some(graph_insights.stress.sector_synchrony),
            pressure_consensus: Some(graph_insights.stress.pressure_consensus),
            momentum_consensus: None,
            pressure_dispersion: None,
            volume_anomaly: None,
        },
        scorecard: LiveScorecard::default(),
        tactical_cases,
        hypothesis_tracks,
        recent_transitions: vec![],
        top_signals: top_signals.clone(),
        convergence_scores: top_signals,
        pressures,
        backward_chains,
        causal_leaders,
        events,
        cross_market_signals: vec![],
        cross_market_anomalies: vec![],
        structural_deltas: vec![],
        propagation_senses: vec![],
        raw_microstructure,
        raw_sources,
        signal_translation_gaps,
        cluster_states: perception.cluster_states,
        symbol_states: perception.symbol_states,
        world_summary: perception.world_summary,
        temporal_bars,
        lineage,
        success_patterns: vec![],
    }
}

fn collect_symbols_from_archive(seen_symbols: &mut HashSet<Symbol>, archive: &TickArchive) {
    for quote in &archive.quotes {
        seen_symbols.insert(quote.symbol.clone());
    }
}

fn object_store_from_symbols(seen_symbols: &HashSet<Symbol>) -> ObjectStore {
    use eden::ontology::objects::Stock;
    use eden::ontology::store::define_sectors;

    let sectors = define_sectors();
    let stocks = seen_symbols
        .iter()
        .cloned()
        .map(|symbol| Stock {
            market: Market::Hk,
            name_en: String::new(),
            name_cn: String::new(),
            name_hk: String::new(),
            exchange: String::new(),
            lot_size: 100,
            sector_id: eden::ontology::store::symbol_sector(&symbol.0),
            total_shares: 0,
            circulating_shares: 0,
            eps_ttm: Decimal::ZERO,
            bps: Decimal::ZERO,
            dividend_yield: Decimal::ZERO,
            symbol,
        })
        .collect();

    ObjectStore::from_parts(Vec::new(), stocks, sectors)
}

fn build_symbol_states(
    reasoning: &ReasoningSnapshot,
    investigations: &[BackwardInvestigation],
    pressures: &[StockPressure],
    decision: &DecisionSnapshot,
    dimensions: &DimensionSnapshot,
    events: &EventSnapshot,
    store: &ObjectStore,
) -> HashMap<Symbol, ReplaySymbolState> {
    let tracks = reasoning
        .hypothesis_tracks
        .iter()
        .filter_map(|track| match &track.scope {
            ReasoningScope::Symbol(symbol) => Some((symbol.clone(), track.clone())),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let backward = investigations
        .iter()
        .filter_map(|investigation| match &investigation.leaf_scope {
            ReasoningScope::Symbol(symbol) => Some((symbol.clone(), investigation.clone())),
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let pressure_map = pressures
        .iter()
        .map(|pressure| (pressure.symbol.clone(), pressure.clone()))
        .collect::<HashMap<_, _>>();

    let mut best_setup_by_symbol: HashMap<Symbol, TacticalSetup> = HashMap::new();
    for setup in &reasoning.tactical_setups {
        let ReasoningScope::Symbol(symbol) = &setup.scope else {
            continue;
        };
        let replace = best_setup_by_symbol
            .get(symbol)
            .map(|current| setup.confidence > current.confidence)
            .unwrap_or(true);
        if replace {
            best_setup_by_symbol.insert(symbol.clone(), setup.clone());
        }
    }

    best_setup_by_symbol
        .into_iter()
        .filter_map(|(symbol, setup)| {
            let convergence = decision.convergence_scores.get(&symbol)?;
            let dims = dimensions
                .dimensions
                .get(&symbol)
                .cloned()
                .unwrap_or_default();
            let sector = store
                .stocks
                .get(&symbol)
                .and_then(|stock| stock.sector_id.as_ref())
                .map(|sector| sector.0.clone());
            let signal = build_live_signal(&symbol, &sector, convergence, &dims);
            let pressure = pressure_map
                .get(&symbol)
                .map(|item| build_live_pressure(&symbol, sector.clone(), item));
            let track = tracks.get(&symbol).cloned();
            let backward_case = backward.get(&symbol).cloned();
            let tactical_case = build_live_tactical_case(&setup, &symbol);
            let symbol_events = build_live_events(&symbol, events);
            let invalidation_rules = setup.risk_notes.clone();
            let market_regime = build_live_market_regime(&decision.market_regime);
            let stress = LiveStressSnapshot {
                composite_stress: Decimal::ZERO,
                sector_synchrony: None,
                pressure_consensus: None,
                momentum_consensus: None,
                pressure_dispersion: None,
                volume_anomaly: None,
            };
            let live_track = track.as_ref().map(build_live_track);
            let live_backward = backward_case
                .as_ref()
                .map(|item| build_live_backward_chain(&symbol, item));
            let predicates = derive_atomic_predicates(&PredicateInputs {
                tactical_case: &tactical_case,
                active_positions: &[],
                chain: live_backward.as_ref(),
                pressure: pressure.as_ref(),
                signal: Some(&signal),
                causal: None,
                track: live_track.as_ref(),
                stress: &stress,
                market_regime: &market_regime,
                all_signals: std::slice::from_ref(&signal),
                all_pressures: pressure.as_ref().map(std::slice::from_ref).unwrap_or(&[]),
                events: &symbol_events,
                cross_market_signals: &[],
                cross_market_anomalies: &[],
            });
            let reasoning_profile = build_reasoning_profile(&predicates, &invalidation_rules, None);

            Some((
                symbol.clone(),
                ReplaySymbolState {
                    symbol,
                    setup,
                    track,
                    signal,
                    pressure,
                    backward: backward_case,
                    reasoning_profile,
                },
            ))
        })
        .collect()
}

fn build_live_signal(
    symbol: &Symbol,
    sector: &Option<String>,
    convergence: &eden::graph::decision::ConvergenceScore,
    dims: &SymbolDimensions,
) -> LiveSignal {
    LiveSignal {
        symbol: symbol.0.clone(),
        sector: sector.clone(),
        composite: convergence.composite,
        mark_price: None,
        dimension_composite: None,
        capital_flow_direction: dims.capital_flow_direction,
        price_momentum: dims.activity_momentum,
        volume_profile: dims.capital_size_divergence,
        pre_post_market_anomaly: Decimal::ZERO,
        valuation: dims.valuation_support,
        cross_stock_correlation: Some(convergence.cross_stock_correlation),
        sector_coherence: convergence.sector_coherence,
        cross_market_propagation: None,
    }
}

fn build_live_pressure(
    symbol: &Symbol,
    sector: Option<String>,
    pressure: &StockPressure,
) -> LivePressure {
    LivePressure {
        symbol: symbol.0.clone(),
        sector,
        capital_flow_pressure: pressure.net_pressure,
        momentum: pressure.net_pressure,
        pressure_delta: pressure.pressure_delta,
        pressure_duration: pressure.pressure_duration,
        accelerating: pressure.accelerating,
    }
}

fn build_live_track(track: &eden::HypothesisTrack) -> LiveHypothesisTrack {
    LiveHypothesisTrack {
        symbol: match &track.scope {
            ReasoningScope::Symbol(symbol) => symbol.0.clone(),
            _ => track.title.clone(),
        },
        title: track.title.clone(),
        status: track.status.to_string(),
        age_ticks: track.age_ticks,
        confidence: track.confidence,
    }
}

fn build_live_backward_chain(
    symbol: &Symbol,
    investigation: &BackwardInvestigation,
) -> LiveBackwardChain {
    LiveBackwardChain {
        symbol: symbol.0.clone(),
        conclusion: investigation.leaf_label.clone(),
        primary_driver: investigation
            .leading_cause
            .as_ref()
            .map(|cause| cause.explanation.clone())
            .unwrap_or_else(|| investigation.leaf_label.clone()),
        confidence: investigation
            .leading_cause
            .as_ref()
            .map(|cause| cause.competitive_score.max(cause.net_conviction))
            .unwrap_or(Decimal::ZERO),
        freshness: None,
        evidence: investigation
            .leading_cause
            .as_ref()
            .map(|cause| {
                cause
                    .supporting_evidence
                    .iter()
                    .map(|evidence| eden::live_snapshot::LiveEvidence {
                        source: evidence.channel.clone(),
                        description: evidence.statement.clone(),
                        weight: evidence.weight,
                        direction: Decimal::ONE,
                    })
                    .collect()
            })
            .unwrap_or_default(),
    }
}

fn build_live_events(symbol: &Symbol, events: &EventSnapshot) -> Vec<LiveEvent> {
    events
        .events
        .iter()
        .filter_map(|event| match &event.value.scope {
            eden::pipeline::signals::SignalScope::Symbol(event_symbol)
                if event_symbol == symbol =>
            {
                Some(LiveEvent {
                    kind: format!("{:?}", event.value.kind),
                    symbol: Some(symbol.0.clone()),
                    magnitude: event.value.magnitude,
                    summary: event.value.summary.clone(),
                    age_secs: None,
                    freshness: None,
                })
            }
            _ => None,
        })
        .collect()
}

fn build_live_tactical_case(setup: &TacticalSetup, symbol: &Symbol) -> LiveTacticalCase {
    LiveTacticalCase {
        setup_id: setup.setup_id.clone(),
        symbol: symbol.0.clone(),
        title: setup.title.clone(),
        action: setup.action.to_string(),
        confidence: setup.confidence,
        confidence_gap: setup.confidence_gap,
        heuristic_edge: setup.heuristic_edge,
        entry_rationale: setup.entry_rationale.clone(),
        causal_narrative: setup.causal_narrative.clone(),
        review_reason_code: setup
            .review_reason_code
            .map(|code| code.as_str().to_string()),
        review_reason_family: None,
        review_reason_subreasons: vec![],
        policy_primary: None,
        policy_reason: None,
        multi_horizon_gate_reason: None,
        family_label: None,
        counter_label: None,
        matched_success_pattern_signature: None,
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
        horizon_bucket: Some(eden::live_snapshot::horizon_bucket_label(
            setup.horizon.primary,
        )),
        horizon_urgency: Some(eden::live_snapshot::horizon_urgency_label(
            setup.horizon.urgency,
        )),
        horizon_secondary: setup
            .horizon
            .secondary
            .iter()
            .map(|s| eden::live_snapshot::horizon_bucket_label(s.bucket))
            .collect(),
        case_signature: None,
        archetype_projections: vec![],
        expectation_bindings: vec![],
        expectation_violations: vec![],
        inferred_intent: None,
        freshness_state: setup
            .risk_notes
            .iter()
            .any(|note| note == "carried_forward=true")
            .then(|| "carried_forward".into())
            .or_else(|| Some("fresh".into())),
        first_enter_tick: None,
        ticks_since_first_enter: None,
        ticks_since_first_seen: None,
        timing_state: Some("range_unknown".into()),
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
    }
}

fn build_live_market_regime(
    regime: &eden::graph::decision::MarketRegimeFilter,
) -> LiveMarketRegime {
    LiveMarketRegime {
        bias: regime.bias.to_string(),
        confidence: regime.confidence,
        breadth_up: regime.breadth_up,
        breadth_down: regime.breadth_down,
        average_return: regime.average_return,
        directional_consensus: Some(regime.directional_consensus),
        pre_market_sentiment: None,
    }
}

fn current_prices(links: &LinkSnapshot) -> HashMap<Symbol, Decimal> {
    let mut prices = HashMap::new();
    for quote in &links.quotes {
        if quote.last_done > Decimal::ZERO {
            prices.insert(quote.symbol.clone(), quote.last_done);
        }
    }
    prices
}

fn enter_reason(state: &ReplaySymbolState) -> Option<String> {
    if !matches!(state.setup.action, TacticalAction::Enter) {
        return None;
    }
    let mechanism = state.reasoning_profile.primary_mechanism.as_ref()?;
    if matches!(
        state.track.as_ref().map(|track| track.status),
        Some(HypothesisTrackStatus::Weakening | HypothesisTrackStatus::Invalidated)
    ) {
        return None;
    }
    if let Some(backward) = state.backward.as_ref() {
        if matches!(
            backward.contest_state,
            eden::CausalContestState::Flipped
                | eden::CausalContestState::Contested
                | eden::CausalContestState::Eroding
        ) {
            return None;
        }
        if backward.leading_cause_streak < 2 {
            return None;
        }
    }

    Some(format!(
        "{} with mechanism `{}` and stable causal context",
        state.setup.action, mechanism.label
    ))
}

fn exit_reason(state: &ReplaySymbolState) -> Option<String> {
    if !matches!(state.setup.action, TacticalAction::Enter) {
        return Some(format!("setup downgraded to `{}`", state.setup.action));
    }
    if matches!(
        state.track.as_ref().map(|track| track.status),
        Some(HypothesisTrackStatus::Weakening)
    ) {
        return Some("hypothesis track weakened".into());
    }
    if matches!(
        state.track.as_ref().map(|track| track.status),
        Some(HypothesisTrackStatus::Invalidated)
    ) {
        return Some("hypothesis invalidated".into());
    }
    if !state.reasoning_profile.automated_invalidations.is_empty() {
        return Some(
            state.reasoning_profile.automated_invalidations[0]
                .reason
                .clone(),
        );
    }
    if let Some(backward) = state.backward.as_ref() {
        if matches!(
            backward.contest_state,
            eden::CausalContestState::Flipped
                | eden::CausalContestState::Contested
                | eden::CausalContestState::Eroding
        ) {
            return Some(format!(
                "backward contest moved to {}",
                backward.contest_state
            ));
        }
    }
    None
}

fn setup_direction(setup: &TacticalSetup, composite: Decimal) -> i8 {
    if let Some(direction) = direction_from_setup(setup) {
        match direction {
            TacticalDirection::Long => 1,
            TacticalDirection::Short => -1,
        }
    } else if composite > Decimal::ZERO {
        1
    } else if composite < Decimal::ZERO {
        -1
    } else {
        0
    }
}

fn direction_label(direction: i8) -> &'static str {
    match direction.cmp(&0) {
        std::cmp::Ordering::Greater => "long",
        std::cmp::Ordering::Less => "short",
        std::cmp::Ordering::Equal => "neutral",
    }
}

fn directional_return(entry_price: Decimal, exit_price: Decimal, direction: i8) -> Decimal {
    if entry_price <= Decimal::ZERO || exit_price <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    let raw = (exit_price - entry_price) / entry_price;
    match direction.cmp(&0) {
        std::cmp::Ordering::Greater => raw,
        std::cmp::Ordering::Less => -raw,
        std::cmp::Ordering::Equal => Decimal::ZERO,
    }
}

fn update_candidate_outcomes(
    candidates: &mut [CandidateReplayObservation],
    tick: u64,
    price_map: &HashMap<Symbol, Decimal>,
) {
    const HORIZONS: [u64; 3] = [15, 50, 150];

    for candidate in candidates {
        if candidate.entry_price <= Decimal::ZERO {
            continue;
        }
        for horizon in HORIZONS {
            if tick == candidate.tick + horizon && !candidate.horizon_returns.contains_key(&horizon)
            {
                if let Some(exit_price) = price_map.get(&candidate.symbol).copied() {
                    let pnl =
                        directional_return(candidate.entry_price, exit_price, candidate.direction);
                    candidate.horizon_returns.insert(horizon, pnl);
                }
            }
        }
    }
}

fn print_candidate_outcomes(candidates: &[CandidateReplayObservation]) {
    println!("=== Candidate Outcome Check ===");
    let horizons = [15u64, 50u64, 150u64];

    for candidate in candidates {
        if candidate.entry_price <= Decimal::ZERO {
            continue;
        }

        println!(
            "{} @ tick {} direction={} action={} track={} mechanism={}",
            candidate.symbol,
            candidate.tick,
            direction_label(candidate.direction),
            candidate.action,
            candidate.track_status.as_deref().unwrap_or("none"),
            candidate.primary_mechanism.as_deref().unwrap_or("none"),
        );
        if let Some(reason) = &candidate.policy_reason {
            println!("  policy_reason={}", reason);
        }
        print!("  outcomes:");
        for horizon in horizons {
            let label = format!(" {}t=", horizon);
            if let Some(pnl) = candidate.horizon_returns.get(&horizon) {
                print!("{}{}", label, format_return_pct(*pnl));
            } else {
                print!("{}NA", label);
            }
        }
        println!();
    }

    println!();
    println!("--- Candidate Horizon Summary ---");
    for horizon in horizons {
        let mut resolved = 0usize;
        let mut wins = 0usize;
        let mut losses = 0usize;
        let mut total = Decimal::ZERO;

        for candidate in candidates {
            let Some(pnl) = candidate.horizon_returns.get(&horizon).copied() else {
                continue;
            };

            resolved += 1;
            total += pnl;
            if pnl > Decimal::ZERO {
                wins += 1;
            } else if pnl < Decimal::ZERO {
                losses += 1;
            }
        }

        if resolved == 0 {
            println!("{}t: resolved=0", horizon);
            continue;
        }

        let avg = total / Decimal::from(resolved as i64);
        println!(
            "{}t: resolved={} wins={} losses={} win_rate={:.1}% avg={}",
            horizon,
            resolved,
            wins,
            losses,
            wins as f64 / resolved as f64 * 100.0,
            format_return_pct(avg),
        );
    }
    println!();
}

fn format_return_pct(value: Decimal) -> String {
    format!("{:+.4}%", value * Decimal::new(100, 0))
}

fn strongest_candidate<'a>(
    symbol_states: &'a HashMap<Symbol, ReplaySymbolState>,
    positions: &HashMap<Symbol, ReplayPosition>,
    printed_candidate_symbols: &HashSet<Symbol>,
) -> Option<&'a ReplaySymbolState> {
    let mut ranked = symbol_states
        .values()
        .filter(|state| !positions.contains_key(&state.symbol))
        .filter(|state| !printed_candidate_symbols.contains(&state.symbol))
        .filter(|state| {
            state.reasoning_profile.primary_mechanism.is_some()
                || state.backward.is_some()
                || state.track.is_some()
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|left, right| {
        right
            .setup
            .confidence
            .cmp(&left.setup.confidence)
            .then_with(|| right.signal.composite.cmp(&left.signal.composite))
            .then_with(|| left.symbol.0.cmp(&right.symbol.0))
    });
    ranked.into_iter().next()
}

fn candidate_reason(state: &ReplaySymbolState) -> String {
    if matches!(state.setup.action, TacticalAction::Enter) {
        if state.reasoning_profile.primary_mechanism.is_none() {
            return "setup wants enter but no primary mechanism was resolved".into();
        }
        if matches!(
            state.track.as_ref().map(|track| track.status),
            Some(HypothesisTrackStatus::Weakening | HypothesisTrackStatus::Invalidated)
        ) {
            return "setup wants enter but hypothesis track is not healthy".into();
        }
        if let Some(backward) = state.backward.as_ref() {
            if matches!(
                backward.contest_state,
                eden::CausalContestState::Flipped
                    | eden::CausalContestState::Contested
                    | eden::CausalContestState::Eroding
            ) {
                return format!(
                    "setup wants enter but backward contest is {}",
                    backward.contest_state
                );
            }
            if backward.leading_cause_streak < 2 {
                return format!(
                    "setup wants enter but leading cause streak is only {}",
                    backward.leading_cause_streak
                );
            }
        }
        return "setup is enter-capable under current Eden reasoning".into();
    }

    let mut blockers = Vec::new();
    blockers.push(format!("setup action is `{}`", state.setup.action));
    if let Some(track) = &state.track {
        blockers.push(format!("track={}", track.status));
    }
    if let Some(backward) = &state.backward {
        blockers.push(format!("contest={}", backward.contest_state));
    }
    if let Some(primary) = &state.reasoning_profile.primary_mechanism {
        blockers.push(format!("mechanism={}", primary.label));
    }
    blockers.join(", ")
}

fn raw_source_is_interesting(item: &RawSourceExport) -> bool {
    !item.summary.starts_with("no recent")
}

fn append_replay_raw_sources(
    path: &str,
    kind: &str,
    tick: u64,
    symbol: &Symbol,
    reason: &str,
    raw_events: &RawEventStore,
    store: &ObjectStore,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let sources = raw_events
        .export_longport_sources(
            symbol,
            eden::pipeline::raw_events::RawQueryWindow::Recent(5),
            store,
        )
        .into_iter()
        .filter(raw_source_is_interesting)
        .collect::<Vec<_>>();
    if sources.is_empty() {
        return Ok(());
    }

    let line = serde_json::json!({
        "tick": tick,
        "kind": kind,
        "symbol": symbol.0,
        "reason": reason,
        "raw_sources": sources,
    });

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{}", serde_json::to_string(&line)?)?;
    Ok(())
}

fn print_replay_raw_sources(symbol: &Symbol, raw_events: &RawEventStore, store: &ObjectStore) {
    let selected = raw_events
        .export_longport_sources(
            symbol,
            eden::pipeline::raw_events::RawQueryWindow::Recent(5),
            store,
        )
        .into_iter()
        .filter(raw_source_is_interesting)
        .take(4)
        .collect::<Vec<_>>();
    if selected.is_empty() {
        return;
    }
    println!("[RawSources]");
    for item in selected {
        println!("  {} => {}", item.source, item.summary);
    }
}

fn print_entry_chain(
    tick: u64,
    state: &ReplaySymbolState,
    entry_price: Decimal,
    reason: &str,
    raw_events: &RawEventStore,
    store: &ObjectStore,
) {
    println!();
    println!("=== ENTRY {} @ tick {} ===", state.symbol, tick);
    println!("[Observation]");
    println!(
        "  depth={:+.3} flow={:+.3} volume={:+.3} candle={:+.3}",
        state.signal.volume_profile,
        state.signal.capital_flow_direction,
        state.signal.volume_profile,
        state.signal.price_momentum,
    );
    if let Some(pressure) = &state.pressure {
        println!(
            "  pressure={:+.3} delta={:+.3} duration={} accelerating={}",
            pressure.capital_flow_pressure,
            pressure.pressure_delta,
            pressure.pressure_duration,
            pressure.accelerating
        );
    }
    println!("[Convergence]");
    println!(
        "  composite={:+.3} sector={:+?} cross_stock={:+?}",
        state.signal.composite, state.signal.sector_coherence, state.signal.cross_stock_correlation,
    );
    println!("[Hypothesis]");
    if let Some(track) = &state.track {
        println!(
            "  action={} status={} streak={} conf={:+.3} gap={:+.3}",
            state.setup.action,
            track.status,
            track.status_streak,
            state.setup.confidence,
            state.setup.confidence_gap,
        );
        println!("  policy_reason={}", track.policy_reason);
        if let Some(reason) = &track.transition_reason {
            println!("  transition_reason={}", reason);
        }
    } else {
        println!(
            "  action={} conf={:+.3} gap={:+.3}",
            state.setup.action, state.setup.confidence, state.setup.confidence_gap
        );
    }
    print_policy_verdict(&state.setup);
    println!("[Mechanism]");
    if let Some(primary) = &state.reasoning_profile.primary_mechanism {
        println!("  primary={} score={:+.3}", primary.label, primary.score);
        if !primary.invalidation.is_empty() {
            println!("  invalidation={}", primary.invalidation.join(" | "));
        }
    } else {
        println!("  primary=none");
    }
    println!("[Backward]");
    if let Some(backward) = &state.backward {
        let leading = backward
            .leading_cause
            .as_ref()
            .map(|cause| cause.explanation.as_str())
            .unwrap_or("-");
        println!(
            "  contest={} streak={} leading={}",
            backward.contest_state, backward.leading_cause_streak, leading
        );
    } else {
        println!("  contest=none");
    }
    println!("[Decision]");
    println!(
        "  enter_price={} reason={}",
        entry_price.round_dp(4),
        reason
    );
    print_replay_raw_sources(&state.symbol, raw_events, store);
}

fn print_candidate_chain(
    tick: u64,
    state: &ReplaySymbolState,
    raw_events: &RawEventStore,
    store: &ObjectStore,
) {
    println!();
    println!("=== CANDIDATE {} @ tick {} ===", state.symbol, tick);
    println!("[Observation]");
    println!(
        "  depth={:+.3} flow={:+.3} volume={:+.3} candle={:+.3}",
        state.signal.volume_profile,
        state.signal.capital_flow_direction,
        state.signal.volume_profile,
        state.signal.price_momentum,
    );
    if let Some(pressure) = &state.pressure {
        println!(
            "  pressure={:+.3} delta={:+.3} duration={} accelerating={}",
            pressure.capital_flow_pressure,
            pressure.pressure_delta,
            pressure.pressure_duration,
            pressure.accelerating
        );
    }
    println!("[Convergence]");
    println!(
        "  composite={:+.3} sector={:+?} cross_stock={:+?}",
        state.signal.composite, state.signal.sector_coherence, state.signal.cross_stock_correlation,
    );
    println!("[Hypothesis]");
    if let Some(track) = &state.track {
        println!(
            "  action={} status={} streak={} conf={:+.3} gap={:+.3}",
            state.setup.action,
            track.status,
            track.status_streak,
            state.setup.confidence,
            state.setup.confidence_gap,
        );
        println!("  policy_reason={}", track.policy_reason);
        if let Some(reason) = &track.transition_reason {
            println!("  transition_reason={}", reason);
        }
    } else {
        println!(
            "  action={} conf={:+.3} gap={:+.3}",
            state.setup.action, state.setup.confidence, state.setup.confidence_gap
        );
    }
    print_policy_verdict(&state.setup);
    println!("[Mechanism]");
    if let Some(primary) = &state.reasoning_profile.primary_mechanism {
        println!("  primary={} score={:+.3}", primary.label, primary.score);
        if !primary.invalidation.is_empty() {
            println!("  invalidation={}", primary.invalidation.join(" | "));
        }
    } else {
        println!("  primary=none");
    }
    println!("[Backward]");
    if let Some(backward) = &state.backward {
        let leading = backward
            .leading_cause
            .as_ref()
            .map(|cause| cause.explanation.as_str())
            .unwrap_or("-");
        println!(
            "  contest={} streak={} leading={}",
            backward.contest_state, backward.leading_cause_streak, leading
        );
    } else {
        println!("  contest=none");
    }
    println!("[Decision]");
    println!("  candidate_reason={}", candidate_reason(state));
    print_replay_raw_sources(&state.symbol, raw_events, store);
}

fn print_exit_chain(
    tick: u64,
    state: &ReplaySymbolState,
    position: &ReplayPosition,
    exit_price: Decimal,
    return_pct: Decimal,
    reason: &str,
    raw_events: &RawEventStore,
    store: &ObjectStore,
) {
    println!();
    println!("=== EXIT {} @ tick {} ===", state.symbol, tick);
    println!(
        "  setup={} direction={} entry_tick={} entry={} exit={} pnl={:+.4}%",
        position.setup_id,
        direction_label(position.direction),
        position.entry_tick,
        position.entry_price.round_dp(4),
        exit_price.round_dp(4),
        return_pct * Decimal::new(100, 0),
    );
    if let Some(primary) = &position.primary_mechanism {
        println!("  primary_mechanism={}", primary);
    }
    if let Some(driver) = &position.primary_driver {
        println!("  primary_driver={}", driver);
    }
    println!("  exit_reason={}", reason);
    print_replay_raw_sources(&state.symbol, raw_events, store);
}

fn print_policy_verdict(setup: &TacticalSetup) {
    let Some(verdict) = &setup.policy_verdict else {
        return;
    };
    println!("[Verdict]");
    println!(
        "  primary={} rationale={}",
        verdict.primary, verdict.rationale
    );
    if let Some(conflict) = &verdict.conflict_reason {
        println!("  conflict_reason={}", conflict);
    }
    if !verdict.horizons.is_empty() {
        let summary = verdict
            .horizons
            .iter()
            .map(|item| format!("{}:{} ({})", item.horizon, item.verdict, item.rationale))
            .collect::<Vec<_>>()
            .join(" | ");
        println!("  horizons={}", summary);
    }
}

struct ReplayRegressionFeed {
    tick: u64,
    reasoning_counts: String,
    convergence_lines: Vec<String>,
    event_lines: Vec<String>,
    derived_signal_lines: Vec<String>,
    symbol_lines: Vec<String>,
    open_position_lines: Vec<String>,
    closed_position_lines: Vec<String>,
}

impl ReplayRegressionFeed {
    fn canonical_text(&self) -> String {
        let mut lines = vec![format!("tick={}", self.tick), self.reasoning_counts.clone()];

        let mut convergence_lines = self.convergence_lines.clone();
        convergence_lines.sort();
        lines.extend(convergence_lines);

        let mut event_lines = self.event_lines.clone();
        event_lines.sort();
        lines.extend(event_lines);

        let mut derived_signal_lines = self.derived_signal_lines.clone();
        derived_signal_lines.sort();
        lines.extend(derived_signal_lines);

        let mut symbol_lines = self.symbol_lines.clone();
        symbol_lines.sort();
        lines.extend(symbol_lines);

        let mut open_position_lines = self.open_position_lines.clone();
        open_position_lines.sort();
        lines.extend(open_position_lines);

        let mut closed_position_lines = self.closed_position_lines.clone();
        closed_position_lines.sort();
        lines.extend(closed_position_lines);

        lines.join("\n")
    }
}

struct ReplayRegressionReport {
    hasher: Sha256,
    ticks: usize,
}

impl ReplayRegressionReport {
    fn new() -> Self {
        Self {
            hasher: Sha256::new(),
            ticks: 0,
        }
    }

    fn record_feed(&mut self, feed: &ReplayRegressionFeed) {
        let signature = feed.canonical_text();
        self.hasher.update(signature.as_bytes());
        self.hasher.update(b"\n");
        self.ticks += 1;
    }

    fn finish(self) -> String {
        let digest = self.hasher.finalize();
        let mut hex = String::with_capacity(digest.len() * 2);
        use std::fmt::Write as _;
        for byte in digest {
            let _ = write!(&mut hex, "{:02x}", byte);
        }
        format!("sha256:{} ticks={}", hex, self.ticks)
    }
}

fn build_replay_regression_feed(
    tick: u64,
    convergence_scores: &HashMap<Symbol, ConvergenceScore>,
    event_snapshot: &EventSnapshot,
    derived_signal_snapshot: &DerivedSignalSnapshot,
    reasoning_snapshot: &ReasoningSnapshot,
    symbol_states: &HashMap<Symbol, ReplaySymbolState>,
    positions: &HashMap<Symbol, ReplayPosition>,
    closed_positions: &[ClosedReplayPosition],
) -> ReplayRegressionFeed {
    ReplayRegressionFeed {
        tick,
        reasoning_counts: format!(
            "reasoning_counts|hypotheses={}|propagation_paths={}|investigation_selections={}|tactical_setups={}|hypothesis_tracks={}|case_clusters={}",
            reasoning_snapshot.hypotheses.len(),
            reasoning_snapshot.propagation_paths.len(),
            reasoning_snapshot.investigation_selections.len(),
            reasoning_snapshot.tactical_setups.len(),
            reasoning_snapshot.hypothesis_tracks.len(),
            reasoning_snapshot.case_clusters.len(),
        ),
        convergence_lines: convergence_scores
            .iter()
            .map(|(symbol, score)| {
                format!(
                    "convergence|{}|institutional_alignment={}|sector_coherence={}|cross_stock_correlation={}|composite={}|edge_stability={}|institutional_edge_age={}|new_edge_fraction={}|microstructure_confirmation={}|component_spread={}|temporal_weight={}",
                    symbol.0,
                    fmt_decimal(score.institutional_alignment),
                    fmt_opt_decimal(score.sector_coherence),
                    fmt_decimal(score.cross_stock_correlation),
                    fmt_decimal(score.composite),
                    fmt_opt_decimal(score.edge_stability),
                    fmt_opt_decimal(score.institutional_edge_age),
                    fmt_opt_decimal(score.new_edge_fraction),
                    fmt_opt_decimal(score.microstructure_confirmation),
                    fmt_opt_decimal(score.component_spread),
                    fmt_opt_decimal(score.temporal_weight),
                )
            })
            .collect(),
        event_lines: event_snapshot
            .events
            .iter()
            .map(|event| {
                format!(
                    "event|{:?}|{:?}|{}|{}",
                    event.value.kind,
                    event.value.scope,
                    sanitize_line(&event.value.summary),
                    fmt_decimal(event.value.magnitude),
                )
            })
            .collect(),
        derived_signal_lines: derived_signal_snapshot
            .signals
            .iter()
            .map(|signal| {
                format!(
                    "derived|{:?}|{:?}|{}|{}",
                    signal.value.kind,
                    signal.value.scope,
                    sanitize_line(&signal.value.summary),
                    fmt_decimal(signal.value.strength),
                )
            })
            .collect(),
        symbol_lines: symbol_states
            .iter()
            .map(|(symbol, state)| {
                let primary = state
                    .reasoning_profile
                    .primary_mechanism
                    .as_ref()
                    .map(|item| format!("{}@{}", sanitize_line(&item.label), fmt_decimal(item.score)))
                    .unwrap_or_else(|| "-".into());
                let invalidation = state
                    .reasoning_profile
                    .automated_invalidations
                    .first()
                    .map(|item| sanitize_line(&item.reason))
                    .unwrap_or_else(|| "-".into());
                let track = state
                    .track
                    .as_ref()
                    .map(|track| {
                        format!(
                            "{}|streak={}|conf={}|gap={}|reason={}",
                            track.status,
                            track.status_streak,
                            fmt_decimal(track.confidence),
                            fmt_decimal(track.confidence_gap),
                            sanitize_line(&track.policy_reason),
                        )
                    })
                    .unwrap_or_else(|| "-".into());
                let _backward = state
                    .backward
                    .as_ref()
                    .map(|item| {
                        let leading = item
                            .leading_cause
                            .as_ref()
                            .map(|cause| sanitize_line(&cause.explanation))
                            .unwrap_or_else(|| "-".into());
                        format!(
                            "{}|streak={}|leading={}",
                            item.contest_state, item.leading_cause_streak, leading
                        )
                    })
                    .unwrap_or_else(|| "-".into());
                let pressure = state
                    .pressure
                    .as_ref()
                    .map(|item| {
                        format!(
                            "pressure={}|delta={}|duration={}|accelerating={}",
                            fmt_decimal(item.capital_flow_pressure),
                            fmt_decimal(item.pressure_delta),
                            item.pressure_duration,
                            item.accelerating
                        )
                    })
                    .unwrap_or_else(|| "-".into());

                format!(
                    "symbol|{}|setup={}@{}|gap={}|edge={}|signal={}@{}|pressure={}|track={}|primary={}|invalid={}",
                    symbol.0,
                    sanitize_line(state.setup.action.as_str()),
                    fmt_decimal(state.setup.confidence),
                    fmt_decimal(state.setup.confidence_gap),
                    fmt_decimal(state.setup.heuristic_edge),
                    fmt_decimal(state.signal.composite),
                    fmt_decimal(state.signal.price_momentum),
                    pressure,
                    track,
                    primary,
                    invalidation,
                )
            })
            .collect(),
        open_position_lines: positions
            .iter()
            .map(|(symbol, position)| {
                format!(
                    "open|{}|setup={}|dir={}|entry_tick={}|entry={}|primary_mech={}|primary_driver={}",
                    symbol.0,
                    sanitize_line(&position.setup_id),
                    position.direction,
                    position.entry_tick,
                    fmt_decimal(position.entry_price),
                    position
                        .primary_mechanism
                        .as_deref()
                        .map(sanitize_line)
                        .unwrap_or_else(|| "-".into()),
                    position
                        .primary_driver
                        .as_deref()
                        .map(sanitize_line)
                        .unwrap_or_else(|| "-".into()),
                )
            })
            .collect(),
        closed_position_lines: closed_positions
            .iter()
            .map(|position| {
                format!(
                    "closed|{}|setup={}|entry_tick={}|exit_tick={}|dir={}|entry={}|exit={}|return={}|primary_mech={}|primary_driver={}|reason={}",
                    sanitize_line(&position.symbol),
                    sanitize_line(&position.setup_id),
                    position.entry_tick,
                    position.exit_tick,
                    sanitize_line(&position.direction),
                    fmt_decimal(position.entry_price),
                    fmt_decimal(position.exit_price),
                    fmt_decimal(position.return_pct),
                    position
                        .primary_mechanism
                        .as_deref()
                        .map(sanitize_line)
                        .unwrap_or_else(|| "-".into()),
                    position
                        .primary_driver
                        .as_deref()
                        .map(sanitize_line)
                        .unwrap_or_else(|| "-".into()),
                    sanitize_line(&position.exit_reason),
                )
            })
            .collect(),
    }
}

fn fmt_decimal(value: Decimal) -> String {
    value.round_dp(6).to_string()
}

fn fmt_opt_decimal(value: Option<Decimal>) -> String {
    value.map(fmt_decimal).unwrap_or_else(|| "-".into())
}

fn sanitize_line(value: &str) -> String {
    value
        .replace('\n', " ")
        .replace('\r', " ")
        .replace('|', " ")
}

async fn load_archives(
    args: &[String],
    parquet_dir: Option<&str>,
    limit: Option<usize>,
    market_filter: Option<&str>,
) -> Result<Vec<TickArchive>, Box<dyn std::error::Error + Send + Sync>> {
    if let Some(dir) = parquet_dir {
        // ── Parquet/JSON mode ──
        println!("=== Eden Replay (Parquet) ===");
        println!("dir:     {dir}");
        let today = chrono::Local::now().format("%Y-%m-%d").to_string();
        let session_dir = std::path::Path::new(dir).join(&today);
        let scan_dir = if session_dir.is_dir() {
            println!("date:    {today}");
            session_dir
        } else {
            println!("date:    (using dir directly)");
            std::path::Path::new(dir).to_path_buf()
        };

        let mut json_files: Vec<String> = std::fs::read_dir(&scan_dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().to_string())
            .filter(|name| name.starts_with("tick_") && name.ends_with(".json"))
            .collect();
        json_files.sort();

        if let Some(max) = limit {
            json_files.truncate(max);
        }

        println!("files:   {}", json_files.len());
        println!();

        let mut archives = Vec::with_capacity(json_files.len());
        for file_name in &json_files {
            let path = scan_dir.join(file_name);
            let content = std::fs::read_to_string(&path)?;
            let archive: TickArchive = serde_json::from_str(&content)?;
            if archive_matches_market(&archive.market, market_filter.as_deref()) {
                archives.push(archive);
            }
        }

        Ok(archives)
    } else {
        // ── SurrealDB mode ──
        #[cfg(feature = "persistence")]
        {
            let chunk_size: usize = parse_flag(args, "--chunk-size").unwrap_or(100).max(1);
            let db_path = parse_flag_str(args, "--db").unwrap_or_else(|| "data/eden.db".into());

            println!("=== Eden Replay (SurrealDB) ===");
            println!("db:      {db_path}");
            if let Some(market) = market_filter {
                println!("market:  {market}");
            }
            println!();

            println!("Opening store...");
            let store = EdenStore::open(&db_path).await?;
            println!("Store opened. Streaming archives...");

            let mut archives = Vec::new();
            let mut next_after_cursor: Option<(String, u64)> = None;
            let mut remaining = limit;

            loop {
                let batch_limit = remaining
                    .map(|left| left.min(chunk_size))
                    .unwrap_or(chunk_size);
                if batch_limit == 0 {
                    break;
                }
                let batch: Vec<TickArchive> = store
                    .replay_market_tick_archives_after_cursor(
                        market_filter.as_deref(),
                        next_after_cursor
                            .as_ref()
                            .map(|(market, tick)| (market.as_str(), *tick)),
                        batch_limit,
                    )
                    .await?;
                if batch.is_empty() {
                    break;
                }
                next_after_cursor = batch.last().map(|a| {
                    let market = if a.market.trim().is_empty() {
                        "unknown".to_string()
                    } else {
                        a.market.clone()
                    };
                    (market, a.tick_number)
                });
                if let Some(left) = remaining.as_mut() {
                    *left = left.saturating_sub(batch.len());
                }
                archives.extend(batch);
            }

            Ok(archives)
        }

        #[cfg(not(feature = "persistence"))]
        {
            let _ = args;
            eprintln!("No --parquet dir specified and persistence feature is not enabled.");
            eprintln!("Use: --parquet <dir>  or  --features persistence --db <path>");
            std::process::exit(1);
        }
    }
}

fn archive_matches_market(archive_market: &str, market_filter: Option<&str>) -> bool {
    market_filter
        .map(|market| {
            let archive_market = archive_market.trim();
            archive_market.is_empty() || archive_market == "unknown" || archive_market == market
        })
        .unwrap_or(true)
}

fn parse_flag<T: std::str::FromStr>(args: &[String], name: &str) -> Option<T> {
    args.windows(2)
        .find(|window| window[0] == name)
        .and_then(|window| window[1].parse().ok())
}

fn parse_flag_str(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find(|window| window[0] == name)
        .map(|window| window[1].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_feed() -> ReplayRegressionFeed {
        ReplayRegressionFeed {
            tick: 42,
            reasoning_counts: "reasoning_counts|hypotheses=1|propagation_paths=0|investigation_selections=1|tactical_setups=1|hypothesis_tracks=1|case_clusters=0".into(),
            convergence_lines: vec![
                "convergence|700.HK|institutional_alignment=0.1|sector_coherence=0.2|cross_stock_correlation=0.3|composite=0.4|edge_stability=-|institutional_edge_age=-|new_edge_fraction=-|microstructure_confirmation=-|component_spread=-|temporal_weight=-".into(),
                "convergence|9988.HK|institutional_alignment=-0.1|sector_coherence=-0.2|cross_stock_correlation=-0.3|composite=-0.4|edge_stability=-|institutional_edge_age=-|new_edge_fraction=-|microstructure_confirmation=-|component_spread=-|temporal_weight=-".into(),
            ],
            event_lines: vec![
                "event|CompositeAcceleration|Symbol(Symbol(\"700.HK\"))|momentum is building|0.12".into(),
                "event|MarketStressElevated|Market|stress elevated|0.8".into(),
            ],
            derived_signal_lines: vec![
                "derived|StructuralComposite|Symbol(Symbol(\"700.HK\"))|composite rising|0.55".into(),
            ],
            symbol_lines: vec![
                "symbol|700.HK|setup=enter@0.6|gap=0.1|edge=0.05|signal=0.4@0.2|pressure=pressure=0.3|delta=0.1|duration=4|accelerating=true|track=-|primary=Momentum@0.7|invalid=none".into(),
                "symbol|9988.HK|setup=observe@0.4|gap=0.2|edge=0.03|signal=-0.4@-0.1|pressure=-|track=-|primary=-|invalid=-".into(),
            ],
            open_position_lines: vec![
                "open|700.HK|setup=setup:700:enter|dir=1|entry_tick=42|entry=10.5|primary_mech=Momentum|primary_driver=trend".into(),
            ],
            closed_position_lines: vec![
                "closed|9988.HK|setup=setup:9988:observe|entry_tick=21|exit_tick=42|dir=neutral|entry=18|exit=17.5|return=-0.027778|primary_mech=-|primary_driver=-|reason=policy exit".into(),
            ],
        }
    }

    #[test]
    fn canonical_text_is_order_independent() {
        let feed_a = sample_feed();
        let mut feed_b = sample_feed();
        feed_b.convergence_lines.reverse();
        feed_b.event_lines.reverse();
        feed_b.derived_signal_lines.reverse();
        feed_b.symbol_lines.reverse();
        feed_b.open_position_lines.reverse();
        feed_b.closed_position_lines.reverse();

        assert_eq!(feed_a.canonical_text(), feed_b.canonical_text());

        let mut report_a = ReplayRegressionReport::new();
        report_a.record_feed(&feed_a);
        let mut report_b = ReplayRegressionReport::new();
        report_b.record_feed(&feed_b);
        assert_eq!(report_a.finish(), report_b.finish());
    }

    #[test]
    fn canonical_text_changes_on_semantic_drift() {
        let feed_a = sample_feed();
        let mut feed_b = sample_feed();
        feed_b.symbol_lines[0] = feed_b.symbol_lines[0].replace("Momentum@0.7", "Momentum@0.8");

        assert_ne!(feed_a.canonical_text(), feed_b.canonical_text());

        let mut report_a = ReplayRegressionReport::new();
        report_a.record_feed(&feed_a);
        let mut report_b = ReplayRegressionReport::new();
        report_b.record_feed(&feed_b);
        assert_ne!(report_a.finish(), report_b.finish());
    }

    #[test]
    fn tick_number_participates_in_signature() {
        let feed_a = sample_feed();
        let mut feed_b = sample_feed();
        feed_b.tick = 43;

        assert_ne!(feed_a.canonical_text(), feed_b.canonical_text());
    }
}
