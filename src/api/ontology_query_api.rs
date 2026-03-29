use axum::extract::Path;
use axum::Json;

use crate::agent;
use crate::ontology::world::WorldStateSnapshot;

use super::core::parse_case_market;
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
