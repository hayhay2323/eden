#[cfg(feature = "persistence")]
use axum::extract::State;
use axum::extract::{Path, Query};
use axum::Json;

use crate::ontology::{world::WorldStateSnapshot, AgentKnowledgeLink, AgentMacroEventCandidate};

use super::agent_api::AgentFeedQuery;
use super::constants::{DEFAULT_LIMIT, MAX_LIMIT};
use super::core::{bounded, normalized_query_value, parse_case_market};
use super::foundation::ApiError;
#[cfg(feature = "persistence")]
use super::foundation::ApiState;
use super::ontology_api::load_contract_snapshot;
#[cfg(feature = "persistence")]
use super::ontology_api::load_enriched_contract_snapshot;

pub(in crate::api) async fn load_world_state_for_market(
    raw_market: &str,
) -> Result<WorldStateSnapshot, ApiError> {
    let market = parse_case_market(raw_market)?;
    let snapshot = load_contract_snapshot(market).await?;
    snapshot
        .world_state()
        .cloned()
        .ok_or_else(|| ApiError::not_found("world state not available for this market"))
}

pub(super) async fn get_ontology_world(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path(market): Path<String>,
) -> Result<Json<WorldStateSnapshot>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    snapshot
        .world_state()
        .cloned()
        .map(Json)
        .ok_or_else(|| ApiError::not_found("world state not available for this market"))
}

pub(super) async fn get_ontology_macro_event_candidates(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Json<Vec<AgentMacroEventCandidate>>, ApiError> {
    let market = parse_case_market(&market)?;
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let mut items = snapshot.sidecars.macro_event_candidates;
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

pub(super) async fn get_ontology_knowledge_links(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Json<Vec<AgentKnowledgeLink>>, ApiError> {
    let market = parse_case_market(&market)?;
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let mut links = snapshot.sidecars.knowledge_links;
    if let Some(symbol) = normalized_query_value(query.symbol.as_deref()) {
        links.retain(|item| crate::agent::knowledge_link_matches_filters(item, Some(symbol), None));
    }
    if let Some(sector) = normalized_query_value(query.sector.as_deref()) {
        links.retain(|item| crate::agent::knowledge_link_matches_filters(item, None, Some(sector)));
    }
    links.truncate(limit);
    Ok(Json(links))
}
