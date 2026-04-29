use std::collections::{HashMap, HashSet, VecDeque};
use std::sync::Arc;
use std::time::Instant;

use crate::bridges::service::FileSystemBridgeService;
#[cfg(feature = "persistence")]
use crate::cases::build_case_list_with_feedback;
#[cfg(feature = "persistence")]
use crate::core::analyst_service::AnalystService;
use crate::core::analyst_service::DefaultAnalystService;
use crate::core::market::{MarketDataCapability, MarketId, MarketRegistry};
use crate::core::projection::{project_us, UsProjectionInputs};
use crate::core::runtime::prepare_runtime_context_or_exit;
#[cfg(feature = "persistence")]
use crate::core::runtime::PreparedRuntimeContext;
use crate::core::runtime_loop::TickState;
use crate::live_snapshot::{
    spawn_write_snapshot, LiveBackwardChain, LiveCausalLeader, LiveCrossMarketAnomaly,
    LiveCrossMarketSignal, LiveEvent, LiveHypothesisTrack, LiveLineageMetric, LiveMarket,
    LiveMarketRegime, LivePressure, LivePropagationSense, LiveScorecard, LiveSignal, LiveSnapshot,
    LiveStressSnapshot, LiveStructuralDelta, LiveTacticalCase,
};
use crate::ontology::objects::{SectorId, Stock, Symbol};
use crate::ontology::reasoning::TacticalSetup;
#[cfg(feature = "persistence")]
use crate::ontology::snapshot::RawSnapshot;
use crate::ontology::store::{us_sector_names, us_symbol_sector, ObjectStore};
use crate::ontology::{action_direction_from_setup, ActionDirection, TacticalAction};
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
    UsDecisionSnapshot, UsOrderDirection, UsSignalRecord, UsSignalScorecard,
    UsSignalScorecardAccumulator,
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
use longport::quote::{CalcIndex, Period, PushEvent, QuoteContext, SubFlags, TradeSessions};
use longport::Config;
use rust_decimal::prelude::Signed;
use rust_decimal::Decimal;
use serde_json::json;
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
// Bumped 8_192 → 32_768 (4x) to absorb pre-market open burst.
// Consumer (debounce loop) processes ~3-4 ticks/sec which couldn't keep up
// with the WebSocket push storm during the open transition. Larger capacity
// gives the consumer ~80 seconds of buffer at peak burst rate before any
// drops, vs the previous ~20 seconds.
const US_PUSH_BATCH_CHANNEL_CAP: usize = 32_768;
#[cfg(feature = "persistence")]
const US_PERSISTENCE_MAX_IN_FLIGHT: usize = 16;
/// Tick interval between US learning-feedback refreshes. Re-exported via
/// `use super::*` into `support::stages::*` where the scheduler calls
/// `maybe_refresh_us_learning_feedback` with this cadence. Cargo's
/// dead-code lint misses the cross-module re-export path and previously
/// flagged this constant as unused — it is not.
#[cfg(feature = "persistence")]
const US_LEARNING_FEEDBACK_REFRESH_INTERVAL: u64 = 30;

fn setup_scorecard_direction(setup: &TacticalSetup) -> Option<UsOrderDirection> {
    match &setup.scope {
        crate::ontology::reasoning::ReasoningScope::Symbol(_) => {}
        _ => return None,
    }
    Some(match action_direction_from_setup(setup) {
        Some(ActionDirection::Short) => UsOrderDirection::Sell,
        _ => UsOrderDirection::Buy,
    })
}

/// Delta over the last `n` samples in a ring buffer (planner U or bull/bear ratio).
fn us_ring_trend_last_n(ring: &VecDeque<f64>, n: usize) -> f64 {
    let v: Vec<f64> = ring.iter().copied().collect();
    let len = v.len();
    if len <= n {
        0.0
    } else {
        v[len - 1] - v[len - 1 - n]
    }
}

fn us_oscillation_observation_symbols(
    tracker: &crate::pipeline::oscillation::OscillationTracker,
    current_symbols: &HashSet<String>,
) -> Vec<String> {
    let mut symbols = tracker
        .tracked_symbols()
        .into_iter()
        .collect::<HashSet<_>>();
    symbols.extend(current_symbols.iter().cloned());
    symbols.into_iter().collect()
}

fn us_drop_inactive_symbol_trackers(
    signal_velocity: &mut crate::pipeline::signal_velocity::SignalVelocityTracker,
    direction_flip: &mut crate::pipeline::direction_flip::DirectionFlipTracker,
    previous_symbols: &HashSet<String>,
    current_symbols: &HashSet<String>,
) {
    for symbol in previous_symbols.difference(current_symbols) {
        signal_velocity.drop(symbol);
        direction_flip.forget(symbol);
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct UsWorkflowAdvance {
    confirmed: bool,
    executed: bool,
    monitoring: bool,
}

fn advance_us_workflow_with_price(
    workflow: &mut UsActionWorkflow,
    price: Option<Decimal>,
    tick: u64,
) -> UsWorkflowAdvance {
    let Some(price) = price else {
        return UsWorkflowAdvance::default();
    };

    let mut advance = UsWorkflowAdvance::default();
    match workflow.stage {
        UsActionStage::Suggested => {
            if workflow.confirm(tick).is_ok() {
                advance.confirmed = true;
            } else {
                return advance;
            }
        }
        UsActionStage::Confirmed => {}
        _ => return advance,
    }

    if workflow.execute(price, tick).is_ok() {
        advance.executed = true;
        advance.monitoring = matches!(workflow.stage, UsActionStage::Monitoring);
    }

    advance
}

#[cfg(feature = "persistence")]
fn push_us_workflow_advance_events(
    workflow: &UsActionWorkflow,
    advance: UsWorkflowAdvance,
    now: time::OffsetDateTime,
    workflow_events: &mut Vec<crate::persistence::action_workflow::ActionWorkflowEventRecord>,
) {
    if advance.confirmed {
        workflow_events.push(
            crate::persistence::action_workflow::ActionWorkflowEventRecord::from_us_workflow_stage(
                workflow,
                Some(crate::action::workflow::ActionStage::Suggest),
                crate::action::workflow::ActionStage::Confirm,
                now,
                Some("tracker".into()),
                Some("workflow confirmed".into()),
            ),
        );
    }
    if advance.executed {
        let note = workflow
            .entry_price
            .map(|price| format!("position executed at {price}"))
            .unwrap_or_else(|| "position executed".into());
        workflow_events.push(
            crate::persistence::action_workflow::ActionWorkflowEventRecord::from_us_workflow_stage(
                workflow,
                Some(crate::action::workflow::ActionStage::Confirm),
                crate::action::workflow::ActionStage::Execute,
                now,
                Some("tracker".into()),
                Some(note),
            ),
        );
    }
    if advance.monitoring {
        workflow_events.push(
            crate::persistence::action_workflow::ActionWorkflowEventRecord::from_us_workflow_stage(
                workflow,
                Some(crate::action::workflow::ActionStage::Execute),
                crate::action::workflow::ActionStage::Monitor,
                now,
                Some("tracker".into()),
                Some("monitoring started".into()),
            ),
        );
    }
}

#[cfg(feature = "persistence")]
fn restore_persisted_us_workflows(
    workflows: &mut Vec<UsActionWorkflow>,
    position_tracker: &mut UsPositionTracker,
    setups: &[TacticalSetup],
    persisted_workflows_by_id: &HashMap<
        String,
        crate::persistence::action_workflow::ActionWorkflowRecord,
    >,
    dim_snapshot: &UsDimensionSnapshot,
) {
    let setups_by_workflow_id = setups
        .iter()
        .map(|setup| {
            let workflow_id = setup.workflow_id.clone().unwrap_or_else(|| {
                crate::persistence::action_workflow::synthetic_workflow_id_for_setup(
                    &setup.setup_id,
                )
            });
            (workflow_id, setup)
        })
        .collect::<HashMap<_, _>>();
    let setups_by_setup_id = setups
        .iter()
        .map(|setup| (setup.setup_id.as_str(), setup))
        .collect::<HashMap<_, _>>();

    for (workflow_id, record) in persisted_workflows_by_id {
        if workflows
            .iter()
            .any(|workflow| workflow.workflow_id == *workflow_id)
        {
            continue;
        }
        let setup = setups_by_workflow_id.get(workflow_id).copied().or_else(|| {
            record
                .payload
                .get("setup_id")
                .and_then(|value| value.as_str())
                .and_then(|setup_id| setups_by_setup_id.get(setup_id).copied())
        });
        if let Some(setup) = setup {
            workflows.push(UsActionWorkflow::from_action_workflow_record(setup, record));
            continue;
        }
        if let Some(workflow) = UsActionWorkflow::from_persisted_action_workflow_record(record) {
            workflows.push(workflow);
        }
    }

    for workflow in workflows.iter_mut() {
        if let Some(record) = persisted_workflows_by_id.get(&workflow.workflow_id) {
            workflow.apply_action_workflow_record(record);
        }
    }

    let symbols_to_remove = workflows
        .iter()
        .filter(|workflow| {
            !matches!(
                workflow.stage,
                UsActionStage::Executed | UsActionStage::Monitoring
            )
        })
        .map(|workflow| workflow.symbol.clone())
        .collect::<Vec<_>>();
    for symbol in symbols_to_remove {
        position_tracker.exit(&symbol);
    }

    for workflow in workflows.iter().filter(|workflow| {
        matches!(
            workflow.stage,
            UsActionStage::Executed | UsActionStage::Monitoring
        )
    }) {
        if position_tracker.is_active(&workflow.symbol) {
            continue;
        }
        if let Some(dims) = dim_snapshot.dimensions.get(&workflow.symbol) {
            position_tracker.enter(UsStructuralFingerprint::capture(
                workflow.symbol.clone(),
                workflow.entry_tick,
                workflow.entry_price,
                dims,
            ));
        }
    }
}

fn emit_setup_scorecard_records(
    signal_records: &mut Vec<UsSignalRecord>,
    tick: u64,
    setups: &[TacticalSetup],
    quotes: &HashMap<Symbol, crate::core::market_snapshot::CanonicalQuote>,
) {
    for setup in setups {
        let Some(symbol) = (match &setup.scope {
            crate::ontology::reasoning::ReasoningScope::Symbol(symbol) => Some(symbol.clone()),
            _ => None,
        }) else {
            continue;
        };
        if signal_records
            .iter()
            .any(|record| record.setup_id == setup.setup_id)
        {
            continue;
        }
        let Some(direction) = setup_scorecard_direction(setup) else {
            continue;
        };
        signal_records.push(UsSignalRecord {
            setup_id: setup.setup_id.clone(),
            symbol: symbol.clone(),
            tick_emitted: tick,
            direction,
            composite_at_emission: setup.convergence_score.unwrap_or(setup.confidence),
            price_at_emission: quotes.get(&symbol).map(|quote| quote.last_done),
            resolved: false,
            price_at_resolution: None,
            hit: None,
            realized_return: None,
            is_actionable_tier: matches!(setup.action, TacticalAction::Enter),
        });
    }
}

fn augment_us_live_snapshot_with_raw_expectations(
    live_snapshot: &mut LiveSnapshot,
    raw_trade_tape: &crate::pipeline::raw_expectation::RawTradeTape,
) {
    let empty_broker = crate::pipeline::raw_expectation::RawBrokerPresence::default();
    let empty_depth = crate::pipeline::raw_expectation::RawDepthLevels::default();
    for state in &mut live_snapshot.symbol_states {
        let outcome = crate::pipeline::raw_expectation::evaluate_raw_expectations(
            state.state_kind,
            &Symbol(state.symbol.clone()),
            &empty_broker,
            &empty_depth,
            raw_trade_tape,
        );
        state.supporting_evidence.extend(outcome.supporting);
        state.opposing_evidence.extend(outcome.opposing);
    }
}

// ── Runtime entry ──

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    eprintln!("[us] run() entered");
    #[allow(unused_mut)]
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
        mut previous_symbol_states,
        mut eden_ledger,
        #[cfg(feature = "persistence")]
        mut cached_us_learning_feedback,
    } = initialize_us_runtime().await?;
    let market_capabilities = MarketRegistry::capabilities(MarketId::Us);
    // Seen-set for outcome_feedback (symmetric with HK) — dedups
    // lineage-window re-emits so each resolution credits IntentBelief
    // exactly once.
    #[cfg(feature = "persistence")]
    let mut outcome_credited_setup_ids: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    // Y#7 parity with HK — market-level wave tracker. Counts are market
    // agnostic (absence demotions, expectation errors, momentum
    // collapses) so the tracker works unchanged; US-specific wiring
    // just points at UsSignalMomentumTracker's convergence/volume_spike
    // hashmaps instead of HK's three microstructure tracks.
    let mut market_waves = crate::temporal::lineage::MarketWaveTracker::default();
    // T4 parity — raw trade tape. HK has broker + depth + trade; US only
    // has trades but that's still actionable for evaluate_raw_expectations
    // (trade_aggressor_up / trade_aggressor_down / block_trade_cluster /
    // aggressor_balanced evidence codes). broker_presence and depth_levels
    // stay empty on US — their code paths no-op cleanly.
    let mut raw_trade_tape = crate::pipeline::raw_expectation::RawTradeTape::default();
    // Sub-KG primary representation (symmetric with HK). Broker nodes stay
    // empty on US (no public broker queue); all other node kinds populate.
    let mut subkg_registry = crate::pipeline::symbol_sub_kg::SubKgRegistry::new();
    let mut subkg_snapshot_tick: u64 = 0;

    // 2026-04-29 Phase A: NDJSON artifact writers run on background
    // tokio tasks. See HK runtime equivalent + src/core/ndjson_writer.rs.
    let bp_marginals_writer = crate::core::ndjson_writer::NdjsonWriter::<
        Vec<crate::pipeline::loopy_bp::MarginalRow>,
    >::spawn("us:bp_marginals", |rows: Vec<
        crate::pipeline::loopy_bp::MarginalRow,
    >| crate::pipeline::loopy_bp::write_marginals("us", &rows));
    let bp_message_trace_writer = crate::core::ndjson_writer::NdjsonWriter::<
        Vec<crate::pipeline::loopy_bp::BpMessageTraceRow>,
    >::spawn("us:bp_message_trace", |rows: Vec<
        crate::pipeline::loopy_bp::BpMessageTraceRow,
    >| crate::pipeline::loopy_bp::write_message_trace("us", &rows));
    let subkg_writer = crate::core::ndjson_writer::NdjsonWriter::<Vec<String>>::spawn(
        "us:subkg",
        |lines: Vec<String>| {
            crate::pipeline::symbol_sub_kg::append_subkg_lines_to_ndjson("us", &lines)
        },
    );
    let sector_subkg_writer = crate::core::ndjson_writer::NdjsonWriter::<Vec<String>>::spawn(
        "us:sector_subkg",
        |lines: Vec<String>| {
            crate::pipeline::sector_sub_kg::append_sector_subkg_lines_to_ndjson("us", &lines)
        },
    );
    let visual_frame_writer = crate::core::ndjson_writer::NdjsonWriter::<
        crate::pipeline::visual_graph_frame::VisualGraphFrame,
    >::spawn("us:visual_graph_frame", |frame: crate::pipeline::visual_graph_frame::VisualGraphFrame| {
        crate::pipeline::visual_graph_frame::write_frame("us", &frame)
    });
    let temporal_delta_writer = crate::core::ndjson_writer::NdjsonWriter::<
        crate::pipeline::temporal_graph_delta::TemporalGraphDelta,
    >::spawn("us:temporal_delta", |delta: crate::pipeline::temporal_graph_delta::TemporalGraphDelta| {
        crate::pipeline::temporal_graph_delta::write_delta("us", &delta)
    });
    let cross_sector_writer = crate::core::ndjson_writer::NdjsonWriter::<
        Vec<crate::pipeline::cross_sector_contrast::SectorContrastEvent>,
    >::spawn("us:cross_sector", |events: Vec<
        crate::pipeline::cross_sector_contrast::SectorContrastEvent,
    >| crate::pipeline::cross_sector_contrast::write_events("us", &events));
    let sector_to_symbol_writer = crate::core::ndjson_writer::NdjsonWriter::<
        Vec<crate::pipeline::sector_to_symbol_propagation::MemberLagEvent>,
    >::spawn("us:sector_to_symbol", |events: Vec<
        crate::pipeline::sector_to_symbol_propagation::MemberLagEvent,
    >| crate::pipeline::sector_to_symbol_propagation::write_events("us", &events));
    let sector_kinematics_writer = crate::core::ndjson_writer::NdjsonWriter::<
        Vec<crate::pipeline::sector_kinematics::SectorKinematicsEvent>,
    >::spawn("us:sector_kinematics", |events: Vec<
        crate::pipeline::sector_kinematics::SectorKinematicsEvent,
    >| crate::pipeline::sector_kinematics::write_events("us", &events));

    // Production BP substrate: event-driven (sync + shadow deleted 2026-04-29).
    let belief_substrate: std::sync::Arc<dyn crate::pipeline::event_driven_bp::BeliefSubstrate> =
        std::sync::Arc::new(
            crate::pipeline::event_driven_bp::EventDrivenSubstrate::default(),
        );

    // 2026-04-29 Phase B: pressure-event bus. Push handler demuxes
    // each PushEvent into PressureEvent variants and publishes here;
    // per-channel workers (Phase C) drain it. Phase B drainer below
    // is a no-op counter to verify the wiring.
    let pressure_event_bus = std::sync::Arc::new(
        crate::pipeline::pressure_events::spawn_bus(),
    );
    {
        let bus = std::sync::Arc::clone(&pressure_event_bus);
        tokio::spawn(async move {
            let mut counter = 0u64;
            loop {
                if bus.pop().await.is_none() {
                    break;
                }
                counter = counter.wrapping_add(1);
                if counter == 1 || counter % 1_000 == 0 {
                    eprintln!(
                        "[us pressure-bus] drained {} events (pending={}, dropped={})",
                        counter,
                        bus.pending_count(),
                        bus.dropped_count(),
                    );
                }
            }
        });
    }

    let mut previous_visual_frame: Option<crate::pipeline::visual_graph_frame::VisualGraphFrame> =
        None;
    let mut kinematics_tracker = crate::pipeline::structural_kinematics::KinematicsTracker::new();
    let us_prev_top_bid: HashMap<Symbol, rust_decimal::Decimal> = HashMap::new();
    let us_prev_top_ask: HashMap<Symbol, rust_decimal::Decimal> = HashMap::new();
    let us_bid1_stable: HashMap<Symbol, u64> = HashMap::new();
    let us_ask1_stable: HashMap<Symbol, u64> = HashMap::new();
    let mut us_halted_today: std::collections::HashSet<Symbol> = std::collections::HashSet::new();
    let mut persistence_tracker =
        crate::pipeline::structural_persistence::PersistenceTracker::new();
    let mut expectation_tracker =
        crate::pipeline::structural_expectation::ExpectationTracker::new();
    // Cluster / world persistent state: in-memory rolling across ticks.
    // See hk/runtime.rs for the parallel pattern; persistence tables are
    // deferred until cross-restart continuity is requested.
    let mut previous_cluster_states: Vec<crate::live_snapshot::LiveClusterState> = Vec::new();
    let mut previous_world_summary: Option<crate::live_snapshot::LiveWorldSummary> = None;
    eprintln!("[us] initialize_us_runtime() completed");
    let mut attention = AttentionBudgetAllocator::from_universe_size(US_WATCHLIST.len());
    let mut vortex_attention = UsVortexAttention::default();
    let mut cached_us_candidate_mechanisms: Vec<
        crate::persistence::candidate_mechanism::CandidateMechanismRecord,
    > = Vec::new();
    // Synthetic outcomes cache — refreshed every SYNTHETIC_OUTCOME_REFRESH
    // ticks from live US tick history. Mirrors HK's cache. V2: feeds the
    // substrate-evidence builder which writes per-symbol mean signed
    // return into `NodeId::OutcomeMemory` for BP to read.
    let mut cached_us_synthetic_outcomes: Vec<crate::temporal::lineage::CaseRealizedOutcome> =
        Vec::new();
    const SYNTHETIC_OUTCOME_REFRESH: u64 = 30;

    // Previous-tick hub summaries — carried across ticks so THIS tick's
    // modulation block can attach hub_member risk notes to setups on
    // symbols that crystallized as hubs LAST tick. Crystallization runs
    // after the modulation block, so in-tick hubs aren't visible yet.
    // Reassigned unconditionally at end of crystallization each tick so
    // stale hubs clear when current tick has no emergent edges.
    let mut prev_tick_hubs: Vec<crate::pipeline::residual::HubSummary> = Vec::new();

    // Sector kinematics tracker — across-tick history of per-(sector,
    // NodeKind) mean activation. Detects sector-level turning points
    // (TopForming / BottomForming) one zoom level above per-symbol
    // structural_kinematics. Stateful across snapshot ticks.
    let mut sector_kinematics_tracker =
        crate::pipeline::sector_kinematics::SectorKinematicsTracker::new();

    // Engram-style regime analog index — deterministic O(1) lookup
    // from regime_fingerprint.bucket_key → historical visits + future
    // outcome stats (T+5 / T+30 / T+100 stress/sync/bias delta).
    let mut us_regime_analog_index = crate::pipeline::regime_analog_index::RegimeAnalogIndex::new();
    if let Ok(n) = us_regime_analog_index.load_from_ndjson("us") {
        if n > 0 {
            eprintln!("[regime_analog] us loaded {} historical records", n);
        }
    }
    let mut latest_us_regime_analog_summary: Option<
        crate::pipeline::regime_analog_index::AnalogSummary,
    > = None;

    // Per-symbol WL graph signature analog index.
    let mut us_symbol_wl_analog_index =
        crate::pipeline::symbol_wl_analog_index::SymbolWlAnalogIndex::new();

    // Lead-lag tracker — rolling time-series per symbol of composite
    // (Pressure + Intent) scalar. Cross-correlation along master KG
    // edges gives directional evidence (which symbol leads / lags).
    let mut us_lead_lag_tracker = crate::pipeline::lead_lag_index::LeadLagTracker::new();

    // Active probe runner — counterfactual BP each tick on top-K
    // high-entropy symbols, enqueue forecast vs reality, accumulate
    // per-symbol forecast accuracy. Accuracy feeds back into sub-KG
    // NodeId::ForecastAccuracy on the next tick.
    let mut us_active_probe = crate::pipeline::active_probe::ActiveProbeRunner::new();
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
    #[cfg(feature = "persistence")]
    if let Some(ref store) = runtime.store {
        if let Ok(Some(record)) = store.load_edge_learning_ledger("us").await {
            edge_ledger = record.into_ledger();
            eprintln!(
                "[us] restored {} learned edges from store",
                edge_ledger.len()
            );
        }
    }
    let mut seen_us_edge_learning_setups = HashSet::new();
    let mut pressure_field =
        crate::pipeline::pressure::PressureField::new(time::OffsetDateTime::now_utc());
    // Shift A (symmetric with HK): latent world state, 5-dim Gaussian
    // SSM. v1 feeds composite_stress + momentum_consensus as
    // observation (US has no sector_synchrony — closest proxy).
    let mut latent_world_state = crate::pipeline::latent_world_state::LatentWorldState::new(
        crate::ontology::objects::Market::Us,
    );
    // Shift B: SCM over same latent dims (semantics cross-market
    // identical even if feed details differ).
    let us_scm = crate::pipeline::structural_causal::StructuralCausalModel::default_latent_scm();
    // Operator surfaces (2026-04-22): presence oscillation, confidence velocity,
    // direction flips, session-quality + regime tags — wired into wake.reasons.
    let mut us_oscillation = crate::pipeline::oscillation::OscillationTracker::new();
    let mut us_signal_velocity = crate::pipeline::signal_velocity::SignalVelocityTracker::new();
    let mut us_direction_flip = crate::pipeline::direction_flip::DirectionFlipTracker::new();
    let mut us_prev_active_symbols: HashSet<String> = HashSet::new();
    let mut us_planner_u_ring: VecDeque<f64> = VecDeque::with_capacity(32);
    let mut us_bull_bear_ring: VecDeque<f64> = VecDeque::with_capacity(32);
    // PressureBeliefField: symmetric to HK runtime. Restored from the
    // latest SurrealDB snapshot if market="us" exists, otherwise fresh.
    let mut belief_field = crate::pipeline::belief_field::PressureBeliefField::new(
        crate::ontology::objects::Market::Us,
    );
    #[cfg(feature = "persistence")]
    if let Some(ref store) = runtime.store {
        match store.latest_belief_snapshot("us").await {
            Ok(Some(snap)) => match crate::persistence::belief_snapshot::restore_field(&snap) {
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
    // KL surprise tracker — per-(symbol, channel) self-referential KL
    // baseline, drives V4 decision unblock when EDEN_ACTION_PROMOTION
    // env-flag is `kl_surprise`. Always allocated so the dispatcher can
    // pass it without conditional plumbing; percentile mode ignores it.
    let mut kl_surprise_tracker = crate::pipeline::kl_surprise::KlSurpriseTracker::new();

    // Sub-KG emergence tracker — per-symbol baseline of the cross-NodeId
    // emergence score. When a symbol's score crosses its own historical
    // 1σ floor, a synthetic TacticalSetup is appended to the tick's
    // setups so symbols not chosen by the reasoning_layer (e.g., AFRM
    // with no pressure vortex) still surface to action_promotion.
    let mut sub_kg_emergence_tracker =
        crate::pipeline::sub_kg_emergence::SubKgEmergenceTracker::new();

    // DecisionLedger: symmetric to HK. Market::Us + crate-prefix paths.
    let mut decision_ledger =
        crate::pipeline::decision_ledger::DecisionLedger::new(crate::ontology::objects::Market::Us);
    {
        use std::path::Path;
        crate::pipeline::decision_ledger::scanner::scan_directory(
            Path::new("decisions"),
            &mut decision_ledger,
        );
    }
    let mut lifecycle_tracker = crate::pipeline::pressure::reasoning::LifecycleTracker::default();
    // Y#0 first piece (symmetric with HK).
    let mut residual_pattern_tracker =
        crate::pipeline::ontology_emergence::ResidualPatternTracker::new(
            crate::ontology::objects::Market::Us,
        );
    // Cross-ontology intent belief (symmetric with HK).
    let mut intent_belief_field = crate::pipeline::intent_belief::IntentBeliefField::new(
        crate::ontology::objects::Market::Us,
    );
    #[cfg(feature = "persistence")]
    if let Some(ref store) = runtime.store {
        match store.latest_intent_belief_snapshot("us").await {
            Ok(Some(snap)) => {
                match crate::persistence::intent_belief_snapshot::restore_field(&snap) {
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
    // Sector id -> name for sector_intent wake emission (duplicates
    // the per-tick build below; kept at runtime scope because wake
    // emits outside the per-tick UsGraph rebuild).
    let outer_sector_names: HashMap<SectorId, String> = store
        .sectors
        .iter()
        .map(|(id, s)| (id.clone(), s.name.clone()))
        .collect();

    // ── Terrain Builder: enrich ontology from Terminal CLI ──
    // Set EDEN_SKIP_TERRAIN=1 to bypass — terrain enriches reasoning context
    // (peer/fund/calendar) but trading uses pressure field directly, so a
    // session can run without it. Useful when terrain CLI is rate-limited or
    // hangs and you need eden up immediately.
    let mut terrain = crate::ontology::terrain::TerrainSnapshot::default();
    let mut terrain_rx = None;
    if std::env::var("EDEN_SKIP_TERRAIN").is_ok() {
        eprintln!("[us] EDEN_SKIP_TERRAIN=1, skipping terrain build");
    } else if !is_us_cash_session_hours(time::OffsetDateTime::now_utc()) {
        eprintln!("[us] skipping terrain build outside cash session");
    } else {
        let us_symbols: Vec<Symbol> = store.stocks.keys().cloned().collect();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        terrain_rx = Some(rx);
        tokio::spawn(async move {
            let terrain_builder = crate::ontology::terrain::TerrainBuilder::new(vec![], us_symbols);
            eprintln!("[us] building terrain from Terminal CLI in background...");
            let terrain = terrain_builder.build_terrain().await;
            let terrain_peer_count: usize = terrain.peer_groups.values().map(|v| v.len()).sum();
            let terrain_holder_count = terrain.institutional_holdings.len();
            let terrain_fund_count = terrain.fund_holdings.len();
            let terrain_event_count: usize =
                terrain.upcoming_events.values().map(|v| v.len()).sum();
            eprintln!(
                "[us] terrain built: {} peer links, {} institutions, {} funds, {} calendar events, {} ratings, {} insider records",
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

    // Last-write timestamp for regime_fingerprint persistence; throttled
    // to 60 seconds (mirror of belief_field snapshot cadence). Wake-line
    // emission is per-tick.
    #[cfg(feature = "persistence")]
    let mut last_us_regime_fp_ts: Option<chrono::DateTime<chrono::Utc>> = None;

    loop {
        if let Some(rx) = terrain_rx.as_mut() {
            if let Ok(new_terrain) = rx.try_recv() {
                terrain = new_terrain;
                terrain_rx = None;
            }
        }
        let Some(tick_advance) = ({
            let mut tick_state = UsTickState {
                live: &mut live,
                rest: &mut rest,
                pressure_event_bus: Some(std::sync::Arc::clone(&pressure_event_bus)),
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
        // T4 parity — feed live trades into the raw trade tape so
        // evaluate_raw_expectations has the same substrate on US as HK.
        for (symbol, trades) in &trades_this_tick {
            raw_trade_tape.record_tick(symbol, trades);
        }

        let market_open = is_us_regular_market_hours(now);
        if !market_open {
            // Still write snapshot but mark as after-hours, skip reasoning
            lifecycle_tracker.decay(tick);
            let timestamp_str = now
                .format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_default();
            let idle_snapshot = build_us_bootstrap_snapshot(
                tick,
                timestamp_str,
                &store,
                &live,
                &rest,
                &previous_symbol_states,
                &previous_cluster_states,
                previous_world_summary.as_ref(),
            );
            previous_symbol_states = idle_snapshot.symbol_states.clone();
            previous_cluster_states = idle_snapshot.cluster_states.clone();
            previous_world_summary = idle_snapshot.world_summary.clone();
            spawn_write_snapshot(runtime.artifacts.live_snapshot_path.clone(), idle_snapshot);
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

        let canonical_market_snapshot = live.to_canonical_snapshot(&rest, now);

        // Build link-level observations
        // Build US dimensions (with VWAP from intraday)
        let dim_snapshot =
            UsDimensionSnapshot::compute_from_canonical(&canonical_market_snapshot, &store);

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
        let obs_snapshot = UsObservationSnapshot::from_canonical_market(&canonical_market_snapshot);

        // 2. Event detection
        let mut event_snapshot = UsEventSnapshot::detect_from_canonical(
            &canonical_market_snapshot,
            &previous_flows,
            &hk_counterpart_moves,
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

        // 4. Pressure reasoning + tactical setups
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
        // Pressure field: inject local pressure, propagate along US graph edges, detect vortices.
        pressure_field.tick_us(now, &dim_snapshot.dimensions, &graph, &mut edge_ledger);
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
        let mut vortex_insights: Vec<(
            crate::pipeline::pressure::reasoning::VortexInsight,
            crate::pipeline::pressure::PressureVortex,
        )> = Vec::new();
        for vortex in &pressure_field.vortices {
            if let Some(insight) =
                crate::pipeline::pressure::reasoning::reason_about_vortex_with_terrain(
                    vortex,
                    &pressure_field,
                    &lifecycle_tracker,
                    &sector_members,
                    &symbol_sector,
                    Some(&terrain.peer_groups),
                )
            {
                eprintln!("[us] {}", insight.summary);
                residual_pattern_tracker.observe(
                    vortex,
                    insight.lifecycle.phase,
                    &insight.attribution.driver,
                    chrono::Utc::now(),
                );
                vortex_insights.push((insight, vortex.clone()));
            }
        }
        let structural_evidence = vortex_insights
            .iter()
            .map(|(insight, _)| (insight.symbol.clone(), insight.evidence.clone()))
            .collect::<HashMap<_, _>>();

        // 5. Decision: convergence + regime + suggestions on shared structural evidence
        let decision = UsDecisionSnapshot::compute_with_evidence(
            &graph,
            &cross_market_signals,
            &structural_evidence,
            tick,
            Some(&edge_ledger),
        );
        let reasoning_decision =
            filter_us_decision_for_reasoning(&decision, &reasoning_active_symbols);

        let mut reasoning = UsReasoningSnapshot::empty(now);

        // Pressure field → tactical setups WITH shared reasoning insight.
        let mut vortex_setups = crate::pipeline::pressure::bridge::insights_to_tactical_setups(
            &vortex_insights,
            now,
            tick,
            10,
        );
        // Closed loop step 1: belief modulates setup.confidence.
        // Closed loop step 4: outcome history further modulates.
        // Phase 3: intent_belief modulation. Symmetric with HK.
        // Refresh synthetic-outcome cache every SYNTHETIC_OUTCOME_REFRESH
        // ticks (mirror of HK runtime). Full scan is expensive; staleness up
        // to ~30 ticks is acceptable for a modulator that already requires ≥5
        // effective resolved samples.
        if tick % SYNTHETIC_OUTCOME_REFRESH == 0 {
            cached_us_synthetic_outcomes =
                crate::us::temporal::outcomes::compute_us_case_realized_outcomes_adaptive(
                    &tick_history,
                    500,
                );
        }

        // V2: pre-BP modulation chain deleted. belief_field + outcome
        // history now flow into BP via NodeId::BeliefEntropy /
        // BeliefSampleCount / OutcomeMemory (set by
        // update_from_substrate_evidence). BP posterior in the post-BP
        // block is the single source of truth for setup.confidence.
        // Hub observation attachment is the only residual risk_note —
        // it's pure operator visibility, no modulation.
        for setup in vortex_setups.iter_mut() {
            if let crate::ontology::ReasoningScope::Symbol(sym) = &setup.scope {
                if let Some(hub) = prev_tick_hubs.iter().find(|h| h.symbol == *sym) {
                    setup
                        .risk_notes
                        .push(crate::pipeline::residual::hub_member_risk_note(hub));
                }
            }
        }
        if !vortex_setups.is_empty() {
            // Wake line removed — base=1.0 saturation made conf field
            // misleading; real confidence delta lives in mod_stack line
            // above and in persisted setups. Tactical setups still flow
            // into reasoning.tactical_setups below.
            // 2026-04-29: deleted apply_feedback_to_tactical_setup —
            // see HK runtime equivalent for full rationale. The function
            // overwrote BP posterior with a magic 5-channel weighted
            // sum and bypassed the "BP posterior is single source of
            // truth" contract.
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
        let us_residual_field =
            crate::us::pipeline::residual::compute_us_residual_field_from_canonical(
                &decision.convergence_scores,
                &canonical_market_snapshot,
            );
        // Residual field + injected hypothesis counts removed from wake.
        // Full residual state is in canonical_market_snapshot / reasoning.
        let us_hidden_forces =
            crate::pipeline::residual::infer_hidden_forces(&us_residual_field, now);
        if !us_hidden_forces.is_empty() {
            let us_hidden_forces = us_hidden_forces;
            #[cfg(feature = "persistence")]
            let mut us_hidden_forces = us_hidden_forces;
            #[cfg(feature = "persistence")]
            if let Some(feedback) = cached_us_learning_feedback.as_ref() {
                for hypothesis in &mut us_hidden_forces {
                    crate::pipeline::learning_loop::apply_feedback_to_hypothesis(
                        hypothesis, feedback,
                    );
                }
            }
            reasoning.hypotheses.extend(us_hidden_forces);
        }
        // Verify hidden forces (tick-level)
        let us_verify = us_hidden_force_state.tick(&us_residual_field, &reasoning.hypotheses, tick);
        let _ = us_verify;
        // Verification counts removed from wake — confirmed forces still
        // flow through crystallization and downstream per-item wakes.
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
        let option_surface_observations =
            if market_capabilities.supports(MarketDataCapability::OptionSurface) {
                canonical_market_snapshot.option_surface_observations()
            } else {
                Vec::new()
            };
        let option_validations = crate::pipeline::residual::cross_validate_with_options(
            &us_hidden_force_state,
            &option_surface_observations,
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
        if !us_crystallization.attention_boosts.is_empty()
            || !us_crystallization.emergent_paths.is_empty()
            || !us_crystallization.emergent_edges.is_empty()
        {
            // Crystallization summary count + per-attention-boost +
            // per-emergent-edge wakes removed. Per-boost and per-edge
            // data are in ndjson and cross-symbol-propagation output.
            // Hub aggregation below is the per-symbol surface that
            // operator actually acts on.
            if !us_crystallization.emergent_paths.is_empty() {
                let emergent = crate::pipeline::residual::emergent_paths_to_propagation_paths(
                    &us_crystallization.emergent_paths,
                    now,
                );
                reasoning.propagation_paths.extend(emergent);
            }
        }
        // Hub aggregation: always run so prev_tick_hubs clears on ticks
        // with no emergent edges (instead of carrying stale hubs forever).
        // Wake line still only emits when the list is non-empty.
        let hubs = crate::pipeline::residual::aggregate_hubs(&us_crystallization.emergent_edges, 3);
        for hub in hubs.iter().take(5) {
            eprintln!(
                "[us] hub: {} anticorr_degree={} corr_degree={} peers={} max_streak={} mean_strength={:.2}",
                hub.symbol.0,
                hub.anticorr_degree,
                hub.corr_degree,
                hub.peers.join(","),
                hub.max_streak,
                hub.mean_strength,
            );
        }
        prev_tick_hubs = hubs;

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
            let mark_price = canonical_market_snapshot
                .quotes
                .get(sym)
                .map(|quote| quote.last_done);
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
        #[cfg(feature = "persistence")]
        let persisted_workflows_by_id = if let Some(ref store) = runtime.store {
            let mut workflow_ids = reasoning
                .tactical_setups
                .iter()
                .chain(previous_setups.iter())
                .map(|setup| {
                    setup.workflow_id.clone().unwrap_or_else(|| {
                        crate::persistence::action_workflow::synthetic_workflow_id_for_setup(
                            &setup.setup_id,
                        )
                    })
                })
                .chain(
                    workflows
                        .iter()
                        .map(|workflow| workflow.workflow_id.clone()),
                )
                .collect::<Vec<_>>();
            workflow_ids.sort();
            workflow_ids.dedup();
            let mut persisted = store
                .action_workflows_by_ids(&workflow_ids)
                .await
                .unwrap_or_default()
                .into_iter()
                .map(|workflow| (workflow.workflow_id.clone(), workflow))
                .collect::<std::collections::HashMap<_, _>>();
            for workflow in store
                .recent_action_workflows_by_market("us", US_WORKFLOW_CAP)
                .await
                .unwrap_or_default()
            {
                if matches!(
                    workflow.current_stage,
                    crate::action::workflow::ActionStage::Review
                ) {
                    continue;
                }
                persisted.insert(workflow.workflow_id.clone(), workflow);
            }
            persisted
        } else {
            std::collections::HashMap::new()
        };
        #[cfg(feature = "persistence")]
        let mut workflow_events =
            Vec::<crate::persistence::action_workflow::ActionWorkflowEventRecord>::new();
        #[cfg(feature = "persistence")]
        restore_persisted_us_workflows(
            &mut workflows,
            &mut position_tracker,
            &reasoning.tactical_setups,
            &persisted_workflows_by_id,
            &dim_snapshot,
        );
        crate::graph::edge_learning::ingest_us_topology_outcomes(
            &mut edge_ledger,
            &mut seen_us_edge_learning_setups,
            &tick_history,
            &graph,
            SIGNAL_RESOLUTION_LAG,
            now,
        );
        edge_ledger.decay(now);
        #[cfg(feature = "persistence")]
        if tick % 10 == 0 {
            let record =
                crate::persistence::edge_learning_ledger::EdgeLearningLedgerRecord::from_ledger(
                    "us",
                    &edge_ledger,
                    now,
                );
            runtime
                .persist_edge_learning_ledger("us", record, i128::from(tick))
                .await;
        }

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
            runtime
                .persist_candidate_mechanisms(
                    "us",
                    cached_us_candidate_mechanisms.clone(),
                    i128::from(tick),
                )
                .await;
        }

        let dynamics = compute_us_dynamics(&tick_history);

        // 7. Scorecard: emit and resolve the same durable setup objects that lineage resolves.
        // This keeps scorecard and lineage on the same closed-loop mother object (`setup_id`),
        // instead of splitting between tactical setups and order suggestions.
        emit_setup_scorecard_records(
            &mut signal_records,
            tick,
            &reasoning.tactical_setups,
            &canonical_market_snapshot.quotes,
        );
        for record in &mut signal_records {
            let current_price = canonical_market_snapshot
                .quotes
                .get(&record.symbol)
                .map(|quote| quote.last_done);
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
            let change_pct = canonical_market_snapshot
                .quotes
                .get(symbol)
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
        previous_flows = canonical_market_snapshot
            .capital_flow_series
            .iter()
            .filter_map(|(symbol, lines)| {
                lines
                    .last()
                    .map(|line| (symbol.clone(), line.inflow * Decimal::from(10_000)))
            })
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
            if matches!(setup.action, TacticalAction::Enter)
                && setup.confidence >= Decimal::new(7, 1)
            {
                let orphan_signal = setup.risk_notes.iter().any(|note| {
                    note == "carried_forward=true"
                        || note.starts_with("driver=orphan_signal")
                        || note.starts_with("driver_class=orphan_signal")
                });
                if orphan_signal
                    || setup.review_reason_code.is_some()
                    || setup.policy_verdict.as_ref().is_some_and(|verdict| {
                        matches!(
                            verdict.primary,
                            crate::ontology::reasoning::PolicyVerdictKind::ReviewRequired
                                | crate::ontology::reasoning::PolicyVerdictKind::Avoid
                        )
                    })
                {
                    continue;
                }
                if let crate::ontology::reasoning::ReasoningScope::Symbol(sym) = &setup.scope {
                    let workflow_id = setup.workflow_id.clone().unwrap_or_else(|| {
                        crate::persistence::action_workflow::synthetic_workflow_id_for_setup(
                            &setup.setup_id,
                        )
                    });
                    #[cfg(feature = "persistence")]
                    let persisted_workflow = { persisted_workflows_by_id.get(&workflow_id) };
                    #[cfg(feature = "persistence")]
                    if matches!(
                        persisted_workflow.map(|workflow| workflow.current_stage),
                        Some(crate::action::workflow::ActionStage::Review)
                    ) {
                        continue;
                    }
                    let health = signal_momentum.signal_health(sym);
                    if matches!(
                        health,
                        crate::us::temporal::lineage::SignalHealth::Peaking
                            | crate::us::temporal::lineage::SignalHealth::Collapsing
                    ) {
                        continue; // skip enter when signal is peaking or collapsing
                    }
                    if !position_tracker.is_active(sym) {
                        let late_chase = canonical_market_snapshot
                            .quotes
                            .get(sym)
                            .and_then(|quote| {
                                let range = quote.high - quote.low;
                                if range <= Decimal::ZERO {
                                    return None;
                                }
                                Some((quote.last_done - quote.low) / range)
                            })
                            .map(|position_in_range| {
                                if matches!(
                                    action_direction_from_setup(setup),
                                    Some(ActionDirection::Short)
                                ) {
                                    position_in_range <= Decimal::new(30, 2)
                                } else {
                                    position_in_range >= Decimal::new(70, 2)
                                }
                            })
                            .unwrap_or(false);
                        if late_chase {
                            continue;
                        }
                        let price = canonical_market_snapshot
                            .quotes
                            .get(sym)
                            .map(|quote| quote.last_done);
                        if let Some(dims) = dim_snapshot.dimensions.get(sym) {
                            if let Some(existing) = workflows
                                .iter_mut()
                                .find(|workflow| workflow.workflow_id == workflow_id)
                            {
                                let advance = advance_us_workflow_with_price(existing, price, tick);
                                #[cfg(feature = "persistence")]
                                push_us_workflow_advance_events(
                                    existing,
                                    advance,
                                    now,
                                    &mut workflow_events,
                                );
                                if advance.monitoring && !position_tracker.is_active(sym) {
                                    let fp = UsStructuralFingerprint::capture(
                                        sym.clone(),
                                        tick,
                                        existing.entry_price,
                                        dims,
                                    );
                                    position_tracker.enter(fp);
                                }
                                continue;
                            }
                            let Some(price) = price else {
                                continue;
                            };
                            let fp = UsStructuralFingerprint::capture(
                                sym.clone(),
                                tick,
                                Some(price),
                                dims,
                            );
                            let mut wf = UsActionWorkflow::from_setup(setup, tick, Some(price));
                            #[cfg(feature = "persistence")]
                            if let Some(record) = persisted_workflows_by_id.get(&wf.workflow_id) {
                                wf.apply_action_workflow_record(record);
                                let advance =
                                    advance_us_workflow_with_price(&mut wf, Some(price), tick);
                                push_us_workflow_advance_events(
                                    &wf,
                                    advance,
                                    now,
                                    &mut workflow_events,
                                );
                            } else {
                                workflow_events.push(
                                    crate::persistence::action_workflow::ActionWorkflowEventRecord::from_us_workflow_stage(
                                        &wf,
                                        None,
                                        crate::action::workflow::ActionStage::Suggest,
                                        now,
                                        Some("tracker".into()),
                                        Some("workflow generated".into()),
                                    ),
                                );
                                let advance =
                                    advance_us_workflow_with_price(&mut wf, Some(price), tick);
                                push_us_workflow_advance_events(
                                    &wf,
                                    advance,
                                    now,
                                    &mut workflow_events,
                                );
                            }
                            #[cfg(not(feature = "persistence"))]
                            {
                                let _ = advance_us_workflow_with_price(&mut wf, Some(price), tick);
                            }
                            if matches!(
                                wf.stage,
                                UsActionStage::Executed | UsActionStage::Monitoring
                            ) {
                                position_tracker.enter(fp);
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
                    } else {
                        #[cfg(feature = "persistence")]
                        workflow_events.push(
                            crate::persistence::action_workflow::ActionWorkflowEventRecord::from_us_workflow_stage(
                                wf,
                                Some(crate::action::workflow::ActionStage::Monitor),
                                crate::action::workflow::ActionStage::Review,
                                now,
                                Some("tracker".into()),
                                Some("auto-exit: structural degradation".into()),
                            ),
                        );
                    }
                }
            }
        }
        // Update monitoring for active workflows
        for wf in &mut workflows {
            if matches!(wf.stage, UsActionStage::Monitoring) {
                let price = canonical_market_snapshot
                    .quotes
                    .get(&wf.symbol)
                    .map(|quote| quote.last_done);
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

        #[cfg(feature = "persistence")]
        if !workflows.is_empty() {
            let workflow_records = workflows
                .iter()
                .map(crate::persistence::action_workflow::ActionWorkflowRecord::from_us_workflow)
                .collect::<Vec<_>>();
            runtime
                .persist_action_workflows(crate::cases::CaseMarket::Us, workflow_records)
                .await;
        }
        #[cfg(feature = "persistence")]
        if !workflow_events.is_empty() {
            runtime
                .persist_action_workflow_events(crate::cases::CaseMarket::Us, workflow_events)
                .await;
        }

        prev_insights = Some(insights.clone());

        // ── Build live snapshot JSON ──
        let timestamp_str = now
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();

        let mut sorted_events = event_snapshot.events.clone();
        sorted_events.sort_by(|a, b| b.value.magnitude.cmp(&a.value.magnitude));

        let previous_symbol_states_for_surface = previous_symbol_states.clone();
        let previous_cluster_states_for_surface = previous_cluster_states.clone();
        let previous_world_summary_for_surface = previous_world_summary.clone();

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
            &live,
            &position_tracker,
            &workflows,
            &propagation_senses,
            &sorted_events,
            &previous_symbol_states,
            &previous_cluster_states,
            previous_world_summary.as_ref(),
        );
        previous_symbol_states = live_snapshot.symbol_states.clone();
        previous_cluster_states = live_snapshot.cluster_states.clone();
        previous_world_summary = live_snapshot.world_summary.clone();
        // T4 parity — augment US symbol states with raw trade-tape evidence.
        // broker_presence and depth_levels stay empty (US has no L2 or
        // broker queue); their code branches no-op. Trade-tape paths emit
        // raw:trade_aggressor_up/down, raw:block_trade_cluster,
        // raw:aggressor_balanced when Continuation or TurningPoint state.
        let mut live_snapshot = live_snapshot;
        augment_us_live_snapshot_with_raw_expectations(&mut live_snapshot, &raw_trade_tape);
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
        let mut artifact_projection = project_us(UsProjectionInputs {
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
        let mut us_setup_surface_dirty = false;

        // T24 — US wake.reasons surface mirror. Parallel to HK's
        // post-projection surface block (src/hk/runtime.rs ~1240-1510).
        // Every data source below is already computed above but went only
        // to eprintln/console; operator couldn't see it. Each push has a
        // stability threshold so tick-level churn doesn't flood wake.
        {
            use rust_decimal::Decimal;
            use rust_decimal_macros::dec;

            // Shift A: latent + SCM + counterfactual planner. Returns best-action
            // summary for session_quality / regime lines below.
            let planner_summary: Option<
                crate::pipeline::counterfactual_planner::BestActionSummary,
            > = {
                use rust_decimal::prelude::ToPrimitive as _;
                let obs = crate::pipeline::latent_world_state::aggregate_observation(
                    &crate::pipeline::latent_world_state::ObservationInputs {
                        market_stress: Some(
                            insights.stress.composite_stress.to_f64().unwrap_or(0.0),
                        ),
                        synchrony: Some(insights.stress.momentum_consensus.to_f64().unwrap_or(0.0)),
                        ..Default::default()
                    },
                );
                latent_world_state.step(tick, obs);
                artifact_projection
                    .agent_snapshot
                    .wake
                    .reasons
                    .push(latent_world_state.summary_line());

                // Shift B: SCM cascade wake line (symmetric with HK).
                let stress_now = latent_world_state.dim_value(0).unwrap_or(0.0);
                let scm_line = us_scm.describe_intervention(0, stress_now + 1.0);
                artifact_projection
                    .agent_snapshot
                    .wake
                    .reasons
                    .push(scm_line);

                // Shift C: counterfactual rollout planner (symmetric).
                let actions = crate::pipeline::counterfactual_planner::default_candidate_set(
                    &latent_world_state,
                );
                let result = crate::pipeline::counterfactual_planner::best_action(
                    &latent_world_state,
                    &us_scm,
                    &actions,
                    10,
                    crate::pipeline::counterfactual_planner::operator_utility,
                );
                if let Some(ref summary) = result {
                    artifact_projection
                        .agent_snapshot
                        .wake
                        .reasons
                        .push(summary.summary_line());
                }
                result
            };

            // Session / regime / microstructure-churn (operator 2026-04-22 session)
            {
                let active = &artifact_projection.agent_snapshot.active_structures;
                let mut us_flip_this_tick: HashSet<String> = HashSet::new();
                let current_symbols: HashSet<String> =
                    active.iter().map(|s| s.symbol.clone()).collect();
                for sym in us_oscillation_observation_symbols(&us_oscillation, &current_symbols) {
                    let present = current_symbols.contains(&sym);
                    us_oscillation.observe(&sym, present);
                }
                for st in active.iter() {
                    us_signal_velocity.observe(&st.symbol, st.confidence, tick);
                    let t = st.title.to_lowercase();
                    let dir = if t.starts_with("short") {
                        crate::pipeline::direction_flip::Direction::Short
                    } else {
                        crate::pipeline::direction_flip::Direction::Long
                    };
                    if let crate::pipeline::direction_flip::FlipEvent::Flipped { previous } =
                        us_direction_flip.observe(&st.symbol, dir)
                    {
                        us_flip_this_tick.insert(st.symbol.clone());
                        let n = us_direction_flip.flip_count(&st.symbol);
                        artifact_projection
                            .agent_snapshot
                            .wake
                            .reasons
                            .push(format!(
                                "direction_flip: {} {} -> {} (flip #{})",
                                st.symbol,
                                previous.label(),
                                dir.label(),
                                n
                            ));
                    }
                }
                us_drop_inactive_symbol_trackers(
                    &mut us_signal_velocity,
                    &mut us_direction_flip,
                    &us_prev_active_symbols,
                    &current_symbols,
                );
                us_prev_active_symbols = current_symbols;

                if let Some(ref s) = planner_summary {
                    us_planner_u_ring.push_back(s.best.utility);
                    if us_planner_u_ring.len() > 32 {
                        us_planner_u_ring.pop_front();
                    }
                }
                let mut bull_n = 0_usize;
                let mut bear_n = 0_usize;
                for st in active.iter() {
                    let t = st.title.to_lowercase();
                    if t.starts_with("short") {
                        bear_n += 1;
                    } else if t.starts_with("long") {
                        bull_n += 1;
                    }
                }
                let bb_ratio = if bear_n == 0 {
                    bull_n as f64
                } else {
                    bull_n as f64 / bear_n as f64
                };
                us_bull_bear_ring.push_back(bb_ratio);
                if us_bull_bear_ring.len() > 32 {
                    us_bull_bear_ring.pop_front();
                }

                let pu_trend = us_ring_trend_last_n(&us_planner_u_ring, 24);
                let bb_trend = us_ring_trend_last_n(&us_bull_bear_ring, 24);

                let pu_dec = planner_summary
                    .as_ref()
                    .and_then(|sum| rust_decimal::Decimal::from_f64_retain(sum.best.utility));
                // session_quality wake line deleted — rule-tier bucketing
                // (aggressive/normal/defensive at 0.70/0.30 thresholds)
                // and conf>=0.9 count are both rule-based aggregate
                // signals. conf>=0.9 also broke after bridge saturation
                // fix (pre-fix every setup hit 1.0). Operator reads
                // active count + planner_summary directly if needed.

                let regime_inputs = crate::pipeline::regime_classifier::RegimeInputs::from_live(
                    Some(insights.stress.composite_stress),
                    Some(insights.stress.momentum_consensus),
                    pu_dec,
                    bull_n,
                    bear_n,
                    active.len(),
                    pu_trend,
                    bb_trend,
                );
                // regime_classifier::classify deleted — categorical
                // RegimeType (BlowOffTop / OrderlyTrend / etc) was
                // rule-bucketed if-else on stress/sync/bull-bear ratios.
                // The continuous regime_fingerprint below captures the
                // same regime structure as a quantized 5-dim vector
                // without the categorical overlay.
                let _ = (bull_n, bear_n, bb_ratio, pu_trend, bb_trend);

                // Regime fingerprint (continuous embedding).
                let snapshot_ts =
                    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true);
                let us_regime_fp = crate::pipeline::regime_fingerprint::build_us_fingerprint(
                    "us",
                    tick,
                    snapshot_ts,
                    regime_inputs,
                    "", // legacy_label deleted with regime_classifier
                );
                artifact_projection.agent_snapshot.wake.reasons.push(format!(
                    "regime_fingerprint: bucket={} stress={:.2} sync={:.2} bias={:.2} act={:.2} turn={:.2} legacy={}",
                    us_regime_fp.bucket_key,
                    us_regime_fp.stress,
                    us_regime_fp.synchrony,
                    us_regime_fp.bull_bias,
                    us_regime_fp.activity,
                    us_regime_fp.turn_pressure,
                    us_regime_fp.legacy_label,
                ));
                // Engram-style historical analog lookup.
                let now_anc = chrono::Utc::now();
                let (analog_summary, realized_outcomes) =
                    us_regime_analog_index.record("us", &us_regime_fp, now_anc);
                let _ = crate::pipeline::regime_analog_index::write_summary("us", &analog_summary);
                let _ =
                    crate::pipeline::regime_analog_index::write_outcomes("us", &realized_outcomes);
                let _ = latest_us_regime_analog_summary.replace(analog_summary.clone());
                // Publish bucket key for downstream stamping on
                // CaseReasoningAssessmentRecord (collection half of
                // regime-conditional learning).
                if let Ok(mut map) = runtime.current_regime_buckets.write() {
                    map.insert(
                        crate::cases::CaseMarket::Us,
                        us_regime_fp.bucket_key.clone(),
                    );
                }

                // Per-symbol regime fingerprint surface (US variant).
                // US has no TemporalNodeRegistry for per-symbol regime
                // stats (data-driven asymmetry — HK has broker queue +
                // StockNode.regime; US has no equivalent per-symbol
                // regime enum). Uses setup-derived proxies instead so
                // the surface exists on both markets. Emits only when
                // the symbol bucket diverges from the market bucket.
                {
                    use crate::pipeline::regime_fingerprint::build_symbol_fingerprint;
                    use rust_decimal::prelude::ToPrimitive as _;
                    let market_bucket = us_regime_fp.bucket_key.clone();
                    let market_turn = us_regime_fp.turn_pressure;
                    let market_bias = us_regime_fp.bull_bias;
                    let snapshot_ts = us_regime_fp.snapshot_ts.clone();
                    let market_legacy = us_regime_fp.legacy_label.clone();
                    let mut candidates: Vec<(String, String, f64)> = Vec::new();
                    for setup in reasoning.tactical_setups.iter() {
                        let symbol = match &setup.scope {
                            crate::ontology::ReasoningScope::Symbol(s) => s,
                            _ => continue,
                        };
                        if !matches!(
                            setup.action,
                            TacticalAction::Enter | TacticalAction::Observe
                        ) {
                            continue;
                        }
                        let action_str = setup.action.as_str();
                        let confidence = setup.confidence.to_f64().unwrap_or(0.0).clamp(0.0, 1.0);
                        let gap = setup.confidence_gap.to_f64().unwrap_or(0.0).clamp(0.0, 1.0);
                        // Setup-title-based direction fallback — US has no
                        // StockNode.regime, so we infer from the setup's
                        // own declared direction label.
                        let title_lower = setup.title.to_ascii_lowercase();
                        let bull_bias = if title_lower.contains("long") {
                            0.8
                        } else if title_lower.contains("short") {
                            0.2
                        } else {
                            0.5
                        };
                        let stress = (1.0 - confidence).clamp(0.0, 1.0);
                        let synchrony = gap;
                        let activity = confidence;
                        let turn_pressure = (1.0 - gap).clamp(0.0, 1.0);
                        let sym_fp = build_symbol_fingerprint(
                            "us",
                            tick,
                            snapshot_ts.as_str(),
                            &symbol.0,
                            stress,
                            synchrony,
                            bull_bias,
                            activity,
                            turn_pressure,
                            None,
                            None,
                            market_legacy.clone(),
                        );
                        if sym_fp.bucket_key == market_bucket {
                            continue;
                        }
                        let divergence = (sym_fp.turn_pressure - market_turn).abs()
                            + (sym_fp.bull_bias - market_bias).abs();
                        let direction_str = match setup.direction {
                            Some(crate::ontology::reasoning::TacticalDirection::Long) => "long",
                            Some(crate::ontology::reasoning::TacticalDirection::Short) => "short",
                            None => "?",
                        };
                        candidates.push((
                            symbol.0.clone(),
                            format!(
                                "[us] sym_regime: {} action={} dir={} conf={:.2} bucket={} stress={:.2} sync={:.2} bias={:.2} act={:.2} turn={:.2} market_bucket={} divergence={:.2}",
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
                #[cfg(feature = "persistence")]
                {
                    let now_utc = chrono::Utc::now();
                    let due = match last_us_regime_fp_ts {
                        None => true,
                        Some(prev) => (now_utc - prev).num_seconds() >= 60,
                    };
                    if due {
                        if let Some(ref store) = runtime.store {
                            let snap: crate::persistence::regime_fingerprint_snapshot::RegimeFingerprintSnapshot =
                                (&us_regime_fp).into();
                            let store_clone = store.clone();
                            let bucket_for_log = us_regime_fp.bucket_key.clone();
                            tokio::spawn(async move {
                                if let Err(e) =
                                    store_clone.write_regime_fingerprint_snapshot(&snap).await
                                {
                                    eprintln!("[regime_fp] snapshot write failed: {}", e);
                                }
                            });
                            last_us_regime_fp_ts = Some(now_utc);
                            eprintln!("[regime_fp] snapshot: market=us bucket={}", bucket_for_log);
                        }
                    }
                }

                let noisy = us_oscillation.noisy_symbols();
                if !noisy.is_empty() {
                    let joined = noisy.join(", ");
                    artifact_projection
                        .agent_snapshot
                        .wake
                        .reasons
                        .push(format!(
                            "oscillation_blacklist: {} symbol(s) exceed churn threshold — {}",
                            noisy.len(),
                            joined
                        ));
                }
                let rising = us_signal_velocity.rising_symbols();
                if !rising.is_empty() {
                    let show = rising.into_iter().take(5).collect::<Vec<_>>().join(", ");
                    artifact_projection
                        .agent_snapshot
                        .wake
                        .reasons
                        .push(format!("signal_velocity rising: {show}"));
                }
                let falling = us_signal_velocity.falling_symbols();
                if !falling.is_empty() {
                    let show = falling.into_iter().take(5).collect::<Vec<_>>().join(", ");
                    artifact_projection
                        .agent_snapshot
                        .wake
                        .reasons
                        .push(format!("signal_velocity falling: {show}"));
                }

                // cross_layer_narrative deleted — rule-based composite
                // narrative synthesis (same family as case_narrative we
                // already deleted). Per first-principles, narrative
                // assembly belongs to the operator, not Eden.
                let _ = (
                    active,
                    &us_flip_this_tick,
                    &us_oscillation,
                    &us_signal_velocity,
                );
            }

            // Market stress — surface full composite with sub-components
            // when elevated (>=0.5). UsMarketStressIndex exposes pressure
            // dispersion / momentum consensus / volume anomaly separately
            // (HK uses sector_synchrony / pressure_consensus).
            if insights.stress.composite_stress >= dec!(0.5) {
                artifact_projection.agent_snapshot.wake.reasons.push(
                    crate::core::wake_surface::format_labeled_decimal_fields(
                        "market stress elevated",
                        &[
                            ("composite", insights.stress.composite_stress),
                            ("pressure_dispersion", insights.stress.pressure_dispersion),
                            ("momentum_consensus", insights.stress.momentum_consensus),
                            ("volume_anomaly", insights.stress.volume_anomaly),
                        ],
                    ),
                );
            }

            // Sector rotations — US tracks pair-wise sector spread (sector A
            // vs sector B), widening or narrowing. Surface top 3 widening
            // spreads so operator sees which sector pairs are decoupling.
            let mut top_rotations: Vec<&crate::us::graph::insights::UsSectorRotation> = insights
                .rotations
                .iter()
                .filter(|r| r.widening && r.spread.abs() >= dec!(0.15))
                .collect();
            top_rotations.sort_by(|a, b| b.spread.abs().cmp(&a.spread.abs()));
            for rot in top_rotations.iter().take(3) {
                artifact_projection
                    .agent_snapshot
                    .wake
                    .reasons
                    .push(format!(
                        "sector spread widening: {} vs {} spread={} (Δ={})",
                        rot.sector_a.0,
                        rot.sector_b.0,
                        rot.spread.round_dp(2),
                        rot.spread_delta.round_dp(2),
                    ));
            }

            // Stock clusters — US graph identifies directionally aligned
            // stock groups (different from HK's shared_holders). Surface
            // the most aligned/persistent cluster with >= 4 members so
            // the operator sees the non-sector-defined clusters.
            let mut top_clusters: Vec<&crate::us::graph::insights::UsStockCluster> = insights
                .clusters
                .iter()
                .filter(|c| {
                    c.members.len() >= 4
                        && c.directional_alignment.abs() >= dec!(0.60)
                        && c.stability >= dec!(0.50)
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

            // Residual field — sector-coherent residual clusters.
            // coherence>=0.70 (HK uses same threshold) means all sector
            // members diverge from graph prediction in the same direction.
            let coherent: Vec<_> = us_residual_field
                .clustered_sectors
                .iter()
                .filter(|c| c.coherence.abs() >= dec!(0.70))
                .take(4)
                .collect();
            for cluster in &coherent {
                let direction = if cluster.mean_residual < Decimal::ZERO {
                    "selling"
                } else {
                    "buying"
                };
                artifact_projection
                    .agent_snapshot
                    .wake
                    .reasons
                    .push(format!(
                        "residual: {} sector coherent {} (residual={}, coherence={}, {} symbols, dim={})",
                        cluster.sector.0,
                        direction,
                        cluster.mean_residual.round_dp(3),
                        cluster.coherence.round_dp(2),
                        cluster.symbol_count,
                        cluster.dominant_dimension.label(),
                    ));
            }

            // Hidden forces — residual model's Le Verrier hypotheses that
            // survived multi-tick verification. Each confirmed force is
            // independent corroboration for a symbol's surfaced state.
            let confirmed_forces = us_hidden_force_state
                .confirmed_forces()
                .iter()
                .map(|tracker| tracker.symbol.0.clone())
                .collect::<Vec<_>>();
            if let Some(line) = crate::core::wake_surface::hidden_forces_reason(&confirmed_forces) {
                artifact_projection.agent_snapshot.wake.reasons.push(line);
            }

            // Backward reasoning — per-symbol causal investigations with
            // the current leading driver and, when the causal timeline
            // agrees, the streak of ticks that driver has held.
            let leader_by_symbol: std::collections::HashMap<String, (String, u64)> =
                artifact_projection
                    .live_snapshot
                    .causal_leaders
                    .iter()
                    .map(|item| {
                        (
                            item.symbol.clone(),
                            (item.current_leader.clone(), item.leader_streak),
                        )
                    })
                    .collect();
            for chain in backward.chains.iter().take(3) {
                let confidence = chain.confidence.round_dp(2);
                if let Some((leader, streak)) = leader_by_symbol.get(chain.symbol.0.as_str()) {
                    if leader == &chain.primary_driver {
                        artifact_projection.agent_snapshot.wake.reasons.push(
                            crate::core::wake_surface::format_backward_reason(
                                &chain.symbol.0,
                                &chain.primary_driver,
                                Some(*streak),
                                confidence,
                                None,
                            ),
                        );
                        continue;
                    }
                }
                artifact_projection.agent_snapshot.wake.reasons.push(
                    crate::core::wake_surface::format_backward_reason(
                        &chain.symbol.0,
                        &chain.primary_driver,
                        None,
                        confidence,
                        None,
                    ),
                );
            }

            // Cross-market signals — HK↔US bridges that propagation already
            // computed. Surface the highest-confidence pairs so operator
            // sees the HK context for an US name.
            artifact_projection.agent_snapshot.wake.reasons.extend(
                crate::core::wake_surface::cross_market_reason_lines(
                    &artifact_projection.agent_snapshot.cross_market_signals,
                    3,
                ),
            );

            // Stability-gated TurningPoint → wake. Raw state_engine output
            // flips symbols between turning_point / low_information / latent
            // tick-to-tick. Only surface symbols whose state_persistence_ticks
            // >= 3 so the operator sees the actual persistent reads, not noise.
            // Cap at 5 so wake.reasons doesn't blow up when many symbols
            // stabilize simultaneously (e.g. regime shift).
            artifact_projection.agent_snapshot.wake.reasons.extend(
                crate::core::wake_surface::stable_state_reason_lines(
                    &artifact_projection.live_snapshot.symbol_states,
                    5,
                ),
            );

            // T25 option-anchored inference chain deleted — like HK's
            // T22, this assembled a narrative by rule-based combination
            // of primitives (option validation + state + cross-market).
            // Option data now flows into sub-KG as 5 nodes so every
            // structural primitive reads it equally; narrative assembly
            // is the operator's job.

            // Y#7 parity — MarketWaveTracker for US. Same three counts
            // as HK (absence demotions from Y#3, expectation errors
            // from Y#6, momentum collapses) feed the tracker; wave
            // narratives (accelerating / peaking / receding) join
            // wake.reasons. SignalMomentumTracker fields differ from
            // HK (convergence/volume_spike vs institutional_flow/depth/
            // trade_aggression) but the aggregation shape is identical.
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
            let momentum_collapse_count =
                [&signal_momentum.convergence, &signal_momentum.volume_spike]
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

            // T27 W3 integration — forward-propagation intervention.
            // US parallel of HK's intervention surface via
            // UsGraphCausalView. Same honest framing: forward BFS,
            // attenuated per hop, not full Pearl do-calculus. Emitted
            // only for the top vortex with meaningful direction so
            // wake.reasons doesn't fill with no-signal lines.
            if let Some(top_vortex) = pressure_field.vortices.first() {
                let direction_magnitude = top_vortex.tick_direction.abs();
                if direction_magnitude >= dec!(0.1) {
                    use rust_decimal::prelude::ToPrimitive;
                    let causal_view = crate::us::graph::causal_view::UsGraphCausalView::new(&graph);
                    let intervention_sign = top_vortex.tick_direction.to_f64().unwrap_or(0.0);
                    let effects = crate::pipeline::intervention::propagate_intervention(
                        &causal_view,
                        &top_vortex.symbol,
                        intervention_sign,
                        2,
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

            // T18 — Eden track record ledger. See hk/runtime.rs for the
            // parallel surface. Only emit when >=10 observations so the
            // line isn't dominated by early-session noise.
            if eden_ledger.len() >= 10 {
                if let Some(summary) = eden_ledger.summary() {
                    artifact_projection
                        .agent_snapshot
                        .wake
                        .reasons
                        .push(summary.wake_line());
                }
            }
        }

        // ── Belief field update + wake + snapshot ──
        // Symmetric to HK runtime: update from tick-layer pressure + current
        // symbol states, emit notable wake lines, snapshot every 60s async.
        {
            use crate::ontology::objects::Symbol as UsSymbol;
            use crate::pipeline::pressure::TimeScale;

            if let Some(tick_layer) = pressure_field.layers.get(&TimeScale::Tick) {
                let samples: Vec<(
                    UsSymbol,
                    crate::pipeline::pressure::PressureChannel,
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
                // Feed intent belief (world-space posterior).
                let mut by_symbol: std::collections::HashMap<
                    UsSymbol,
                    Vec<(
                        crate::pipeline::pressure::PressureChannel,
                        rust_decimal::Decimal,
                    )>,
                > = std::collections::HashMap::new();
                for (sym, ch, p) in &samples {
                    by_symbol.entry(sym.clone()).or_default().push((*ch, *p));
                }
                for (symbol, channel_samples) in &by_symbol {
                    intent_belief_field.record_channel_samples(symbol, channel_samples);
                }
                belief_field.update_from_pressure_samples(samples, tick);
            }
            for state in &artifact_projection.live_snapshot.symbol_states {
                belief_field.record_state_sample(&UsSymbol(state.symbol.clone()), state.state_kind);
            }
            for notable in belief_field.top_notable_beliefs(5) {
                let symbol_for_decisions = match &notable {
                    crate::pipeline::belief_field::NotableBelief::Gaussian { symbol, .. }
                    | crate::pipeline::belief_field::NotableBelief::Categorical {
                        symbol, ..
                    } => symbol.clone(),
                };
                artifact_projection
                    .agent_snapshot
                    .wake
                    .reasons
                    .push(crate::pipeline::belief_field::format_wake_line(&notable));
                if let Some(summary) = decision_ledger.summary_for(&symbol_for_decisions) {
                    if summary.total_decisions >= 1 {
                        artifact_projection
                            .agent_snapshot
                            .wake
                            .reasons
                            .push(
                                crate::pipeline::decision_ledger::wake_format::format_prior_decisions_line(
                                    &symbol_for_decisions,
                                    summary,
                                ),
                            );
                    }
                }
            }

            // Attention wake: top-5 symbols by state-posterior entropy.
            // Complements belief notable — notable is event-driven (this
            // symbol just shifted); attention is state-snapshot (this
            // symbol is currently most uncertain).
            for item in belief_field.top_attention(5) {
                artifact_projection
                    .agent_snapshot
                    .wake
                    .reasons
                    .push(crate::pipeline::belief_field::format_attention_line(&item));
            }

            // Cross-ontology intent wake (symmetric with HK).
            for decision in intent_belief_field.top_decisive(5, 10, 0.5) {
                artifact_projection.agent_snapshot.wake.reasons.push(
                    crate::pipeline::intent_belief::format_intent_wake_line(&decision),
                );
            }

            // Sector-level intent belief: aggregate per-symbol
            // IntentBelief posteriors up to KG Sector entity.
            // Top-3 sectors with confident consensus intent (symmetric
            // with HK).
            for sector_verdict in crate::pipeline::sector_intent::top_confident_sectors(
                &outer_sector_names,
                &sector_members,
                crate::ontology::objects::Market::Us,
                &intent_belief_field,
                3,
                0.40,
            ) {
                artifact_projection.agent_snapshot.wake.reasons.push(
                    crate::pipeline::sector_intent::format_sector_intent_line(&sector_verdict),
                );
            }

            // Ontology-gap wake (Y#0 seed, symmetric with HK).
            for summary in residual_pattern_tracker.top_residual_patterns(3, 8) {
                artifact_projection.agent_snapshot.wake.reasons.push(
                    crate::pipeline::ontology_emergence::ResidualPatternTracker::format_wake_line(
                        &summary,
                    ),
                );
            }

            // Ontology PROPOSAL wake (Y#0 second piece, symmetric).
            for proposal in residual_pattern_tracker.evaluate_proposals(chrono::Utc::now()) {
                artifact_projection.agent_snapshot.wake.reasons.push(
                    crate::pipeline::ontology_emergence::ResidualPatternTracker::format_proposal_wake_line(
                        &proposal,
                    ),
                );
            }

            // === Sub-KG pipeline (symmetric with HK, broker-free) ===
            {
                use crate::pipeline::symbol_sub_kg as sk;
                use rust_decimal::prelude::ToPrimitive;
                use rust_decimal::Decimal;
                let now_utc = chrono::Utc::now();

                // (1) Quotes + depths → sub-KG nodes (brokers empty on US)
                let quotes_map: HashMap<String, sk::QuoteData> = live
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
                                volume: Decimal::from(q.volume),
                                turnover: q.turnover,
                            },
                        )
                    })
                    .collect();
                // US has no depth book in live state (Nasdaq Basic limitation).
                let depths_map: HashMap<String, sk::DepthData> = HashMap::new();
                sk::update_from_quotes_depths_brokers(
                    &mut subkg_registry,
                    &quotes_map,
                    &depths_map,
                    &HashMap::new(),
                    now_utc,
                    tick,
                );

                // (2) Pressure → 6 Pressure nodes
                use crate::pipeline::pressure::{PressureChannel, TimeScale};
                if let Some(layer) = pressure_field.layers.get(&TimeScale::Tick) {
                    let pressures_map: HashMap<String, sk::PressureSnapshot> = layer
                        .pressures
                        .iter()
                        .map(|(sym, np)| {
                            let net_or_zero = |c: PressureChannel| {
                                np.channels
                                    .get(&c)
                                    .map(|cp| cp.net())
                                    .unwrap_or(Decimal::ZERO)
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
                    sk::update_from_pressure(&mut subkg_registry, &pressures_map, now_utc, tick);
                }

                // (3) Intent belief → 5 IntentMode nodes
                use crate::pipeline::intent_belief::IntentKind;
                let intents_map: HashMap<String, sk::IntentSnapshot> = intent_belief_field
                    .per_symbol_iter()
                    .map(|(sym, belief)| {
                        let prob = |k: IntentKind| -> Decimal {
                            belief
                                .variants
                                .iter()
                                .position(|v| *v == k)
                                .map(|i| belief.probs[i])
                                .unwrap_or(Decimal::ZERO)
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
                sk::update_from_intent(&mut subkg_registry, &intents_map, now_utc, tick);

                // (4) State classification (use previous_symbol_states which
                // was cloned from live_snapshot this tick before the snapshot
                // was moved into artifact_projection)
                let states_map: HashMap<String, String> = previous_symbol_states
                    .iter()
                    .map(|s| (s.symbol.clone(), format!("{:?}", s.state_kind)))
                    .collect();
                sk::update_from_state(&mut subkg_registry, &states_map, now_utc, tick);

                // (5) Session phase (US regular hours 13:30-20:00 UTC)
                {
                    let t = time::OffsetDateTime::now_utc();
                    let min_of_day = t.hour() as u32 * 60 + t.minute() as u32;
                    let is_regular = min_of_day >= 810 && min_of_day < 1200; // 13:30-20:00
                    let phase = if is_regular { "Regular" } else { "OffHours" };
                    sk::update_from_session_phase(&mut subkg_registry, phase, now_utc, tick);
                }

                // (6) Microstructure (trade tape + depth asymmetry + VWAP + queue stability)
                use longport::quote::TradeDirection;
                use longport::quote::TradeStatus;
                let mut micro: HashMap<String, sk::MicrostructureSnapshot> = HashMap::new();
                for (sym, trades) in raw_trade_tape.per_symbol.iter() {
                    let nt = time::OffsetDateTime::now_utc();
                    let c30 = nt - time::Duration::seconds(30);
                    let c60 = nt - time::Duration::seconds(60);
                    let c120 = nt - time::Duration::seconds(120);
                    let mut buy = 0i64;
                    let mut sell = 0i64;
                    let mut cnt1 = 0i64;
                    let mut cnt_prev = 0i64;
                    for t in trades {
                        if t.timestamp >= c30 {
                            match t.direction {
                                TradeDirection::Up => buy += t.volume,
                                TradeDirection::Down => sell += t.volume,
                                _ => {}
                            }
                        }
                        if t.timestamp >= c60 {
                            cnt1 += 1;
                        } else if t.timestamp >= c120 {
                            cnt_prev += 1;
                        }
                    }
                    let entry = micro.entry(sym.0.clone()).or_default();
                    entry.trade_tape_buy_minus_sell_30s = Decimal::from(buy - sell);
                    entry.trade_tape_accel_last_1m = Decimal::from(cnt1 - cnt_prev);
                }
                // US has no live depth book (Nasdaq Basic limitation) so
                // DepthAsymmetry / QueueStability nodes stay unset. Stealth
                // accumulation still works via Volume vs Price kinematics.
                // Suppress unused-warning on the helpers kept for symmetry.
                let _ = (
                    &us_prev_top_bid,
                    &us_prev_top_ask,
                    &us_bid1_stable,
                    &us_ask1_stable,
                );
                for (sym, q) in &live.quotes {
                    let entry = micro.entry(sym.0.clone()).or_default();
                    if q.volume > 0 {
                        let vwap = q.turnover / Decimal::from(q.volume);
                        entry.vwap = vwap;
                        if vwap > Decimal::ZERO {
                            entry.vwap_deviation_pct =
                                (q.last_done - vwap) / vwap * Decimal::from(100);
                        }
                    }
                }
                sk::update_from_microstructure(&mut subkg_registry, &micro, now_utc, tick);

                // (7) Events (halt + big trade + volume spike)
                let mut events: HashMap<String, sk::EventSnapshot> = HashMap::new();
                for (sym, q) in &live.quotes {
                    if !matches!(q.trade_status, TradeStatus::Normal) {
                        us_halted_today.insert(sym.clone());
                    }
                }
                for sym in us_halted_today.iter() {
                    events.entry(sym.0.clone()).or_default().has_halted_today = true;
                }
                for (sym, trades) in raw_trade_tape.per_symbol.iter() {
                    let nt = time::OffsetDateTime::now_utc();
                    let c1h = nt - time::Duration::hours(1);
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
                    let big = vols.iter().filter(|v| **v > median * 5).count() as u32;
                    events
                        .entry(sym.0.clone())
                        .or_default()
                        .big_trade_count_last_1h = big;
                }
                for (sym, d) in &dim_snapshot.dimensions {
                    if d.volume_profile.to_f64().unwrap_or(0.0) > 0.5 {
                        events.entry(sym.0.clone()).or_default().volume_spike_fresh = true;
                    }
                }
                sk::update_from_events(&mut subkg_registry, &events, now_utc, tick);

                // (8) Roles (leader/laggard + sector relative)
                let mut roles: HashMap<String, sk::RoleSnapshot> = HashMap::new();
                for (sid, members) in &sector_members {
                    if members.len() < 3 {
                        continue;
                    }
                    let mut vels: Vec<(Symbol, f64)> = Vec::new();
                    for sym in members {
                        if let Some(q) = live.quotes.get(sym) {
                            let prev = q.prev_close.to_f64().unwrap_or(0.0);
                            let last = q.last_done.to_f64().unwrap_or(0.0);
                            if prev > 0.0 {
                                vels.push((sym.clone(), (last - prev) / prev));
                            }
                        }
                    }
                    if vels.is_empty() {
                        continue;
                    }
                    let avg: f64 = vels.iter().map(|(_, v)| *v).sum::<f64>() / vels.len() as f64;
                    let max_v = vels
                        .iter()
                        .map(|(_, v)| *v)
                        .fold(f64::NEG_INFINITY, f64::max);
                    let min_v = vels.iter().map(|(_, v)| *v).fold(f64::INFINITY, f64::min);
                    for (sym, v) in &vels {
                        let entry = roles.entry(sym.0.clone()).or_default();
                        entry.sector_relative_strength =
                            Decimal::from_f64_retain((v - avg) * 100.0).unwrap_or(Decimal::ZERO);
                        let score = if (*v - max_v).abs() < 1e-9 {
                            1.0
                        } else if (*v - min_v).abs() < 1e-9 {
                            -1.0
                        } else {
                            (v - avg) / (max_v - min_v + 1e-9)
                        };
                        entry.leader_laggard_score =
                            Decimal::from_f64_retain(score).unwrap_or(Decimal::ZERO);
                        let _ = sid;
                    }
                }
                sk::update_from_roles(&mut subkg_registry, &roles, now_utc, tick);

                // (8a) Cross-market bridge: US → HK counterpart label.
                // Static mapping of 14 dual-listed pairs from
                // crate::bridges::pairs::CROSS_MARKET_PAIRS.
                {
                    let bridges: HashMap<String, String> =
                        crate::bridges::pairs::CROSS_MARKET_PAIRS
                            .iter()
                            .map(|p| (p.us_symbol.to_string(), p.hk_symbol.to_string()))
                            .collect();
                    sk::update_cross_market_bridge(&mut subkg_registry, &bridges, now_utc, tick);
                }
                // Holdings: institutional_holder_count + etf_holding_pct
                // (count proxy) from terrain. Same shape as HK.
                if !terrain.institutional_holdings.is_empty() || !terrain.fund_holdings.is_empty() {
                    let mut holdings: HashMap<String, sk::HoldingSnapshot> = HashMap::new();
                    for (_name, rows) in &terrain.institutional_holdings {
                        for (sym, _pct) in rows {
                            let entry = holdings.entry(sym.0.clone()).or_insert_with(|| {
                                sk::HoldingSnapshot {
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
                                sk::HoldingSnapshot {
                                    insider_holding_pct: Decimal::ZERO,
                                    institutional_holder_count: 0,
                                    southbound_flow_today: Decimal::ZERO,
                                    etf_holding_pct: Decimal::ZERO,
                                }
                            });
                            entry.etf_holding_pct += Decimal::ONE;
                        }
                    }
                    sk::update_from_holdings(&mut subkg_registry, &holdings, now_utc, tick);
                }
                // Earnings: derive days-until + in-window from terrain
                // upcoming_events. Skips symbols with no upcoming event.
                if !terrain.upcoming_events.is_empty() {
                    let today = time::OffsetDateTime::now_utc().date();
                    let mut earnings: HashMap<String, sk::EarningsSnapshot> = HashMap::new();
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
                                    sk::EarningsSnapshot {
                                        days_until_next: days,
                                        in_window: days <= 3,
                                    },
                                );
                            }
                        }
                    }
                    if !earnings.is_empty() {
                        sk::update_from_earnings(&mut subkg_registry, &earnings, now_utc, tick);
                    }
                }

                // (8b) Option surface → 5 Option nodes per symbol.
                // US has 639 option surfaces per tick; HK has no option
                // surface (warrant_pool covers HK's equivalent). Reads
                // from canonical_market_snapshot built earlier this tick.
                if market_capabilities.supports(MarketDataCapability::OptionSurface) {
                    let mut option_surfaces: HashMap<String, sk::OptionSurfaceFields> =
                        HashMap::new();
                    for obs in &option_surface_observations {
                        option_surfaces.insert(
                            obs.underlying.0.clone(),
                            sk::OptionSurfaceFields {
                                atm_call_iv: obs.atm_call_iv,
                                atm_put_iv: obs.atm_put_iv,
                                put_call_skew: obs.put_call_skew,
                                put_call_oi_ratio: obs.put_call_oi_ratio,
                                total_oi: obs.total_call_oi + obs.total_put_oi,
                            },
                        );
                    }
                    if !option_surfaces.is_empty() {
                        sk::update_from_option_surfaces(
                            &mut subkg_registry,
                            &option_surfaces,
                            now_utc,
                            tick,
                        );
                    }
                }

                // (9) Snapshot + cluster_sync + propagation + contrast + kinematics + consistency every 5 ticks
                subkg_snapshot_tick += 1;
                if subkg_snapshot_tick >= 5 {
                    subkg_snapshot_tick = 0;
                    use crate::pipeline::runtime_stage_trace::{
                        RuntimeStage, RuntimeStagePlan, RuntimeStageTrace,
                    };
                    let stage_plan = RuntimeStagePlan::canonical();
                    let mut runtime_trace = RuntimeStageTrace::new("us", tick, now_utc);
                    runtime_trace
                        .record_planned(stage_plan, RuntimeStage::RegimeAnalogRecord)
                        .expect("US runtime stage is declared in canonical plan");
                    // WL graph signature per sub-KG — typed-graph
                    // structural fingerprint via Weisfeiler-Lehman
                    // relabeling. Per-symbol row in ndjson; signature_hash
                    // is the key for graph-structural analog lookup.
                    let us_wl_analogs_by_symbol: HashMap<
                        String,
                        crate::pipeline::symbol_wl_analog_index::AnalogMatch,
                    >;
                    {
                        let rows = crate::pipeline::wl_graph_signature::build_signature_rows(
                            "us",
                            &subkg_registry,
                            crate::pipeline::wl_graph_signature::WL_ITERATIONS,
                            now_utc,
                        );
                        let _ =
                            crate::pipeline::wl_graph_signature::write_signature_rows("us", &rows);
                        // Per-symbol structural analog lookup.
                        let mut analogs = Vec::with_capacity(rows.len());
                        for row in &rows {
                            let m = us_symbol_wl_analog_index.record(
                                "us",
                                &row.symbol,
                                &row.signature_hash,
                                row.ts,
                            );
                            analogs.push(m);
                        }
                        let _ =
                            crate::pipeline::symbol_wl_analog_index::write_matches("us", &analogs);
                        us_wl_analogs_by_symbol = analogs
                            .iter()
                            .map(|a| (a.symbol.clone(), a.clone()))
                            .collect();
                    }
                    runtime_trace
                        .record_planned(stage_plan, RuntimeStage::WlAnalogRecord)
                        .expect("US runtime stage is declared in canonical plan");
                    // V2 Phase 4: feed accumulated forecast accuracy from
                    // ActiveProbeRunner into sub-KG ForecastAccuracy NodeId.
                    // Lag = 1 tick + horizon (probe → realize → next builder).
                    let us_probe_accuracy = us_active_probe.accuracy_by_symbol();
                    runtime_trace
                        .record_planned(stage_plan, RuntimeStage::ActiveProbeAccuracyRead)
                        .expect("US runtime stage is declared in canonical plan");
                    // V4 KL surprise: feed the freshly-updated belief field
                    // into the per-(symbol, channel) baseline tracker, then
                    // export per-symbol (magnitude, direction) for sub-KG
                    // ingestion. Tracker observation must precede the
                    // substrate evidence build so the next BP pass picks up
                    // surprises that emerged this tick.
                    kl_surprise_tracker.observe_from_belief_field(&belief_field);
                    let us_kl_surprise_by_symbol =
                        kl_surprise_tracker.surprise_summary(&belief_field);
                    let substrate_evidence =
                        crate::pipeline::symbol_sub_kg::build_substrate_evidence_snapshots(
                            &subkg_registry,
                            crate::pipeline::symbol_sub_kg::SubstrateEvidenceInput {
                                decision_ledger: Some(&decision_ledger),
                                synthetic_outcomes: &cached_us_synthetic_outcomes,
                                engram_summary: latest_us_regime_analog_summary.as_ref(),
                                wl_analogs_by_symbol: Some(&us_wl_analogs_by_symbol),
                                belief_field: Some(&belief_field),
                                forecast_accuracy_by_symbol: Some(&us_probe_accuracy),
                                // V3.2 cross-ontology — sector intent
                                // not yet computed for US in this tick
                                // path. Defer to follow-up wiring.
                                sector_intent_by_symbol: None,
                                kl_surprise_by_symbol: Some(&us_kl_surprise_by_symbol),
                            },
                        );
                    runtime_trace
                        .record_planned(stage_plan, RuntimeStage::SubKgEvidenceBuild)
                        .expect("US runtime stage is declared in canonical plan");
                    crate::pipeline::symbol_sub_kg::update_from_substrate_evidence(
                        &mut subkg_registry,
                        &substrate_evidence,
                        now_utc,
                        tick,
                    );
                    runtime_trace
                        .record_planned(stage_plan, RuntimeStage::SubKgEvidenceApply)
                        .expect("US runtime stage is declared in canonical plan");
                    let graph_frontier =
                        crate::pipeline::frontier::GraphFrontier::from_subkg_registry(
                            tick as u64,
                            &subkg_registry,
                        );
                    let frontier_propagation = graph_frontier.local_propagation_plan();
                    let frontier_candidates = frontier_propagation.propagation_candidates();
                    let frontier_dry_run =
                        crate::pipeline::frontier::FrontierPropagationDryRun::from_candidates(
                            tick as u64,
                            &frontier_candidates,
                        );
                    let frontier_pressure_cache =
                        crate::pipeline::frontier::FrontierPressureCandidateCache::from_dry_run(
                            &frontier_dry_run,
                        );
                    let frontier_pressure_gate =
                        crate::pipeline::frontier::FrontierPressureConvergenceGate::from_cache(
                            &frontier_pressure_cache,
                        );
                    let frontier_next_proposal =
                        crate::pipeline::frontier::FrontierNextProposal::from_pressure_gate(
                            &frontier_pressure_gate,
                        );
                    let frontier_loop_summary =
                        graph_frontier.bounded_propagation_summary(&frontier_next_proposal, 2);
                    runtime_trace
                        .record_planned(stage_plan, RuntimeStage::FrontierBuild)
                        .expect("US runtime stage is declared in canonical plan");
                    // sub_kg summary line removed from wake — ndjson
                    // snapshot carries full per-symbol structure, including
                    // ontology memory/belief/causal evidence nodes.
                    match subkg_registry.serialize_active_to_lines() {
                        Ok(lines) => {
                            let _ = subkg_writer.try_send_batch(lines);
                        }
                        Err(e) => eprintln!("[sub_kg] us serialize failed: {}", e),
                    }
                    runtime_trace
                        .record_planned(stage_plan, RuntimeStage::SubKgSnapshotWrite)
                        .expect("US runtime stage is declared in canonical plan");
                    let mut artifact_write_errors = Vec::new();
                    // Sector sub-KG: forward composition Symbol → Sector.
                    // Aggregates each sector's member sub-KGs by NodeKind
                    // (mean / variance / outlier_count). Feeds into
                    // structural_contrast as the second contrast axis
                    // (vs own-sector mean), and dumps to ndjson for
                    // operator inspection. Pure stateless aggregation.
                    let sector_subkgs = crate::pipeline::sector_sub_kg::build_from_registry(
                        &subkg_registry,
                        &sector_members,
                        &outer_sector_names,
                        now_utc,
                    );
                    runtime_trace
                        .record_planned(stage_plan, RuntimeStage::SectorSubKgBuild)
                        .expect("US runtime stage is declared in canonical plan");
                    match crate::pipeline::sector_sub_kg::serialize_active_to_lines(
                        &sector_subkgs,
                        "us",
                    ) {
                        Ok(lines) => {
                            let _ = sector_subkg_writer.try_send_batch(lines);
                        }
                        Err(e) => eprintln!("[sector_sub_kg] us serialize failed: {}", e),
                    }
                    // Cross-sector contrast — second hop of visual model.
                    // Asks "which SECTOR is the standout this snapshot?" by
                    // applying center-surround DoG one zoom level up.
                    let sector_contrast_events =
                        crate::pipeline::cross_sector_contrast::detect_sector_contrasts(
                            "us",
                            &sector_subkgs,
                            now_utc,
                        );
                    let _ = cross_sector_writer.try_send_batch(sector_contrast_events.clone());
                    // Backward propagation: hot sector → quiet members lag.
                    let member_lag_events =
                        crate::pipeline::sector_to_symbol_propagation::detect_member_lag(
                            "us",
                            &subkg_registry,
                            &sector_subkgs,
                            &sector_members,
                            now_utc,
                        );
                    let _ = sector_to_symbol_writer.try_send_batch(member_lag_events.clone());
                    // Sector kinematics — cross-tick velocity / acceleration
                    // / zero-crossing turning points on sector-mean signal.
                    let sector_kin_events = crate::pipeline::sector_kinematics::update_and_detect(
                        "us",
                        &sector_subkgs,
                        &mut sector_kinematics_tracker,
                        now_utc,
                    );
                    let _ = sector_kinematics_writer.try_send_batch(sector_kin_events.clone());
                    let symbol_to_sector_str: HashMap<String, String> = symbol_sector
                        .iter()
                        .map(|(sym, sid)| (sym.0.clone(), sid.0.clone()))
                        .collect();

                    // Cluster sync
                    let clusters_str: HashMap<String, Vec<String>> = sector_members
                        .iter()
                        .map(|(sid, syms)| {
                            (sid.0.clone(), syms.iter().map(|s| s.0.clone()).collect())
                        })
                        .collect();
                    let cs_events = crate::pipeline::cluster_sync::detect_cluster_sync(
                        "us",
                        &subkg_registry,
                        &clusters_str,
                        now_utc,
                    );
                    if !cs_events.is_empty() {
                        let _ = crate::pipeline::cluster_sync::write_events("us", &cs_events);
                    }

                    // Cross-symbol propagation along UsGraph StockToStock
                    use crate::pipeline::cross_symbol_propagation as csp;
                    let mut master_edges: Vec<csp::MasterEdge> = Vec::new();
                    let mut bp_master_graph_edges = 0usize;
                    for edge_idx in graph.graph.edge_indices() {
                        if let crate::us::graph::graph::UsEdgeKind::StockToStock(s2s) =
                            &graph.graph[edge_idx]
                        {
                            bp_master_graph_edges += 1;
                            let (a, b) = graph.graph.edge_endpoints(edge_idx).unwrap();
                            let sa = graph
                                .stock_nodes
                                .iter()
                                .find(|(_, i)| **i == a)
                                .map(|(s, _)| s.0.clone());
                            let sb = graph
                                .stock_nodes
                                .iter()
                                .find(|(_, i)| **i == b)
                                .map(|(s, _)| s.0.clone());
                            if let (Some(sa), Some(sb)) = (sa, sb) {
                                let w = s2s.similarity.to_f64().unwrap_or(0.0);
                                if w > 0.0 {
                                    master_edges.push(csp::MasterEdge {
                                        from: sa,
                                        to: sb,
                                        weight: w,
                                        edge_type: "StockToStock".into(),
                                    });
                                }
                            }
                        }
                    }
                    let prop_snaps = csp::propagate(
                        "us",
                        &subkg_registry,
                        &master_edges,
                        csp::DEFAULT_PROPAGATION_RATE,
                        now_utc,
                    );
                    if !prop_snaps.is_empty() {
                        let _ = csp::write_snapshots("us", &prop_snaps);
                    }
                    runtime_trace
                        .record_planned(stage_plan, RuntimeStage::CrossSymbolPropagation)
                        .expect("US runtime stage is declared in canonical plan");
                    // Loopy BP — Pearl-style sum-product on master KG.
                    {
                        let bp_input_edges: Vec<crate::pipeline::loopy_bp::BpInputEdge> =
                            master_edges
                                .iter()
                                .map(|e| crate::pipeline::loopy_bp::BpInputEdge {
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
                        us_lead_lag_tracker.ingest(&subkg_registry);
                        let lead_lag_evs = crate::pipeline::lead_lag_index::detect_lead_lag(
                            "us",
                            &us_lead_lag_tracker,
                            &bp_edges,
                            now_utc,
                        );
                        crate::core::runtime_artifacts::record_artifact_result(
                            &mut artifact_write_errors,
                            "lead_lag_events",
                            crate::pipeline::lead_lag_index::write_events("us", &lead_lag_evs),
                        );
                        runtime_trace
                            .record_planned(stage_plan, RuntimeStage::LeadLagDetect)
                            .expect("US runtime stage is declared in canonical plan");

                        // V4 Phase 1.5: sub-KG emergence detection. Walks
                        // every symbol's sub-KG, computes the cross-NodeId
                        // emergence score, and synthesizes a TacticalSetup
                        // for any symbol whose score crosses its own
                        // self-referential 1σ baseline. Inserted *before*
                        // BP build_inputs so posterior confidence flows
                        // back onto emergence-derived setups in the
                        // apply_posterior_confidence loop below.
                        // V5.1: graph-attention budget — derive per-symbol
                        // centrality from the most recent hub aggregation
                        // (US uses `prev_tick_hubs` populated upstream).
                        // High-centrality symbols processed every tick;
                        // low-centrality symbols throttled per the budget.
                        let centrality =
                            crate::pipeline::graph_attention::centrality_from_hubs(&prev_tick_hubs);
                        // V7.2: same frontier-gated emergence as HK runtime —
                        // walk only symbols whose pressure convergence gate
                        // passed the self-referential noise floor. Empty
                        // gate (cold-start ticks) falls back to walking
                        // every symbol via None.
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
                            reasoning.tactical_setups.push(
                                crate::pipeline::sub_kg_emergence::synthesize_setup_from_emergence(
                                    emergence,
                                ),
                            );
                        }
                        if !emergence_events.is_empty() {
                            artifact_projection
                                .agent_snapshot
                                .wake
                                .reasons
                                .push(format!(
                                    "sub_kg_emergence: {} symbols (top z={:.2})",
                                    emergence_events.len(),
                                    emergence_events.iter().map(|e| e.z).fold(0.0_f64, f64::max),
                                ));
                        }

                        // V2: BP single entry. Priors come from sub-KG
                        // (already populated with Memory/Belief/Causal
                        // evidence by update_from_substrate_evidence above).
                        // Lead-lag stays as the only explicit input — it's
                        // an edge property, not a node property.
                        let bp_build_inputs_start = Instant::now();
                        let (priors, edges) = crate::pipeline::loopy_bp::build_inputs(
                            &subkg_registry,
                            &bp_input_edges,
                            &lead_lag_evs,
                        );
                        let bp_pruning_shadow =
                            crate::pipeline::loopy_bp::build_pruning_shadow_summary(
                                &priors, &edges,
                            );
                        let bp_build_inputs_elapsed = bp_build_inputs_start.elapsed();
                        runtime_trace
                            .record_planned(stage_plan, RuntimeStage::BpBuildInputs)
                            .expect("US runtime stage is declared in canonical plan");
                        use crate::pipeline::event_driven_bp::BeliefSubstrate as _;
                        let bp_run_start = Instant::now();
                        belief_substrate.observe_tick(&priors, &edges, tick as u64);
                        let bp_run_elapsed = bp_run_start.elapsed();
                        let view = belief_substrate.posterior_snapshot();
                        runtime_trace
                            .record_planned(stage_plan, RuntimeStage::BpRun)
                            .expect("US runtime stage is declared in canonical plan");
                        let bp_message_trace_write_start = Instant::now();
                        let bp_trace_rows = crate::pipeline::loopy_bp::build_belief_only_trace_rows(
                            "us", tick as u64, &priors, &edges, &view.beliefs, now_utc,
                        );
                        let _ = bp_message_trace_writer.try_send_batch(bp_trace_rows);
                        let bp_message_trace_write_elapsed = bp_message_trace_write_start.elapsed();
                        let iterations = view.iterations;
                        let converged = view.converged;
                        let visual_frame =
                            crate::pipeline::visual_graph_frame::build_visual_graph_frame(
                                "us",
                                tick,
                                &subkg_registry,
                                &edges,
                                &priors,
                                &view.beliefs,
                                now_utc,
                            );
                        if let Some(previous) = previous_visual_frame.as_ref() {
                            let delta = crate::pipeline::temporal_graph_delta::build_delta(
                                "us",
                                tick,
                                previous,
                                &visual_frame,
                                now_utc,
                            );
                            let _ = temporal_delta_writer.try_send_batch(delta);
                        }
                        let _ = visual_frame_writer.try_send_batch(visual_frame.clone());
                        previous_visual_frame = Some(visual_frame);
                        let bp_marginals_write_start = Instant::now();
                        let rows = crate::pipeline::loopy_bp::build_marginal_rows(
                            "us",
                            &priors,
                            &view.beliefs,
                            iterations,
                            converged,
                            now_utc,
                        );
                        let _ = bp_marginals_writer.try_send_batch(rows);
                        let bp_marginals_write_elapsed = bp_marginals_write_start.elapsed();
                        runtime_trace
                            .record_planned(stage_plan, RuntimeStage::BpMarginalsWrite)
                            .expect("US runtime stage is declared in canonical plan");
                        let beliefs = view.beliefs.clone();
                        // V2: BP posterior is the single source of truth.
                        // No post-BP belief/history modulation — those
                        // signals already entered BP via NodeId values.
                        // V5.3 + 2026-04-29 ordering fix: reconcile_direction
                        // must run BEFORE apply_posterior_confidence. The
                        // confidence write reads setup.direction to pick which
                        // posterior cell (Bull vs Bear) becomes p_target;
                        // running reconcile after would leave emerge:* setups
                        // whose direction got flipped with confidence stuck
                        // on the pre-flip (now-wrong) side, systematically
                        // losing the percentile race.
                        let _us_emerge_dir_touched =
                            crate::pipeline::sub_kg_emergence::reconcile_direction_with_bp(
                                &mut reasoning.tactical_setups,
                                &beliefs,
                            );
                        let mut us_bp_conf_applied = 0usize;
                        let mut us_bp_conf_skipped = 0usize;
                        for setup in reasoning.tactical_setups.iter_mut() {
                            if crate::pipeline::loopy_bp::apply_posterior_confidence(
                                setup, &beliefs,
                            ) {
                                us_bp_conf_applied += 1;
                            } else {
                                us_bp_conf_skipped += 1;
                            }
                        }
                        runtime_trace
                            .record_planned(stage_plan, RuntimeStage::BpPosteriorConfidence)
                            .expect("US runtime stage is declared in canonical plan");
                        // V2/V4 cleanup: action upgrade (Observe→Review→
                        // Enter) is data-driven on either the tick's
                        // confidence distribution (default percentile) or
                        // the per-symbol KL surprise z-score (opt-in via
                        // EDEN_ACTION_PROMOTION=kl_surprise). Both modes
                        // replace the deleted hardcoded tension thresholds.
                        crate::pipeline::action_promotion::apply_action_promotion(
                            &mut reasoning.tactical_setups,
                            &kl_surprise_tracker,
                            &belief_field,
                        );
                        us_setup_surface_dirty = true;
                        if us_bp_conf_applied + us_bp_conf_skipped > 0 {
                            artifact_projection
                                .agent_snapshot
                                .wake
                                .reasons
                                .push(format!(
                                    "bp_posterior_confidence: applied={} skipped={} \
                                 bp_iters={} converged={} lead_lag_events={}",
                                    us_bp_conf_applied,
                                    us_bp_conf_skipped,
                                    iterations,
                                    converged,
                                    lead_lag_evs.len(),
                                ));
                        }

                        // V2 Phase 4: active probing — counterfactual BP
                        // experiments. (1) evaluate any due probes against
                        // current beliefs (accumulates per-symbol forecast
                        // accuracy). (2) pick top-3 high-entropy symbols.
                        // (3) run bull/bear intervention BP per target,
                        // enqueue forecast for evaluation in PROBE_HORIZON
                        // ticks. Pure Pearl do-calculus subset — no learning.
                        let probe_outcomes =
                            us_active_probe.evaluate_due(tick, &beliefs, now_utc, "us");
                        crate::core::runtime_artifacts::record_artifact_result(
                            &mut artifact_write_errors,
                            "active_probe_outcomes",
                            crate::pipeline::active_probe::write_outcomes("us", &probe_outcomes),
                        );
                        runtime_trace
                            .record_planned(stage_plan, RuntimeStage::ActiveProbeEvaluate)
                            .expect("US runtime stage is declared in canonical plan");
                        let probe_targets = crate::pipeline::active_probe::pick_probe_targets(
                            &beliefs,
                            crate::pipeline::active_probe::PROBE_TARGETS_PER_TICK,
                        );
                        let probe_forecasts = us_active_probe.emit_probes(
                            &probe_targets,
                            &priors,
                            &edges,
                            tick,
                            now_utc,
                            "us",
                        );
                        crate::core::runtime_artifacts::record_artifact_result(
                            &mut artifact_write_errors,
                            "active_probe_forecasts",
                            crate::pipeline::active_probe::write_forecasts("us", &probe_forecasts),
                        );
                        runtime_trace
                            .record_planned(stage_plan, RuntimeStage::ActiveProbeEmit)
                            .expect("US runtime stage is declared in canonical plan");
                        let probe_mean_acc = if probe_outcomes.is_empty() {
                            None
                        } else {
                            let sum: f64 = probe_outcomes.iter().map(|o| o.mean_accuracy).sum();
                            Some(sum / probe_outcomes.len() as f64)
                        };
                        if !probe_forecasts.is_empty() || !probe_outcomes.is_empty() {
                            let acc_str = probe_mean_acc
                                .map(|a| format!("{:.2}", a))
                                .unwrap_or_else(|| "n/a".to_string());
                            artifact_projection
                                .agent_snapshot
                                .wake
                                .reasons
                                .push(format!(
                                "active_probe: emitted={} evaluated={} mean_accuracy={} pending={}",
                                probe_forecasts.len(),
                                probe_outcomes.len(),
                                acc_str,
                                us_active_probe.pending_count(),
                            ));
                        }
                        runtime_trace
                            .record_planned(stage_plan, RuntimeStage::ArtifactHealth)
                            .expect("US runtime stage is declared in canonical plan");
                        let plan_coverage = runtime_trace.plan_coverage(stage_plan);
                        if let Err(e) = runtime_trace.write_ndjson() {
                            eprintln!("[runtime_stage] us write failed: {}", e);
                            artifact_write_errors.push(
                                crate::core::runtime_artifacts::RuntimeArtifactWriteError {
                                    artifact: "runtime_stage_trace".to_string(),
                                    error: e.to_string(),
                                },
                            );
                        }
                        let health_tick = crate::core::runtime_artifacts::RuntimeHealthTick {
                            ts: now_utc,
                            market: "us".to_string(),
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
                            probe_pending: us_active_probe.pending_count(),
                            probe_mean_accuracy: probe_mean_acc,
                            artifact_write_errors,
                        };
                        if let Err(e) = crate::core::runtime_artifacts::write_runtime_health_tick(
                            crate::core::market::MarketId::Us,
                            &health_tick,
                        ) {
                            eprintln!("[runtime_health] us write failed: {}", e);
                        }
                    }

                    // Contrast (build neighbor map from UsGraph StockToStock)
                    let mut neighbors: crate::pipeline::structural_contrast::NeighborMap =
                        HashMap::new();
                    for e in &master_edges {
                        neighbors
                            .entry(e.from.clone())
                            .or_default()
                            .push(e.to.clone());
                    }
                    let contrast_events = crate::pipeline::structural_contrast::detect_contrasts(
                        "us",
                        &subkg_registry,
                        &neighbors,
                        Some(&sector_subkgs),
                        &symbol_to_sector_str,
                        now_utc,
                    );
                    if !contrast_events.is_empty() {
                        // Aggregate count + sample wake removed;
                        // full events in eden-contrast-us.ndjson.
                        let _ = crate::pipeline::structural_contrast::write_events(
                            "us",
                            &contrast_events,
                        );
                    }

                    // Kinematics
                    let kin_events = crate::pipeline::structural_kinematics::update_and_detect(
                        "us",
                        &subkg_registry,
                        &mut kinematics_tracker,
                        now_utc,
                    );
                    if !kin_events.is_empty() {
                        // Count + sample wake removed;
                        // full turning points in eden-kinematics-us.ndjson.
                        let _ =
                            crate::pipeline::structural_kinematics::write_events("us", &kin_events);
                    }

                    // Consistency gauge (broker-free subset: 6 relationships)
                    {
                        use crate::pipeline::consistency_gauge as cg;
                        let mut cs_events_all = Vec::new();

                        // Stealth
                        let mut pairs_vp: Vec<(String, f64, f64)> = Vec::new();
                        for (sym, _kg) in &subkg_registry.graphs {
                            let vv = kinematics_tracker
                                .velocity(sym, &sk::NodeId::Volume)
                                .unwrap_or(0.0);
                            let pv = kinematics_tracker
                                .velocity(sym, &sk::NodeId::LastPrice)
                                .unwrap_or(0.0)
                                .abs();
                            if vv.abs() > f64::EPSILON {
                                pairs_vp.push((sym.clone(), vv, pv));
                            }
                        }
                        cs_events_all.extend(cg::residuals_2d(
                            "us",
                            "stealth_volume_price_decoupling",
                            &pairs_vp,
                            cg::OUTLIER_PERCENTILE,
                            now_utc,
                        ));

                        // Depth × trade decoupling
                        let mut dtd: Vec<(String, f64, f64)> = Vec::new();
                        for (sym, kg) in &subkg_registry.graphs {
                            let da = kg
                                .nodes
                                .get(&sk::NodeId::DepthAsymmetryTop3)
                                .and_then(|n| n.value)
                                .map(|v| v.to_f64().unwrap_or(0.0))
                                .unwrap_or(0.0);
                            let tt = kg
                                .nodes
                                .get(&sk::NodeId::TradeTapeBuyMinusSell30s)
                                .and_then(|n| n.value)
                                .map(|v| v.to_f64().unwrap_or(0.0))
                                .unwrap_or(0.0);
                            if da.abs() > f64::EPSILON || tt.abs() > f64::EPSILON {
                                dtd.push((sym.clone(), da - 0.5, tt));
                            }
                        }
                        cs_events_all.extend(cg::residuals_2d(
                            "us",
                            "depth_trade_decoupling",
                            &dtd,
                            cg::OUTLIER_PERCENTILE,
                            now_utc,
                        ));

                        // Pressure vs Intent coherence
                        let mut pi: Vec<(String, f64, f64)> = Vec::new();
                        for (sym, kg) in &subkg_registry.graphs {
                            let pc = kg
                                .nodes
                                .get(&sk::NodeId::PressureCapitalFlow)
                                .and_then(|n| n.value)
                                .map(|v| v.to_f64().unwrap_or(0.0))
                                .unwrap_or(0.0);
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
                            if pc.abs() > f64::EPSILON {
                                pi.push((sym.clone(), pc, ia - id));
                            }
                        }
                        cs_events_all.extend(cg::residuals_2d(
                            "us",
                            "pressure_intent_coherence",
                            &pi,
                            cg::OUTLIER_PERCENTILE,
                            now_utc,
                        ));

                        // TradeTape acceleration
                        let mut tta: Vec<(String, f64)> = Vec::new();
                        for (sym, _) in &subkg_registry.graphs {
                            if let Some(a) = kinematics_tracker
                                .acceleration(sym, &sk::NodeId::TradeTapeBuyMinusSell30s)
                            {
                                if a.abs() > f64::EPSILON {
                                    tta.push((sym.clone(), a));
                                }
                            }
                        }
                        cs_events_all.extend(cg::outliers_1d(
                            "us",
                            "trade_tape_acceleration",
                            &tta,
                            cg::OUTLIER_PERCENTILE,
                            now_utc,
                        ));

                        // VWAP deviation velocity
                        let mut vv: Vec<(String, f64)> = Vec::new();
                        for (sym, _) in &subkg_registry.graphs {
                            if let Some(v) =
                                kinematics_tracker.velocity(sym, &sk::NodeId::VwapDeviationPct)
                            {
                                if v.abs() > f64::EPSILON {
                                    vv.push((sym.clone(), v));
                                }
                            }
                        }
                        cs_events_all.extend(cg::outliers_1d(
                            "us",
                            "vwap_deviation_velocity",
                            &vv,
                            cg::OUTLIER_PERCENTILE,
                            now_utc,
                        ));

                        // Symbol-Sector orphan
                        let mut sec_counts: HashMap<String, HashMap<String, u32>> = HashMap::new();
                        for (sid, members) in &sector_members {
                            let c = sec_counts.entry(sid.0.clone()).or_default();
                            for m in members {
                                if let Some(kg) = subkg_registry.get(&m.0) {
                                    if let Some(n) = kg.nodes.get(&sk::NodeId::StateClassification)
                                    {
                                        if let Some(l) = n.label.as_ref() {
                                            *c.entry(l.clone()).or_insert(0) += 1;
                                        }
                                    }
                                }
                            }
                        }
                        let mut orphans: Vec<(String, f64)> = Vec::new();
                        for (sid, members) in &sector_members {
                            let c = match sec_counts.get(&sid.0) {
                                Some(x) if !x.is_empty() => x,
                                _ => continue,
                            };
                            let dom = c.iter().max_by_key(|(_, n)| *n).map(|(l, _)| l.clone());
                            if let Some(d) = dom {
                                for m in members {
                                    if let Some(kg) = subkg_registry.get(&m.0) {
                                        if let Some(n) =
                                            kg.nodes.get(&sk::NodeId::StateClassification)
                                        {
                                            let l = n.label.clone().unwrap_or_default();
                                            if !l.is_empty() && l != d {
                                                orphans.push((m.0.clone(), 1.0));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                        cs_events_all.extend(cg::outliers_1d(
                            "us",
                            "symbol_sector_state_orphan",
                            &orphans,
                            cg::OUTLIER_PERCENTILE,
                            now_utc,
                        ));

                        // Consistency aggregate count removed;
                        // 10-relationship events in eden-consistency-us.ndjson.
                        let _ = cg::write_events("us", &cs_events_all);
                    }

                    // Persistence tracker
                    {
                        use crate::pipeline::structural_persistence as sp;
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
                            "us",
                            "structure_salience",
                            &salience,
                            &mut persistence_tracker,
                            now_utc,
                        );
                        // Persistence aggregate count + top streak wake
                        // removed; full streaks in eden-persistence-us.ndjson.
                        let _ = sp::write_events("us", &p_salience);
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
                            "us",
                            "intent_imbalance",
                            &imbalance,
                            &mut persistence_tracker,
                            now_utc,
                        );
                        let _ = sp::write_events("us", &p_imb);
                    }
                    // Expectation / Surprise (Layer 5)
                    {
                        let surprise_events =
                            crate::pipeline::structural_expectation::update_and_measure(
                                "us",
                                &subkg_registry,
                                &mut expectation_tracker,
                                now_utc,
                            );
                        // Surprise aggregate + top wake removed;
                        // full surprise events in eden-surprise-us.ndjson.
                        let _ = crate::pipeline::structural_expectation::write_events(
                            "us",
                            &surprise_events,
                        );
                    }
                }
            }

            let post_bp_projection_refresh = us_setup_surface_dirty;
            if post_bp_projection_refresh {
                let preserved_wake_reasons =
                    artifact_projection.agent_snapshot.wake.reasons.clone();
                let mut live_snapshot = build_us_live_snapshot(
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
                    &live,
                    &position_tracker,
                    &workflows,
                    &propagation_senses,
                    &sorted_events,
                    &previous_symbol_states_for_surface,
                    &previous_cluster_states_for_surface,
                    previous_world_summary_for_surface.as_ref(),
                );
                augment_us_live_snapshot_with_raw_expectations(
                    &mut live_snapshot,
                    &raw_trade_tape,
                );
                previous_symbol_states = live_snapshot.symbol_states.clone();
                previous_cluster_states = live_snapshot.cluster_states.clone();
                previous_world_summary = live_snapshot.world_summary.clone();
                artifact_projection = project_us(UsProjectionInputs {
                    live_snapshot,
                    history: &tick_history,
                    reasoning: &reasoning,
                    backward: &backward,
                    store: &store,
                    lineage_stats: &lineage_stats,
                    previous_agent_snapshot: runtime
                        .projection_state
                        .previous_agent_snapshot
                        .as_ref(),
                    previous_agent_session: runtime.projection_state.previous_agent_session.as_ref(),
                    previous_agent_scoreboard: runtime
                        .projection_state
                        .previous_agent_scoreboard
                        .as_ref(),
                });
                for reason in preserved_wake_reasons {
                    if !artifact_projection
                        .agent_snapshot
                        .wake
                        .reasons
                        .iter()
                        .any(|existing| existing == &reason)
                    {
                        artifact_projection.agent_snapshot.wake.reasons.push(reason);
                    }
                }
            }

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

            // 60s rescan — picks up new decisions written during session.
            let should_rescan_decisions = match decision_ledger.last_scan_ts() {
                None => true,
                Some(prev) => (chrono::Utc::now() - prev).num_seconds() >= 60,
            };
            if should_rescan_decisions {
                use std::path::Path;
                crate::pipeline::decision_ledger::scanner::rescan_recent(
                    Path::new("decisions"),
                    &mut decision_ledger,
                    chrono::Utc::now(),
                );
            }

            // Horizon live-settle sweep (symmetric with HK; audit Finding 1,
            // 2026-04-19). Pending → Due when due_at passed.
            #[cfg(feature = "persistence")]
            if should_rescan_decisions {
                if let Some(ref store) = runtime.store {
                    let now_offset = time::OffsetDateTime::now_utc();
                    crate::core::runtime::sweep_pending_horizons_to_due(store, now_offset).await;
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
                        let snap = crate::persistence::belief_snapshot::serialize_field(
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
                            crate::persistence::intent_belief_snapshot::serialize_field(
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
                    }
                }
            }
        }
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
            &mut eden_ledger,
            &mut intent_belief_field,
            &mut outcome_credited_setup_ids,
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
                    .filter(|hypothesis| matches!(
                        hypothesis.kind,
                        Some(crate::ontology::reasoning::HypothesisKind::LatentVortex)
                    ))
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

    #[cfg(feature = "persistence")]
    {
        let record =
            crate::persistence::edge_learning_ledger::EdgeLearningLedgerRecord::from_ledger(
                "us",
                &edge_ledger,
                time::OffsetDateTime::now_utc(),
            );
        runtime
            .persist_edge_learning_ledger("us", record, i128::from(tick))
            .await;
    }

    Ok(())
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
