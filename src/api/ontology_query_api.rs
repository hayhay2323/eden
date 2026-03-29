use axum::extract::{Path, Query};
use axum::Json;

use crate::agent;
use crate::ontology::{
    build_macro_event_contracts, world::WorldStateSnapshot, AgentKnowledgeLink,
    AgentMacroEventCandidate, MacroEventContract,
};

use super::agent_api::AgentFeedQuery;
use super::core::{bounded, normalized_query_value, parse_case_market};
use super::constants::{DEFAULT_LIMIT, MAX_LIMIT};
use super::foundation::ApiError;

pub(in crate::api) async fn load_world_state_for_market(
    raw_market: &str,
) -> Result<WorldStateSnapshot, ApiError> {
    let market = parse_case_market(raw_market)?;
    let snapshot = agent::load_snapshot(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load agent snapshot: {error}")))?;
    snapshot
        .world_state
        .ok_or_else(|| ApiError::not_found("world state not available for this market"))
}

pub(super) async fn get_ontology_world(
    Path(market): Path<String>,
) -> Result<Json<WorldStateSnapshot>, ApiError> {
    Ok(Json(load_world_state_for_market(&market).await?))
}

pub(super) async fn get_ontology_macro_event_candidates(
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Json<Vec<AgentMacroEventCandidate>>, ApiError> {
    let market = parse_case_market(&market)?;
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let snapshot = agent::load_snapshot(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load agent snapshot: {error}")))?;
    let mut items = snapshot.macro_event_candidates.clone();
    if let Some(since_tick) = query.since_tick {
        items.retain(|item| item.tick > since_tick);
    }
    if let Some(symbol) = normalized_query_value(query.symbol.as_deref()) {
        items.retain(|item| {
            item.impact
                .affected_symbols
                .iter()
                .any(|value| value.eq_ignore_ascii_case(symbol))
        });
    }
    if let Some(sector) = normalized_query_value(query.sector.as_deref()) {
        items.retain(|item| {
            item.impact
                .affected_sectors
                .iter()
                .any(|value| value.eq_ignore_ascii_case(sector))
        });
    }
    items.truncate(limit);
    Ok(Json(items))
}

pub(super) async fn get_ontology_macro_event_contracts_view(
    Path(market): Path<String>,
) -> Result<Json<Vec<MacroEventContract>>, ApiError> {
    let market = parse_case_market(&market)?;
    let snapshot = agent::load_snapshot(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load agent snapshot: {error}")))?;
    Ok(Json(
        build_macro_event_contracts(&snapshot).map_err(ApiError::internal)?,
    ))
}

pub(super) async fn get_ontology_knowledge_links(
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Json<Vec<AgentKnowledgeLink>>, ApiError> {
    let market = parse_case_market(&market)?;
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let snapshot = agent::load_snapshot(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load agent snapshot: {error}")))?;
    let session = agent::load_session(market).await.ok();
    let recommendations = agent::build_recommendations(&snapshot, session.as_ref());
    let mut links = snapshot.knowledge_links.clone();
    links.extend(recommendations.knowledge_links);
    if let Some(symbol) = normalized_query_value(query.symbol.as_deref()) {
        links.retain(|item| crate::agent::knowledge_link_matches_filters(item, Some(symbol), None));
    }
    if let Some(sector) = normalized_query_value(query.sector.as_deref()) {
        links.retain(|item| crate::agent::knowledge_link_matches_filters(item, None, Some(sector)));
    }
    links.truncate(limit);
    Ok(Json(links))
}
