#[cfg(feature = "persistence")]
use axum::extract::{Path, Query, State};
use axum::Json;
#[cfg(feature = "persistence")]
use serde::{Deserialize, Serialize};

#[cfg(feature = "persistence")]
use super::agent_api::AgentFeedQuery;
#[cfg(feature = "persistence")]
use super::core::{bounded, case_market_slug, normalized_query_value, parse_case_market};
use super::foundation::ApiError;
#[cfg(feature = "persistence")]
use super::foundation::ApiState;
#[cfg(feature = "persistence")]
use super::constants::{DEFAULT_LIMIT, MAX_LIMIT};

#[cfg(feature = "persistence")]
use crate::ontology::{
    sector_node_id, symbol_node_id, KnowledgeEventKind, KnowledgeLinkAttributes,
    KnowledgeRelation,
};
#[cfg(feature = "persistence")]
use crate::persistence::agent_graph::{
    KnowledgeEventHistoryRecord, KnowledgeEventStateRecord, KnowledgeLinkHistoryRecord,
    KnowledgeLinkStateRecord, KnowledgeNodeHistoryRecord, KnowledgeNodeStateRecord,
    MacroEventHistoryRecord, MacroEventStateRecord,
};

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
pub(crate) struct AgentGraphNodeResponse {
    node: Option<KnowledgeNodeStateRecord>,
    current_links: Vec<KnowledgeLinkStateRecord>,
    current_events: Vec<KnowledgeEventStateRecord>,
    node_history: Vec<KnowledgeNodeHistoryRecord>,
    link_history: Vec<KnowledgeLinkHistoryRecord>,
    event_history: Vec<KnowledgeEventHistoryRecord>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Serialize)]
pub(crate) struct AgentGraphLinksResponse {
    current_links: Vec<KnowledgeLinkStateRecord>,
    current_events: Vec<KnowledgeEventStateRecord>,
    link_history: Vec<KnowledgeLinkHistoryRecord>,
    event_history: Vec<KnowledgeEventHistoryRecord>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize, Default, Clone)]
pub(crate) struct AgentGraphQuery {
    since_tick: Option<u64>,
    limit: Option<usize>,
    node_id: Option<String>,
    relation: Option<String>,
    event_kind: Option<String>,
    relation_schema: Option<String>,
    source_kind: Option<String>,
    target_kind: Option<String>,
    subject_kind: Option<String>,
    object_kind: Option<String>,
    scope_kind: Option<String>,
    best_action: Option<String>,
    leader_streak_min: Option<u64>,
}

#[cfg(feature = "persistence")]
pub(super) async fn get_macro_event_history(
    State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Json<Vec<MacroEventHistoryRecord>>, ApiError> {
    let market = parse_case_market(&market)?;
    let market_key = case_market_slug(market);
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let mut records = state
        .store
        .recent_macro_event_history(market_key, query.since_tick, limit)
        .await
        .map_err(|error| {
            ApiError::internal(format!("failed to query macro event history: {error}"))
        })?;
    if let Some(symbol) = normalized_query_value(query.symbol.as_deref()) {
        records.retain(|record| {
            record
                .affected_symbols
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(symbol))
        });
    }
    if let Some(sector) = normalized_query_value(query.sector.as_deref()) {
        records.retain(|record| {
            record
                .affected_sectors
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(sector))
        });
    }
    records.truncate(limit);
    Ok(Json(records))
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_macro_event_history() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "macro event history requires building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
pub(super) async fn get_knowledge_link_history(
    State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Json<Vec<KnowledgeLinkHistoryRecord>>, ApiError> {
    let market = parse_case_market(&market)?;
    let market_key = case_market_slug(market);
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let mut records = state
        .store
        .recent_knowledge_link_history(market_key, query.since_tick, limit)
        .await
        .map_err(|error| {
            ApiError::internal(format!("failed to query knowledge link history: {error}"))
        })?;
    if let Some(symbol) = normalized_query_value(query.symbol.as_deref()) {
        let node_id = symbol_node_id(symbol);
        records.retain(|record| {
            record.source_node_id.eq_ignore_ascii_case(&node_id)
                || record.target_node_id.eq_ignore_ascii_case(&node_id)
                || record.source_label.eq_ignore_ascii_case(symbol)
                || record.target_label.eq_ignore_ascii_case(symbol)
        });
    }
    if let Some(sector) = normalized_query_value(query.sector.as_deref()) {
        let node_id = sector_node_id(sector);
        records.retain(|record| {
            record.source_node_id.eq_ignore_ascii_case(&node_id)
                || record.target_node_id.eq_ignore_ascii_case(&node_id)
                || record.source_label.eq_ignore_ascii_case(sector)
                || record.target_label.eq_ignore_ascii_case(sector)
        });
    }
    records.truncate(limit);
    Ok(Json(records))
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_knowledge_link_history(
) -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "knowledge link history requires building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
pub(super) async fn get_macro_event_state(
    State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Json<Vec<MacroEventStateRecord>>, ApiError> {
    let market = parse_case_market(&market)?;
    let market_key = case_market_slug(market);
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let mut records = state
        .store
        .current_macro_event_state(market_key, limit)
        .await
        .map_err(|error| ApiError::internal(format!("failed to query macro event state: {error}")))?;
    if let Some(symbol) = normalized_query_value(query.symbol.as_deref()) {
        records.retain(|record| {
            record
                .affected_symbols
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(symbol))
        });
    }
    if let Some(sector) = normalized_query_value(query.sector.as_deref()) {
        records.retain(|record| {
            record
                .affected_sectors
                .iter()
                .any(|candidate| candidate.eq_ignore_ascii_case(sector))
        });
    }
    records.truncate(limit);
    Ok(Json(records))
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_macro_event_state() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "macro event state requires building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
pub(super) async fn get_knowledge_link_state(
    State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Json<Vec<KnowledgeLinkStateRecord>>, ApiError> {
    let market = parse_case_market(&market)?;
    let market_key = case_market_slug(market);
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let mut records = state
        .store
        .current_knowledge_link_state(market_key, limit)
        .await
        .map_err(|error| ApiError::internal(format!("failed to query knowledge link state: {error}")))?;
    if let Some(symbol) = normalized_query_value(query.symbol.as_deref()) {
        let node_id = symbol_node_id(symbol);
        records.retain(|record| {
            record.source_node_id.eq_ignore_ascii_case(&node_id)
                || record.target_node_id.eq_ignore_ascii_case(&node_id)
                || record.source_label.eq_ignore_ascii_case(symbol)
                || record.target_label.eq_ignore_ascii_case(symbol)
        });
    }
    if let Some(sector) = normalized_query_value(query.sector.as_deref()) {
        let node_id = sector_node_id(sector);
        records.retain(|record| {
            record.source_node_id.eq_ignore_ascii_case(&node_id)
                || record.target_node_id.eq_ignore_ascii_case(&node_id)
                || record.source_label.eq_ignore_ascii_case(sector)
                || record.target_label.eq_ignore_ascii_case(sector)
        });
    }
    records.truncate(limit);
    Ok(Json(records))
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_knowledge_link_state(
) -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "knowledge link state requires building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
pub(super) async fn get_graph_node(
    State(state): State<ApiState>,
    Path((market, node_id)): Path<(String, String)>,
    Query(query): Query<AgentGraphQuery>,
) -> Result<Json<AgentGraphNodeResponse>, ApiError> {
    let market = parse_case_market(&market)?;
    let market_key = case_market_slug(market);
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let node = state
        .store
        .knowledge_node_state_by_id(market_key, &node_id)
        .await
        .map_err(|error| ApiError::internal(format!("failed to query node state: {error}")))?;
    let mut current_links = state
        .store
        .current_knowledge_link_state_for_node(market_key, &node_id, limit)
        .await
        .map_err(|error| ApiError::internal(format!("failed to query current node links: {error}")))?;
    let mut current_events = state
        .store
        .current_knowledge_event_state_for_node(market_key, &node_id, limit)
        .await
        .map_err(|error| ApiError::internal(format!("failed to query current node events: {error}")))?;
    let node_history = state
        .store
        .recent_knowledge_node_history_for_id(market_key, &node_id, query.since_tick, limit)
        .await
        .map_err(|error| ApiError::internal(format!("failed to query node history: {error}")))?;
    let mut link_history = state
        .store
        .recent_knowledge_link_history_for_node(market_key, &node_id, query.since_tick, limit)
        .await
        .map_err(|error| ApiError::internal(format!("failed to query link history: {error}")))?;
    let mut event_history = state
        .store
        .recent_knowledge_event_history_for_node(market_key, &node_id, query.since_tick, limit)
        .await
        .map_err(|error| ApiError::internal(format!("failed to query event history: {error}")))?;

    current_links.retain(|link| matches_graph_link_state(link, &query));
    link_history.retain(|link| matches_graph_link_history(link, &query));
    current_events.retain(|event| matches_graph_event_state(event, &query));
    event_history.retain(|event| matches_graph_event_history(event, &query));
    current_links.truncate(limit);
    link_history.truncate(limit);
    current_events.truncate(limit);
    event_history.truncate(limit);

    Ok(Json(AgentGraphNodeResponse {
        node,
        current_links,
        current_events,
        node_history,
        link_history,
        event_history,
    }))
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_graph_node() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "graph node queries require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
pub(super) async fn get_graph_links(
    State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<AgentGraphQuery>,
) -> Result<Json<AgentGraphLinksResponse>, ApiError> {
    let market = parse_case_market(&market)?;
    let market_key = case_market_slug(market);
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let mut current_links = state
        .store
        .current_knowledge_link_state(market_key, limit)
        .await
        .map_err(|error| ApiError::internal(format!("failed to query graph links: {error}")))?;
    let mut current_events = state
        .store
        .current_knowledge_event_state(market_key, limit)
        .await
        .map_err(|error| ApiError::internal(format!("failed to query graph events: {error}")))?;
    let mut link_history = state
        .store
        .recent_knowledge_link_history(market_key, query.since_tick, limit)
        .await
        .map_err(|error| ApiError::internal(format!("failed to query graph link history: {error}")))?;
    let mut event_history = state
        .store
        .recent_knowledge_event_history(market_key, query.since_tick, limit)
        .await
        .map_err(|error| ApiError::internal(format!("failed to query graph event history: {error}")))?;

    current_links.retain(|link| matches_graph_link_state(link, &query));
    link_history.retain(|link| matches_graph_link_history(link, &query));
    current_events.retain(|event| matches_graph_event_state(event, &query));
    event_history.retain(|event| matches_graph_event_history(event, &query));
    current_links.truncate(limit);
    link_history.truncate(limit);
    current_events.truncate(limit);
    event_history.truncate(limit);

    Ok(Json(AgentGraphLinksResponse {
        current_links,
        current_events,
        link_history,
        event_history,
    }))
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_graph_links() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "graph link queries require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
fn matches_graph_link_state(link: &KnowledgeLinkStateRecord, query: &AgentGraphQuery) -> bool {
    graph_link_filters_match(
        &link.relation,
        &link.attributes,
        &link.source_node_id,
        &link.target_node_id,
        &link.source_node_kind,
        &link.target_node_kind,
        query,
    )
}

#[cfg(feature = "persistence")]
fn matches_graph_link_history(link: &KnowledgeLinkHistoryRecord, query: &AgentGraphQuery) -> bool {
    graph_link_filters_match(
        &link.relation,
        &link.attributes,
        &link.source_node_id,
        &link.target_node_id,
        &link.source_node_kind,
        &link.target_node_kind,
        query,
    )
}

#[cfg(feature = "persistence")]
fn matches_graph_event_state(event: &KnowledgeEventStateRecord, query: &AgentGraphQuery) -> bool {
    graph_event_filters_match(
        event.kind,
        &event.subject_node_id,
        event.object_node_id.as_deref(),
        &event.subject_node_kind,
        event.object_node_kind.as_deref(),
        query,
    )
}

#[cfg(feature = "persistence")]
fn matches_graph_event_history(
    event: &KnowledgeEventHistoryRecord,
    query: &AgentGraphQuery,
) -> bool {
    graph_event_filters_match(
        event.kind,
        &event.subject_node_id,
        event.object_node_id.as_deref(),
        &event.subject_node_kind,
        event.object_node_kind.as_deref(),
        query,
    )
}

#[cfg(feature = "persistence")]
fn graph_event_filters_match(
    kind: KnowledgeEventKind,
    subject_node_id: &str,
    object_node_id: Option<&str>,
    subject_node_kind: &str,
    object_node_kind: Option<&str>,
    query: &AgentGraphQuery,
) -> bool {
    if let Some(node_id) = normalized_query_value(query.node_id.as_deref()) {
        let matches_node = subject_node_id.eq_ignore_ascii_case(node_id)
            || object_node_id
                .map(|value| value.eq_ignore_ascii_case(node_id))
                .unwrap_or(false);
        if !matches_node {
            return false;
        }
    }

    if let Some(event_kind_filter) = normalized_query_value(query.event_kind.as_deref()) {
        if !kind.as_str().eq_ignore_ascii_case(event_kind_filter) {
            return false;
        }
    }

    if let Some(subject_kind_filter) = normalized_query_value(query.subject_kind.as_deref()) {
        if !subject_node_kind.eq_ignore_ascii_case(subject_kind_filter) {
            return false;
        }
    }

    if let Some(object_kind_filter) = normalized_query_value(query.object_kind.as_deref()) {
        let matches_object_kind = object_node_kind
            .map(|value| value.eq_ignore_ascii_case(object_kind_filter))
            .unwrap_or(false);
        if !matches_object_kind {
            return false;
        }
    }

    true
}

#[cfg(feature = "persistence")]
fn graph_link_filters_match(
    relation: &KnowledgeRelation,
    attributes: &KnowledgeLinkAttributes,
    source_node_id: &str,
    target_node_id: &str,
    source_node_kind: &str,
    target_node_kind: &str,
    query: &AgentGraphQuery,
) -> bool {
    if let Some(node_id) = normalized_query_value(query.node_id.as_deref()) {
        let matches_node = source_node_id.eq_ignore_ascii_case(node_id)
            || target_node_id.eq_ignore_ascii_case(node_id);
        if !matches_node {
            return false;
        }
    }

    if let Some(relation_filter) = normalized_query_value(query.relation.as_deref()) {
        if !relation.as_str().eq_ignore_ascii_case(relation_filter) {
            return false;
        }
    }

    if let Some(schema_filter) = normalized_query_value(query.relation_schema.as_deref()) {
        if !link_attribute_schema(attributes).eq_ignore_ascii_case(schema_filter) {
            return false;
        }
    }

    if let Some(source_kind_filter) = normalized_query_value(query.source_kind.as_deref()) {
        if !source_node_kind.eq_ignore_ascii_case(source_kind_filter) {
            return false;
        }
    }

    if let Some(target_kind_filter) = normalized_query_value(query.target_kind.as_deref()) {
        if !target_node_kind.eq_ignore_ascii_case(target_kind_filter) {
            return false;
        }
    }

    if let Some(scope_kind_filter) = normalized_query_value(query.scope_kind.as_deref()) {
        let matches_scope_kind = match attributes {
            KnowledgeLinkAttributes::SupportsDecision {
                decision_scope_kind,
                ..
            }
            | KnowledgeLinkAttributes::DominatesScope {
                decision_scope_kind,
                ..
            } => decision_scope_kind.eq_ignore_ascii_case(scope_kind_filter),
            KnowledgeLinkAttributes::TargetsScope { scope_kind, .. } => {
                scope_kind.eq_ignore_ascii_case(scope_kind_filter)
            }
            _ => true,
        };
        if !matches_scope_kind {
            return false;
        }
    }

    if let Some(best_action_filter) = normalized_query_value(query.best_action.as_deref()) {
        let matches_best_action = match attributes {
            KnowledgeLinkAttributes::SupportsDecision { best_action, .. }
            | KnowledgeLinkAttributes::DominatesScope { best_action, .. } => {
                best_action.eq_ignore_ascii_case(best_action_filter)
            }
            _ => true,
        };
        if !matches_best_action {
            return false;
        }
    }

    if let Some(leader_streak_min) = query.leader_streak_min {
        let matches_leader_streak = match attributes {
            KnowledgeLinkAttributes::LeadingCauseForLeaf { leader_streak, .. } => {
                *leader_streak >= leader_streak_min
            }
            _ => true,
        };
        if !matches_leader_streak {
            return false;
        }
    }

    true
}

#[cfg(feature = "persistence")]
fn link_attribute_schema(attributes: &KnowledgeLinkAttributes) -> &'static str {
    match attributes {
        KnowledgeLinkAttributes::Generic => "generic",
        KnowledgeLinkAttributes::ImpactsMarket { .. } => "impacts_market",
        KnowledgeLinkAttributes::ImpactsSector { .. } => "impacts_sector",
        KnowledgeLinkAttributes::ImpactsSymbol { .. } => "impacts_symbol",
        KnowledgeLinkAttributes::SupportsDecision { .. } => "supports_decision",
        KnowledgeLinkAttributes::DominatesScope { .. } => "dominates_scope",
        KnowledgeLinkAttributes::DescribesScope { .. } => "describes_scope",
        KnowledgeLinkAttributes::TargetsScope { .. } => "targets_scope",
        KnowledgeLinkAttributes::CandidateForLeaf { .. } => "candidate_for_leaf",
        KnowledgeLinkAttributes::LeadingCauseForLeaf { .. } => "leading_cause_for_leaf",
        KnowledgeLinkAttributes::TracksSymbol { .. } => "tracks_symbol",
        KnowledgeLinkAttributes::TracksSector { .. } => "tracks_sector",
        KnowledgeLinkAttributes::InstantiatesHypothesis { .. } => "instantiates_hypothesis",
        KnowledgeLinkAttributes::PrimaryMechanism { .. } => "primary_mechanism",
        KnowledgeLinkAttributes::CompetingMechanism { .. } => "competing_mechanism",
    }
}
