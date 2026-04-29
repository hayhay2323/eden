#[cfg(feature = "persistence")]
use super::*;

#[cfg(feature = "persistence")]
// Replaced by adaptive multi-horizon evaluation (15/50/150 ticks)
#[cfg(feature = "persistence")]
pub(super) const PERSISTENCE_MAX_IN_FLIGHT: usize = 16;

#[cfg(feature = "persistence")]
pub(super) async fn persist_hk_workflow_event(
    store_ref: EdenStore,
    event: ActionWorkflowEventRecord,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    store_ref.write_action_workflow_event(&event).await
}

#[cfg(feature = "persistence")]
pub(super) async fn persist_hk_workflow_events(
    runtime: &PreparedRuntimeContext,
    label: &'static str,
    issue_code: &'static str,
    error_prefix: &'static str,
    events: Vec<ActionWorkflowEventRecord>,
) {
    runtime
        .schedule_store_batch_operations(
            label,
            issue_code,
            error_prefix,
            events,
            persist_hk_workflow_event,
        )
        .await;
}

#[cfg(feature = "persistence")]
pub(super) async fn run_hk_persistence_stage(
    runtime: &PreparedRuntimeContext,
    tick: u64,
    now: time::OffsetDateTime,
    raw: &RawSnapshot,
    links: &LinkSnapshot,
    tick_record: &TickRecord,
    workflow_records: &[ActionWorkflowRecord],
    workflow_events: &[ActionWorkflowEventRecord],
    reasoning_snapshot: &ReasoningSnapshot,
) {
    runtime.persist_hk_tick(tick_record.clone()).await;

    let is_full_snapshot = tick % 30 == 0 || tick <= 1;
    let archive = if is_full_snapshot {
        crate::ontology::microstructure::TickArchive::from_raw(tick, raw)
    } else {
        crate::ontology::microstructure::TickArchive {
            tick_number: tick,
            timestamp: raw.timestamp,
            calc_indexes: Vec::new(),
            order_books: Vec::new(),
            candlesticks: Vec::new(),
            trades: crate::ontology::microstructure::archive_trades_pub(raw),
            capital_flows: Vec::new(),
            capital_distributions: Vec::new(),
            quotes: crate::ontology::microstructure::archive_quotes_pub(raw),
            intraday: Vec::new(),
            option_surfaces: Vec::new(),
            market_temperature: None,
            broker_queues: crate::ontology::microstructure::archive_broker_queues_pub(raw),
        }
    };
    runtime.persist_market_tick_archive(archive).await;

    if tick % 30 == 0 {
        runtime
            .persist_hk_institution_states(links.cross_stock_presences.clone(), now)
            .await;
    }

    if !workflow_records.is_empty() || !workflow_events.is_empty() {
        if !workflow_records.is_empty() {
            runtime
                .persist_action_workflows(crate::cases::CaseMarket::Hk, workflow_records.to_vec())
                .await;
        }
        if !workflow_events.is_empty() {
            persist_hk_workflow_events(
                runtime,
                "write action workflow events",
                "write_hk_action_workflow_events_failed",
                "failed to write action workflow events",
                workflow_events.to_vec(),
            )
            .await;
        }
    }

    if !reasoning_snapshot.tactical_setups.is_empty() {
        let hypothesis_by_id = reasoning_snapshot
            .hypotheses
            .iter()
            .map(|hypothesis| (hypothesis.hypothesis_id.as_str(), hypothesis))
            .collect::<std::collections::HashMap<_, _>>();
        let tactical_setup_records = reasoning_snapshot
            .tactical_setups
            .iter()
            .map(|setup| {
                let hypothesis = hypothesis_by_id.get(setup.hypothesis_id.as_str()).copied();
                TacticalSetupRecord::from_setup_with_hypothesis(setup, hypothesis, now)
            })
            .collect::<Vec<_>>();
        runtime
            .persist_tactical_setups(crate::cases::CaseMarket::Hk, tactical_setup_records)
            .await;
    }

    if !reasoning_snapshot.hypothesis_tracks.is_empty() {
        let hypothesis_track_records = reasoning_snapshot
            .hypothesis_tracks
            .iter()
            .map(HypothesisTrackRecord::from_track)
            .collect::<Vec<_>>();
        runtime
            .persist_hypothesis_tracks(crate::cases::CaseMarket::Hk, hypothesis_track_records)
            .await;
    }
}

#[cfg(feature = "persistence")]
#[allow(clippy::too_many_arguments)]
pub(super) async fn run_hk_projection_stage<S: AnalystService>(
    runtime: &mut PreparedRuntimeContext,
    analyst_service: &S,
    tick: u64,
    now: time::OffsetDateTime,
    tick_started_at: std::time::Instant,
    received_push: bool,
    received_update: bool,
    live_push_count: u64,
    history: &TickHistory,
    reasoning_snapshot: &crate::pipeline::reasoning::ReasoningSnapshot,
    world_snapshots: &crate::pipeline::world::WorldSnapshots,
    lineage_stats: &crate::temporal::lineage::LineageStats,
    bridge_snapshot_path: &str,
    hk_bridge_snapshot: &HkSnapshot,
    artifact_projection: &crate::core::projection::ProjectionBundle,
    eden_ledger: &mut crate::persistence::case_realized_outcome::EdenLedgerAccumulator,
    intent_belief_field: &mut crate::pipeline::intent_belief::IntentBeliefField,
    outcome_credited_setup_ids: &mut std::collections::HashSet<String>,
    broker_archetype_field: &mut crate::pipeline::broker_archetype::BrokerArchetypeBeliefField,
    broker_entry_snapshots: &mut std::collections::HashMap<
        String,
        crate::pipeline::broker_outcome_feedback::BrokerEntrySnapshot,
    >,
    broker_credited_setup_ids: &mut std::collections::HashSet<String>,
) {
    let live_snapshot = &artifact_projection.live_snapshot;
    let agent_snapshot = &artifact_projection.agent_snapshot;
    let agent_recommendations = &artifact_projection.agent_recommendations;
    let case_list_for_graph = build_case_list_with_feedback(live_snapshot, None);
    let case_primary_lens = case_list_for_graph
        .cases
        .iter()
        .map(|case| (case.setup_id.clone(), case.primary_lens.clone()))
        .collect::<std::collections::HashMap<_, _>>();
    let realized_outcomes = compute_case_realized_outcomes_adaptive(history, LINEAGE_WINDOW)
        .into_iter()
        .map(|outcome| {
            let primary_lens = case_primary_lens.get(&outcome.setup_id).cloned().flatten();
            CaseRealizedOutcomeRecord::from_outcome(&outcome, "hk", primary_lens)
        })
        .collect::<Vec<_>>();
    eden_ledger.record_batch(&realized_outcomes);

    // Backward pass: each resolved outcome credits/debits the focal
    // symbol's IntentBelief with one confirmation sample. Closes the
    // KG forward loop (pressure → belief → modulation → decision)
    // back to the belief layer that shaped the decision.
    if !realized_outcomes.is_empty() {
        let summary = crate::pipeline::outcome_feedback::apply_outcome_batch(
            &realized_outcomes,
            intent_belief_field,
            outcome_credited_setup_ids,
        );
        if summary.applied() > 0 {
            eprintln!("{}", summary.summary_line("hk"));
        }

        // Broker backward pass: winning setups credit right-side
        // brokers with +1 archetype sample. HK only — US has no
        // broker queue.
        let broker_summary = crate::pipeline::broker_outcome_feedback::apply_broker_outcome_batch(
            &realized_outcomes,
            broker_entry_snapshots,
            broker_archetype_field,
            broker_credited_setup_ids,
        );
        if broker_summary.applied() > 0 {
            eprintln!("{}", broker_summary.summary_line("hk"));
        }
        // GC credited snapshots to bound memory growth.
        crate::pipeline::broker_outcome_feedback::gc_credited_snapshots(
            broker_entry_snapshots,
            broker_credited_setup_ids,
        );
    }

    runtime
        .publish_projection_with_followups_from_inputs(
            MarketId::Hk,
            crate::cases::CaseMarket::Hk,
            artifact_projection,
            match json_payload(hk_bridge_snapshot) {
                Ok(payload) => vec![(bridge_snapshot_path.to_string(), payload)],
                Err(error) => {
                    eprintln!(
                        "Warning: failed to serialize HK bridge snapshot for tick {}: {}",
                        tick, error
                    );
                    vec![]
                }
            },
            analyst_service,
            tick,
            live_push_count,
            tick_started_at,
            received_push,
            received_update,
            &case_list_for_graph.cases,
            now,
            "runtime",
            &agent_snapshot.knowledge_links,
            &agent_recommendations.knowledge_links,
            &agent_snapshot.macro_events,
            &agent_recommendations.decisions,
            &reasoning_snapshot.hypotheses,
            &reasoning_snapshot.tactical_setups,
            Some(&world_snapshots.world_state),
            Some(&world_snapshots.backward_reasoning),
            &live_snapshot.active_position_nodes,
            (!realized_outcomes.is_empty()).then_some(realized_outcomes),
        )
        .await;

    if let Some(ref store) = runtime.store {
        crate::core::runtime::settle_live_horizons_hk(
            store,
            history,
            &reasoning_snapshot.tactical_setups,
            &reasoning_snapshot.hypotheses,
            now,
        )
        .await;
    }

    let symbol_state_records = live_snapshot
        .symbol_states
        .iter()
        .map(|state| {
            crate::persistence::symbol_perception_state::SymbolPerceptionStateRecord::from_state(
                live_snapshot.market,
                now,
                state,
            )
        })
        .collect::<Vec<_>>();
    if !symbol_state_records.is_empty() {
        runtime
            .persist_symbol_perception_states(crate::cases::CaseMarket::Hk, symbol_state_records)
            .await;
    }

    runtime
        .persist_hk_lineage_stats(tick, now, LINEAGE_WINDOW, lineage_stats)
        .await;
}

#[cfg(feature = "persistence")]
pub(super) const HK_LEARNING_FEEDBACK_REFRESH_INTERVAL: u64 = 30;

/// HK mirror of `maybe_refresh_us_learning_feedback`. Pulls recent HK case
/// reasoning assessments + ranked lineage rows, derives outcome learning
/// context with `derive_outcome_learning_context_from_hk_rows`, and
/// applies the resulting feedback to the shared knowledge store. Without
/// this loop, HK setups never receive conditioned/intent/archetype
/// learning adjustments — the entire learning pipeline lived US-only.
#[cfg(feature = "persistence")]
pub(super) async fn maybe_refresh_hk_learning_feedback(
    store: &EdenStore,
    object_store: &Arc<crate::ontology::store::ObjectStore>,
    tick: u64,
    refresh_interval: u64,
    cached_feedback: &mut Option<crate::pipeline::learning_loop::ReasoningLearningFeedback>,
) {
    if cached_feedback.is_some() && tick % refresh_interval != 0 {
        return;
    }

    let Ok(assessments) = store
        .recent_case_reasoning_assessments_by_market("hk", 240)
        .await
    else {
        return;
    };
    if assessments.is_empty() {
        return;
    }

    let rows = store
        .recent_ranked_lineage_metric_rows(12, 5)
        .await
        .unwrap_or_default();
    let outcome_ctx =
        crate::pipeline::learning_loop::derive_outcome_learning_context_from_hk_rows(&rows);
    let feedback =
        crate::pipeline::learning_loop::derive_learning_feedback(&assessments, &outcome_ctx);
    object_store
        .knowledge
        .write()
        .unwrap()
        .apply_calibration(&feedback);
    *cached_feedback = Some(feedback);
}
