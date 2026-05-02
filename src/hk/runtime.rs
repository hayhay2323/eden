use super::watchlist::WATCHLIST;
use crate as eden;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Instant;

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
use crate::core::runtime_loop::TickState;
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
use crate::ontology::links::LinkSnapshot;
use crate::ontology::objects::{BrokerId, Symbol};
use crate::ontology::reasoning::HypothesisTrack;
use crate::ontology::snapshot::{self, RawSnapshot};
use crate::ontology::store;
use crate::ontology::{TacticalAction, TacticalSetup};
use crate::persistence::action_workflow::{ActionWorkflowEventRecord, ActionWorkflowRecord};
#[cfg(feature = "persistence")]
use crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord;
use crate::pipeline::dimensions::DimensionSnapshot;
use crate::pipeline::reasoning::{path_has_family, path_is_mixed_multi_hop, ReasoningSnapshot};
use crate::pipeline::signals::{
    DerivedSignalSnapshot, EventSnapshot, MarketEventKind, ObservationSnapshot, SignalScope,
};
use crate::pipeline::tension::TensionSnapshot;
use crate::pipeline::world::{derive_with_backward_confirmation, WorldSnapshots};
use crate::temporal::analysis::compute_dynamics;
use crate::temporal::buffer::TickHistory;
use crate::temporal::causality::{compute_causal_timelines, CausalTimeline};
use crate::temporal::lineage::compute_case_realized_outcomes_adaptive;
use crate::temporal::lineage::{compute_family_context_outcomes, compute_lineage_stats};
use crate::temporal::record::TickRecord;
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
use crate::persistence::hypothesis_track::HypothesisTrackRecord;
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

pub async fn run() {
    #[allow(unused_mut)]
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
        bridge_service: _bridge_service,
        analyst_service,
        bridge_snapshot_path,
        mut runtime,
        mut push_rx,
        mut rest_rx,
        pressure_event_bus,
        mut tick,
        debounce,
        mut bootstrap_pending,
        mut previous_symbol_states,
        mut lineage_accumulator,
        mut lineage_prev_resolved,
        mut eden_ledger,
    } = initialize_hk_runtime().await;
    // Seen-set for outcome_feedback: lineage stream re-emits the same
    // resolved outcome every tick while it stays inside the lookback
    // window. EdenLedgerAccumulator dedupes via HashMap; outcome_feedback
    // (which actually writes to IntentBeliefField) needs its own
    // setup_id dedup or it would over-credit ~LINEAGE_WINDOW times per
    // resolution.
    #[cfg(feature = "persistence")]
    let mut outcome_credited_setup_ids: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    // Broker backward pass: snapshot bid/ask broker IDs at setup entry
    // so winning outcomes can credit the right-side brokers with an
    // archetype sample. In-memory only — orphans on restart, fine
    // because horizons are session-local.
    let mut broker_entry_snapshots: std::collections::HashMap<
        String,
        eden::pipeline::broker_outcome_feedback::BrokerEntrySnapshot,
    > = std::collections::HashMap::new();
    #[cfg(feature = "persistence")]
    let mut broker_credited_setup_ids: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    // Cluster / world persistent state lives in-memory across ticks so
    // age_ticks, state_persistence_ticks and trend can accumulate without
    // a new persistence table. Reseed empty at startup — symbol-level
    // persistence already handles cross-restart continuity, and cluster /
    // world state converges within a few ticks from symbol states alone.
    let mut previous_cluster_states: Vec<eden::live_snapshot::LiveClusterState> = Vec::new();
    let mut previous_world_summary: Option<eden::live_snapshot::LiveWorldSummary> = None;
    // HK SignalMomentumTracker — in-memory sequence of per-tick
    // institutional_flow / depth_imbalance / trade_aggression so
    // second-derivative health ("signal peaking") can be read against a
    // 10-tick window. Not persisted yet (parallel to cluster/world state
    // in-memory-only pattern).
    let mut hk_momentum = eden::temporal::lineage::HkSignalMomentumTracker::default();
    // Y#7 first pass — market-level wave tracker. Aggregates tick-scale
    // event counts (Y#3 demotions, Y#6 errors, HK momentum collapses)
    // across a 10-tick window so the operator surface can flag when
    // small-scale events are themselves accelerating.
    let mut market_waves = eden::temporal::lineage::MarketWaveTracker::default();
    // Y#1 first pass — raw broker / depth trackers. These do NOT aggregate
    // into scalars; they keep per-broker-identity and per-depth-level
    // sequences so evaluate_raw_expectations can judge "did my continuation
    // thesis match raw microstructure this tick?" at the identity layer.
    // In-memory only for now (parallel to cluster/world state and
    // HkSignalMomentumTracker pattern).
    let mut raw_broker_presence = eden::pipeline::raw_expectation::RawBrokerPresence::default();
    let mut raw_depth_levels = eden::pipeline::raw_expectation::RawDepthLevels::default();
    let mut raw_trade_tape = eden::pipeline::raw_expectation::RawTradeTape::default();
    // Per-symbol sub-KG with max-granularity typed nodes. Mirrors raw
    // microstructure (price + 10 bid/ask levels + brokers) into a graph
    // structure that downstream cluster_sync detection can read.
    // No direction inference, no thresholds — pure data faithful mirror.
    let mut subkg_registry = eden::pipeline::symbol_sub_kg::SubKgRegistry::new();
    let mut subkg_snapshot_tick: u64 = 0;

    // 2026-04-29 Phase A: NDJSON artifact writers run on background
    // tokio tasks so the tick body never blocks on file IO. Each writer
    // owns a bounded mpsc; on backpressure the new batch is dropped and
    // a counter increments. See src/core/ndjson_writer.rs.
    let bp_marginals_writer = eden::core::ndjson_writer::NdjsonWriter::<
        Vec<eden::pipeline::loopy_bp::MarginalRow>,
    >::spawn("hk:bp_marginals", |rows: Vec<
        eden::pipeline::loopy_bp::MarginalRow,
    >| eden::pipeline::loopy_bp::write_marginals("hk", &rows));
    let bp_message_trace_writer = eden::core::ndjson_writer::NdjsonWriter::<
        Vec<eden::pipeline::loopy_bp::BpMessageTraceRow>,
    >::spawn("hk:bp_message_trace", |rows: Vec<
        eden::pipeline::loopy_bp::BpMessageTraceRow,
    >| eden::pipeline::loopy_bp::write_message_trace("hk", &rows));
    let subkg_writer = eden::core::ndjson_writer::NdjsonWriter::<Vec<String>>::spawn(
        "hk:subkg",
        |lines: Vec<String>| {
            eden::pipeline::symbol_sub_kg::append_subkg_lines_to_ndjson("hk", &lines)
        },
    );
    let sector_subkg_writer = eden::core::ndjson_writer::NdjsonWriter::<Vec<String>>::spawn(
        "hk:sector_subkg",
        |lines: Vec<String>| {
            eden::pipeline::sector_sub_kg::append_sector_subkg_lines_to_ndjson("hk", &lines)
        },
    );
    let visual_frame_writer = eden::core::ndjson_writer::NdjsonWriter::<
        eden::pipeline::visual_graph_frame::VisualGraphFrame,
    >::spawn("hk:visual_graph_frame", |frame: eden::pipeline::visual_graph_frame::VisualGraphFrame| {
        eden::pipeline::visual_graph_frame::write_frame("hk", &frame)
    });
    let temporal_delta_writer = eden::core::ndjson_writer::NdjsonWriter::<
        eden::pipeline::temporal_graph_delta::TemporalGraphDelta,
    >::spawn("hk:temporal_delta", |delta: eden::pipeline::temporal_graph_delta::TemporalGraphDelta| {
        eden::pipeline::temporal_graph_delta::write_delta("hk", &delta)
    });
    let cross_sector_writer = eden::core::ndjson_writer::NdjsonWriter::<
        Vec<eden::pipeline::cross_sector_contrast::SectorContrastEvent>,
    >::spawn("hk:cross_sector", |events: Vec<
        eden::pipeline::cross_sector_contrast::SectorContrastEvent,
    >| eden::pipeline::cross_sector_contrast::write_events("hk", &events));
    let sector_to_symbol_writer = eden::core::ndjson_writer::NdjsonWriter::<
        Vec<eden::pipeline::sector_to_symbol_propagation::MemberLagEvent>,
    >::spawn("hk:sector_to_symbol", |events: Vec<
        eden::pipeline::sector_to_symbol_propagation::MemberLagEvent,
    >| eden::pipeline::sector_to_symbol_propagation::write_events("hk", &events));
    let sector_kinematics_writer = eden::core::ndjson_writer::NdjsonWriter::<
        Vec<eden::pipeline::sector_kinematics::SectorKinematicsEvent>,
    >::spawn("hk:sector_kinematics", |events: Vec<
        eden::pipeline::sector_kinematics::SectorKinematicsEvent,
    >| eden::pipeline::sector_kinematics::write_events("hk", &events));

    // Production BP substrate: event-driven async residual scheduler
    // backed by Arc<DashMap> shared graph state, with wait-free
    // posterior reads via ArcSwap. The sync + shadow substrates were
    // deleted 2026-04-29 once the event substrate's per-tick fixpoint
    // semantics were restored (inbox-clear on prior change).
    let belief_substrate: std::sync::Arc<dyn eden::pipeline::event_driven_bp::BeliefSubstrate> =
        std::sync::Arc::new(
            eden::pipeline::event_driven_bp::EventDrivenSubstrate::default(),
        );

    // 2026-04-29 Phase B + C1: pressure-event bus + per-channel
    // workers (OrderBook + Structure). See US runtime for the full
    // architecture comment. C4 fix: bus is now created in startup
    // (before push forwarder) so the upstream tap can publish even
    // when the bounded batch channel drops events; we just receive
    // it through `bootstrap.pressure_event_bus`.
    let pressure_channel_states = std::sync::Arc::new(
        eden::pipeline::pressure_events::ChannelStates::default(),
    );
    let setup_registry = std::sync::Arc::new(
        eden::pipeline::pressure_events::SetupRegistry::new(),
    );
    let pressure_aggregator = eden::pipeline::pressure_events::spawn_aggregator(
        std::sync::Arc::clone(&pressure_channel_states),
        std::sync::Arc::clone(&belief_substrate),
        std::sync::Arc::clone(&setup_registry),
    );
    let _pressure_worker_pool = eden::pipeline::pressure_events::spawn_worker_pool(
        std::sync::Arc::clone(&pressure_event_bus),
        std::sync::Arc::clone(&pressure_channel_states),
        pressure_aggregator,
    );

    let mut previous_visual_frame: Option<eden::pipeline::visual_graph_frame::VisualGraphFrame> =
        None;
    // Per-symbol prior tick top-of-book for queue stability counting
    let mut prev_top_bid: std::collections::HashMap<Symbol, rust_decimal::Decimal> =
        std::collections::HashMap::new();
    let mut prev_top_ask: std::collections::HashMap<Symbol, rust_decimal::Decimal> =
        std::collections::HashMap::new();
    let mut bid1_stable_ticks: std::collections::HashMap<Symbol, u64> =
        std::collections::HashMap::new();
    let mut ask1_stable_ticks: std::collections::HashMap<Symbol, u64> =
        std::collections::HashMap::new();
    // Per-symbol haltflag carried across ticks (sticky for the day)
    let mut halted_today: std::collections::HashSet<Symbol> = std::collections::HashSet::new();
    // Kinematics: per-symbol rolling activation histories for velocity/accel
    let mut kinematics_tracker = eden::pipeline::structural_kinematics::KinematicsTracker::new();
    // Broker cumulative presence rate (today session):
    // broker_id -> count of (symbol, tick) it appeared in top-3
    let mut broker_today_presence: std::collections::HashMap<String, u64> =
        std::collections::HashMap::new();
    // Previous-tick broker archetype posterior (for drift rate)
    let mut broker_prev_archetype: std::collections::HashMap<
        String,
        (String, rust_decimal::Decimal),
    > = std::collections::HashMap::new();
    // Persistence tracker: sustained structural salience across cycles
    let mut persistence_tracker = eden::pipeline::structural_persistence::PersistenceTracker::new();
    // Expectation tracker: Layer 5 predictive perception (FEP primitive).
    // Compares observation vs extrapolation; emits surprise events.
    let mut expectation_tracker = eden::pipeline::structural_expectation::ExpectationTracker::new();
    // Broker-level ontology-entity belief. Each broker accumulates a
    // posterior over {Accumulative, Distributive, Arbitrage, Algo,
    // Unknown}. Cross-symbol aggregation: broker identity is ontology-
    // level so a broker accumulating on one symbol is evidence about
    // its trading desk behavior generally. First piece of the work
    // that brings KG entities back into the reasoning loop.
    // Shift A: latent world state (Kalman-filtered SSM). Provides a
    // unified "what does the market look like right now" object that
    // downstream planner/causal rollout can anchor on. v1 is a 5-dim
    // linear Gaussian SSM fed from graph_insights stress aggregates.
    let mut latent_world_state = eden::pipeline::latent_world_state::LatentWorldState::new(
        eden::ontology::objects::Market::Hk,
    );
    // Shift B: structural causal model over the same 5 latent dims.
    // Replaces edge-weight propagation with true do-calculus for
    // "what if stress spikes" reasoning. v1 uses default hand-
    // specified structural equations (acyclic, linear Gaussian).
    let hk_scm = eden::pipeline::structural_causal::StructuralCausalModel::default_latent_scm();
    let mut broker_archetype_field =
        eden::pipeline::broker_archetype::BrokerArchetypeBeliefField::new(
            eden::ontology::objects::Market::Hk,
        );
    #[cfg(feature = "persistence")]
    if let Some(ref store) = runtime.store {
        match store.latest_broker_archetype_snapshot("hk").await {
            Ok(Some(snap)) => {
                match eden::persistence::broker_archetype_snapshot::restore_field(&snap) {
                    Ok(restored) => {
                        eprintln!(
                            "[broker_archetype] restored {} brokers from ts={}",
                            snap.rows.len(),
                            snap.snapshot_ts,
                        );
                        broker_archetype_field = restored;
                    }
                    Err(e) => eprintln!("[broker_archetype] restore failed: {}; starting fresh", e),
                }
            }
            Ok(None) => eprintln!("[broker_archetype] no prior snapshot; starting uninformed"),
            Err(e) => eprintln!("[broker_archetype] snapshot load failed: {}", e),
        }
    }
    let pct = Decimal::new(100, 0);
    let mut last_idle_log_at = Instant::now();
    let mut integration = integration::RuntimeIntegration::new(WATCHLIST.len());
    #[cfg(feature = "persistence")]
    if let Some(ref store) = runtime.store {
        eden::persistence::case_reasoning_assessment::backfill_doctrine_assessments(store, "hk")
            .await;
    }

    // T3 — Terrain Builder: enrich ontology from Longbridge Terminal CLI
    // (shareholders, fund holders, valuation peers, calendar, ratings).
    // US runtime has been doing this since Codex's original wiring; HK was
    // never connected even though the same CLI feeds HK symbols equally
    // well. Set EDEN_SKIP_TERRAIN=1 to bypass — trading uses pressure
    // field directly, so a session can run without it. Outside regular
    // market hours we skip because the CLI returns stale / partial data
    // and throws off attention allocation.
    let mut terrain = eden::ontology::terrain::TerrainSnapshot::default();
    let mut terrain_rx: Option<tokio::sync::mpsc::UnboundedReceiver<_>> = None;
    if std::env::var("EDEN_SKIP_TERRAIN").is_ok() {
        eprintln!("[hk] EDEN_SKIP_TERRAIN=1, skipping terrain build");
    } else if !eden::temporal::session::is_hk_regular_market_hours(time::OffsetDateTime::now_utc())
    {
        eprintln!("[hk] skipping terrain build outside regular market hours");
    } else {
        let hk_symbols: Vec<Symbol> = store.stocks.keys().cloned().collect();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        terrain_rx = Some(rx);
        tokio::spawn(async move {
            let terrain_builder = eden::ontology::terrain::TerrainBuilder::new(hk_symbols, vec![]);
            eprintln!("[hk] building terrain from Terminal CLI in background...");
            let terrain = terrain_builder.build_terrain().await;
            let terrain_peer_count: usize = terrain.peer_groups.values().map(|v| v.len()).sum();
            let terrain_holder_count = terrain.institutional_holdings.len();
            let terrain_fund_count = terrain.fund_holdings.len();
            let terrain_event_count: usize =
                terrain.upcoming_events.values().map(|v| v.len()).sum();
            eprintln!(
                "[hk] terrain built: {} peer links, {} institutions, {} funds, {} calendar events, {} ratings, {} insider records",
                terrain_peer_count,
                terrain_holder_count,
                terrain_fund_count,
                terrain_event_count,
                terrain.ratings.len(),
                terrain.insider_activity.len(),
            );
            let _ = tx.send(terrain);
        });
    }

    let mut hidden_force_state = eden::pipeline::residual::HiddenForceVerificationState::default();
    let mut edge_ledger = eden::graph::edge_learning::EdgeLearningLedger::default();
    #[cfg(feature = "persistence")]
    if let Some(ref store) = runtime.store {
        if let Ok(Some(record)) = store.load_edge_learning_ledger("hk").await {
            edge_ledger = record.into_ledger();
            eprintln!(
                "[hk] restored {} learned edges from store",
                edge_ledger.len()
            );
        }
    }
    let mut seen_hk_edge_learning_setups = HashSet::new();
    let mut energy_momentum = eden::graph::energy::EnergyMomentum::default();
    let mut pressure_field =
        eden::pipeline::pressure::PressureField::new(time::OffsetDateTime::now_utc());
    // PressureBeliefField: cross-tick persistent belief over pressure values
    // and state posterior. Restored from the latest SurrealDB snapshot if
    // one exists, otherwise starts uninformed. Graceful degrade on any
    // restore error — beliefs are rebuildable, not golden data.
    let mut belief_field =
        eden::pipeline::belief_field::PressureBeliefField::new(eden::ontology::objects::Market::Hk);
    #[cfg(feature = "persistence")]
    if let Some(ref store) = runtime.store {
        match store.latest_belief_snapshot("hk").await {
            Ok(Some(snap)) => match eden::persistence::belief_snapshot::restore_field(&snap) {
                Ok(restored) => {
                    eprintln!(
                        "[belief] restored {} gaussian, {} categorical from ts={}",
                        restored.gaussian_count(),
                        restored.categorical_count(),
                        snap.snapshot_ts,
                    );
                    belief_field = restored;
                }
                Err(e) => {
                    eprintln!("[belief] restore failed: {}; starting fresh", e);
                }
            },
            Ok(None) => eprintln!("[belief] no prior snapshot; starting uninformed"),
            Err(e) => eprintln!("[belief] snapshot load failed: {}; starting fresh", e),
        }
    }

    // KL surprise tracker — symmetric with US runtime. Always allocated;
    // dispatcher passes it through whether or not EDEN_ACTION_PROMOTION
    // is set to `kl_surprise`.
    let mut kl_surprise_tracker = eden::pipeline::kl_surprise::KlSurpriseTracker::new();

    // Sub-KG emergence tracker — symmetric with US runtime. Synthesizes
    // setups for symbols whose cross-NodeId emergence score crosses
    // their own self-referential 1σ baseline.
    let mut sub_kg_emergence_tracker =
        eden::pipeline::sub_kg_emergence::SubKgEmergenceTracker::new();
    // DecisionLedger: Eden reads Claude Code's own decision history from
    // the decisions/ tree. Startup scan builds full index; per-tick rescan
    // (every 60s, piggybacking belief snapshot cadence) picks up new files.
    let mut decision_ledger =
        eden::pipeline::decision_ledger::DecisionLedger::new(eden::ontology::objects::Market::Hk);
    {
        use std::path::Path;
        eden::pipeline::decision_ledger::scanner::scan_directory(
            Path::new("decisions"),
            &mut decision_ledger,
        );
    }
    let mut lifecycle_tracker = eden::pipeline::pressure::reasoning::LifecycleTracker::default();
    // Y#0 first piece: track vortex fingerprints across the session so
    // classifier-forced fits surface as ontology gaps. Purely observational —
    // does not change how vortices are classified today. Seeds future
    // ontology-emergence proposer.
    let mut residual_pattern_tracker =
        eden::pipeline::ontology_emergence::ResidualPatternTracker::new(
            eden::ontology::objects::Market::Hk,
        );
    // Cross-ontology intent belief: world-space categorical posterior
    // per symbol (Accumulation / Distribution / Rotation / Volatility /
    // Unknown) derived from channel pressures. Coexists with the
    // channel-space PressureBeliefField — second projection, not
    // replacement.
    let mut intent_belief_field =
        eden::pipeline::intent_belief::IntentBeliefField::new(eden::ontology::objects::Market::Hk);

    // HK learning feedback cache — refreshed periodically from persisted
    // case reasoning assessments + lineage rows. Mirrors US
    // `cached_us_learning_feedback`. Without this, HK setups never receive
    // intent/archetype/conditioned learning deltas. Persistence-only.
    #[cfg(feature = "persistence")]
    let mut cached_hk_learning_feedback: Option<
        eden::pipeline::learning_loop::ReasoningLearningFeedback,
    > = None;

    // Synthetic outcomes cache — refreshed every `SYNTHETIC_OUTCOME_REFRESH`
    // ticks from live tick history. Feeds the substrate-evidence builder
    // (V2: `build_substrate_evidence_snapshots`) which writes mean signed
    // return into `NodeId::OutcomeMemory` per symbol. BP reads it via
    // `observe_from_subkg` as part of the standard prior — no
    // post-BP modulation chain.
    let mut cached_synthetic_outcomes: Vec<eden::temporal::lineage::CaseRealizedOutcome> =
        Vec::new();
    const SYNTHETIC_OUTCOME_REFRESH: u64 = 30;

    // Previous-tick hub summaries. Carried across ticks because
    // crystallization runs AFTER the modulation block in each tick; this
    // tick's modulation can only see LAST tick's hubs. Reassigned
    // unconditionally at end of crystallization so stale hubs clear.
    let mut prev_tick_hubs: Vec<eden::pipeline::residual::HubSummary> = Vec::new();

    // Sector kinematics tracker — across-tick history of per-(sector,
    // NodeKind) mean activation. Detects sector-level turning points
    // (TopForming / BottomForming) one zoom level above per-symbol
    // structural_kinematics. Stateful across snapshot ticks.
    let mut sector_kinematics_tracker =
        eden::pipeline::sector_kinematics::SectorKinematicsTracker::new();

    // Engram-style regime analog index — deterministic O(1) lookup
    // from regime_fingerprint.bucket_key → historical visits + future
    // outcome stats (T+5 / T+30 / T+100 stress/sync/bias delta).
    // Pure structural memory, no ML / training. Reload from prior ndjson
    // logs so cross-session memory accumulates.
    let mut hk_regime_analog_index = eden::pipeline::regime_analog_index::RegimeAnalogIndex::new();
    if let Ok(n) = hk_regime_analog_index.load_from_ndjson("hk") {
        if n > 0 {
            eprintln!("[regime_analog] hk loaded {} historical records", n);
        }
    }
    // Per-symbol WL graph signature analog index — proper graph-typed
    // structural lookup ("which past symbol's sub-KG had this exact
    // h-WL signature"). Uses signature_hash from wl_graph_signature.
    let mut hk_symbol_wl_analog_index =
        eden::pipeline::symbol_wl_analog_index::SymbolWlAnalogIndex::new();

    // Lead-lag tracker — rolling time-series per symbol of composite
    // (Pressure + Intent) scalar. Cross-correlation along master KG
    // edges gives directional evidence (which symbol leads / lags).
    let mut hk_lead_lag_tracker = eden::pipeline::lead_lag_index::LeadLagTracker::new();

    // Active probe runner — counterfactual BP each tick on top-K
    // high-entropy symbols. Mirrors US.
    let mut hk_active_probe = eden::pipeline::active_probe::ActiveProbeRunner::new();
    #[cfg(feature = "persistence")]
    if let Some(ref store) = runtime.store {
        match store.latest_intent_belief_snapshot("hk").await {
            Ok(Some(snap)) => {
                match eden::persistence::intent_belief_snapshot::restore_field(&snap) {
                    Ok(restored) => {
                        eprintln!(
                            "[intent_belief] restored {} rows from ts={}",
                            snap.rows.len(),
                            snap.snapshot_ts,
                        );
                        intent_belief_field = restored;
                    }
                    Err(e) => eprintln!("[intent_belief] restore failed: {}; starting fresh", e),
                }
            }
            Ok(None) => eprintln!("[intent_belief] no prior snapshot; starting uninformed"),
            Err(e) => eprintln!("[intent_belief] snapshot load failed: {}", e),
        }
    }
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
    // Build sector_id -> name lookup for sector_intent wake emission.
    let sector_names: HashMap<eden::ontology::objects::SectorId, String> = store
        .sectors
        .iter()
        .map(|(id, sector)| (id.clone(), sector.name.clone()))
        .collect();

    // Last-write timestamp for regime_fingerprint persistence; throttled
    // to 60 seconds (mirror of belief_field snapshot cadence and US
    // runtime). Wake-line emission is per-tick.
    #[cfg(feature = "persistence")]
    let mut last_hk_regime_fp_ts: Option<chrono::DateTime<chrono::Utc>> = None;

    loop {
        // T3 — pick up terrain result from background builder when ready.
        // Match US runtime's try_recv pattern: non-blocking poll each tick,
        // drop rx once received. `terrain` stays at its default until the
        // CLI job completes.
        if let Some(rx) = terrain_rx.as_mut() {
            if let Ok(new_terrain) = rx.try_recv() {
                terrain = new_terrain;
                terrain_rx = None;
            }
        }
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
        let mut stage_timer = crate::core::runtime::TickStageTimer::new();

        if tick_advance.received_update {
            for idx in rest.calc_indexes.values() {
                if let (Some(vr), Some(tr)) = (idx.volume_ratio, idx.turnover_rate) {
                    if vr > Decimal::TWO {
                        // NOTE: `5m_chg_rate` is Longport's raw `five_minutes_change_rate`
                        // index, not a verified 5-minute price % move. Operator must NOT
                        // read "+125" as a +125% price move — it has been empirically
                        // observed to reach 100+ on low-float small caps with modest
                        // underlying price change. Use candlesticks to confirm price.
                        println!(
                            "  [VOLUME] {}  vol_ratio={:.1}x  turnover={:.2}%  5m_chg_rate={:+.2}",
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
        // 2026-04-29: collapsed double-clone. Previously `to_canonical_snapshot`
        // and `to_raw_snapshot` each cloned all 8 HashMaps (brokers, depths,
        // candlesticks, quotes, calc_indexes, capital_flows, capital_distributions,
        // intraday_lines) of 501 symbols — 2× the necessary work per tick.
        // Now build raw once, then derive canonical from raw via &self method
        // on RawSnapshot. Push channel overflow root-cause finding from
        // 2026-04-29 audit (agent aa09698316feb28c3, fix #1).
        let raw = live.to_raw_snapshot(&rest);
        let canonical_market_snapshot =
            raw.to_canonical_snapshot(MarketId::Hk, &rest.intraday_lines);

        // Show trade activity if any
        let trade_symbols: Vec<_> = raw
            .trades
            .iter()
            .filter(|(_, t)| !t.is_empty())
            .map(|(s, t)| (s.clone(), t.len(), t.iter().map(|t| t.volume).sum::<i64>()))
            .collect();

        // S01 raw microstructure feed — mirror US trade-tape ingestion, with
        // HK's broker/depth queues included. This must run before reasoning
        // snapshots broker/depth/trade evidence for current-tick setups.
        for (symbol, brokers) in &live.brokers {
            raw_broker_presence.record_tick(symbol, brokers);
        }
        broker_archetype_field.observe_tick(&raw_broker_presence);
        for (symbol, depth) in &live.depths {
            raw_depth_levels.record_tick(symbol, depth);
        }
        for (symbol, trades) in &live.trades {
            raw_trade_tape.record_tick(symbol, trades);
        }
        stage_timer.mark("S01_trade_tape_feed");
        // S02 after-hours branch — HK keeps processing live pushed state
        // outside regular session, so this is an explicit no-op branch.
        stage_timer.mark("S02_S03_canonical");

        let links = LinkSnapshot::from_canonical_market(&canonical_market_snapshot, &store);
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
        let decision = DecisionSnapshot::compute(
            &brain,
            &links,
            &active_fps,
            &store,
            Some(&temporal_ctx),
            Some(&edge_ledger),
        );

        display_hk_temporal_debug(tick, &decision, &graph_node_delta, &broker_delta);

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
        let observation_snapshot =
            ObservationSnapshot::from_canonical_market(&canonical_market_snapshot);
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
            // Residual field count + per-cluster summary wakes removed.
            // Full residual cluster data is in reasoning pipeline and
            // downstream hub/emergent-edge per-item surfaces.
            for pair in residual_field.divergent_pairs.iter().take(3) {
                eprintln!(
                    "[hk]   divergence: {} ({:+.4}) vs {} ({:+.4}) strength={:.4}",
                    pair.symbol_a.0,
                    pair.residual_a,
                    pair.symbol_b.0,
                    pair.residual_b,
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
            &mut edge_ledger,
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
        let mut vortex_insights: Vec<(
            eden::pipeline::pressure::reasoning::VortexInsight,
            eden::pipeline::pressure::PressureVortex,
        )> = Vec::new();
        for vortex in pressure_field.vortices.iter().take(10) {
            if let Some(insight) = eden::pipeline::pressure::reasoning::reason_about_vortex(
                vortex,
                &pressure_field,
                &lifecycle_tracker,
                &sector_members,
                &symbol_sector,
            ) {
                eprintln!("[hk] {}", insight.summary);
                residual_pattern_tracker.observe(
                    vortex,
                    insight.lifecycle.phase,
                    &insight.attribution.driver,
                    chrono::Utc::now(),
                );
                vortex_insights.push((insight, vortex.clone()));
            }
        }
        stage_timer.mark("S04_S06_perception_pressure");
        let mut reasoning_snapshot = ReasoningSnapshot::empty(deep_reasoning_decision.timestamp);

        // Inject vortex-derived tactical setups from pressure field WITH reasoning.
        let mut vortex_setups = eden::pipeline::pressure::bridge::insights_to_tactical_setups(
            &vortex_insights,
            deep_reasoning_decision.timestamp,
            tick,
            10,
        );

        // Closed loop step 5: refresh learning feedback (HK) from persisted
        // case assessments + lineage rows. Throttled by tick mod
        // HK_LEARNING_FEEDBACK_REFRESH_INTERVAL inside the helper.
        // Mirror of US runtime — without this HK has no learning loop.
        #[cfg(feature = "persistence")]
        if let Some(ref eden_store) = runtime.store {
            persistence::maybe_refresh_hk_learning_feedback(
                eden_store,
                &store,
                tick,
                persistence::HK_LEARNING_FEEDBACK_REFRESH_INTERVAL,
                &mut cached_hk_learning_feedback,
            )
            .await;
        }
        // Refresh synthetic-outcome cache every SYNTHETIC_OUTCOME_REFRESH
        // ticks. The full scan over LINEAGE_WINDOW is expensive for 494
        // symbols so we don't do it every tick — the slight staleness (up
        // to ~30 ticks) is acceptable for a hit-rate-scaled modulator that
        // needs ≥5 resolved samples before firing anyway.
        if tick % SYNTHETIC_OUTCOME_REFRESH == 0 {
            cached_synthetic_outcomes =
                compute_case_realized_outcomes_adaptive(&history, LINEAGE_WINDOW);
        }

        // Closed loop step 1: belief_field modulates setup.confidence.
        // V2: pre-BP modulation chain deleted. belief_field + outcome
        // history + intent + broker + sector alignment all gone — those
        // signals (the data-driven ones) now flow into BP via NodeId
        // (Belief* / OutcomeMemory). Hub note + learning feedback stay.
        for setup in vortex_setups.iter_mut() {
            if let eden::ontology::ReasoningScope::Symbol(sym) = &setup.scope {
                if let Some(hub) = prev_tick_hubs.iter().find(|h| h.symbol == *sym) {
                    setup
                        .risk_notes
                        .push(eden::pipeline::residual::hub_member_risk_note(hub));
                }
            }
            // 2026-04-29: deleted apply_feedback_to_tactical_setup —
            // a rogue 5-channel weighted modulator that overwrote BP
            // posterior with a magic constant sum (intent_delta * 0.6 +
            // archetype_delta * 0.4 + signature_delta * 0.3 +
            // violation_delta * 0.2 + conditioned_delta * 0.5). It
            // bypassed the "BP posterior is single source of truth"
            // contract and slipped through the architecture invariants
            // test because it wasn't named *_modulation. Audit finding
            // CRITICAL #1 from 2026-04-29 legacy sweep.

            // Broker backward pass: snapshot presence at setup entry.
            // Called here, not at outcome resolution, because presence
            // changes tick-by-tick — we want the entry-time picture.
            // Snapshot is idempotent per setup_id so re-ticking the
            // same setup keeps the original picture.
            if let eden::ontology::ReasoningScope::Symbol(sym) = &setup.scope {
                let should_snapshot =
                    matches!(setup.action, TacticalAction::Enter | TacticalAction::Review);
                if should_snapshot {
                    eden::pipeline::broker_outcome_feedback::snapshot_setup_brokers(
                        &setup.setup_id,
                        sym,
                        &raw_broker_presence,
                        &mut broker_entry_snapshots,
                    );
                }
            }
        }
        if !vortex_setups.is_empty() {
            // pressure→action wake removed — base=1.0 saturation made
            // conf field misleading. mod_stack line above carries the
            // real modulation story; setups flow through to reasoning.
            reasoning_snapshot.tactical_setups.extend(vortex_setups);
        }

        // Inject hidden force hypotheses from residual field
        let hidden_force_hypotheses =
            eden::pipeline::residual::infer_hidden_forces(&residual_field, decision.timestamp);
        if !hidden_force_hypotheses.is_empty() {
            // Injected hypothesis count wake removed.
            reasoning_snapshot
                .hypotheses
                .extend(hidden_force_hypotheses);
        }

        // Verify hidden forces against current residuals (tick-level outcome)
        let verification_result =
            hidden_force_state.tick(&residual_field, &reasoning_snapshot.hypotheses, tick);
        let _ = &verification_result;
        // Verification count wake removed — confirmed forces still flow
        // through crystallization + hub/attention-boost per-item wakes.
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
        #[cfg(feature = "persistence")]
        if tick % 10 == 0 {
            let record =
                eden::persistence::edge_learning_ledger::EdgeLearningLedgerRecord::from_ledger(
                    "hk",
                    &edge_ledger,
                    deep_reasoning_decision.timestamp,
                );
            runtime
                .persist_edge_learning_ledger("hk", record, i128::from(tick))
                .await;
        }

        // Crystallize confirmed forces → attention boosts + emergent paths + graph edges
        let crystallization =
            eden::pipeline::residual::crystallize_confirmed_forces(&hidden_force_state);
        if !crystallization.attention_boosts.is_empty()
            || !crystallization.emergent_paths.is_empty()
            || !crystallization.emergent_edges.is_empty()
        {
            // Crystallization aggregate count removed;
            // per-attention-boost wake also removed — boost reasons are
            // in the ndjson crystallization output and hub aggregates.
            // Inject emergent propagation paths into reasoning
            let emergent_prop_paths = eden::pipeline::residual::emergent_paths_to_propagation_paths(
                &crystallization.emergent_paths,
                decision.timestamp,
            );
            reasoning_snapshot
                .propagation_paths
                .extend(emergent_prop_paths);
            // Per-edge wake removed — emergent edges are rolled up into
            // the per-symbol hub wake below, and raw edges reach
            // BrainGraph::compute next tick.
        }
        // Hub aggregation: always runs so prev_tick_hubs clears when this
        // tick has no emergent edges (rather than keeping stale ones).
        // Wake line still only emits when hubs are present.
        let hubs = eden::pipeline::residual::aggregate_hubs(&crystallization.emergent_edges, 3);
        for hub in hubs.iter().take(5) {
            eprintln!(
                "[hk] hub: {} anticorr_degree={} corr_degree={} peers={} max_streak={} mean_strength={:.2}",
                hub.symbol.0,
                hub.anticorr_degree,
                hub.corr_degree,
                hub.peers.join(","),
                hub.max_streak,
                hub.mean_strength,
            );
        }
        prev_tick_hubs = hubs;

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
            &hk_momentum,
        );
        stage_timer.mark("S07_S13_setups_bp_hub");
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
        let causal_timelines = compute_causal_timelines(&history);
        // Feed each tick's SignalDynamics into the HK momentum tracker so we
        // can read second-derivative health (institutional flow /
        // depth_imbalance / trade aggression) in a few ticks' time.
        for dyn_entry in dynamics.values() {
            hk_momentum.record_tick(dyn_entry);
        }
        stage_timer.mark("S18_signal_momentum_feed");
        // Raw tracker ingestion moved to S01 so current-tick broker /
        // depth / trade evidence is available before setup reasoning
        // snapshots.
        // Per-symbol sub-KG update: mirror live quotes/depths/brokers into
        // typed-node graphs. Pure mechanical wiring (one Eden field → one
        // sub-KG node), no inference. Snapshot every 5 ticks to NDJSON
        // for operator inspection (.run/eden-subkg-hk.ndjson).
        {
            use eden::pipeline::symbol_sub_kg as sk;
            let now = chrono::Utc::now();
            let quotes_map: std::collections::HashMap<String, sk::QuoteData> = live
                .quotes
                .iter()
                .map(|(sym, q)| {
                    (
                        sym.0.clone(),
                        sk::QuoteData {
                            last_done: q.last_done,
                            prev_close: q.prev_close,
                            day_high: q.high,
                            day_low: q.low,
                            volume: rust_decimal::Decimal::from(q.volume),
                            turnover: q.turnover,
                        },
                    )
                })
                .collect();
            let depths_map: std::collections::HashMap<String, sk::DepthData> = live
                .depths
                .iter()
                .map(|(sym, d)| {
                    (
                        sym.0.clone(),
                        sk::DepthData {
                            bids: d
                                .bids
                                .iter()
                                .map(|l| sk::DepthLevel {
                                    price: l.price.unwrap_or(rust_decimal::Decimal::ZERO),
                                    volume: l.volume as u64,
                                })
                                .collect(),
                            asks: d
                                .asks
                                .iter()
                                .map(|l| sk::DepthLevel {
                                    price: l.price.unwrap_or(rust_decimal::Decimal::ZERO),
                                    volume: l.volume as u64,
                                })
                                .collect(),
                        },
                    )
                })
                .collect();
            let brokers_map: std::collections::HashMap<String, Vec<sk::BrokerSeat>> = live
                .brokers
                .iter()
                .map(|(sym, sb)| {
                    let mut seats = Vec::new();
                    for grp in &sb.bid_brokers {
                        if grp.position as u8 > 10 {
                            continue;
                        }
                        for &bid in &grp.broker_ids {
                            seats.push(sk::BrokerSeat {
                                broker_id: bid.to_string(),
                                side: sk::Side::Bid,
                                position: grp.position as u8,
                            });
                        }
                    }
                    for grp in &sb.ask_brokers {
                        if grp.position as u8 > 10 {
                            continue;
                        }
                        for &bid in &grp.broker_ids {
                            seats.push(sk::BrokerSeat {
                                broker_id: bid.to_string(),
                                side: sk::Side::Ask,
                                position: grp.position as u8,
                            });
                        }
                    }
                    (sym.0.clone(), seats)
                })
                .collect();
            sk::update_from_quotes_depths_brokers(
                &mut subkg_registry,
                &quotes_map,
                &depths_map,
                &brokers_map,
                now,
                tick,
            );
            // Pressure snapshot — write 6 channel pressures per symbol into
            // sub-KG Pressure nodes. Uses Tick layer (instant snapshot).
            // composite/convergence/conflict piggyback on PressureOrderBook aux.
            {
                use eden::pipeline::pressure::{PressureChannel, TimeScale};
                if let Some(layer) = pressure_field.layers.get(&TimeScale::Tick) {
                    let pressures_map: std::collections::HashMap<String, sk::PressureSnapshot> =
                        layer
                            .pressures
                            .iter()
                            .map(|(sym, np)| {
                                let net_or_zero = |c: PressureChannel| {
                                    np.channels
                                        .get(&c)
                                        .map(|cp| cp.net())
                                        .unwrap_or(rust_decimal::Decimal::ZERO)
                                };
                                (
                                    sym.0.clone(),
                                    sk::PressureSnapshot {
                                        order_book: net_or_zero(PressureChannel::OrderBook),
                                        capital_flow: net_or_zero(PressureChannel::CapitalFlow),
                                        institutional: net_or_zero(PressureChannel::Institutional),
                                        momentum: net_or_zero(PressureChannel::Momentum),
                                        volume: net_or_zero(PressureChannel::Volume),
                                        structure: net_or_zero(PressureChannel::Structure),
                                        composite: np.composite,
                                        convergence: np.convergence,
                                        conflict: np.conflict,
                                    },
                                )
                            })
                            .collect();
                    sk::update_from_pressure(&mut subkg_registry, &pressures_map, now, tick);
                }
            }
            // Intent belief snapshot — write 5 IntentMode posteriors per
            // symbol into sub-KG IntentMode nodes.
            {
                use eden::pipeline::intent_belief::IntentKind;
                let intents_map: std::collections::HashMap<String, sk::IntentSnapshot> =
                    intent_belief_field
                        .per_symbol_iter()
                        .map(|(sym, belief)| {
                            let prob = |k: IntentKind| -> rust_decimal::Decimal {
                                belief
                                    .variants
                                    .iter()
                                    .position(|v| *v == k)
                                    .map(|i| belief.probs[i])
                                    .unwrap_or(rust_decimal::Decimal::ZERO)
                            };
                            (
                                sym.0.clone(),
                                sk::IntentSnapshot {
                                    accumulation: prob(IntentKind::Accumulation),
                                    distribution: prob(IntentKind::Distribution),
                                    rotation: prob(IntentKind::Rotation),
                                    volatility: prob(IntentKind::Volatility),
                                    unknown: prob(IntentKind::Unknown),
                                    n: belief.sample_count as u64,
                                },
                            )
                        })
                        .collect();
                sk::update_from_intent(&mut subkg_registry, &intents_map, now, tick);
            }
            // Broker archetype → sub-KG Broker node attribute. Iterates
            // every Broker node across all sub-KGs and writes the
            // dominant posterior + sample count.
            {
                use eden::pipeline::broker_archetype::BrokerArchetype;
                let archetypes_map: std::collections::HashMap<String, sk::BrokerArchetypeSnapshot> =
                    broker_archetype_field
                        .per_broker_iter()
                        .filter_map(|(bid, belief)| {
                            // dominant variant: argmax probs
                            let (idx, &p) =
                                belief.probs.iter().enumerate().max_by(|(_, a), (_, b)| {
                                    a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal)
                                })?;
                            let archetype = &belief.variants[idx];
                            let label = match archetype {
                                BrokerArchetype::Accumulative => "Accumulative",
                                BrokerArchetype::Distributive => "Distributive",
                                BrokerArchetype::Arbitrage => "Arbitrage",
                                BrokerArchetype::Algo => "Algo",
                                BrokerArchetype::Unknown => "Unknown",
                            };
                            Some((
                                bid.0.to_string(),
                                sk::BrokerArchetypeSnapshot {
                                    broker_id: bid.0.to_string(),
                                    archetype_label: label.into(),
                                    posterior_prob: p,
                                    sample_count: belief.sample_count as u64,
                                },
                            ))
                        })
                        .collect();
                sk::update_from_broker_archetype(&mut subkg_registry, &archetypes_map, now, tick);
            }
            // Warrant pool (HK only) → 5 Warrant nodes per underlying
            {
                let pools_map: std::collections::HashMap<String, sk::WarrantPoolSnapshot> = rest
                    .warrants
                    .iter()
                    .map(|(sym, w)| {
                        let iv_gap = match (w.weighted_call_iv, w.weighted_put_iv) {
                            (Some(c), Some(p)) => c - p,
                            _ => rust_decimal::Decimal::ZERO,
                        };
                        (
                            sym.0.clone(),
                            sk::WarrantPoolSnapshot {
                                call_warrant_count: w.call_warrant_count as u32,
                                put_warrant_count: w.put_warrant_count as u32,
                                iv_gap,
                            },
                        )
                    })
                    .collect();
                sk::update_from_warrant_pool(&mut subkg_registry, &pools_map, now, tick);
            }
            // Capital flow → CapitalFlowCum + AccelLast30m nodes
            {
                let flows_map: std::collections::HashMap<String, sk::CapitalFlowSnapshot> = rest
                    .capital_flows
                    .iter()
                    .map(|(sym, lines)| {
                        let cum: rust_decimal::Decimal = lines.iter().map(|l| l.inflow).sum();
                        // 30-min accel: latest minus value 30 entries back
                        let n = lines.len();
                        let accel = if n >= 30 {
                            lines[n - 1].inflow - lines[n - 30].inflow
                        } else {
                            rust_decimal::Decimal::ZERO
                        };
                        (
                            sym.0.clone(),
                            sk::CapitalFlowSnapshot {
                                cumulative_inflow: cum,
                                accel_last_30m: accel,
                            },
                        )
                    })
                    .collect();
                sk::update_from_capital_flow(&mut subkg_registry, &flows_map, now, tick);
            }
            // Session phase → broadcast SessionPhase node label to all symbols
            {
                let phase = if eden::temporal::session::is_hk_regular_market_hours(
                    time::OffsetDateTime::now_utc(),
                ) {
                    "Regular"
                } else {
                    "OffHours"
                };
                sk::update_from_session_phase(&mut subkg_registry, phase, now, tick);
            }
            // Microstructure: trade tape balance + depth asymmetry from raw data
            {
                use longport::quote::TradeDirection;
                let mut micro: std::collections::HashMap<String, sk::MicrostructureSnapshot> =
                    std::collections::HashMap::new();
                // Trade tape last 30s + accel last 1m
                for (sym, trades) in raw_trade_tape.per_symbol.iter() {
                    let now_t = time::OffsetDateTime::now_utc();
                    let c30 = now_t - time::Duration::seconds(30);
                    let c60 = now_t - time::Duration::seconds(60);
                    let c120 = now_t - time::Duration::seconds(120);
                    let mut buy_vol = 0i64;
                    let mut sell_vol = 0i64;
                    let mut count_last1m = 0i64;
                    let mut count_prev1m = 0i64;
                    for t in trades {
                        if t.timestamp >= c30 {
                            match t.direction {
                                TradeDirection::Up => buy_vol += t.volume,
                                TradeDirection::Down => sell_vol += t.volume,
                                _ => {}
                            }
                        }
                        if t.timestamp >= c60 {
                            count_last1m += 1;
                        } else if t.timestamp >= c120 {
                            count_prev1m += 1;
                        }
                    }
                    let accel = rust_decimal::Decimal::from(count_last1m - count_prev1m);
                    let entry = micro.entry(sym.0.clone()).or_default();
                    entry.trade_tape_buy_minus_sell_30s =
                        rust_decimal::Decimal::from(buy_vol - sell_vol);
                    entry.trade_tape_accel_last_1m = accel;
                }
                // Depth asymmetry + queue stability + VWAP from live.depths/quotes
                for (sym, depth) in &live.depths {
                    let bid_top3: i64 = depth.bids.iter().take(3).map(|l| l.volume as i64).sum();
                    let ask_top3: i64 = depth.asks.iter().take(3).map(|l| l.volume as i64).sum();
                    let total = bid_top3 + ask_top3;
                    let asym = if total > 0 {
                        rust_decimal::Decimal::from(bid_top3) / rust_decimal::Decimal::from(total)
                    } else {
                        rust_decimal::Decimal::new(5, 1)
                    };
                    let entry = micro.entry(sym.0.clone()).or_default();
                    entry.depth_asymmetry_top3 = asym;

                    // Queue stability: compare top-of-book to prev tick
                    let cur_bid = depth
                        .bids
                        .first()
                        .and_then(|l| l.price)
                        .unwrap_or(rust_decimal::Decimal::ZERO);
                    let cur_ask = depth
                        .asks
                        .first()
                        .and_then(|l| l.price)
                        .unwrap_or(rust_decimal::Decimal::ZERO);
                    let stable_bid = if prev_top_bid.get(sym).copied() == Some(cur_bid) {
                        let n = bid1_stable_ticks.entry(sym.clone()).or_insert(0);
                        *n += 1;
                        *n
                    } else {
                        bid1_stable_ticks.insert(sym.clone(), 0);
                        0
                    };
                    let stable_ask = if prev_top_ask.get(sym).copied() == Some(cur_ask) {
                        let n = ask1_stable_ticks.entry(sym.clone()).or_insert(0);
                        *n += 1;
                        *n
                    } else {
                        ask1_stable_ticks.insert(sym.clone(), 0);
                        0
                    };
                    prev_top_bid.insert(sym.clone(), cur_bid);
                    prev_top_ask.insert(sym.clone(), cur_ask);
                    entry.queue_stability_bid1 = rust_decimal::Decimal::from(stable_bid);
                    entry.queue_stability_ask1 = rust_decimal::Decimal::from(stable_ask);
                }
                // VWAP: turnover / volume per symbol
                for (sym, q) in &live.quotes {
                    let entry = micro.entry(sym.0.clone()).or_default();
                    if q.volume > 0 {
                        let vwap = q.turnover / rust_decimal::Decimal::from(q.volume);
                        entry.vwap = vwap;
                        if vwap > rust_decimal::Decimal::ZERO {
                            let dev =
                                (q.last_done - vwap) / vwap * rust_decimal::Decimal::from(100);
                            entry.vwap_deviation_pct = dev;
                        }
                    }
                }
                sk::update_from_microstructure(&mut subkg_registry, &micro, now, tick);
            }
            // Events: big trade count + halt + volume spike
            {
                let mut events: std::collections::HashMap<String, sk::EventSnapshot> =
                    std::collections::HashMap::new();
                // Halt detection from quote trade_status
                use longport::quote::TradeStatus;
                for (sym, q) in &live.quotes {
                    if !matches!(q.trade_status, TradeStatus::Normal) {
                        halted_today.insert(sym.clone());
                    }
                }
                for sym in halted_today.iter() {
                    events.entry(sym.0.clone()).or_default().has_halted_today = true;
                }
                // Big trade count last 1h: trades with volume > 5x median (universal)
                use longport::quote::TradeDirection;
                let _ = TradeDirection::Up; // silence
                for (sym, trades) in raw_trade_tape.per_symbol.iter() {
                    let now_t = time::OffsetDateTime::now_utc();
                    let c1h = now_t - time::Duration::hours(1);
                    let mut vols: Vec<i64> = trades
                        .iter()
                        .filter(|t| t.timestamp >= c1h)
                        .map(|t| t.volume)
                        .collect();
                    if vols.is_empty() {
                        continue;
                    }
                    vols.sort();
                    let median = vols[vols.len() / 2];
                    let big_count = vols.iter().filter(|v| **v > median * 5).count() as u32;
                    events
                        .entry(sym.0.clone())
                        .or_default()
                        .big_trade_count_last_1h = big_count;
                }
                // Volume spike fresh: activity_momentum > 0.5 (dim is normalized)
                use rust_decimal::prelude::ToPrimitive;
                for (sym, d) in &dim_snapshot.dimensions {
                    let vr = d.activity_momentum.to_f64().unwrap_or(0.0);
                    if vr > 0.5 {
                        events.entry(sym.0.clone()).or_default().volume_spike_fresh = true;
                    }
                }
                sk::update_from_events(&mut subkg_registry, &events, now, tick);
            }
            // Holders: terrain-based holder data not surfaced in current
            // LinkSnapshot. Holder nodes (InsiderHoldingPct etc.) remain
            // empty until terrain wiring lands. Skipped this tick.
            // Roles: leader/laggard within sector cluster (5-tick price velocity)
            {
                let mut roles: std::collections::HashMap<String, sk::RoleSnapshot> =
                    std::collections::HashMap::new();
                use rust_decimal::prelude::ToPrimitive;
                for (sector_id, members) in &sector_members {
                    if members.len() < 3 {
                        continue;
                    }
                    let mut velocities: Vec<(Symbol, f64)> = Vec::new();
                    for sym in members {
                        if let Some(q) = live.quotes.get(sym) {
                            let prev = q.prev_close.to_f64().unwrap_or(0.0);
                            let last = q.last_done.to_f64().unwrap_or(0.0);
                            if prev > 0.0 {
                                velocities.push((sym.clone(), (last - prev) / prev));
                            }
                        }
                    }
                    if velocities.is_empty() {
                        continue;
                    }
                    let sector_avg: f64 =
                        velocities.iter().map(|(_, v)| *v).sum::<f64>() / velocities.len() as f64;
                    let max_v = velocities
                        .iter()
                        .map(|(_, v)| *v)
                        .fold(f64::NEG_INFINITY, f64::max);
                    let min_v = velocities
                        .iter()
                        .map(|(_, v)| *v)
                        .fold(f64::INFINITY, f64::min);
                    for (sym, v) in &velocities {
                        let entry = roles.entry(sym.0.clone()).or_default();
                        entry.sector_relative_strength =
                            rust_decimal::Decimal::from_f64_retain((v - sector_avg) * 100.0)
                                .unwrap_or(rust_decimal::Decimal::ZERO);
                        // leader/laggard: -1 if min, +1 if max, 0 otherwise (signed by rank position)
                        let _ = sector_id;
                        let score = if (*v - max_v).abs() < 1e-9 {
                            1.0
                        } else if (*v - min_v).abs() < 1e-9 {
                            -1.0
                        } else {
                            (v - sector_avg) / (max_v - min_v + 1e-9)
                        };
                        entry.leader_laggard_score = rust_decimal::Decimal::from_f64_retain(score)
                            .unwrap_or(rust_decimal::Decimal::ZERO);
                    }
                }
                sk::update_from_roles(&mut subkg_registry, &roles, now, tick);
                // Cross-market bridge: HK → US counterpart label.
                // Static mapping of 14 dual-listed pairs from watchlist.
                {
                    let bridges: std::collections::HashMap<String, String> =
                        eden::bridges::pairs::CROSS_MARKET_PAIRS
                            .iter()
                            .map(|p| (p.hk_symbol.to_string(), p.us_symbol.to_string()))
                            .collect();
                    sk::update_cross_market_bridge(&mut subkg_registry, &bridges, now, tick);
                }
                // Earnings: derive days-until + in-window flag from terrain
                // calendar. Skips symbols with no upcoming event.
                if !terrain.upcoming_events.is_empty() {
                    let today = time::OffsetDateTime::now_utc().date();
                    let mut earnings: std::collections::HashMap<
                        String,
                        eden::pipeline::symbol_sub_kg::EarningsSnapshot,
                    > = std::collections::HashMap::new();
                    for (sym, events) in &terrain.upcoming_events {
                        let next = events
                            .iter()
                            .filter_map(|ev| {
                                time::Date::parse(
                                    &ev.date,
                                    &time::format_description::well_known::Iso8601::DATE,
                                )
                                .ok()
                            })
                            .min();
                        if let Some(date) = next {
                            let days = (date - today).whole_days() as i32;
                            if days >= 0 {
                                earnings.insert(
                                    sym.0.clone(),
                                    eden::pipeline::symbol_sub_kg::EarningsSnapshot {
                                        days_until_next: days,
                                        in_window: days <= 3,
                                    },
                                );
                            }
                        }
                    }
                    if !earnings.is_empty() {
                        sk::update_from_earnings(&mut subkg_registry, &earnings, now, tick);
                    }
                }
            }
            // HK/US symmetry: compute and record the current-tick regime
            // analog before sub-KG substrate evidence, so EngramAlignment
            // is current for BP just like US. The later live snapshot
            // surface may build a display-only fingerprint, but it does
            // not record into the analog index again.
            let mut hk_current_regime_analog_summary: Option<
                eden::pipeline::regime_analog_index::AnalogSummary,
            > = None;
            let _ = history.latest().and_then(|latest| {
                let captured_at =
                    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                let live_snapshot_for_regime = build_hk_live_snapshot(
                    tick,
                    captured_at,
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
                    &live,
                    &tracker,
                    &causal_timelines,
                    &dynamics,
                    &previous_symbol_states,
                    &previous_cluster_states,
                    previous_world_summary.as_ref(),
                );
                let Some(world_summary) = live_snapshot_for_regime.world_summary.as_ref() else {
                    return None;
                };
                use rust_decimal::prelude::ToPrimitive as _;
                let stress = graph_insights
                    .stress
                    .composite_stress
                    .to_f64()
                    .unwrap_or(0.0);
                let synchrony = graph_insights
                    .stress
                    .sector_synchrony
                    .to_f64()
                    .unwrap_or(0.0);
                let dominant_driver = world_summary
                    .dominant_clusters
                    .iter()
                    .find_map(|s| s.strip_prefix("driver:"))
                    .map(|s| s.to_string());
                let snapshot_ts =
                    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                let hk_regime_fp = eden::pipeline::regime_fingerprint::build_hk_fingerprint(
                    "hk",
                    tick,
                    snapshot_ts,
                    stress,
                    synchrony,
                    &live_snapshot_for_regime.cluster_states,
                    world_summary,
                    dominant_driver,
                );
                eprintln!(
                    "[hk] regime_fingerprint: bucket={} stress={:.2} sync={:.2} bias={:.2} act={:.2} turn={:.2} legacy={}{}",
                    hk_regime_fp.bucket_key,
                    hk_regime_fp.stress,
                    hk_regime_fp.synchrony,
                    hk_regime_fp.bull_bias,
                    hk_regime_fp.activity,
                    hk_regime_fp.turn_pressure,
                    hk_regime_fp.legacy_label,
                    hk_regime_fp
                        .dominant_driver
                        .as_ref()
                        .map(|d| format!(" driver={}", d))
                        .unwrap_or_default(),
                );
                let now_anc = chrono::Utc::now();
                let (analog_summary, realized_outcomes) =
                    hk_regime_analog_index.record("hk", &hk_regime_fp, now_anc);
                let _ = eden::pipeline::regime_analog_index::write_summary(
                    "hk",
                    &analog_summary,
                );
                let _ = eden::pipeline::regime_analog_index::write_outcomes(
                    "hk",
                    &realized_outcomes,
                );
                hk_current_regime_analog_summary = Some(analog_summary);
                if let Ok(mut map) = runtime.current_regime_buckets.write() {
                    map.insert(eden::cases::CaseMarket::Hk, hk_regime_fp.bucket_key.clone());
                }
                #[cfg(feature = "persistence")]
                {
                    let now_utc = chrono::Utc::now();
                    let due = match last_hk_regime_fp_ts {
                        None => true,
                        Some(prev) => (now_utc - prev).num_seconds() >= 60,
                    };
                    if due {
                        if let Some(ref eden_store) = runtime.store {
                            let snap: eden::persistence::regime_fingerprint_snapshot::RegimeFingerprintSnapshot =
                                (&hk_regime_fp).into();
                            let store_clone = eden_store.clone();
                            let bucket_for_log = hk_regime_fp.bucket_key.clone();
                            tokio::spawn(async move {
                                if let Err(e) =
                                    store_clone.write_regime_fingerprint_snapshot(&snap).await
                                {
                                    eprintln!("[regime_fp] snapshot write failed: {}", e);
                                }
                            });
                            last_hk_regime_fp_ts = Some(now_utc);
                            eprintln!("[regime_fp] snapshot: market=hk bucket={}", bucket_for_log);
                        }
                    }
                }
                Some(hk_regime_fp)
            });

            // Snapshot every 5 ticks to keep file growth bounded
            subkg_snapshot_tick += 1;
            if subkg_snapshot_tick >= 5 {
                subkg_snapshot_tick = 0;
                use eden::pipeline::runtime_stage_trace::{
                    RuntimeStage, RuntimeStagePlan, RuntimeStageTrace,
                };
                let stage_plan = RuntimeStagePlan::canonical();
                let mut runtime_trace = RuntimeStageTrace::new("hk", tick, now);
                runtime_trace
                    .record_planned(stage_plan, RuntimeStage::RegimeAnalogRecord)
                    .expect("HK runtime stage is declared in canonical plan");
                // Terrain → sub-KG Holder nodes. institutional_holder_count
                // aggregates distinct institutions that hold each symbol
                // above the 0.5% threshold baked into terrain fetch.
                // etf_holding_pct is a count-based proxy — fund_holdings
                // lacks per-holder share-of-symbol, so we surface "how
                // many funds include this symbol" as a crude magnitude
                // so structural primitives can detect relative outliers.
                // insider_holding_pct + southbound_flow_today stay at
                // zero until dedicated sources are wired.
                if !terrain.institutional_holdings.is_empty() || !terrain.fund_holdings.is_empty() {
                    let mut holdings: std::collections::HashMap<
                        String,
                        eden::pipeline::symbol_sub_kg::HoldingSnapshot,
                    > = std::collections::HashMap::new();
                    for (_name, rows) in &terrain.institutional_holdings {
                        for (sym, _pct) in rows {
                            let entry = holdings.entry(sym.0.clone()).or_insert_with(|| {
                                eden::pipeline::symbol_sub_kg::HoldingSnapshot {
                                    insider_holding_pct: Decimal::ZERO,
                                    institutional_holder_count: 0,
                                    southbound_flow_today: Decimal::ZERO,
                                    etf_holding_pct: Decimal::ZERO,
                                }
                            });
                            entry.institutional_holder_count += 1;
                        }
                    }
                    for (_fund, members) in &terrain.fund_holdings {
                        for sym in members {
                            let entry = holdings.entry(sym.0.clone()).or_insert_with(|| {
                                eden::pipeline::symbol_sub_kg::HoldingSnapshot {
                                    insider_holding_pct: Decimal::ZERO,
                                    institutional_holder_count: 0,
                                    southbound_flow_today: Decimal::ZERO,
                                    etf_holding_pct: Decimal::ZERO,
                                }
                            });
                            entry.etf_holding_pct += Decimal::ONE;
                        }
                    }
                    eden::pipeline::symbol_sub_kg::update_from_holdings(
                        &mut subkg_registry,
                        &holdings,
                        now,
                        tick,
                    );
                }
                // WL graph signature per sub-KG — typed-graph structural
                // fingerprint that uses node TYPES + edge topology, not
                // flat scalar aggregation. Per-symbol per-snapshot row in
                // ndjson; signature_hash becomes the natural key for
                // graph-structural analog lookup.
                let hk_wl_analogs_by_symbol: std::collections::HashMap<
                    String,
                    eden::pipeline::symbol_wl_analog_index::AnalogMatch,
                >;
                {
                    let rows = eden::pipeline::wl_graph_signature::build_signature_rows(
                        "hk",
                        &subkg_registry,
                        eden::pipeline::wl_graph_signature::WL_ITERATIONS,
                        now,
                    );
                    let _ = eden::pipeline::wl_graph_signature::write_signature_rows("hk", &rows);
                    // Per-symbol structural analog lookup — per signature_hash,
                    // count prior visits across (symbol, tick), surface
                    // most-recent matches.
                    let mut analogs = Vec::with_capacity(rows.len());
                    for row in &rows {
                        let m = hk_symbol_wl_analog_index.record(
                            "hk",
                            &row.symbol,
                            &row.signature_hash,
                            row.ts,
                        );
                        analogs.push(m);
                    }
                    let _ = eden::pipeline::symbol_wl_analog_index::write_matches("hk", &analogs);
                    hk_wl_analogs_by_symbol = analogs
                        .iter()
                        .map(|a| (a.symbol.clone(), a.clone()))
                        .collect();
                }
                runtime_trace
                    .record_planned(stage_plan, RuntimeStage::WlAnalogRecord)
                    .expect("HK runtime stage is declared in canonical plan");
                // V2 Phase 4: feed accumulated forecast accuracy from
                // ActiveProbeRunner into sub-KG ForecastAccuracy NodeId.
                let hk_probe_accuracy = hk_active_probe.accuracy_by_symbol();
                runtime_trace
                    .record_planned(stage_plan, RuntimeStage::ActiveProbeAccuracyRead)
                    .expect("HK runtime stage is declared in canonical plan");

                // V3.2 cross-ontology: compute parent-sector intent verdicts
                // and broadcast each verdict's (Accumulation, Distribution)
                // posterior to its member symbols. Each symbol gets its
                // sector's verdict as a per-sub-KG NodeId pair, so BP picks
                // the sector signal up via observe_from_subkg without any
                // schema change in the BP entity type.
                let hk_sector_intent_by_symbol: Option<
                    std::collections::HashMap<String, (f64, f64)>,
                > = {
                    let mut map: std::collections::HashMap<String, (f64, f64)> =
                        std::collections::HashMap::new();
                    for (sector_id, members) in &sector_members {
                        let Some(sector_name) = sector_names.get(sector_id) else {
                            continue;
                        };
                        let Some(verdict) = eden::pipeline::sector_intent::compute_sector_intent(
                            sector_id.clone(),
                            sector_name,
                            eden::ontology::objects::Market::Hk,
                            members,
                            &intent_belief_field,
                        ) else {
                            continue;
                        };
                        // Posterior order = [Accumulation, Distribution,
                        // Rotation, Volatility, Unknown]. Bull = idx 0,
                        // Bear = idx 1. Other variants stay neutral.
                        let bull = verdict.posterior[0];
                        let bear = verdict.posterior[1];
                        for member in members {
                            map.insert(member.0.clone(), (bull, bear));
                        }
                    }
                    if map.is_empty() {
                        None
                    } else {
                        Some(map)
                    }
                };

                // V4 KL surprise: pre-update belief_field from this tick's
                // pressure samples *before* the tracker observes, so the
                // tracker sees fresh KL change for this tick (otherwise the
                // tracker would always read 1-tick-stale beliefs and KL
                // surprise NodeIds would stay at 0).
                //
                // Mirrors what the late-tick belief block does at line ~3833;
                // moved early because every consumer between here and the
                // late-tick block only depends on the just-fresh state.
                {
                    use eden::ontology::objects::Symbol as HkSymbol;
                    use eden::pipeline::pressure::TimeScale;
                    if let Some(tick_layer) = pressure_field.layers.get(&TimeScale::Tick) {
                        let samples: Vec<(
                            HkSymbol,
                            eden::pipeline::pressure::PressureChannel,
                            rust_decimal::Decimal,
                        )> = tick_layer
                            .pressures
                            .iter()
                            .flat_map(|(symbol, node)| {
                                node.channels
                                    .iter()
                                    .map(|(channel, cp)| (symbol.clone(), *channel, cp.net()))
                            })
                            .collect();
                        belief_field.update_from_pressure_samples(samples, tick);
                    }
                }
                kl_surprise_tracker.observe_from_belief_field(&belief_field);
                let hk_kl_surprise_by_symbol = kl_surprise_tracker.surprise_summary(&belief_field);
                let substrate_evidence =
                    eden::pipeline::symbol_sub_kg::build_substrate_evidence_snapshots(
                        &subkg_registry,
                        eden::pipeline::symbol_sub_kg::SubstrateEvidenceInput {
                            decision_ledger: Some(&decision_ledger),
                            synthetic_outcomes: &cached_synthetic_outcomes,
                            engram_summary: hk_current_regime_analog_summary.as_ref(),
                            wl_analogs_by_symbol: Some(&hk_wl_analogs_by_symbol),
                            belief_field: Some(&belief_field),
                            forecast_accuracy_by_symbol: Some(&hk_probe_accuracy),
                            // V3.2 cross-ontology — wired below.
                            sector_intent_by_symbol: hk_sector_intent_by_symbol.as_ref(),
                            kl_surprise_by_symbol: Some(&hk_kl_surprise_by_symbol),
                        },
                    );
                runtime_trace
                    .record_planned(stage_plan, RuntimeStage::SubKgEvidenceBuild)
                    .expect("HK runtime stage is declared in canonical plan");
                eden::pipeline::symbol_sub_kg::update_from_substrate_evidence(
                    &mut subkg_registry,
                    &substrate_evidence,
                    now,
                    tick,
                );
                runtime_trace
                    .record_planned(stage_plan, RuntimeStage::SubKgEvidenceApply)
                    .expect("HK runtime stage is declared in canonical plan");
                let graph_frontier = eden::pipeline::frontier::GraphFrontier::from_subkg_registry(
                    tick as u64,
                    &subkg_registry,
                );
                let frontier_propagation = graph_frontier.local_propagation_plan();
                let frontier_candidates = frontier_propagation.propagation_candidates();
                let frontier_dry_run =
                    eden::pipeline::frontier::FrontierPropagationDryRun::from_candidates(
                        tick as u64,
                        &frontier_candidates,
                    );
                let frontier_pressure_cache =
                    eden::pipeline::frontier::FrontierPressureCandidateCache::from_dry_run(
                        &frontier_dry_run,
                    );
                let frontier_pressure_gate =
                    eden::pipeline::frontier::FrontierPressureConvergenceGate::from_cache(
                        &frontier_pressure_cache,
                    );
                let frontier_next_proposal =
                    eden::pipeline::frontier::FrontierNextProposal::from_pressure_gate(
                        &frontier_pressure_gate,
                    );
                let frontier_loop_summary =
                    graph_frontier.bounded_propagation_summary(&frontier_next_proposal, 2);
                runtime_trace
                    .record_planned(stage_plan, RuntimeStage::FrontierBuild)
                    .expect("HK runtime stage is declared in canonical plan");
                // sub_kg summary line removed from wake — ndjson snapshot
                // carries full per-symbol structure, including ontology
                // memory/belief/causal evidence nodes.
                // 2026-04-29 Phase A: serialize on consumer (sync, fast),
                // ship to background writer task. Drops on backpressure.
                match subkg_registry.serialize_active_to_lines() {
                    Ok(lines) => {
                        let _ = subkg_writer.try_send_batch(lines);
                    }
                    Err(e) => eprintln!("[sub_kg] hk serialize failed: {}", e),
                }
                runtime_trace
                    .record_planned(stage_plan, RuntimeStage::SubKgSnapshotWrite)
                    .expect("HK runtime stage is declared in canonical plan");
                let mut artifact_write_errors = Vec::new();
                // Sector sub-KG: forward composition Symbol → Sector.
                // Aggregates each sector's member sub-KGs by NodeKind
                // (mean / variance / outlier_count). Feeds into
                // structural_contrast as the second contrast axis
                // (vs own-sector mean), and dumps to ndjson for
                // operator inspection. Pure stateless aggregation.
                let sector_subkgs = eden::pipeline::sector_sub_kg::build_from_registry(
                    &subkg_registry,
                    &sector_members,
                    &sector_names,
                    now,
                );
                runtime_trace
                    .record_planned(stage_plan, RuntimeStage::SectorSubKgBuild)
                    .expect("HK runtime stage is declared in canonical plan");
                match eden::pipeline::sector_sub_kg::serialize_active_to_lines(
                    &sector_subkgs,
                    "hk",
                ) {
                    Ok(lines) => {
                        let _ = sector_subkg_writer.try_send_batch(lines);
                    }
                    Err(e) => eprintln!("[sector_sub_kg] hk serialize failed: {}", e),
                }
                // Cross-sector contrast — second hop of visual model.
                // Asks "which SECTOR is the standout this snapshot?" by
                // applying center-surround DoG one zoom level up.
                let sector_contrast_events =
                    eden::pipeline::cross_sector_contrast::detect_sector_contrasts(
                        "hk",
                        &sector_subkgs,
                        now,
                    );
                let _ = cross_sector_writer.try_send_batch(sector_contrast_events.clone());
                // Backward propagation: hot sector → quiet members lag.
                // Closes Symbol↔Sector bidirectional loop. No mutation
                // of sub-KG (observation stays clean); pure event emission.
                let member_lag_events =
                    eden::pipeline::sector_to_symbol_propagation::detect_member_lag(
                        "hk",
                        &subkg_registry,
                        &sector_subkgs,
                        &sector_members,
                        now,
                    );
                let _ = sector_to_symbol_writer.try_send_batch(member_lag_events.clone());
                // Sector kinematics — cross-tick velocity / acceleration
                // / zero-crossing turning points on sector-mean signal.
                let sector_kin_events = eden::pipeline::sector_kinematics::update_and_detect(
                    "hk",
                    &sector_subkgs,
                    &mut sector_kinematics_tracker,
                    now,
                );
                let _ = sector_kinematics_writer.try_send_batch(sector_kin_events.clone());
                let symbol_to_sector_str: std::collections::HashMap<String, String> = symbol_sector
                    .iter()
                    .map(|(sym, sid)| (sym.0.clone(), sid.0.clone()))
                    .collect();
                // Cluster sync detection over sector clusters.
                // Pure structural emergence: when N≥3 sector members
                // are lit on same K≥2 NodeKinds, that's the signal.
                let clusters_str: std::collections::HashMap<String, Vec<String>> = sector_members
                    .iter()
                    .map(|(sid, syms)| (sid.0.clone(), syms.iter().map(|s| s.0.clone()).collect()))
                    .collect();
                // Cross-symbol activation propagation along master KG.
                // Pure graph physics: source sub-KG Pressure/Intent values
                // transfer to neighbors via master-KG StockToStock edges
                // weighted by similarity. No rules, just topology.
                {
                    use eden::pipeline::cross_symbol_propagation as csp;
                    use rust_decimal::prelude::ToPrimitive;
                    let mut master_edges: Vec<csp::MasterEdge> = Vec::new();
                    let mut bp_master_graph_edges = 0usize;
                    // Iterate StockToStock edges from BrainGraph
                    for edge_idx in brain.graph.edge_indices() {
                        if let eden::graph::graph::EdgeKind::StockToStock(s2s) =
                            &brain.graph[edge_idx]
                        {
                            bp_master_graph_edges += 1;
                            let (a_idx, b_idx) = brain.graph.edge_endpoints(edge_idx).unwrap();
                            // Find the symbols for these node indices
                            let a_sym = brain
                                .stock_nodes
                                .iter()
                                .find(|(_, idx)| **idx == a_idx)
                                .map(|(s, _)| s.0.clone());
                            let b_sym = brain
                                .stock_nodes
                                .iter()
                                .find(|(_, idx)| **idx == b_idx)
                                .map(|(s, _)| s.0.clone());
                            if let (Some(a), Some(b)) = (a_sym, b_sym) {
                                let weight = s2s.similarity.to_f64().unwrap_or(0.0);
                                if weight > 0.0 {
                                    master_edges.push(csp::MasterEdge {
                                        from: a,
                                        to: b,
                                        weight,
                                        edge_type: "StockToStock".into(),
                                    });
                                }
                            }
                        }
                    }
                    let prop_snaps = csp::propagate(
                        "hk",
                        &subkg_registry,
                        &master_edges,
                        csp::DEFAULT_PROPAGATION_RATE,
                        now,
                    );
                    if !prop_snaps.is_empty() {
                        // Propagation count wake removed;
                        // full snapshots in eden-propagation-hk.ndjson.
                        if let Err(e) = csp::write_snapshots("hk", &prop_snaps) {
                            eprintln!("[propagation] hk write failed: {}", e);
                        }
                    }
                    runtime_trace
                        .record_planned(stage_plan, RuntimeStage::CrossSymbolPropagation)
                        .expect("HK runtime stage is declared in canonical plan");
                    // Loopy BP — Pearl-style sum-product on master KG.
                    // Given partial observation (some symbols' Pressure +
                    // Intent state), compute marginal posterior over ALL
                    // symbols' state {Bull, Bear, Neutral}. True 局部推全局
                    // mathematics on the typed KG.
                    let bp_input_edges: Vec<eden::pipeline::loopy_bp::BpInputEdge> = master_edges
                        .iter()
                        .map(|e| eden::pipeline::loopy_bp::BpInputEdge {
                            from: e.from.clone(),
                            to: e.to.clone(),
                            weight: e.weight,
                            edge_type: e.edge_type.clone(),
                        })
                        .collect();
                    let bp_edges: Vec<(String, String, f64)> = bp_input_edges
                        .iter()
                        .map(|e| (e.from.clone(), e.to.clone(), e.weight))
                        .collect();

                    // Lead-lag is BP evidence now: compute it before
                    // constructing priors/edges so directional weights
                    // participate in the single BP fusion pass.
                    hk_lead_lag_tracker.ingest(&subkg_registry);
                    let lead_lag_evs = eden::pipeline::lead_lag_index::detect_lead_lag(
                        "hk",
                        &hk_lead_lag_tracker,
                        &bp_edges,
                        now,
                    );
                    eden::core::runtime_artifacts::record_artifact_result(
                        &mut artifact_write_errors,
                        "lead_lag_events",
                        eden::pipeline::lead_lag_index::write_events("hk", &lead_lag_evs),
                    );
                    runtime_trace
                        .record_planned(stage_plan, RuntimeStage::LeadLagDetect)
                        .expect("HK runtime stage is declared in canonical plan");

                    // V4 Phase 1.5: sub-KG emergence detection (HK side,
                    // symmetric with US runtime). Synthesizes setups for
                    // symbols whose cross-NodeId emergence score crosses
                    // their own self-referential 1σ baseline. Inserted
                    // before BP build_inputs so apply_posterior_confidence
                    // below picks the new setups up.
                    // V5.1: graph-attention budget — high-centrality
                    // symbols (hub anticorr=17+) every tick, isolated
                    // symbols throttled. Eliminates O(N=639) per-tick walk.
                    let centrality =
                        eden::pipeline::graph_attention::centrality_from_hubs(&prev_tick_hubs);
                    // V7.2: build the per-tick frontier whitelist from the
                    // pressure convergence gate. Symbols whose Contributes /
                    // FlowToPressure proposed_delta cleared the self-
                    // referential noise floor get processed; the rest are
                    // skipped this tick, replacing the prior O(N=493) walk.
                    let frontier_symbols: std::collections::HashSet<String> =
                        frontier_pressure_gate
                            .passed
                            .iter()
                            .map(|update| update.symbol.clone())
                            .collect();
                    let frontier_filter = if frontier_symbols.is_empty() {
                        None
                    } else {
                        Some(&frontier_symbols)
                    };
                    let emergence_events = sub_kg_emergence_tracker.detect_emergences(
                        &subkg_registry,
                        &centrality,
                        frontier_filter,
                    );
                    for emergence in &emergence_events {
                        reasoning_snapshot.tactical_setups.push(
                            eden::pipeline::sub_kg_emergence::synthesize_setup_from_emergence(
                                emergence,
                            ),
                        );
                    }

                    // V2: BP single entry. Priors come from sub-KG (already
                    // populated by update_from_substrate_evidence above).
                    let bp_build_inputs_start = Instant::now();
                    let (priors, edges) = eden::pipeline::loopy_bp::build_inputs(
                        &subkg_registry,
                        &bp_input_edges,
                        &lead_lag_evs,
                    );
                    let bp_pruning_shadow =
                        eden::pipeline::loopy_bp::build_pruning_shadow_summary(&priors, &edges);
                    let bp_build_inputs_elapsed = bp_build_inputs_start.elapsed();
                    runtime_trace
                        .record_planned(stage_plan, RuntimeStage::BpBuildInputs)
                        .expect("HK runtime stage is declared in canonical plan");
                    use eden::pipeline::event_driven_bp::BeliefSubstrate as _;
                    let bp_run_start = Instant::now();
                    belief_substrate.observe_tick(&priors, &edges, tick as u64);
                    // C3 fix: barrier between observe_tick (fire-and-forget)
                    // and posterior_snapshot — without this, downstream reads
                    // see either the previous tick's posterior or a freshly
                    // reset prior, never the converged tick-N posterior.
                    let _quiesced = belief_substrate
                        .wait_until_quiescent(std::time::Duration::from_millis(50))
                        .await;
                    let bp_run_elapsed = bp_run_start.elapsed();
                    let view = belief_substrate.posterior_snapshot();
                    runtime_trace
                        .record_planned(stage_plan, RuntimeStage::BpRun)
                        .expect("HK runtime stage is declared in canonical plan");
                    let bp_message_trace_write_start = Instant::now();
                    // bp_message_trace now uses the substrate's posterior view
                    // (beliefs only) — message-level detail was deleted with
                    // sync substrate; trace is now a per-tick belief snapshot
                    // keyed by tick + symbol.
                    let bp_trace_rows = eden::pipeline::loopy_bp::build_belief_only_trace_rows(
                        "hk", tick as u64, &priors, &edges, &view.beliefs, now,
                    );
                    let _ = bp_message_trace_writer.try_send_batch(bp_trace_rows);
                    let bp_message_trace_write_elapsed = bp_message_trace_write_start.elapsed();
                    let iterations = view.iterations;
                    let converged = view.converged;
                    let mut encoded_tick_frame =
                        eden::pipeline::encoded_tick_frame::EncodedTickFrame::from_pressure_field(
                            "hk",
                            tick,
                            now,
                            &pressure_field,
                        );
                    encoded_tick_frame.attach_subkg_registry(&subkg_registry);
                    encoded_tick_frame.attach_bp_state(&priors, &view.beliefs, &edges);
                    eden::core::runtime_artifacts::record_artifact_result(
                        &mut artifact_write_errors,
                        "encoded_tick_frame",
                        eden::pipeline::encoded_tick_frame::write_frame(
                            "hk",
                            &encoded_tick_frame,
                        ),
                    );
                    let visual_frame =
                        eden::pipeline::visual_graph_frame::build_visual_graph_frame_from_encoded(
                            &encoded_tick_frame,
                        );
                    if let Some(previous) = previous_visual_frame.as_ref() {
                        let delta = eden::pipeline::temporal_graph_delta::build_delta(
                            "hk",
                            tick,
                            previous,
                            &visual_frame,
                            now,
                        );
                        let _ = temporal_delta_writer.try_send_batch(delta);
                    }
                    let _ = visual_frame_writer.try_send_batch(visual_frame.clone());
                    previous_visual_frame = Some(visual_frame);
                    let bp_marginals_write_start = Instant::now();
                    let rows = eden::pipeline::loopy_bp::build_marginal_rows(
                        "hk",
                        &priors,
                        &view.beliefs,
                        iterations,
                        converged,
                        now,
                    );
                    let _ = bp_marginals_writer.try_send_batch(rows);
                    let bp_marginals_write_elapsed = bp_marginals_write_start.elapsed();
                    runtime_trace
                        .record_planned(stage_plan, RuntimeStage::BpMarginalsWrite)
                        .expect("HK runtime stage is declared in canonical plan");
                    let beliefs = view.beliefs.clone();
                    // 2026-05-01: P1a — predict-realize calibration loop.
                    // Write current beliefs as naive-baseline predictions
                    // for tick + horizon (random-walk null model). Then
                    // realize the prediction made `horizon` ticks ago.
                    // Skeleton — future predictors swap in without
                    // changing the on-disk schema.
                    let _ = eden::pipeline::prediction_calibration::write_predictions(
                        "hk",
                        tick as u64,
                        &beliefs,
                        eden::pipeline::prediction_calibration::PREDICTION_HORIZON_TICKS,
                    );
                    let _ = eden::pipeline::prediction_calibration::realize_predictions(
                        "hk",
                        tick as u64,
                        &beliefs,
                        eden::pipeline::prediction_calibration::PREDICTION_HORIZON_TICKS,
                    );
                    // 2026-05-01: P1b — signature replay. Observe current
                    // (signature, belief) pairs for future replay; lookup
                    // historical occurrences for current symbols.
                    let _hk_signature_replays =
                        eden::pipeline::signature_replay::observe_and_replay(
                            "hk",
                            tick as u64,
                            &beliefs,
                            20,
                        );
                    // 2026-05-01: feed BP posterior into lead-lag tracker.
                    // The original ingest path reads sub-KG channel nodes
                    // (PressureCapitalFlow / Momentum / Intent*) which are
                    // rarely populated for most symbols → tracker history
                    // becomes constant zero → no events ever written.
                    // (p_bull - p_bear) is always populated and captures
                    // eden's directional belief — exactly what lead-lag
                    // wants to correlate across the master KG edges.
                    hk_lead_lag_tracker.ingest_from_beliefs(&beliefs);
                    // V2: BP posterior is single source of truth for
                    // setup.confidence. No post-BP belief/history
                    // modulation — those signals already entered BP via
                    // sub-KG NodeId values.
                    // V5.3 + 2026-04-29 ordering fix: reconcile_direction
                    // must run BEFORE apply_posterior_confidence. Mirrors
                    // US runtime fix — confidence write reads
                    // setup.direction to pick which posterior cell becomes
                    // p_target; running reconcile after leaves emerge:*
                    // setups whose direction got flipped with confidence
                    // stuck on the pre-flip side.
                    let _hk_emerge_dir_touched =
                        eden::pipeline::sub_kg_emergence::reconcile_direction_with_bp(
                            &mut reasoning_snapshot.tactical_setups,
                            &beliefs,
                        );
                    let mut hk_bp_conf_applied = 0usize;
                    let mut hk_bp_conf_skipped = 0usize;
                    for setup in reasoning_snapshot.tactical_setups.iter_mut() {
                        if eden::pipeline::loopy_bp::apply_posterior_confidence(setup, &beliefs) {
                            hk_bp_conf_applied += 1;
                        } else {
                            hk_bp_conf_skipped += 1;
                        }
                    }
                    runtime_trace
                        .record_planned(stage_plan, RuntimeStage::BpPosteriorConfidence)
                        .expect("HK runtime stage is declared in canonical plan");
                    // V2/V4 cleanup: action upgrade is data-driven —
                    // percentile by default, KL surprise when env-flag
                    // EDEN_ACTION_PROMOTION=kl_surprise.
                    eden::pipeline::action_promotion::apply_action_promotion(
                        &mut reasoning_snapshot.tactical_setups,
                        &kl_surprise_tracker,
                        &belief_field,
                    );
                    setup_registry.refresh_from_setups(&reasoning_snapshot.tactical_setups);
                    if hk_bp_conf_applied + hk_bp_conf_skipped > 0 {
                        eprintln!(
                            "[hk] bp_posterior_confidence: applied={} skipped={} \
                             bp_iters={} converged={} lead_lag_events={}",
                            hk_bp_conf_applied,
                            hk_bp_conf_skipped,
                            iterations,
                            converged,
                            lead_lag_evs.len(),
                        );
                    }

                    // V2 Phase 4: active probing — counterfactual BP
                    // experiments. Mirrors US wiring.
                    let probe_outcomes = hk_active_probe.evaluate_due(tick, &beliefs, now, "hk");
                    eden::core::runtime_artifacts::record_artifact_result(
                        &mut artifact_write_errors,
                        "active_probe_outcomes",
                        eden::pipeline::active_probe::write_outcomes("hk", &probe_outcomes),
                    );
                    runtime_trace
                        .record_planned(stage_plan, RuntimeStage::ActiveProbeEvaluate)
                        .expect("HK runtime stage is declared in canonical plan");
                    let probe_targets = eden::pipeline::active_probe::pick_probe_targets(
                        &beliefs,
                        eden::pipeline::active_probe::PROBE_TARGETS_PER_TICK,
                    );
                    let probe_forecasts = hk_active_probe.emit_probes(
                        &probe_targets,
                        &priors,
                        &edges,
                        tick,
                        now,
                        "hk",
                    );
                    eden::core::runtime_artifacts::record_artifact_result(
                        &mut artifact_write_errors,
                        "active_probe_forecasts",
                        eden::pipeline::active_probe::write_forecasts("hk", &probe_forecasts),
                    );
                    runtime_trace
                        .record_planned(stage_plan, RuntimeStage::ActiveProbeEmit)
                        .expect("HK runtime stage is declared in canonical plan");
                    let probe_mean_accuracy = if probe_outcomes.is_empty() {
                        None
                    } else {
                        let sum: f64 = probe_outcomes.iter().map(|o| o.mean_accuracy).sum();
                        Some(sum / probe_outcomes.len() as f64)
                    };
                    if !probe_forecasts.is_empty() || !probe_outcomes.is_empty() {
                        let acc_str = probe_mean_accuracy
                            .map(|a| format!("{:.2}", a))
                            .unwrap_or_else(|| "n/a".to_string());
                        eprintln!(
                            "[hk] active_probe: emitted={} evaluated={} \
                             mean_accuracy={} pending={}",
                            probe_forecasts.len(),
                            probe_outcomes.len(),
                            acc_str,
                            hk_active_probe.pending_count(),
                        );
                    }
                    runtime_trace
                        .record_planned(stage_plan, RuntimeStage::ArtifactHealth)
                        .expect("HK runtime stage is declared in canonical plan");
                    let plan_coverage = runtime_trace.plan_coverage(stage_plan);
                    if let Err(e) = runtime_trace.write_ndjson() {
                        eprintln!("[runtime_stage] hk write failed: {}", e);
                        artifact_write_errors.push(
                            eden::core::runtime_artifacts::RuntimeArtifactWriteError {
                                artifact: "runtime_stage_trace".to_string(),
                                error: e.to_string(),
                            },
                        );
                    }
                    let health_tick = eden::core::runtime_artifacts::RuntimeHealthTick {
                        ts: now,
                        market: "hk".to_string(),
                        tick,
                        stage_count: runtime_trace.stages.len(),
                        stage_plan: plan_coverage.plan,
                        stage_plan_expected_count: plan_coverage.expected_stage_count,
                        stage_plan_covered: plan_coverage.covered,
                        bp_iterations: iterations,
                        bp_converged: converged,
                        bp_nodes: beliefs.len(),
                        bp_edges: edges.len(),
                        bp_master_graph_edges,
                        bp_master_runtime_edges: master_edges.len(),
                        bp_build_inputs_ms: bp_build_inputs_elapsed.as_millis() as u64,
                        bp_run_ms: bp_run_elapsed.as_millis() as u64,
                        bp_message_trace_write_ms: bp_message_trace_write_elapsed.as_millis()
                            as u64,
                        bp_marginals_write_ms: bp_marginals_write_elapsed.as_millis() as u64,
                        bp_shadow_observed_incident_edges: bp_pruning_shadow
                            .observed_incident_edges,
                        bp_shadow_low_weight_edges: bp_pruning_shadow.low_weight_edges,
                        bp_shadow_retained_edges: bp_pruning_shadow.shadow_retained_edges,
                        bp_shadow_pruned_edges: bp_pruning_shadow.shadow_pruned_edges,
                        bp_shadow_stock_to_stock_edges: bp_pruning_shadow.stock_to_stock_edges,
                        bp_shadow_unknown_edges: bp_pruning_shadow.unknown_edges,
                        observed_priors: priors.values().filter(|prior| prior.observed).count(),
                        frontier_symbols: graph_frontier.symbol_count(),
                        frontier_nodes: graph_frontier.node_count(),
                        frontier_edges: graph_frontier.edge_count(),
                        frontier_hops: frontier_propagation.hops.len(),
                        frontier_candidates: frontier_candidates.len(),
                        frontier_dry_run_updates: frontier_dry_run.updates.len(),
                        frontier_dry_run_mean_abs_delta: frontier_dry_run.mean_abs_delta,
                        frontier_dry_run_max_abs_delta: frontier_dry_run.max_abs_delta,
                        frontier_pressure_cache_updates: frontier_pressure_cache.updates.len(),
                        frontier_pressure_cache_mean_abs_delta: frontier_pressure_cache
                            .mean_abs_delta,
                        frontier_pressure_cache_max_abs_delta: frontier_pressure_cache
                            .max_abs_delta,
                        frontier_pressure_gate_passed: frontier_pressure_gate.passed.len(),
                        frontier_pressure_gate_noise_floor: frontier_pressure_gate.noise_floor,
                        frontier_next_proposals: frontier_next_proposal.entries.len(),
                        frontier_loop_rounds: frontier_loop_summary.rounds.len(),
                        frontier_loop_final_proposals: frontier_loop_summary.final_proposals,
                        lead_lag_events: lead_lag_evs.len(),
                        probe_emitted: probe_forecasts.len(),
                        probe_evaluated: probe_outcomes.len(),
                        probe_pending: hk_active_probe.pending_count(),
                        probe_mean_accuracy,
                        artifact_write_errors,
                    };
                    if let Err(e) = eden::core::runtime_artifacts::write_runtime_health_tick(
                        eden::core::market::MarketId::Hk,
                        &health_tick,
                    ) {
                        eprintln!("[runtime_health] hk write failed: {}", e);
                    }
                }

                // Cluster sync: kept (noise reducer via master KG clusters).
                let cs_events = eden::pipeline::cluster_sync::detect_cluster_sync(
                    "hk",
                    &subkg_registry,
                    &clusters_str,
                    now,
                );
                if !cs_events.is_empty() {
                    if let Err(e) = eden::pipeline::cluster_sync::write_events("hk", &cs_events) {
                        eprintln!("[cluster_sync] hk write failed: {}", e);
                    }
                }
                // Structural contrast (spatial derivative along master KG).
                // Symbol vs master-KG-neighbor mean activation per NodeKind.
                // Market-wide rise cancels (no false signal); local standouts
                // fire. No history, no baseline.
                {
                    use rust_decimal::prelude::ToPrimitive;
                    // Build neighbor map from BrainGraph StockToStock edges
                    let mut neighbors: eden::pipeline::structural_contrast::NeighborMap =
                        std::collections::HashMap::new();
                    for edge_idx in brain.graph.edge_indices() {
                        if let eden::graph::graph::EdgeKind::StockToStock(s2s) =
                            &brain.graph[edge_idx]
                        {
                            let _ = s2s.similarity.to_f64();
                            let (a, b) = brain.graph.edge_endpoints(edge_idx).unwrap();
                            let sa = brain
                                .stock_nodes
                                .iter()
                                .find(|(_, i)| **i == a)
                                .map(|(s, _)| s.0.clone());
                            let sb = brain
                                .stock_nodes
                                .iter()
                                .find(|(_, i)| **i == b)
                                .map(|(s, _)| s.0.clone());
                            if let (Some(sa), Some(sb)) = (sa, sb) {
                                neighbors.entry(sa.clone()).or_default().push(sb.clone());
                                neighbors.entry(sb).or_default().push(sa);
                            }
                        }
                    }
                    let contrast_events = eden::pipeline::structural_contrast::detect_contrasts(
                        "hk",
                        &subkg_registry,
                        &neighbors,
                        Some(&sector_subkgs),
                        &symbol_to_sector_str,
                        now,
                    );
                    if !contrast_events.is_empty() {
                        // Contrast count + sample wake removed;
                        // full events in eden-contrast-hk.ndjson.
                        if let Err(e) = eden::pipeline::structural_contrast::write_events(
                            "hk",
                            &contrast_events,
                        ) {
                            eprintln!("[contrast] hk write failed: {}", e);
                        }
                    }
                }
                // Graph kinematics: velocity + acceleration + force balance
                // + zero-crossing turning-point detection. Pure physics of
                // activation field, no rules.
                {
                    let kin_events = eden::pipeline::structural_kinematics::update_and_detect(
                        "hk",
                        &subkg_registry,
                        &mut kinematics_tracker,
                        now,
                    );
                    if !kin_events.is_empty() {
                        // Kinematics count + sample wake removed;
                        // full turning points in eden-kinematics-hk.ndjson.
                        if let Err(e) =
                            eden::pipeline::structural_kinematics::write_events("hk", &kin_events)
                        {
                            eprintln!("[kinematics] hk write failed: {}", e);
                        }
                    }
                }
                // Consistency gauge — universal "broken equation of state"
                // detector. Stealth accumulation = Volume × |Price velocity|
                // decorrelation. Uses 2D residual outlier on linear fit.
                {
                    use eden::pipeline::consistency_gauge as cg;
                    let mut cs_events = Vec::new();

                    // (1) Stealth accumulation: pairs of (volume_velocity, |price_velocity|)
                    let mut pairs_vp: Vec<(String, f64, f64)> = Vec::new();
                    for (sym, _kg) in &subkg_registry.graphs {
                        let vol_v = kinematics_tracker
                            .velocity(sym, &eden::pipeline::symbol_sub_kg::NodeId::Volume)
                            .unwrap_or(0.0);
                        let pr_v = kinematics_tracker
                            .velocity(sym, &eden::pipeline::symbol_sub_kg::NodeId::LastPrice)
                            .unwrap_or(0.0)
                            .abs();
                        if vol_v.abs() > f64::EPSILON {
                            pairs_vp.push((sym.clone(), vol_v, pr_v));
                        }
                    }
                    let stealth_events = cg::residuals_2d(
                        "hk",
                        "stealth_volume_price_decoupling",
                        &pairs_vp,
                        cg::OUTLIER_PERCENTILE,
                        now,
                    );
                    // Stealth accumulation count wake removed;
                    // full events in eden-consistency-hk.ndjson.
                    cs_events.extend(stealth_events);

                    // (2) Broker role entropy: brokers whose bid/ask role
                    // distribution is unusually high-entropy (role switching)
                    let mut broker_role_sides: std::collections::HashMap<String, (u32, u32)> =
                        std::collections::HashMap::new();
                    for (_sym, kg) in &subkg_registry.graphs {
                        for edge in &kg.edges {
                            if edge.kind != eden::pipeline::symbol_sub_kg::EdgeKind::BrokerSits {
                                continue;
                            }
                            if let eden::pipeline::symbol_sub_kg::NodeId::Broker(bid) = &edge.from {
                                let entry = broker_role_sides.entry(bid.clone()).or_insert((0, 0));
                                match &edge.to {
                                    eden::pipeline::symbol_sub_kg::NodeId::BidLevel(_) => {
                                        entry.0 += 1;
                                    }
                                    eden::pipeline::symbol_sub_kg::NodeId::AskLevel(_) => {
                                        entry.1 += 1;
                                    }
                                    _ => {}
                                }
                            }
                        }
                    }
                    let broker_entropies: Vec<(String, f64)> = broker_role_sides
                        .iter()
                        .filter(|(_, (b, a))| b + a >= 3)
                        .map(|(bid, (b, a))| {
                            let total = (b + a) as f64;
                            let pb = *b as f64 / total;
                            let pa = *a as f64 / total;
                            (bid.clone(), cg::entropy(&[pb, pa]))
                        })
                        .collect();
                    let role_events = cg::outliers_1d(
                        "hk",
                        "broker_role_switch_entropy",
                        &broker_entropies,
                        cg::OUTLIER_PERCENTILE,
                        now,
                    );
                    // Broker role entropy count wake removed.
                    cs_events.extend(role_events);

                    // (3) Depth × Trade direction decoupling
                    // Normal: bid-heavy depth → net buying. Decoupled: bid
                    // stacked but net selling = defensive wall / distribution.
                    use rust_decimal::prelude::ToPrimitive;
                    let mut depth_trade_pairs: Vec<(String, f64, f64)> = Vec::new();
                    for (sym, kg) in &subkg_registry.graphs {
                        let da = kg
                            .nodes
                            .get(&eden::pipeline::symbol_sub_kg::NodeId::DepthAsymmetryTop3)
                            .and_then(|n| n.value)
                            .map(|v| v.to_f64().unwrap_or(0.0))
                            .unwrap_or(0.0);
                        let tt = kg
                            .nodes
                            .get(&eden::pipeline::symbol_sub_kg::NodeId::TradeTapeBuyMinusSell30s)
                            .and_then(|n| n.value)
                            .map(|v| v.to_f64().unwrap_or(0.0))
                            .unwrap_or(0.0);
                        if da.abs() > f64::EPSILON || tt.abs() > f64::EPSILON {
                            depth_trade_pairs.push((sym.clone(), da - 0.5, tt));
                        }
                    }
                    let depth_trade_events = cg::residuals_2d(
                        "hk",
                        "depth_trade_decoupling",
                        &depth_trade_pairs,
                        cg::OUTLIER_PERCENTILE,
                        now,
                    );
                    cs_events.extend(depth_trade_events);

                    // (4) Pressure vs Intent coherence
                    // Normal: positive capital flow → intent leans accumulation.
                    // Decoupled: pressure and belief disagree (pressure driving
                    // against stated intent).
                    let mut pi_pairs: Vec<(String, f64, f64)> = Vec::new();
                    for (sym, kg) in &subkg_registry.graphs {
                        let pc = kg
                            .nodes
                            .get(&eden::pipeline::symbol_sub_kg::NodeId::PressureCapitalFlow)
                            .and_then(|n| n.value)
                            .map(|v| v.to_f64().unwrap_or(0.0))
                            .unwrap_or(0.0);
                        let ia = kg
                            .nodes
                            .get(&eden::pipeline::symbol_sub_kg::NodeId::IntentAccumulation)
                            .and_then(|n| n.value)
                            .map(|v| v.to_f64().unwrap_or(0.0))
                            .unwrap_or(0.0);
                        let id = kg
                            .nodes
                            .get(&eden::pipeline::symbol_sub_kg::NodeId::IntentDistribution)
                            .and_then(|n| n.value)
                            .map(|v| v.to_f64().unwrap_or(0.0))
                            .unwrap_or(0.0);
                        let intent_signed = ia - id;
                        if pc.abs() > f64::EPSILON {
                            pi_pairs.push((sym.clone(), pc, intent_signed));
                        }
                    }
                    let pi_events = cg::residuals_2d(
                        "hk",
                        "pressure_intent_coherence",
                        &pi_pairs,
                        cg::OUTLIER_PERCENTILE,
                        now,
                    );
                    cs_events.extend(pi_events);

                    // (5) TradeTape 2nd derivative (acceleration of order flow)
                    let mut ttv_accels: Vec<(String, f64)> = Vec::new();
                    for (sym, _kg) in &subkg_registry.graphs {
                        let accel = kinematics_tracker.acceleration(
                            sym,
                            &eden::pipeline::symbol_sub_kg::NodeId::TradeTapeBuyMinusSell30s,
                        );
                        if let Some(a) = accel {
                            if a.abs() > f64::EPSILON {
                                ttv_accels.push((sym.clone(), a));
                            }
                        }
                    }
                    let ttv_events = cg::outliers_1d(
                        "hk",
                        "trade_tape_acceleration",
                        &ttv_accels,
                        cg::OUTLIER_PERCENTILE,
                        now,
                    );
                    cs_events.extend(ttv_events);

                    // (6) VWAP deviation velocity
                    let mut vwap_vels: Vec<(String, f64)> = Vec::new();
                    for (sym, _kg) in &subkg_registry.graphs {
                        let vel = kinematics_tracker.velocity(
                            sym,
                            &eden::pipeline::symbol_sub_kg::NodeId::VwapDeviationPct,
                        );
                        if let Some(v) = vel {
                            if v.abs() > f64::EPSILON {
                                vwap_vels.push((sym.clone(), v));
                            }
                        }
                    }
                    let vwap_events = cg::outliers_1d(
                        "hk",
                        "vwap_deviation_velocity",
                        &vwap_vels,
                        cg::OUTLIER_PERCENTILE,
                        now,
                    );
                    cs_events.extend(vwap_events);

                    // (7) Broker appearance rate (cumulative today)
                    for (sym, _kg) in &subkg_registry.graphs {
                        if let Some(per) = raw_broker_presence.for_symbol(&Symbol(sym.clone())) {
                            for (broker_id, entry) in per.bid.iter().chain(per.ask.iter()) {
                                if entry.count_present() > 0 {
                                    *broker_today_presence
                                        .entry(broker_id.to_string())
                                        .or_insert(0) += 1;
                                }
                            }
                        }
                    }
                    let broker_presence_vec: Vec<(String, f64)> = broker_today_presence
                        .iter()
                        .map(|(bid, n)| (bid.clone(), *n as f64))
                        .collect();
                    let presence_events = cg::outliers_1d(
                        "hk",
                        "broker_presence_density",
                        &broker_presence_vec,
                        cg::OUTLIER_PERCENTILE,
                        now,
                    );
                    cs_events.extend(presence_events);

                    // (8) Broker archetype drift (posterior change this cycle)
                    let mut arch_drifts: Vec<(String, f64)> = Vec::new();
                    for (bid, belief) in broker_archetype_field.per_broker_iter() {
                        // dominant posterior
                        let (idx, p) = belief
                            .probs
                            .iter()
                            .enumerate()
                            .max_by(|a, b| {
                                a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal)
                            })
                            .map(|(i, p)| (i, *p))
                            .unwrap_or((0, rust_decimal::Decimal::ZERO));
                        let arch_label = format!("{:?}", belief.variants[idx]);
                        let bkey = bid.0.to_string();
                        let drift = match broker_prev_archetype.get(&bkey) {
                            Some((prev_label, prev_p)) => {
                                if *prev_label != arch_label {
                                    1.0
                                } else {
                                    (p - prev_p).to_f64().unwrap_or(0.0).abs()
                                }
                            }
                            None => 0.0,
                        };
                        broker_prev_archetype.insert(bkey.clone(), (arch_label, p));
                        if drift > f64::EPSILON {
                            arch_drifts.push((bkey, drift));
                        }
                    }
                    let drift_events = cg::outliers_1d(
                        "hk",
                        "broker_archetype_drift",
                        &arch_drifts,
                        cg::OUTLIER_PERCENTILE,
                        now,
                    );
                    cs_events.extend(drift_events);

                    // (9) Symbol-Sector state divergence
                    // Count sector's dominant state label; emit symbols that differ
                    let mut sector_state_counts: std::collections::HashMap<
                        String,
                        std::collections::HashMap<String, u32>,
                    > = std::collections::HashMap::new();
                    for (sid, members) in &sector_members {
                        let counts = sector_state_counts.entry(sid.0.clone()).or_default();
                        for m in members {
                            if let Some(kg) = subkg_registry.get(&m.0) {
                                if let Some(node) = kg.nodes.get(
                                    &eden::pipeline::symbol_sub_kg::NodeId::StateClassification,
                                ) {
                                    if let Some(label) = node.label.as_ref() {
                                        *counts.entry(label.clone()).or_insert(0) += 1;
                                    }
                                }
                            }
                        }
                    }
                    // For each symbol, "divergence" = 1 if its label != sector's dominant label else 0
                    let mut orphan_scores: Vec<(String, f64)> = Vec::new();
                    for (sid, members) in &sector_members {
                        let counts = match sector_state_counts.get(&sid.0) {
                            Some(c) if !c.is_empty() => c,
                            _ => continue,
                        };
                        let dominant = counts
                            .iter()
                            .max_by_key(|(_, c)| *c)
                            .map(|(l, _)| l.clone());
                        if let Some(dom) = dominant {
                            for m in members {
                                if let Some(kg) = subkg_registry.get(&m.0) {
                                    if let Some(node) = kg.nodes.get(
                                        &eden::pipeline::symbol_sub_kg::NodeId::StateClassification,
                                    ) {
                                        let label = node.label.clone().unwrap_or_default();
                                        if !label.is_empty() && label != dom {
                                            orphan_scores.push((m.0.clone(), 1.0));
                                        }
                                    }
                                }
                            }
                        }
                    }
                    let orphan_events = cg::outliers_1d(
                        "hk",
                        "symbol_sector_state_orphan",
                        &orphan_scores,
                        cg::OUTLIER_PERCENTILE,
                        now,
                    );
                    cs_events.extend(orphan_events);

                    // (10) Institutional coord MI (limited to active brokers)
                    // Only brokers seen on >=10 symbols today qualify. Pair MI
                    // computed across current-tick presence.
                    let active_brokers: Vec<String> = broker_today_presence
                        .iter()
                        .filter(|(_, n)| **n >= 10)
                        .map(|(bid, _)| bid.clone())
                        .collect();
                    if active_brokers.len() >= 4 && active_brokers.len() <= 300 {
                        // Build presence set per broker this tick
                        let mut broker_sym_set: std::collections::HashMap<
                            &str,
                            std::collections::HashSet<&str>,
                        > = std::collections::HashMap::new();
                        for bid in &active_brokers {
                            broker_sym_set.insert(bid.as_str(), std::collections::HashSet::new());
                        }
                        for (sym, kg) in &subkg_registry.graphs {
                            for nid in kg.nodes.keys() {
                                if let eden::pipeline::symbol_sub_kg::NodeId::Broker(bid) = nid {
                                    if let Some(set) = broker_sym_set.get_mut(bid.as_str()) {
                                        set.insert(sym.as_str());
                                    }
                                }
                            }
                        }
                        let total_syms = subkg_registry.graphs.len() as f64;
                        let mut pair_mis: Vec<(String, f64)> = Vec::new();
                        for i in 0..active_brokers.len() {
                            for j in (i + 1)..active_brokers.len() {
                                let a = &active_brokers[i];
                                let b = &active_brokers[j];
                                let sa = broker_sym_set.get(a.as_str()).unwrap();
                                let sb = broker_sym_set.get(b.as_str()).unwrap();
                                if sa.is_empty() || sb.is_empty() {
                                    continue;
                                }
                                let inter: usize = sa.iter().filter(|x| sb.contains(*x)).count();
                                let p_a = sa.len() as f64 / total_syms;
                                let p_b = sb.len() as f64 / total_syms;
                                let p_ab = inter as f64 / total_syms;
                                let mi = cg::mutual_information_binary(p_a, p_b, p_ab);
                                pair_mis.push((format!("{}|{}", a, b), mi));
                            }
                        }
                        let mi_events = cg::outliers_1d(
                            "hk",
                            "institutional_coord_mi",
                            &pair_mis,
                            cg::OUTLIER_PERCENTILE,
                            now,
                        );
                        cs_events.extend(mi_events);
                    }

                    if let Err(e) = cg::write_events("hk", &cs_events) {
                        eprintln!("[consistency] hk write failed: {}", e);
                    }
                    // All-relationships aggregate count wake removed.
                }
                // Persistence tracker: sustained structural salience
                // (captures Case A/D organic trends that event detectors miss)
                {
                    use eden::pipeline::structural_persistence as sp;
                    use eden::pipeline::symbol_sub_kg as sk;
                    use rust_decimal::prelude::ToPrimitive;
                    // Metric A: per-symbol Pressure+Intent structural salience
                    let mut salience: Vec<(String, f64)> = Vec::new();
                    for (sym, kg) in &subkg_registry.graphs {
                        let mut s = 0.0_f64;
                        for id in [
                            sk::NodeId::PressureOrderBook,
                            sk::NodeId::PressureCapitalFlow,
                            sk::NodeId::PressureInstitutional,
                            sk::NodeId::PressureMomentum,
                            sk::NodeId::PressureVolume,
                            sk::NodeId::PressureStructure,
                            sk::NodeId::IntentAccumulation,
                            sk::NodeId::IntentDistribution,
                        ] {
                            s += kg
                                .nodes
                                .get(&id)
                                .and_then(|n| n.value)
                                .map(|v| v.abs().to_f64().unwrap_or(0.0))
                                .unwrap_or(0.0);
                        }
                        salience.push((sym.clone(), s));
                    }
                    let p_salience = sp::update_and_surface(
                        "hk",
                        "structure_salience",
                        &salience,
                        &mut persistence_tracker,
                        now,
                    );
                    // Persistence aggregate count wake removed;
                    // streaks in eden-persistence-hk.ndjson.
                    let _ = sp::write_events("hk", &p_salience);
                    // Metric B: buy/sell force imbalance
                    let mut imbalance: Vec<(String, f64)> = Vec::new();
                    for (sym, kg) in &subkg_registry.graphs {
                        let ia = kg
                            .nodes
                            .get(&sk::NodeId::IntentAccumulation)
                            .and_then(|n| n.value)
                            .map(|v| v.to_f64().unwrap_or(0.0))
                            .unwrap_or(0.0);
                        let id = kg
                            .nodes
                            .get(&sk::NodeId::IntentDistribution)
                            .and_then(|n| n.value)
                            .map(|v| v.to_f64().unwrap_or(0.0))
                            .unwrap_or(0.0);
                        imbalance.push((sym.clone(), (ia - id).abs()));
                    }
                    let p_imb = sp::update_and_surface(
                        "hk",
                        "intent_imbalance",
                        &imbalance,
                        &mut persistence_tracker,
                        now,
                    );
                    let _ = sp::write_events("hk", &p_imb);
                }
                // Expectation / Surprise (Layer 5): predictive perception.
                // Forms expectation of each tracked node for each symbol,
                // then measures squared error when observed. Surprise =
                // information content beyond what the graph already
                // models. 99%ile floor; only novel structure fires.
                {
                    let surprise_events =
                        eden::pipeline::structural_expectation::update_and_measure(
                            "hk",
                            &subkg_registry,
                            &mut expectation_tracker,
                            now,
                        );
                    // Surprise aggregate count wake removed;
                    // full surprise events in eden-surprise-hk.ndjson.
                    let _ = eden::pipeline::structural_expectation::write_events(
                        "hk",
                        &surprise_events,
                    );
                }
            }
        }
        let mut lineage_stats = compute_lineage_stats(&history, LINEAGE_WINDOW);
        lineage_accumulator.ingest(&lineage_stats, &lineage_prev_resolved);
        lineage_prev_resolved = lineage_stats
            .family_contexts
            .iter()
            .map(|entry| (entry.family.clone(), entry.resolved))
            .collect();
        lineage_stats.enrich_with_cumulative(&lineage_accumulator);

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
                &live,
                &tracker,
                &causal_timelines,
                &dynamics,
                &previous_symbol_states,
                &previous_cluster_states,
                previous_world_summary.as_ref(),
            );
            previous_symbol_states = live_snapshot.symbol_states.clone();
            previous_cluster_states = live_snapshot.cluster_states.clone();
            previous_world_summary = live_snapshot.world_summary.clone();
            // Y#1 — augment each symbol's state with raw-layer evidence.
            // Eden has already classified state_kind via the feature
            // pipeline above; now we ask the hand-coded raw templates
            // whether the actual broker / depth sequence this tick
            // confirms or contradicts that classification. The output
            // flows into the existing supporting_evidence /
            // opposing_evidence arrays (Y-70 reason_codes preserved —
            // every new code is prefixed `raw:`).
            let mut live_snapshot = live_snapshot;
            for state in &mut live_snapshot.symbol_states {
                let outcome = eden::pipeline::raw_expectation::evaluate_raw_expectations(
                    state.state_kind,
                    &Symbol(state.symbol.clone()),
                    &raw_broker_presence,
                    &raw_depth_levels,
                    &raw_trade_tape,
                );
                state.supporting_evidence.extend(outcome.supporting);
                state.opposing_evidence.extend(outcome.opposing);

                // Warrant sentiment (activated 2026-04-17 from dead-code
                // scaffolding). Call/put count imbalance + weighted IV gap
                // feed into the underlying's evidence — retail warrant
                // positioning is a real HK driver, and its presence /
                // absence is now part of Eden's read.
                if let Some(warrant) = rest.warrants.get(&Symbol(state.symbol.clone())) {
                    let total = (warrant.call_warrant_count + warrant.put_warrant_count) as i64;
                    if total > 0 {
                        let call_share =
                            rust_decimal::Decimal::from(warrant.call_warrant_count as i64)
                                / rust_decimal::Decimal::from(total);
                        let put_share =
                            rust_decimal::Decimal::from(warrant.put_warrant_count as i64)
                                / rust_decimal::Decimal::from(total);
                        // Strong call dominance (>= 67% of warrants are calls) ⇒
                        // retail lean bullish. Surfaces as supporting evidence for
                        // buy-direction states, opposing for sell-direction states.
                        let direction = state.direction.as_deref();
                        if call_share >= rust_decimal_macros::dec!(0.67) {
                            let code = "raw:warrant_call_dominant".to_string();
                            let summary = format!(
                                "{} warrant market {}/{} calls (retail lean bullish)",
                                state.symbol, warrant.call_warrant_count, total
                            );
                            let weight = rust_decimal_macros::dec!(0.10);
                            if direction == Some("buy") {
                                state.supporting_evidence.push(
                                    eden::pipeline::state_engine::PersistentStateEvidence {
                                        code,
                                        summary,
                                        weight,
                                    },
                                );
                            } else if direction == Some("sell") {
                                state.opposing_evidence.push(
                                    eden::pipeline::state_engine::PersistentStateEvidence {
                                        code,
                                        summary,
                                        weight,
                                    },
                                );
                            }
                        } else if put_share >= rust_decimal_macros::dec!(0.67) {
                            let code = "raw:warrant_put_dominant".to_string();
                            let summary = format!(
                                "{} warrant market {}/{} puts (retail lean bearish)",
                                state.symbol, warrant.put_warrant_count, total
                            );
                            let weight = rust_decimal_macros::dec!(0.10);
                            if direction == Some("sell") {
                                state.supporting_evidence.push(
                                    eden::pipeline::state_engine::PersistentStateEvidence {
                                        code,
                                        summary,
                                        weight,
                                    },
                                );
                            } else if direction == Some("buy") {
                                state.opposing_evidence.push(
                                    eden::pipeline::state_engine::PersistentStateEvidence {
                                        code,
                                        summary,
                                        weight,
                                    },
                                );
                            }
                        }
                        // IV skew: weighted put IV materially above weighted
                        // call IV ⇒ market paying up for downside insurance.
                        if let (Some(call_iv), Some(put_iv)) =
                            (warrant.weighted_call_iv, warrant.weighted_put_iv)
                        {
                            let skew = put_iv - call_iv;
                            if skew >= rust_decimal_macros::dec!(0.05) {
                                state.opposing_evidence.push(
                                    eden::pipeline::state_engine::PersistentStateEvidence {
                                        code: "raw:warrant_put_iv_premium".into(),
                                        summary: format!(
                                            "{} warrant IV skew: put {} vs call {} (downside insurance demand)",
                                            state.symbol,
                                            put_iv.round_dp(3),
                                            call_iv.round_dp(3)
                                        ),
                                        weight: rust_decimal_macros::dec!(0.08),
                                    },
                                );
                            }
                        }
                    }
                }
            }
            // State classification → sub-KG StateClassification node label.
            // Done here (not earlier sub-KG block) because live_snapshot is
            // built later in the tick.
            {
                use eden::pipeline::symbol_sub_kg as sk;
                let now = chrono::Utc::now();
                let states_map: std::collections::HashMap<String, String> = live_snapshot
                    .symbol_states
                    .iter()
                    .map(|s| (s.symbol.clone(), format!("{:?}", s.state_kind)))
                    .collect();
                sk::update_from_state(&mut subkg_registry, &states_map, now, tick);
            }

            // Regime analog was recorded before sub-KG evidence
            // construction. Build a local post-BP fingerprint here only
            // for operator-facing per-symbol regime contrast; do not
            // record it into the analog index a second time.
            if let Some(ref world_summary) = live_snapshot.world_summary {
                use rust_decimal::prelude::ToPrimitive as _;
                let stress = graph_insights
                    .stress
                    .composite_stress
                    .to_f64()
                    .unwrap_or(0.0);
                let synchrony = graph_insights
                    .stress
                    .sector_synchrony
                    .to_f64()
                    .unwrap_or(0.0);
                let dominant_driver = world_summary
                    .dominant_clusters
                    .iter()
                    .find_map(|s| s.strip_prefix("driver:"))
                    .map(|s| s.to_string());
                let snapshot_ts =
                    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                let hk_regime_fp = eden::pipeline::regime_fingerprint::build_hk_fingerprint(
                    "hk",
                    tick,
                    snapshot_ts,
                    stress,
                    synchrony,
                    &live_snapshot.cluster_states,
                    world_summary,
                    dominant_driver,
                );

                // Per-symbol regime fingerprint surface. Lets the operator
                // see "this symbol is cleanly trending despite the market
                // being locked in chop" — the 9636 / 1171 case from
                // 2026-04-23. Emits only when the symbol's bucket_key
                // diverges from the market bucket AND the setup is
                // actionable (enter/observe). Caps to top 5 most-divergent
                // per tick so wake stream isn't swamped on chop days with
                // many candidates.
                {
                    use eden::pipeline::regime_fingerprint::build_symbol_fingerprint;
                    let market_bucket = hk_regime_fp.bucket_key.clone();
                    let market_turn = hk_regime_fp.turn_pressure;
                    let market_bias = hk_regime_fp.bull_bias;
                    let market_driver = hk_regime_fp.dominant_driver.clone();
                    let market_legacy = hk_regime_fp.legacy_label.clone();
                    let snapshot_ts = hk_regime_fp.snapshot_ts.clone();
                    let mut candidates: Vec<(String, String, f64)> = Vec::new();
                    for setup in reasoning_snapshot.tactical_setups.iter() {
                        let symbol = match &setup.scope {
                            eden::ontology::ReasoningScope::Symbol(s) => s,
                            _ => continue,
                        };
                        if !matches!(
                            setup.action,
                            TacticalAction::Enter | TacticalAction::Observe
                        ) {
                            continue;
                        }
                        let action_str = setup.action.as_str();
                        let Some(stats) = node_registry.stock_regime_stats(symbol) else {
                            continue;
                        };
                        use rust_decimal::prelude::ToPrimitive as _;
                        let confidence = setup.confidence.to_f64().unwrap_or(0.0).clamp(0.0, 1.0);
                        let stress = stats.turn_pressure();
                        let bull_bias = stats.bull_bias();
                        let activity = confidence;
                        let synchrony =
                            setup.confidence_gap.to_f64().unwrap_or(0.0).clamp(0.0, 1.0);
                        let turn_pressure = stress;
                        let sym_fp = build_symbol_fingerprint(
                            "hk",
                            tick,
                            snapshot_ts.as_str(),
                            &symbol.0,
                            stress,
                            synchrony,
                            bull_bias,
                            activity,
                            turn_pressure,
                            Some(stats.last_seen_tick.saturating_sub(stats.first_seen_tick)),
                            market_driver.clone(),
                            market_legacy.clone(),
                        );
                        if sym_fp.bucket_key == market_bucket {
                            continue;
                        }
                        let divergence = (sym_fp.turn_pressure - market_turn).abs()
                            + (sym_fp.bull_bias - market_bias).abs();
                        let direction_str = match setup.direction {
                            Some(eden::ontology::reasoning::TacticalDirection::Long) => "long",
                            Some(eden::ontology::reasoning::TacticalDirection::Short) => "short",
                            None => "?",
                        };
                        candidates.push((
                            symbol.0.clone(),
                            format!(
                                "[hk] sym_regime: {} action={} dir={} conf={:.2} bucket={} stress={:.2} sync={:.2} bias={:.2} act={:.2} turn={:.2} market_bucket={} divergence={:.2}",
                                symbol.0,
                                action_str,
                                direction_str,
                                confidence,
                                sym_fp.bucket_key,
                                sym_fp.stress,
                                sym_fp.synchrony,
                                sym_fp.bull_bias,
                                sym_fp.activity,
                                sym_fp.turn_pressure,
                                market_bucket,
                                divergence,
                            ),
                            divergence,
                        ));
                    }
                    candidates
                        .sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
                    for (_, line, _) in candidates.into_iter().take(5) {
                        eprintln!("{}", line);
                    }
                }
            }

            // Self-doubt surface: for each enter/review setup, look up the
            // underlying symbol's opposing_evidence + missing_evidence and
            // emit a top-level wake line if contradicting_* codes are
            // present. Eden already records these inside Symbol states
            // section, but operators routinely miss it (cycle 9/16/17 of
            // 2026-04-23 each had stale-mod_stack-vs-live-raw mismatches
            // that Eden flagged as `against=contradicting_raw_channels`
            // but operator did not grep the buried sub-section). This
            // promotes the doubt signal into the main wake stream.
            let symbol_state_lookup: std::collections::HashMap<&str, _> = live_snapshot
                .symbol_states
                .iter()
                .map(|state| (state.symbol.as_str(), state))
                .collect();
            for setup in &reasoning_snapshot.tactical_setups {
                if !matches!(setup.action, TacticalAction::Enter | TacticalAction::Review) {
                    continue;
                }
                let action_str = setup.action.as_str();
                let symbol_label = match &setup.scope {
                    eden::ontology::ReasoningScope::Symbol(sym) => sym.0.as_str(),
                    _ => continue,
                };
                let Some(state) = symbol_state_lookup.get(symbol_label) else {
                    continue;
                };
                // Severity gate: only emit when the doubt is structurally
                // significant — single-channel contradictions in a chop
                // regime fire too often to be useful (~1 per setup per
                // tick during reversal_prone). Require either:
                //   (a) total contradicting weight >= 0.18 (≥2 channels),
                //   (b) contradicting + at least one missing piece (multi
                //       source doubt), OR
                //   (c) action == enter AND any contradicting (entry
                //       commitment deserves the lower bar)
                let contradicting_iter = || {
                    state
                        .opposing_evidence
                        .iter()
                        .filter(|item| item.code.starts_with("contradicting_"))
                };
                let against: Vec<&str> = contradicting_iter()
                    .map(|item| item.code.as_str())
                    .collect();
                let contradict_weight: rust_decimal::Decimal =
                    contradicting_iter().map(|item| item.weight).sum();
                let missing: Vec<&str> = state
                    .missing_evidence
                    .iter()
                    .map(|item| item.code.as_str())
                    .take(2)
                    .collect();
                let is_enter = matches!(setup.action, TacticalAction::Enter);
                let multi_channel = contradict_weight >= rust_decimal_macros::dec!(0.18);
                let multi_source = !against.is_empty() && !missing.is_empty();
                if !multi_channel && !multi_source && !is_enter {
                    continue;
                }
                if against.is_empty() && missing.is_empty() {
                    continue;
                }
                eprintln!(
                    "[hk] self-doubt: {} action={} state={} weight={} against=[{}] missing=[{}]",
                    symbol_label,
                    action_str,
                    state.state_kind,
                    contradict_weight.round_dp(2),
                    against.join(","),
                    missing.join(","),
                );
            }

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
                hk_momentum: Some(&hk_momentum),
            });
            // Y#7 — feed this tick's perception event counts into the
            // market wave tracker and append any accelerating / peaking /
            // receding wave narrative to the agent snapshot's
            // wake.reasons before the snapshot is persisted downstream.
            let mut artifact_projection = artifact_projection;
            stage_timer.mark("S14_S19_state_workflow_projection");
            let absence_count = artifact_projection
                .agent_snapshot
                .perception_states
                .iter()
                .filter(|state| {
                    state
                        .reason_codes
                        .iter()
                        .any(|code| code == "demoted_by_absence")
                })
                .count();
            let expectation_error_count = artifact_projection
                .agent_snapshot
                .perception_states
                .iter()
                .filter(|state| {
                    state
                        .reason_codes
                        .iter()
                        .any(|code| code.starts_with("expectation_error:"))
                })
                .count();
            let momentum_collapse_count = [
                &hk_momentum.institutional_flow,
                &hk_momentum.depth_imbalance,
                &hk_momentum.trade_aggression,
            ]
            .into_iter()
            .flat_map(|map| map.values())
            .filter(|entry| entry.is_collapsing())
            .count();
            market_waves.record_tick(
                absence_count,
                expectation_error_count,
                momentum_collapse_count,
            );
            let wave_reasons = market_waves.describe();
            artifact_projection
                .agent_snapshot
                .wake
                .reasons
                .extend(wave_reasons);

            // T5 — surface residual field + hidden force output to wake
            // reasons. Both had been live in the runtime since Codex's
            // original residual.rs work but only reached stdout via
            // eprintln!; LLM analyst and operator never saw them.
            //
            // Residual clusters with coherence >= 0.70 represent
            // coordinated institutional action across a sector (e.g.
            // insurance residual=-0.287 coherence=1.00 means all four
            // tracked insurance names diverge from graph prediction in
            // the same institutional direction — Le Verrier-style hidden
            // force signal).
            let coherent_clusters: Vec<_> = residual_field
                .clustered_sectors
                .iter()
                .filter(|c| c.coherence.abs() >= rust_decimal_macros::dec!(0.70))
                .collect();
            for cluster in coherent_clusters.iter().take(4) {
                let direction = if cluster.mean_residual < rust_decimal::Decimal::ZERO {
                    "selling"
                } else {
                    "buying"
                };
                artifact_projection.agent_snapshot.wake.reasons.push(format!(
                    "residual: {} sector coherent {} (residual={}, coherence={}, {} symbols, dim={})",
                    cluster.sector.0,
                    direction,
                    cluster.mean_residual.round_dp(3),
                    cluster.coherence.round_dp(2),
                    cluster.symbol_count,
                    cluster.dominant_dimension.label()
                ));
            }
            // Hidden force summary — currently confirmed forces that
            // residual verification has validated. Each confirmed force
            // is a Le Verrier-style hidden driver that Eden inferred
            // from residual patterns and watched materialize over
            // subsequent ticks.
            let confirmed_forces = hidden_force_state
                .confirmed_forces()
                .iter()
                .map(|tracker| tracker.symbol.0.clone())
                .collect::<Vec<_>>();
            if let Some(line) = crate::core::wake_surface::hidden_forces_reason(&confirmed_forces) {
                artifact_projection.agent_snapshot.wake.reasons.push(line);
            }

            // T5 continued — GraphInsights inst_exoduses / inst_rotations /
            // MarketStressIndex. These are computed every tick inside
            // graph_insights but never made it to wake.reasons. The
            // InstitutionExodus case in particular ("this institution
            // held 30 symbols last tick, now holds 18") is a leading
            // indicator that the existing aggregate institutional_flow
            // scalar cannot surface.
            let top_exoduses: Vec<_> = graph_insights.inst_exoduses.iter().take(3).collect();
            for exodus in &top_exoduses {
                let name = store
                    .institutions
                    .get(&exodus.institution_id)
                    .map(|i| i.name_en.clone())
                    .unwrap_or_else(|| format!("{:?}", exodus.institution_id));
                artifact_projection
                    .agent_snapshot
                    .wake
                    .reasons
                    .push(format!(
                        "institution exodus: {} dropped {} stocks ({} → {})",
                        name,
                        exodus.dropped_count,
                        exodus.prev_stock_count,
                        exodus.curr_stock_count,
                    ));
            }
            let top_rotations: Vec<_> = graph_insights
                .inst_rotations
                .iter()
                .filter(|r| {
                    r.buy_symbols.len() + r.sell_symbols.len() >= 3
                        && r.net_direction.abs() >= rust_decimal_macros::dec!(0.2)
                })
                .take(3)
                .collect();
            for rot in &top_rotations {
                let name = store
                    .institutions
                    .get(&rot.institution_id)
                    .map(|i| i.name_en.clone())
                    .unwrap_or_else(|| format!("{:?}", rot.institution_id));
                artifact_projection
                    .agent_snapshot
                    .wake
                    .reasons
                    .push(format!(
                        "institution rotation: {} (buy {}, sell {}, net={})",
                        name,
                        rot.buy_symbols.len(),
                        rot.sell_symbols.len(),
                        rot.net_direction.round_dp(2),
                    ));
            }
            // Shift A: advance latent_world_state from current tick
            // aggregates. Kalman filter step then emit a summary line.
            // v1 feeds stress + synchrony; breadth/institutional/retail
            // dims stay masked (SSM mean-reverts them toward zero) —
            // those aggregators come in a follow-up.
            {
                use rust_decimal::prelude::ToPrimitive as _;
                let obs = eden::pipeline::latent_world_state::aggregate_observation(
                    &eden::pipeline::latent_world_state::ObservationInputs {
                        market_stress: Some(
                            graph_insights
                                .stress
                                .composite_stress
                                .to_f64()
                                .unwrap_or(0.0),
                        ),
                        synchrony: Some(
                            graph_insights
                                .stress
                                .sector_synchrony
                                .to_f64()
                                .unwrap_or(0.0),
                        ),
                        ..Default::default()
                    },
                );
                latent_world_state.step(tick, obs);
                artifact_projection
                    .agent_snapshot
                    .wake
                    .reasons
                    .push(latent_world_state.summary_line());

                // Shift B: one SCM cascade wake line — "if stress
                // jumped +1 above current, here's what propagates."
                // Provides operator + LLM a "what would move together"
                // counterfactual read.
                let stress_now = latent_world_state.dim_value(0).unwrap_or(0.0);
                let scm_line = hk_scm.describe_intervention(0, stress_now + 1.0);
                artifact_projection
                    .agent_snapshot
                    .wake
                    .reasons
                    .push(scm_line);

                // Shift C: counterfactual rollout planner. Score a
                // small action set (baseline + ±0.5 stress / inst_flow)
                // by rolling each forward 10 ticks through SCM + SSM
                // dynamics under operator utility. Emits best action
                // plus top runners-up for operator attention.
                let actions = eden::pipeline::counterfactual_planner::default_candidate_set(
                    &latent_world_state,
                );
                if let Some(summary) = eden::pipeline::counterfactual_planner::best_action(
                    &latent_world_state,
                    &hk_scm,
                    &actions,
                    10,
                    eden::pipeline::counterfactual_planner::operator_utility,
                ) {
                    artifact_projection
                        .agent_snapshot
                        .wake
                        .reasons
                        .push(summary.summary_line());
                }
            }

            // Market stress composite — surface when elevated so it
            // contextualizes everything else in wake.reasons.
            if graph_insights.stress.composite_stress >= rust_decimal_macros::dec!(0.5) {
                artifact_projection.agent_snapshot.wake.reasons.push(
                    crate::core::wake_surface::format_labeled_decimal_fields(
                        "market stress elevated",
                        &[
                            ("composite", graph_insights.stress.composite_stress),
                            ("synchrony", graph_insights.stress.sector_synchrony),
                            ("consensus", graph_insights.stress.pressure_consensus),
                        ],
                    ),
                );
            }

            // T6 — backward reasoning investigations → wake. Each
            // investigation is a leaf entity with a leading hypothesized
            // cause plus a runner-up, plus the streak of ticks the leader
            // has held. Eden already prints these to the console but the
            // LLM analyst has never seen them structured in wake.
            let top_investigations: Vec<_> = artifact_projection
                .agent_snapshot
                .backward_reasoning
                .as_ref()
                .map(|bw| bw.investigations.iter().take(3).collect::<Vec<_>>())
                .unwrap_or_default();
            for inv in &top_investigations {
                if let Some(cause) = inv.leading_cause.as_ref() {
                    artifact_projection.agent_snapshot.wake.reasons.push(
                        crate::core::wake_surface::format_backward_reason(
                            &inv.leaf_label,
                            &cause.explanation,
                            Some(inv.leading_cause_streak),
                            cause.confidence,
                            Some("leading cause "),
                        ),
                    );
                }
            }

            // T7 — cross-market signals → wake. HK↔US bridges already
            // computed into live.cross_market_signals; most live HK
            // symbols never had these reach operator-facing narrative.
            artifact_projection.agent_snapshot.wake.reasons.extend(
                crate::core::wake_surface::cross_market_reason_lines(
                    &artifact_projection.agent_snapshot.cross_market_signals,
                    3,
                ),
            );

            // T8 — graph insights shared_holders (portfolio correlation
            // anomalies across sector boundaries). High Jaccard with many
            // shared institutions = structural correlation even without
            // sector overlap.
            let top_shared: Vec<_> = graph_insights
                .shared_holders
                .iter()
                .filter(|s| {
                    s.jaccard >= rust_decimal_macros::dec!(0.60) && s.shared_institutions >= 3
                })
                .take(2)
                .collect();
            for shared in &top_shared {
                artifact_projection
                    .agent_snapshot
                    .wake
                    .reasons
                    .push(format!(
                        "shared holders anomaly: {} ~ {} (jaccard={}, {} institutions)",
                        shared.symbol_a,
                        shared.symbol_b,
                        shared.jaccard.round_dp(2),
                        shared.shared_institutions,
                    ));
            }

            // T22 symbol-anchored inference chain deleted — the chain
            // assembled a forest-level narrative ("9 brokers cross 29
            // clusters, 157 buy/26 sell, backed by 5 institutions") by
            // rule-based combination of primitives. Each primitive now
            // stands alone in the sub-KG / ndjson streams; narrative
            // assembly is the operator's job, not Eden's.

            // Drift #6 parity — stock cluster alignment. US had this
            // wake line (UsStockCluster); HK has the same shape
            // (graph::insights::StockCluster: members,
            // directional_alignment, stability, age) but never pushed
            // it. Surface top cluster with >= 4 members, aligned
            // magnitude >= 0.6, stability >= 0.5.
            {
                let mut top_clusters: Vec<&eden::graph::insights::StockCluster> = graph_insights
                    .clusters
                    .iter()
                    .filter(|c| {
                        c.members.len() >= 4
                            && c.directional_alignment.abs() >= rust_decimal_macros::dec!(0.60)
                            && c.stability >= rust_decimal_macros::dec!(0.50)
                    })
                    .collect();
                top_clusters.sort_by(|a, b| b.stability.cmp(&a.stability));
                if let Some(cluster) = top_clusters.first() {
                    let members = cluster
                        .members
                        .iter()
                        .map(|symbol| symbol.0.clone())
                        .collect::<Vec<_>>();
                    artifact_projection.agent_snapshot.wake.reasons.push(
                        crate::core::wake_surface::format_stock_cluster_reason(
                            &members,
                            cluster.directional_alignment,
                            cluster.stability,
                            cluster.age,
                        ),
                    );
                }
            }

            // Drift #4 parity — stability-gated TurningPoint /
            // Continuation surface. US had this; HK didn't. The
            // state_engine produces high-confidence reads at tick
            // boundaries that flip with the wind; surfacing only
            // symbols with state_persistence_ticks >= 3 gives the
            // operator a low-noise persistent view.
            artifact_projection.agent_snapshot.wake.reasons.extend(
                crate::core::wake_surface::stable_state_reason_lines(
                    &artifact_projection.live_snapshot.symbol_states,
                    5,
                ),
            );

            // T27 W3 integration — forward-propagation intervention
            // effect estimator. For the top vortex this tick, simulate a
            // do(X = top_direction) intervention and surface the 3
            // strongest cascade targets along the BrainGraph's stock↔
            // stock similarity edges. Honest framing: this is NOT full
            // Pearl do-calculus — no Structural Causal Model, just
            // attenuated forward BFS on correlation-as-proxy. Emitted
            // only when the top vortex has a meaningful direction so
            // wake.reasons doesn't fill with intervention-lines that say
            // nothing.
            if let Some(top_vortex) = pressure_field.vortices.first() {
                let direction_magnitude = top_vortex.tick_direction.abs();
                if direction_magnitude >= rust_decimal_macros::dec!(0.1) {
                    use rust_decimal::prelude::ToPrimitive;
                    let causal_view = eden::graph::causal_view::BrainGraphCausalView::new(&brain);
                    let intervention_sign = top_vortex.tick_direction.to_f64().unwrap_or(0.0);
                    let effects = eden::pipeline::intervention::propagate_intervention(
                        &causal_view,
                        &top_vortex.symbol,
                        intervention_sign,
                        2, // 2 hops — beyond that effect is usually below noise floor
                        0.7,
                    );
                    if !effects.is_empty() {
                        let summary: Vec<String> = effects
                            .iter()
                            .take(3)
                            .map(|e| {
                                format!(
                                    "{}({}{:.2})",
                                    e.target.0,
                                    if e.expected_effect >= 0.0 { "+" } else { "" },
                                    e.expected_effect,
                                )
                            })
                            .collect();
                        artifact_projection
                            .agent_snapshot
                            .wake
                            .reasons
                            .push(format!(
                                "intervention: if {} moves {:+.2}, cascade targets: {}",
                                top_vortex.symbol.0,
                                intervention_sign,
                                summary.join(", "),
                            ));
                    }
                }
            }

            // T18 — Eden track record ledger. Rolling summary of all
            // resolved tactical outcomes accumulated across this process
            // lifetime (hydrated on startup from the last 500 DB records).
            // This is the "is Y making money" surface — without it, every
            // Y claim is unfalsifiable. We emit only when there are at
            // least 10 observations so the line isn't dominated by noise.
            if eden_ledger.len() >= 10 {
                if let Some(summary) = eden_ledger.summary() {
                    artifact_projection
                        .agent_snapshot
                        .wake
                        .reasons
                        .push(summary.wake_line());
                }
            }

            // T2 warrant surface — aggregate call/put dominance across
            // the underlyings we fetched this cycle. Per-symbol warrant
            // evidence is already on each PersistentSymbolState's
            // reason_codes, but operator was blind to the market-wide
            // picture. Also flag the top IV skew (demand for downside
            // insurance) when present.
            if !rest.warrants.is_empty() {
                let mut call_dominant = 0usize;
                let mut put_dominant = 0usize;
                let mut balanced = 0usize;
                let mut put_iv_premium_symbols: Vec<String> = Vec::new();
                for (symbol, warrant) in &rest.warrants {
                    let total = warrant.call_warrant_count + warrant.put_warrant_count;
                    if total == 0 {
                        continue;
                    }
                    let call_share = rust_decimal::Decimal::from(warrant.call_warrant_count as i64)
                        / rust_decimal::Decimal::from(total as i64);
                    if call_share >= rust_decimal_macros::dec!(0.67) {
                        call_dominant += 1;
                    } else if call_share <= rust_decimal_macros::dec!(0.33) {
                        put_dominant += 1;
                    } else {
                        balanced += 1;
                    }
                    if let (Some(call_iv), Some(put_iv)) =
                        (warrant.weighted_call_iv, warrant.weighted_put_iv)
                    {
                        if put_iv - call_iv >= rust_decimal_macros::dec!(0.05) {
                            put_iv_premium_symbols.push(symbol.0.clone());
                        }
                    }
                }
                let total_tracked = call_dominant + put_dominant + balanced;
                if total_tracked > 0 {
                    artifact_projection.agent_snapshot.wake.reasons.push(format!(
                        "warrant market ({} underlyings): {} call-dominant, {} put-dominant, {} balanced",
                        total_tracked, call_dominant, put_dominant, balanced,
                    ));
                }
                if !put_iv_premium_symbols.is_empty() {
                    put_iv_premium_symbols.sort();
                    let shown = put_iv_premium_symbols
                        .iter()
                        .take(5)
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ");
                    let suffix = if put_iv_premium_symbols.len() > 5 {
                        format!(" (+{} more)", put_iv_premium_symbols.len() - 5)
                    } else {
                        String::new()
                    };
                    artifact_projection
                        .agent_snapshot
                        .wake
                        .reasons
                        .push(format!(
                            "warrant put-IV premium ({}): {}{}",
                            put_iv_premium_symbols.len(),
                            shown,
                            suffix,
                        ));
                }
            }
            // ── Belief field update + wake + snapshot ──
            stage_timer.mark("S20_wake_surface");
            // Update from freshly-built pressure field (tick-scale) plus
            // current symbol states, emit notable belief wake lines, and
            // write a snapshot every 60s via tokio::spawn (non-blocking).
            {
                use eden::ontology::objects::Symbol as HkSymbol;
                use eden::pipeline::pressure::TimeScale;

                if let Some(tick_layer) = pressure_field.layers.get(&TimeScale::Tick) {
                    let samples: Vec<(
                        HkSymbol,
                        eden::pipeline::pressure::PressureChannel,
                        rust_decimal::Decimal,
                    )> = tick_layer
                        .pressures
                        .iter()
                        .flat_map(|(symbol, node)| {
                            node.channels
                                .iter()
                                .map(|(channel, cp)| (symbol.clone(), *channel, cp.net()))
                        })
                        .collect();
                    // Intent belief: group per-symbol slices and feed.
                    // (Gaussian belief keeps the flat iterator form.)
                    let mut by_symbol: std::collections::HashMap<
                        HkSymbol,
                        Vec<(
                            eden::pipeline::pressure::PressureChannel,
                            rust_decimal::Decimal,
                        )>,
                    > = std::collections::HashMap::new();
                    for (sym, ch, p) in &samples {
                        by_symbol.entry(sym.clone()).or_default().push((*ch, *p));
                    }
                    for (symbol, channel_samples) in &by_symbol {
                        intent_belief_field.record_channel_samples(symbol, channel_samples);
                    }
                    // belief_field.update_from_pressure_samples moved earlier
                    // (next to kl_surprise_tracker.observe_from_belief_field)
                    // so the V4 KL surprise NodeIds populate this tick rather
                    // than always reading 1-tick-stale beliefs.
                    let _ = samples;
                }
                for state in &artifact_projection.live_snapshot.symbol_states {
                    belief_field
                        .record_state_sample(&HkSymbol(state.symbol.clone()), state.state_kind);
                }
                for notable in belief_field.top_notable_beliefs(5) {
                    let symbol_for_decisions = match &notable {
                        eden::pipeline::belief_field::NotableBelief::Gaussian {
                            symbol, ..
                        }
                        | eden::pipeline::belief_field::NotableBelief::Categorical {
                            symbol, ..
                        } => symbol.clone(),
                    };
                    artifact_projection
                        .agent_snapshot
                        .wake
                        .reasons
                        .push(eden::pipeline::belief_field::format_wake_line(&notable));
                    if let Some(summary) = decision_ledger.summary_for(&symbol_for_decisions) {
                        if summary.total_decisions >= 1 {
                            artifact_projection
                                .agent_snapshot
                                .wake
                                .reasons
                                .push(eden::pipeline::decision_ledger::wake_format::format_prior_decisions_line(
                                    &symbol_for_decisions,
                                    summary,
                                ));
                        }
                    }
                }

                // Attention wake: top-5 symbols by state-posterior entropy.
                // Complements belief notable — notable is event-driven
                // (this symbol just shifted); attention is state-snapshot
                // (this symbol is currently most uncertain).
                for item in belief_field.top_attention(5) {
                    artifact_projection
                        .agent_snapshot
                        .wake
                        .reasons
                        .push(eden::pipeline::belief_field::format_attention_line(&item));
                }

                // Cross-ontology intent wake: top-5 symbols where the
                // world-space posterior has a dominant interpretation
                // (accumulation / distribution / rotation / volatility).
                // min_samples=10 and min_dominance=0.5 keep this from
                // firing on single-tick jitter.
                for decision in intent_belief_field.top_decisive(5, 10, 0.5) {
                    artifact_projection.agent_snapshot.wake.reasons.push(
                        eden::pipeline::intent_belief::format_intent_wake_line(&decision),
                    );
                }

                // Broker-level belief (ontology-entity posterior).
                // Top-5 brokers where a behavioral archetype dominates.
                // This is the first wake line driven by ontology-entity
                // (not symbol) state — re-centering KG/ontology in
                // reasoning after a day of symbol-keyed additions.
                for verdict in broker_archetype_field.top_confident_archetypes(5, 20, 0.45) {
                    artifact_projection.agent_snapshot.wake.reasons.push(
                        eden::pipeline::broker_archetype::format_broker_archetype_line(&verdict),
                    );
                }

                // Institution-level belief: broker archetypes rolled up
                // to KG Institution entity. Sample-weighted posterior
                // average across each institution's brokers. Top-3
                // institutions where a blended archetype dominates.
                for inst_verdict in
                    eden::pipeline::institution_archetype::top_confident_institutions(
                        &store.institutions,
                        &broker_archetype_field,
                        3,
                        0.40,
                    )
                {
                    let name = store
                        .institutions
                        .get(&inst_verdict.institution_id)
                        .map(|i| i.name_en.as_str())
                        .unwrap_or("?");
                    artifact_projection.agent_snapshot.wake.reasons.push(
                        eden::pipeline::institution_archetype::format_institution_archetype_line(
                            &inst_verdict,
                            name,
                        ),
                    );
                }

                // Sector-level intent belief: aggregate per-symbol
                // IntentBelief posteriors up to KG Sector entity.
                // Top-3 sectors with confident consensus intent.
                for sector_verdict in eden::pipeline::sector_intent::top_confident_sectors(
                    &sector_names,
                    &sector_members,
                    eden::ontology::objects::Market::Hk,
                    &intent_belief_field,
                    3,
                    0.40,
                ) {
                    artifact_projection.agent_snapshot.wake.reasons.push(
                        eden::pipeline::sector_intent::format_sector_intent_line(&sector_verdict),
                    );
                }

                // Ontology-gap wake (Y#0 seed): vortex fingerprints the
                // classifier labels inconsistently + that recur enough
                // to be real. Seeds the emergence proposer.
                for summary in residual_pattern_tracker.top_residual_patterns(3, 8) {
                    artifact_projection
                        .agent_snapshot
                        .wake
                        .reasons
                        .push(
                            eden::pipeline::ontology_emergence::ResidualPatternTracker::format_wake_line(
                                &summary,
                            ),
                        );
                }

                // Ontology PROPOSAL wake (Y#0 second piece): when a
                // residual pattern persists above threshold for N
                // consecutive ticks, emit a proposal suggesting a new
                // entity type. Each proposal fires once per session.
                for proposal in residual_pattern_tracker.evaluate_proposals(chrono::Utc::now()) {
                    artifact_projection.agent_snapshot.wake.reasons.push(
                        eden::pipeline::ontology_emergence::ResidualPatternTracker::format_proposal_wake_line(
                            &proposal,
                        ),
                    );
                }

                // 60s rescan — picks up new decisions Claude Code wrote
                // during the session. Independent of persistence feature
                // (it's just filesystem).
                let should_rescan_decisions = match decision_ledger.last_scan_ts() {
                    None => true,
                    Some(prev) => (chrono::Utc::now() - prev).num_seconds() >= 60,
                };
                if should_rescan_decisions {
                    use std::path::Path;
                    eden::pipeline::decision_ledger::scanner::rescan_recent(
                        Path::new("decisions"),
                        &mut decision_ledger,
                        chrono::Utc::now(),
                    );
                }

                // Horizon live-settle sweep (audit Finding 1, 2026-04-19).
                // Flips Pending → Due when due_at <= now. Piggybacks the
                // decisions-rescan cadence (same 60s budget). Cheap query +
                // bounded write (LIMIT 256 inside helper).
                #[cfg(feature = "persistence")]
                if should_rescan_decisions {
                    if let Some(ref store) = runtime.store {
                        let now_offset = time::OffsetDateTime::now_utc();
                        eden::core::runtime::sweep_pending_horizons_to_due(store, now_offset).await;
                    }
                }

                #[cfg(feature = "persistence")]
                {
                    let snapshot_due = match belief_field.last_snapshot_ts() {
                        None => true,
                        Some(prev) => (chrono::Utc::now() - prev).num_seconds() >= 60,
                    };
                    if snapshot_due {
                        if let Some(ref store) = runtime.store {
                            let now_utc = chrono::Utc::now();
                            let snap = eden::persistence::belief_snapshot::serialize_field(
                                &belief_field,
                                now_utc,
                            );
                            belief_field.set_last_snapshot_ts(now_utc);
                            let gauss_n = snap.gaussian.len();
                            let cat_n = snap.categorical.len();
                            let store_clone = store.clone();
                            tokio::spawn(async move {
                                if let Err(e) = store_clone.write_belief_snapshot(&snap).await {
                                    eprintln!("[belief] snapshot write failed: {}", e);
                                }
                            });
                            eprintln!(
                                "[belief] snapshot: {} gaussian, {} categorical",
                                gauss_n, cat_n
                            );

                            let intent_snap =
                                eden::persistence::intent_belief_snapshot::serialize_field(
                                    &intent_belief_field,
                                    now_utc,
                                );
                            let intent_rows = intent_snap.rows.len();
                            let store_clone2 = store.clone();
                            tokio::spawn(async move {
                                if let Err(e) = store_clone2
                                    .write_intent_belief_snapshot(&intent_snap)
                                    .await
                                {
                                    eprintln!("[intent_belief] snapshot write failed: {}", e);
                                }
                            });
                            eprintln!("[intent_belief] snapshot: {} rows", intent_rows);

                            let broker_snap =
                                eden::persistence::broker_archetype_snapshot::serialize_field(
                                    &broker_archetype_field,
                                    now_utc,
                                );
                            let broker_rows = broker_snap.rows.len();
                            let store_clone3 = store.clone();
                            tokio::spawn(async move {
                                if let Err(e) = store_clone3
                                    .write_broker_archetype_snapshot(&broker_snap)
                                    .await
                                {
                                    eprintln!("[broker_archetype] snapshot write failed: {}", e);
                                }
                            });
                            eprintln!("[broker_archetype] snapshot: {} brokers", broker_rows);
                        }
                    }
                }
            }
            stage_timer.mark("S21a_sk_snapshots");
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
                &mut eden_ledger,
                &mut intent_belief_field,
                &mut outcome_credited_setup_ids,
                &mut broker_archetype_field,
                &mut broker_entry_snapshots,
                &mut broker_credited_setup_ids,
                &mut stage_timer,
            )
            .await;
            #[cfg(not(feature = "persistence"))]
            runtime.publish_projection(
                MarketId::Hk,
                crate::cases::CaseMarket::Hk,
                &artifact_projection,
                match json_payload(&hk_bridge_snapshot) {
                    Ok(payload) => vec![(bridge_snapshot_path.clone(), payload)],
                    Err(error) => {
                        eprintln!(
                            "Warning: failed to serialize HK bridge snapshot for tick {}: {}",
                            tick, error
                        );
                        vec![]
                    }
                },
                &analyst_service,
                tick,
                live.push_count,
                tick_started_at,
                tick_advance.received_push,
                tick_advance.received_update,
            );

            if !readiness.bootstrap_mode(tick) {
                display_hk_live_summary(
                    &artifact_projection.live_snapshot,
                    &artifact_projection.agent_briefing,
                    &artifact_projection.agent_session,
                );
            }
        }

        let bootstrap_mode = readiness.bootstrap_mode(tick);
        if bootstrap_mode {
            display_hk_bootstrap_preview(
                &readiness,
                &action_stage.workflow_snapshots,
                &reasoning_snapshot.propagation_paths,
            );
        } else if history.latest().is_none() {
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
            &store,
            tick,
            bootstrap_mode,
            history.len(),
            &dynamics,
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

        stage_timer.mark("S21c_heartbeat_tail");
        let stage_top = stage_timer.top_n(5);
        if tick % 10 == 0 {
            let parts: Vec<String> = stage_top
                .iter()
                .map(|(name, dur)| format!("{}={}ms", name, dur.as_millis()))
                .collect();
            eprintln!(
                "[hk tick {}] tick_ms={} stage_top={}",
                tick,
                tick_started_at.elapsed().as_millis(),
                parts.join(",")
            );
        }
        let stage_top_json: Vec<serde_json::Value> = stage_top
            .iter()
            .map(|(name, dur)| json!({ "stage": name, "ms": dur.as_millis() }))
            .collect();
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
                "stage_top5_ms": stage_top_json,
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

    #[cfg(feature = "persistence")]
    {
        let record = eden::persistence::edge_learning_ledger::EdgeLearningLedgerRecord::from_ledger(
            "hk",
            &edge_ledger,
            time::OffsetDateTime::now_utc(),
        );
        runtime
            .persist_edge_learning_ledger("hk", record, i128::from(tick))
            .await;
    }
}
