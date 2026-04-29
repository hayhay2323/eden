use axum::extract::Path;
use axum::response::sse::Sse;

use super::core::{case_market_slug, parse_case_market};
use super::foundation::{ApiError, JsonEventStream};
use super::ontology_query_api::load_world_state_for_market;
use super::stream_support::{json_poll_sse, latest_file_revision};
use crate::agent;
use crate::cases::CaseMarket;

pub(super) async fn stream_ontology_world(
    Path(market): Path<String>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(json_poll_sse(
        move || async move { load_world_state_for_market(case_market_slug(market)).await },
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
