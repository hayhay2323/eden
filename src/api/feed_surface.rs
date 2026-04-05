use axum::extract::{Path, Query};
use axum::response::sse::Sse;

use super::agent_api::AgentFeedQuery;
use super::core::{case_market_slug, parse_case_market};
use super::feed_api::{build_feed_notices_response, build_feed_transitions_response};
use super::foundation::{ApiError, JsonEventStream};
use super::stream_support::{json_poll_sse, latest_file_revision};
use crate::agent;
use crate::cases::CaseMarket;

pub(super) async fn stream_feed_notices(
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(json_poll_sse(
        move || {
            let query = query.clone();
            async move { build_feed_notices_response(case_market_slug(market), &query).await }
        },
        move || async move { snapshot_stream_revision(market).await },
    ))
}

pub(super) async fn stream_feed_transitions(
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(json_poll_sse(
        move || {
            let query = query.clone();
            async move { build_feed_transitions_response(case_market_slug(market), &query).await }
        },
        move || async move { snapshot_stream_revision(market).await },
    ))
}

async fn snapshot_stream_revision(market: CaseMarket) -> Result<String, ApiError> {
    let (env_var, default_path) = agent::load_agent_snapshot_path(market);
    latest_file_revision(vec![
        std::env::var(env_var).unwrap_or_else(|_| default_path.to_string())
    ])
    .await
}
