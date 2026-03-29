#[cfg(feature = "persistence")]
use axum::extract::{Path, Query, State};
use axum::Json;
#[cfg(feature = "persistence")]
use serde::Deserialize;

#[cfg(feature = "persistence")]
use super::foundation::ApiState;
use super::foundation::ApiError;

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize)]
pub(crate) struct ArchiveTimeQuery {
    from: String,
    to: String,
}

#[cfg(feature = "persistence")]
pub(super) async fn get_archive_order_books(
    State(state): State<ApiState>,
    Path(symbol): Path<String>,
    Query(query): Query<ArchiveTimeQuery>,
) -> Result<Json<Vec<crate::ontology::microstructure::ArchivedOrderBook>>, ApiError> {
    let from =
        time::OffsetDateTime::parse(&query.from, &time::format_description::well_known::Rfc3339)
            .map_err(|e| ApiError::bad_request(format!("invalid 'from': {e}")))?;
    let to = time::OffsetDateTime::parse(&query.to, &time::format_description::well_known::Rfc3339)
        .map_err(|e| ApiError::bad_request(format!("invalid 'to': {e}")))?;
    let sym = crate::ontology::objects::Symbol(symbol);
    let results = state
        .store
        .query_order_books(&sym, from, to)
        .await
        .map_err(|e| ApiError::internal(format!("query failed: {e}")))?;
    Ok(Json(results))
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_archive_order_books() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "archive endpoints require --features persistence",
    ))
}

#[cfg(feature = "persistence")]
pub(super) async fn get_archive_trades(
    State(state): State<ApiState>,
    Path(symbol): Path<String>,
    Query(query): Query<ArchiveTimeQuery>,
) -> Result<Json<Vec<crate::ontology::microstructure::ArchivedTrade>>, ApiError> {
    let from =
        time::OffsetDateTime::parse(&query.from, &time::format_description::well_known::Rfc3339)
            .map_err(|e| ApiError::bad_request(format!("invalid 'from': {e}")))?;
    let to = time::OffsetDateTime::parse(&query.to, &time::format_description::well_known::Rfc3339)
        .map_err(|e| ApiError::bad_request(format!("invalid 'to': {e}")))?;
    let sym = crate::ontology::objects::Symbol(symbol);
    let results = state
        .store
        .query_trades(&sym, from, to)
        .await
        .map_err(|e| ApiError::internal(format!("query failed: {e}")))?;
    Ok(Json(results))
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_archive_trades() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "archive endpoints require --features persistence",
    ))
}

#[cfg(feature = "persistence")]
pub(super) async fn get_archive_capital_flows(
    State(state): State<ApiState>,
    Path(symbol): Path<String>,
    Query(query): Query<ArchiveTimeQuery>,
) -> Result<Json<Vec<crate::ontology::microstructure::ArchivedCapitalFlowSeries>>, ApiError> {
    let from =
        time::OffsetDateTime::parse(&query.from, &time::format_description::well_known::Rfc3339)
            .map_err(|e| ApiError::bad_request(format!("invalid 'from': {e}")))?;
    let to = time::OffsetDateTime::parse(&query.to, &time::format_description::well_known::Rfc3339)
        .map_err(|e| ApiError::bad_request(format!("invalid 'to': {e}")))?;
    let sym = crate::ontology::objects::Symbol(symbol);
    let results = state
        .store
        .query_capital_flows(&sym, from, to)
        .await
        .map_err(|e| ApiError::internal(format!("query failed: {e}")))?;
    Ok(Json(results))
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_archive_capital_flows() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "archive endpoints require --features persistence",
    ))
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize)]
pub(crate) struct SymbolHistoryQuery {
    last_n: Option<usize>,
}

#[cfg(feature = "persistence")]
pub(super) async fn get_symbol_history(
    State(state): State<ApiState>,
    Path(symbol): Path<String>,
    Query(query): Query<SymbolHistoryQuery>,
) -> Result<Json<Vec<crate::temporal::record::SymbolSignals>>, ApiError> {
    let last_n = query.last_n.unwrap_or(50).min(500);
    let sym = crate::ontology::objects::Symbol(symbol);
    let results = state
        .store
        .query_symbol_history(&sym, last_n)
        .await
        .map_err(|e| ApiError::internal(format!("query failed: {e}")))?;
    Ok(Json(results))
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_symbol_history() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "history endpoints require --features persistence",
    ))
}
