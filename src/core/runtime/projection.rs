use std::sync::Arc;
use std::time::Instant;

use tokio::sync::Semaphore;

use crate::agent::{build_recommendation_journal_record, update_recommendation_journal};
use crate::cases::CaseMarket;
use crate::core::analyst_service::AnalystService;
use crate::core::market::MarketId;
use crate::core::projection::ProjectionBundle;
use crate::live_snapshot::{
    json_payload, spawn_mutate_text_file, spawn_write_json_snapshots_batch,
};
use crate::ontology::build_operational_snapshot;

use super::telemetry::log_runtime_tick_summary;
use super::{AgentArtifactPaths, ProjectionStateCache, RuntimeCounters, RuntimeInfraConfig};

pub fn write_projection_artifacts(
    market: MarketId,
    paths: &AgentArtifactPaths,
    projection: &ProjectionBundle,
    extra_artifacts: Vec<(String, String)>,
) {
    let operational_snapshot = build_operational_snapshot(
        &projection.live_snapshot,
        &projection.agent_snapshot,
        &projection.agent_session,
        &projection.agent_recommendations,
        Some(&projection.agent_narration),
    )
    .ok();

    let mut artifacts = vec![
        (
            paths.live_snapshot_path.clone(),
            json_payload(&projection.live_snapshot),
        ),
        (
            paths.agent_snapshot_path.clone(),
            json_payload(&projection.agent_snapshot),
        ),
    ];
    if let Some(snapshot) = operational_snapshot {
        artifacts.push((
            paths.operational_snapshot_path.clone(),
            json_payload(&snapshot),
        ));
    }
    artifacts.extend(vec![
        (
            paths.agent_briefing_path.clone(),
            json_payload(&projection.agent_briefing),
        ),
        (
            paths.agent_session_path.clone(),
            json_payload(&projection.agent_session),
        ),
        (
            paths.agent_watchlist_path.clone(),
            json_payload(&projection.agent_watchlist),
        ),
        (
            paths.agent_recommendations_path.clone(),
            json_payload(&projection.agent_recommendations),
        ),
        (
            paths.agent_scoreboard_path.clone(),
            json_payload(&projection.agent_scoreboard),
        ),
        (
            paths.agent_eod_review_path.clone(),
            json_payload(&projection.agent_eod_review),
        ),
        (
            paths.agent_runtime_narration_path.clone(),
            json_payload(&projection.agent_narration),
        ),
    ]);
    artifacts.extend(extra_artifacts);

    let market_slug = market.slug();
    spawn_write_json_snapshots_batch(
        format!("agent:{market_slug}").into(),
        projection.agent_snapshot.tick,
        artifacts,
    );

    let recommendation_journal = build_recommendation_journal_record(
        &projection.agent_snapshot,
        &projection.agent_recommendations,
    );
    let journal_snapshot = projection.agent_snapshot.clone();
    spawn_mutate_text_file(
        format!("agent:{market_slug}:recommendation_journal").into(),
        paths.agent_recommendation_journal_path.clone(),
        move |existing| {
            update_recommendation_journal(&existing, &journal_snapshot, &recommendation_journal)
        },
    );
}

pub fn trigger_projection_analysis<S: AnalystService>(
    analyst_service: &S,
    market: CaseMarket,
    projection: &ProjectionBundle,
    analyst_limit: &Arc<Semaphore>,
) {
    analyst_service.trigger_runtime_analysis(
        market,
        projection.agent_snapshot.clone(),
        projection.agent_briefing.clone(),
        projection.agent_session.clone(),
        analyst_limit,
    );
}

pub fn advance_projection_state(
    projection_state: &mut ProjectionStateCache,
    projection: &ProjectionBundle,
) {
    projection_state.previous_agent_snapshot = Some(projection.agent_snapshot.clone());
    projection_state.previous_agent_session = Some(projection.agent_session.clone());
    projection_state.previous_agent_scoreboard = Some(projection.agent_scoreboard.clone());
}

pub fn finalize_runtime_projection<S: AnalystService>(
    analyst_service: &S,
    market: CaseMarket,
    projection: &ProjectionBundle,
    projection_state: &mut ProjectionStateCache,
    runtime_config: &RuntimeInfraConfig,
    runtime_counters: &RuntimeCounters,
    tick: u64,
    push_count: u64,
    tick_started_at: Instant,
    received_push: bool,
    received_update: bool,
) {
    trigger_projection_analysis(
        analyst_service,
        market,
        projection,
        &projection_state.analyst_limit,
    );
    advance_projection_state(projection_state, projection);
    log_runtime_tick_summary(
        runtime_config,
        tick,
        push_count,
        runtime_counters,
        tick_started_at,
        received_push,
        received_update,
    );
}
