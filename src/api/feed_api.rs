use axum::extract::{Path, Query};
use axum::Json;
use serde::Serialize;

use crate::agent::{self, AgentNotice, AgentSnapshot, AgentTransition};

use super::agent_api::AgentFeedQuery;
use super::constants::{DEFAULT_LIMIT, MAX_LIMIT};
use super::core::{bounded, normalized_query_value, parse_case_market};
use super::foundation::ApiError;

#[derive(Debug, Serialize)]
pub(super) struct FeedNoticesResponse {
    tick: u64,
    total: usize,
    notices: Vec<AgentNotice>,
}

#[derive(Debug, Serialize)]
pub(super) struct FeedTransitionsResponse {
    tick: u64,
    total: usize,
    transitions: Vec<AgentTransition>,
}

pub(in crate::api) async fn build_feed_notices_response(
    raw_market: &str,
    query: &AgentFeedQuery,
) -> Result<FeedNoticesResponse, ApiError> {
    let snapshot = load_feed_snapshot_for_market(raw_market).await?;
    let mut notices = snapshot.notices.clone();
    if let Some(since_tick) = query.since_tick {
        notices.retain(|item| item.tick > since_tick);
    }
    if let Some(symbol) = normalized_query_value(query.symbol.as_deref()) {
        notices.retain(|item| {
            item.symbol
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case(symbol))
                .unwrap_or(false)
        });
    }
    if let Some(sector) = normalized_query_value(query.sector.as_deref()) {
        notices.retain(|item| {
            item.sector
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case(sector))
                .unwrap_or(false)
        });
    }
    let total = notices.len();
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    if notices.len() > limit {
        notices.truncate(limit);
    }
    Ok(FeedNoticesResponse {
        tick: snapshot.tick,
        total,
        notices,
    })
}

pub(in crate::api) async fn build_feed_transitions_response(
    raw_market: &str,
    query: &AgentFeedQuery,
) -> Result<FeedTransitionsResponse, ApiError> {
    let snapshot = load_feed_snapshot_for_market(raw_market).await?;
    let mut transitions = snapshot.recent_transitions.clone();
    if let Some(since_tick) = query.since_tick {
        transitions.retain(|item| item.to_tick > since_tick);
    }
    if let Some(symbol) = normalized_query_value(query.symbol.as_deref()) {
        transitions.retain(|item| item.symbol.eq_ignore_ascii_case(symbol));
    }
    if let Some(sector) = normalized_query_value(query.sector.as_deref()) {
        transitions.retain(|item| {
            item.sector
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case(sector))
                .unwrap_or(false)
        });
    }
    let total = transitions.len();
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    if transitions.len() > limit {
        transitions.truncate(limit);
    }
    Ok(FeedTransitionsResponse {
        tick: snapshot.tick,
        total,
        transitions,
    })
}

pub(super) async fn get_feed_notices(
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Json<FeedNoticesResponse>, ApiError> {
    Ok(Json(build_feed_notices_response(&market, &query).await?))
}

pub(super) async fn get_feed_transitions(
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Json<FeedTransitionsResponse>, ApiError> {
    Ok(Json(
        build_feed_transitions_response(&market, &query).await?,
    ))
}

async fn load_feed_snapshot_for_market(raw_market: &str) -> Result<AgentSnapshot, ApiError> {
    let market = parse_case_market(raw_market)?;
    agent::load_snapshot(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load agent snapshot: {error}")))
}
