#[cfg(feature = "persistence")]
use super::*;

#[cfg(feature = "persistence")]
pub(crate) async fn maybe_refresh_us_learning_feedback(
    store: &EdenStore,
    object_store: &Arc<ObjectStore>,
    tick: u64,
    refresh_interval: u64,
    cached_feedback: &mut Option<ReasoningLearningFeedback>,
) {
    if cached_feedback.is_some() && tick % refresh_interval != 0 {
        return;
    }

    let Ok(assessments) = store.recent_case_reasoning_assessments_by_market("us", 240).await else {
        return;
    };
    if assessments.is_empty() {
        return;
    }

    let rows = store
        .recent_ranked_us_lineage_metric_rows(12, 5)
        .await
        .unwrap_or_default();
    let outcome_ctx = derive_outcome_learning_context_from_us_rows(&rows);
    let feedback = derive_learning_feedback(&assessments, &outcome_ctx);
    object_store
        .knowledge
        .write()
        .unwrap()
        .apply_calibration(&feedback);
    *cached_feedback = Some(feedback);
}

#[cfg(feature = "persistence")]
pub(crate) async fn run_us_projection_stage<S: AnalystService>(
    runtime: &mut PreparedRuntimeContext,
    analyst_service: &S,
    tick: u64,
    now: time::OffsetDateTime,
    tick_started_at: std::time::Instant,
    received_push: bool,
    received_update: bool,
    live_push_count: u64,
    cached_feedback: &mut Option<ReasoningLearningFeedback>,
    reasoning: &UsReasoningSnapshot,
    artifact_projection: &crate::core::projection::ProjectionBundle,
    object_store: &Arc<ObjectStore>,
) {
    if let Some(ref store) = runtime.store {
        maybe_refresh_us_learning_feedback(
            store,
            object_store,
            tick,
            US_LEARNING_FEEDBACK_REFRESH_INTERVAL,
            cached_feedback,
        )
        .await;
    }

    let live_snapshot = &artifact_projection.live_snapshot;
    let agent_snapshot = &artifact_projection.agent_snapshot;
    let agent_recommendations = &artifact_projection.agent_recommendations;
    let case_list_for_graph =
        build_case_list_with_feedback(live_snapshot, cached_feedback.as_ref());

    runtime
        .publish_projection_with_followups_from_inputs(
            MarketId::Us,
            crate::cases::CaseMarket::Us,
            artifact_projection,
            Vec::new(),
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
            &reasoning.hypotheses,
            &reasoning.tactical_setups,
            agent_snapshot.world_state.as_ref(),
            agent_snapshot.backward_reasoning.as_ref(),
            &live_snapshot.active_position_nodes,
            None,
        )
        .await;
}

#[cfg(feature = "persistence")]
pub(crate) async fn run_us_persistence_stage(
    runtime: &PreparedRuntimeContext,
    tick: u64,
    now: time::OffsetDateTime,
    live: &UsLiveState,
    rest: &UsRestSnapshot,
    tick_record: &UsTickRecord,
) {
    runtime.persist_us_tick(tick_record.clone()).await;

    let raw_snapshot = RawSnapshot {
        timestamp: now,
        brokers: HashMap::new(),
        calc_indexes: rest.calc_indexes.clone(),
        candlesticks: live.candlesticks.clone(),
        capital_flows: rest.capital_flows.clone(),
        capital_distributions: HashMap::new(),
        depths: HashMap::new(),
        market_temperature: None,
        quotes: live.quotes.clone(),
        trades: HashMap::new(),
    };
    let archive = crate::ontology::microstructure::TickArchive::from_raw(tick, &raw_snapshot);
    runtime.persist_market_tick_archive(archive).await;
}

#[cfg(feature = "persistence")]
pub(crate) async fn maybe_persist_us_lineage_stage(
    runtime: &PreparedRuntimeContext,
    tick: u64,
    now: time::OffsetDateTime,
    tick_history_len: usize,
    lineage_stats: &UsLineageStats,
) {
    if tick % 30 != 0 || tick_history_len <= 1 {
        return;
    }
    runtime
        .persist_us_lineage_stats(
            tick,
            now,
            tick_history_len,
            SIGNAL_RESOLUTION_LAG,
            lineage_stats,
        )
        .await;
}
