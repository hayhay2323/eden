use axum::extract::{Path, Query};
use axum::Json;
use serde::{Deserialize, Serialize};

use crate::agent::{
    self, AgentAlertScoreboard, AgentBriefing, AgentBrokerState, AgentDepthState, AgentEodReview,
    AgentInvalidationState, AgentRecommendations, AgentSectorFlow, AgentSession, AgentSnapshot,
    AgentStructureState, AgentSymbolState, AgentThread, AgentToolOutput, AgentToolRequest,
    AgentToolSpec, AgentTurn, AgentWakeState, AgentWatchlist,
};
use crate::agent::codex::CodexCliAnalyzeBody;
use crate::agent::llm::{AgentAnalysis, AgentAnalystReview, AgentAnalystScoreboard, AgentNarration};
use crate::live_snapshot::spawn_write_json_snapshot;
use crate::ontology::world::{BackwardInvestigation, WorldStateSnapshot};

use super::agent_surface::{
    load_agent_analyst_review_for_market, load_agent_analyst_scoreboard_for_market,
    load_agent_briefing_for_market, load_agent_eod_review_for_market,
    load_agent_narration_for_market, load_agent_recommendations_for_market,
    load_agent_scoreboard_for_market, load_agent_session_for_market,
    load_agent_watchlist_for_market,
};
use super::constants::{DEFAULT_LIMIT, MAX_LIMIT};
use super::core::{bounded, normalized_query_value, parse_case_market};
use super::feed_api::{
    build_feed_notices_response, build_feed_transitions_response,
    FeedNoticesResponse as AgentNoticesResponse,
    FeedTransitionsResponse as AgentTransitionsResponse,
};
use super::foundation::ApiError;
use super::ontology_api::load_or_build_operational_snapshot;
use super::ontology_query_api::load_world_state_for_market;

#[derive(Debug, Serialize)]
pub(super) struct AgentStructuresResponse {
    tick: u64,
    total: usize,
    structures: Vec<AgentStructureState>,
}

#[derive(Debug, Serialize)]
pub(super) struct AgentSectorFlowsResponse {
    tick: u64,
    total: usize,
    flows: Vec<AgentSectorFlow>,
}

#[derive(Debug, Serialize)]
pub(super) struct AgentThreadsResponse {
    pub(super) tick: u64,
    pub(super) total: usize,
    pub(super) threads: Vec<AgentThread>,
}

#[derive(Debug, Serialize)]
pub(super) struct AgentTurnsResponse {
    pub(super) tick: u64,
    pub(super) total: usize,
    pub(super) turns: Vec<AgentTurn>,
}

#[derive(Debug, Serialize)]
pub(super) struct AgentAnalyzeResponse {
    analysis: AgentAnalysis,
    narration: AgentNarration,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub(super) struct AgentFeedQuery {
    pub(super) since_tick: Option<u64>,
    pub(super) limit: Option<usize>,
    pub(super) symbol: Option<String>,
    pub(super) sector: Option<String>,
}

#[derive(Debug, Deserialize, Default, Clone)]
pub(super) struct AgentAnalyzeBody {
    #[serde(default)]
    deterministic_only: bool,
}

pub(super) async fn get_agent_snapshot(
    Path(market): Path<String>,
) -> Result<Json<AgentSnapshot>, ApiError> {
    let market = parse_case_market(&market)?;
    let snapshot = agent::load_snapshot(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load agent snapshot: {error}")))?;
    Ok(Json(snapshot))
}

pub(super) async fn get_agent_tools(
    Path(market): Path<String>,
) -> Result<Json<Vec<AgentToolSpec>>, ApiError> {
    let _ = parse_case_market(&market)?;
    Ok(Json(agent::tool_catalog()))
}

pub(super) async fn get_agent_wake(
    Path(market): Path<String>,
) -> Result<Json<AgentWakeState>, ApiError> {
    let snapshot = load_agent_snapshot_for_market(&market).await?;
    Ok(Json(snapshot.wake))
}

pub(super) async fn get_agent_briefing(
    Path(market): Path<String>,
) -> Result<Json<AgentBriefing>, ApiError> {
    Ok(Json(load_agent_briefing_for_market(&market).await?))
}

pub(super) async fn get_agent_analysis(
    Path(market): Path<String>,
) -> Result<Json<AgentAnalysis>, ApiError> {
    let market = parse_case_market(&market)?;
    let analysis = crate::agent::llm::load_analysis(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load agent analysis: {error}")))?;
    Ok(Json(analysis))
}

pub(super) async fn get_agent_narration(
    Path(market): Path<String>,
) -> Result<Json<AgentNarration>, ApiError> {
    Ok(Json(load_agent_narration_for_market(&market).await?))
}

pub(super) async fn get_agent_analyst_review(
    Path(market): Path<String>,
) -> Result<Json<AgentAnalystReview>, ApiError> {
    Ok(Json(load_agent_analyst_review_for_market(&market).await?))
}

pub(super) async fn get_agent_analyst_scoreboard(
    Path(market): Path<String>,
) -> Result<Json<AgentAnalystScoreboard>, ApiError> {
    Ok(Json(
        load_agent_analyst_scoreboard_for_market(&market).await?,
    ))
}

pub(super) async fn get_agent_session(
    Path(market): Path<String>,
) -> Result<Json<AgentSession>, ApiError> {
    Ok(Json(load_agent_session_for_market(&market).await?))
}

pub(super) async fn get_agent_watchlist(
    Path(market): Path<String>,
) -> Result<Json<AgentWatchlist>, ApiError> {
    Ok(Json(load_agent_watchlist_for_market(&market).await?))
}

pub(super) async fn get_agent_recommendations(
    Path(market): Path<String>,
) -> Result<Json<AgentRecommendations>, ApiError> {
    Ok(Json(load_agent_recommendations_for_market(&market).await?))
}

pub(super) async fn get_agent_scoreboard(
    Path(market): Path<String>,
) -> Result<Json<AgentAlertScoreboard>, ApiError> {
    Ok(Json(load_agent_scoreboard_for_market(&market).await?))
}

pub(super) async fn get_agent_eod_review(
    Path(market): Path<String>,
) -> Result<Json<AgentEodReview>, ApiError> {
    Ok(Json(load_agent_eod_review_for_market(&market).await?))
}

pub(super) async fn get_agent_query(
    Path(market): Path<String>,
    Query(query): Query<AgentToolRequest>,
) -> Result<Json<AgentToolOutput>, ApiError> {
    let _ = parse_case_market(&market)?;
    let snapshot = load_agent_snapshot_for_market(&market).await?;
    let session = load_agent_session_for_market(&market).await.ok();
    let result =
        agent::execute_tool(&snapshot, session.as_ref(), &query).map_err(ApiError::bad_request)?;
    Ok(Json(result))
}

pub(super) async fn post_agent_analyze(
    Path(market): Path<String>,
    Json(body): Json<AgentAnalyzeBody>,
) -> Result<Json<AgentAnalyzeResponse>, ApiError> {
    let market = parse_case_market(&market)?;
    let snapshot = agent::load_snapshot(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load agent snapshot: {error}")))?;
    let briefing = agent::load_briefing(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load agent briefing: {error}")))?;
    let session = agent::load_session(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load agent session: {error}")))?;

    let analysis = if body.deterministic_only {
        crate::agent::llm::deterministic_analysis(&snapshot, &briefing)
    } else {
        crate::agent::llm::run_or_fallback_analysis(
            snapshot.clone(),
            briefing.clone(),
            session.clone(),
        )
        .await
    };
    let recommendations = agent::build_recommendations(&snapshot, Some(&session));
    let watchlist = agent::build_watchlist(&snapshot, Some(&session), Some(&recommendations), 8);
    let narration = crate::agent::llm::build_narration(
        &snapshot,
        &briefing,
        &session,
        Some(&watchlist),
        Some(&recommendations),
        Some(&analysis),
    );

    let analysis_path = {
        let (env_var, default_path) = crate::agent::llm::analysis_path(market);
        std::env::var(env_var).unwrap_or_else(|_| default_path.to_string())
    };
    let narration_path = {
        let (env_var, default_path) = crate::agent::llm::narration_path(market);
        std::env::var(env_var).unwrap_or_else(|_| default_path.to_string())
    };
    spawn_write_json_snapshot(analysis_path, analysis.clone());
    spawn_write_json_snapshot(narration_path, narration.clone());

    Ok(Json(AgentAnalyzeResponse {
        analysis,
        narration,
    }))
}

pub(super) async fn post_agent_analyze_codex_cli(
    Path(market): Path<String>,
    Json(body): Json<CodexCliAnalyzeBody>,
) -> Result<Json<AgentAnalyzeResponse>, ApiError> {
    let market = parse_case_market(&market)?;
    let (analysis, narration) = crate::agent::codex::run_codex_cli_analysis(market, &body)
        .await
        .map_err(ApiError::internal)?;
    Ok(Json(AgentAnalyzeResponse {
        analysis,
        narration,
    }))
}

pub(super) async fn get_agent_threads(
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Json<AgentThreadsResponse>, ApiError> {
    let market = parse_case_market(&market)?;
    let operational = load_or_build_operational_snapshot(market).await?;
    let mut threads = operational
        .threads
        .into_iter()
        .map(|item| item.thread)
        .collect::<Vec<_>>();
    if let Some(symbol) = normalized_query_value(query.symbol.as_deref()) {
        threads.retain(|item| item.symbol.eq_ignore_ascii_case(symbol));
    }
    if let Some(sector) = normalized_query_value(query.sector.as_deref()) {
        threads.retain(|item| {
            item.sector
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case(sector))
                .unwrap_or(false)
        });
    }
    let total = threads.len();
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    if threads.len() > limit {
        threads.truncate(limit);
    }
    Ok(Json(AgentThreadsResponse {
        tick: operational.source_tick,
        total,
        threads,
    }))
}

pub(super) async fn get_agent_thread(
    Path((market, symbol)): Path<(String, String)>,
) -> Result<Json<AgentThread>, ApiError> {
    let market = parse_case_market(&market)?;
    let operational = load_or_build_operational_snapshot(market).await?;
    let thread = operational
        .threads
        .iter()
        .map(|item| &item.thread)
        .find(|item| item.symbol.eq_ignore_ascii_case(&symbol))
        .cloned()
        .ok_or_else(|| ApiError::not_found(format!("no thread found for `{symbol}`")))?;
    Ok(Json(thread))
}

pub(super) async fn get_agent_turns(
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Json<AgentTurnsResponse>, ApiError> {
    let session = load_agent_session_for_market(&market).await?;
    let mut turns = session.recent_turns.clone();
    if let Some(since_tick) = query.since_tick {
        turns.retain(|item| item.tick > since_tick);
    }
    if let Some(symbol) = normalized_query_value(query.symbol.as_deref()) {
        turns.retain(|item| {
            item.focus_symbols
                .iter()
                .any(|value| value.eq_ignore_ascii_case(symbol))
        });
    }
    let total = turns.len();
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    if turns.len() > limit {
        turns = turns[turns.len().saturating_sub(limit)..].to_vec();
    }
    Ok(Json(AgentTurnsResponse {
        tick: session.tick,
        total,
        turns,
    }))
}

pub(super) async fn get_agent_world(
    Path(market): Path<String>,
) -> Result<Json<WorldStateSnapshot>, ApiError> {
    Ok(Json(load_world_state_for_market(&market).await?))
}

pub(super) async fn get_agent_notices(
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Json<AgentNoticesResponse>, ApiError> {
    Ok(Json(build_feed_notices_response(&market, &query).await?))
}

pub(super) async fn get_agent_transitions(
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Json<AgentTransitionsResponse>, ApiError> {
    Ok(Json(
        build_feed_transitions_response(&market, &query).await?,
    ))
}

pub(super) async fn get_agent_structures(
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Json<AgentStructuresResponse>, ApiError> {
    let market = parse_case_market(&market)?;
    let operational = load_or_build_operational_snapshot(market).await?;
    let mut structures = operational
        .symbols
        .iter()
        .filter_map(|item| item.state.structure.clone())
        .collect::<Vec<_>>();
    if let Some(symbol) = normalized_query_value(query.symbol.as_deref()) {
        structures.retain(|item| item.symbol.eq_ignore_ascii_case(symbol));
    }
    if let Some(sector) = normalized_query_value(query.sector.as_deref()) {
        structures.retain(|item| {
            item.sector
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case(sector))
                .unwrap_or(false)
        });
    }
    let total = structures.len();
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    if structures.len() > limit {
        structures.truncate(limit);
    }
    Ok(Json(AgentStructuresResponse {
        tick: operational.source_tick,
        total,
        structures,
    }))
}

pub(super) async fn get_agent_structure(
    Path((market, symbol)): Path<(String, String)>,
) -> Result<Json<AgentStructureState>, ApiError> {
    let market = parse_case_market(&market)?;
    let operational = load_or_build_operational_snapshot(market).await?;
    let structure = operational
        .symbol(&symbol)
        .and_then(|item| item.state.structure.clone())
        .ok_or_else(|| ApiError::not_found(format!("no active structure found for `{symbol}`")))?;
    Ok(Json(structure))
}

pub(super) async fn get_agent_symbol(
    Path((market, symbol)): Path<(String, String)>,
) -> Result<Json<AgentSymbolState>, ApiError> {
    let market = parse_case_market(&market)?;
    let operational = load_or_build_operational_snapshot(market).await?;
    let symbol_state = operational
        .symbol(&symbol)
        .map(|item| item.state.clone())
        .ok_or_else(|| ApiError::not_found(format!("no symbol state found for `{symbol}`")))?;
    Ok(Json(symbol_state))
}

pub(super) async fn get_agent_depth(
    Path((market, symbol)): Path<(String, String)>,
) -> Result<Json<AgentDepthState>, ApiError> {
    let market = parse_case_market(&market)?;
    let operational = load_or_build_operational_snapshot(market).await?;
    let depth = operational
        .symbol(&symbol)
        .and_then(|item| item.state.depth.clone())
        .ok_or_else(|| ApiError::not_found(format!("no depth state found for `{symbol}`")))?;
    Ok(Json(depth))
}

pub(super) async fn get_agent_brokers(
    Path((market, symbol)): Path<(String, String)>,
) -> Result<Json<AgentBrokerState>, ApiError> {
    let market = parse_case_market(&market)?;
    let operational = load_or_build_operational_snapshot(market).await?;
    let brokers = operational
        .symbol(&symbol)
        .and_then(|item| item.state.brokers.clone())
        .ok_or_else(|| ApiError::not_found(format!("no broker state found for `{symbol}`")))?;
    Ok(Json(brokers))
}

pub(super) async fn get_agent_invalidation(
    Path((market, symbol)): Path<(String, String)>,
) -> Result<Json<AgentInvalidationState>, ApiError> {
    let market = parse_case_market(&market)?;
    let operational = load_or_build_operational_snapshot(market).await?;
    let invalidation = operational
        .symbol(&symbol)
        .and_then(|item| item.state.invalidation.clone())
        .ok_or_else(|| {
            ApiError::not_found(format!("no invalidation state found for `{symbol}`"))
        })?;
    Ok(Json(invalidation))
}

pub(super) async fn get_agent_sector_flows(
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Json<AgentSectorFlowsResponse>, ApiError> {
    let market = parse_case_market(&market)?;
    let operational = load_or_build_operational_snapshot(market).await?;
    let mut flows = operational.sidecars.sector_flows.clone();
    if let Some(sector) = normalized_query_value(query.sector.as_deref()) {
        flows.retain(|item| item.sector.eq_ignore_ascii_case(sector));
    }
    let total = flows.len();
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    if flows.len() > limit {
        flows.truncate(limit);
    }
    Ok(Json(AgentSectorFlowsResponse {
        tick: operational.source_tick,
        total,
        flows,
    }))
}

pub(super) async fn get_agent_backward(
    Path((market, symbol)): Path<(String, String)>,
) -> Result<Json<BackwardInvestigation>, ApiError> {
    let market = parse_case_market(&market)?;
    let operational = load_or_build_operational_snapshot(market).await?;
    let investigation = operational
        .backward_investigation(&symbol)
        .cloned()
        .ok_or_else(|| {
            ApiError::not_found(format!("no backward investigation found for `{symbol}`"))
        })?;
    Ok(Json(investigation))
}

async fn load_agent_snapshot_for_market(raw: &str) -> Result<AgentSnapshot, ApiError> {
    let market = parse_case_market(raw)?;
    agent::load_snapshot(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load agent snapshot: {error}")))
}
