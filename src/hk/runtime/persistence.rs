#[cfg(feature = "persistence")]
use super::*;

#[cfg(feature = "persistence")]
// Replaced by adaptive multi-horizon evaluation (15/50/150 ticks)
#[cfg(feature = "persistence")]
pub(super) const PERSISTENCE_MAX_IN_FLIGHT: usize = 16;

#[cfg(feature = "persistence")]
pub(super) enum HkPersistenceItem {
    Workflow(ActionWorkflowRecord),
    WorkflowEvent(ActionWorkflowEventRecord),
    TacticalSetup(TacticalSetupRecord),
    HypothesisTrack(HypothesisTrackRecord),
}

#[cfg(feature = "persistence")]
pub(super) async fn persist_hk_item(
    store_ref: EdenStore,
    item: HkPersistenceItem,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    match item {
        HkPersistenceItem::Workflow(record) => store_ref.write_action_workflow(&record).await,
        HkPersistenceItem::WorkflowEvent(event) => {
            store_ref.write_action_workflow_event(&event).await
        }
        HkPersistenceItem::TacticalSetup(record) => store_ref.write_tactical_setup(&record).await,
        HkPersistenceItem::HypothesisTrack(record) => {
            store_ref.write_hypothesis_track(&record).await
        }
    }
}

#[cfg(feature = "persistence")]
pub(super) async fn persist_hk_items(
    runtime: &PreparedRuntimeContext,
    label: &'static str,
    issue_code: &'static str,
    error_prefix: &'static str,
    items: Vec<HkPersistenceItem>,
) {
    runtime
        .schedule_store_batch_operations(label, issue_code, error_prefix, items, persist_hk_item)
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
            order_books: Vec::new(),
            candlesticks: Vec::new(),
            trades: crate::ontology::microstructure::archive_trades_pub(raw),
            capital_flows: Vec::new(),
            capital_distributions: Vec::new(),
            quotes: crate::ontology::microstructure::archive_quotes_pub(raw),
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
        let workflow_items = workflow_records
            .iter()
            .cloned()
            .map(HkPersistenceItem::Workflow)
            .chain(
                workflow_events
                    .iter()
                    .cloned()
                    .map(HkPersistenceItem::WorkflowEvent),
            )
            .collect::<Vec<_>>();
        persist_hk_items(
            runtime,
            "write action workflows",
            "write_hk_action_workflows_failed",
            "failed to write action workflows",
            workflow_items,
        )
        .await;
    }

    if !reasoning_snapshot.tactical_setups.is_empty() {
        let tactical_setup_records = reasoning_snapshot
            .tactical_setups
            .iter()
            .map(|setup| TacticalSetupRecord::from_setup(setup, now))
            .map(HkPersistenceItem::TacticalSetup)
            .collect::<Vec<_>>();
        persist_hk_items(
            runtime,
            "write tactical setups",
            "write_hk_tactical_setups_failed",
            "failed to write tactical setups",
            tactical_setup_records,
        )
        .await;
    }

    if !reasoning_snapshot.hypothesis_tracks.is_empty() {
        let hypothesis_track_records = reasoning_snapshot
            .hypothesis_tracks
            .iter()
            .map(HypothesisTrackRecord::from_track)
            .map(HkPersistenceItem::HypothesisTrack)
            .collect::<Vec<_>>();
        persist_hk_items(
            runtime,
            "write hypothesis tracks",
            "write_hk_hypothesis_tracks_failed",
            "failed to write hypothesis tracks",
            hypothesis_track_records,
        )
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

    runtime
        .publish_projection_with_followups_from_inputs(
            MarketId::Hk,
            crate::cases::CaseMarket::Hk,
            artifact_projection,
            vec![(
                bridge_snapshot_path.to_string(),
                json_payload(hk_bridge_snapshot),
            )],
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

    runtime
        .persist_hk_lineage_stats(tick, now, LINEAGE_WINDOW, lineage_stats)
        .await;
}
