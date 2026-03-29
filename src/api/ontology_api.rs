use axum::extract::{Path, Query};
#[cfg(feature = "persistence")]
use axum::extract::State;
use axum::Json;
use serde::Deserialize;

use crate::agent;
use crate::agent::AgentSectorFlow;
use crate::agent_llm;
use crate::cases;
#[cfg(feature = "persistence")]
use crate::cases::enrich_case_summaries;
use crate::ontology::{
    build_operational_snapshot, load_operational_snapshot, CaseContract, MacroEventContract,
    MarketSessionContract, OperationalSnapshot, RecommendationContract, SymbolStateContract,
    ThreadContract, WorkflowContract,
};
use crate::ontology::world::BackwardInvestigation;

use super::core::parse_case_market;
#[cfg(feature = "persistence")]
use super::foundation::ApiState;
use super::foundation::ApiError;

#[derive(Debug, Default, Deserialize)]
pub(super) struct OntologyObjectQuery {
    symbol: Option<String>,
    sector: Option<String>,
    action: Option<String>,
}

pub(in crate::api) async fn load_or_build_operational_snapshot(
    market: crate::cases::CaseMarket,
) -> Result<OperationalSnapshot, ApiError> {
    if let Ok(snapshot) = load_operational_snapshot(market).await {
        return Ok(snapshot);
    }

    let snapshot = agent::load_snapshot(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load agent snapshot: {error}")))?;
    let live_snapshot = cases::load_snapshot(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load market snapshot: {error}")))?;
    let session = agent::load_session(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load agent session: {error}")))?;
    let recommendations = agent::load_recommendations(market)
        .await
        .unwrap_or_else(|_| agent::build_recommendations(&snapshot, Some(&session)));
    let narration = agent_llm::load_narration(market).await.ok();

    let operational = build_operational_snapshot(
        &live_snapshot,
        &snapshot,
        &session,
        &recommendations,
        narration.as_ref(),
    )
    .map_err(ApiError::internal)?;

    Ok(operational)
}

#[cfg(feature = "persistence")]
async fn enrich_with_persistent_workflows(
    state: &ApiState,
    market: crate::cases::CaseMarket,
    snapshot: &mut OperationalSnapshot,
) -> Result<(), ApiError> {
    let live_snapshot = cases::load_snapshot(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load market snapshot: {error}")))?;
    let mut cases = cases::build_case_summaries(&live_snapshot);
    enrich_case_summaries(&state.store, &mut cases)
        .await
        .map_err(|error| ApiError::internal(format!("failed to enrich operational cases: {error}")))?;
    rebuild_operational_case_graph(snapshot, &cases);
    Ok(())
}

pub(in crate::api) async fn load_contract_snapshot(
    market: crate::cases::CaseMarket,
) -> Result<OperationalSnapshot, ApiError> {
    load_or_build_operational_snapshot(market).await
}

#[cfg(feature = "persistence")]
pub(in crate::api) async fn load_enriched_contract_snapshot(
    state: &ApiState,
    market: crate::cases::CaseMarket,
) -> Result<OperationalSnapshot, ApiError> {
    let mut snapshot = load_contract_snapshot(market).await?;
    enrich_with_persistent_workflows(state, market, &mut snapshot).await?;
    Ok(snapshot)
}

pub(super) async fn get_operational_snapshot(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path(market): Path<String>,
) -> Result<Json<OperationalSnapshot>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    Ok(Json(snapshot))
}

pub(super) async fn get_market_session_contract(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path(market): Path<String>,
) -> Result<Json<MarketSessionContract>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    Ok(Json(snapshot.market_session))
}

pub(super) async fn get_symbol_state_contracts(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<OntologyObjectQuery>,
) -> Result<Json<Vec<SymbolStateContract>>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let mut items = snapshot.symbols;
    if let Some(symbol) = normalized_query(query.symbol.as_deref()) {
        items.retain(|item| item.symbol.eq_ignore_ascii_case(symbol));
    }
    if let Some(sector) = normalized_query(query.sector.as_deref()) {
        items.retain(|item| {
            item.sector
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case(sector))
                .unwrap_or(false)
        });
    }
    Ok(Json(items))
}

pub(super) async fn get_case_contracts(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<OntologyObjectQuery>,
) -> Result<Json<Vec<CaseContract>>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let mut items = snapshot.cases;
    if let Some(symbol) = normalized_query(query.symbol.as_deref()) {
        items.retain(|item| item.symbol.eq_ignore_ascii_case(symbol));
    }
    if let Some(sector) = normalized_query(query.sector.as_deref()) {
        items.retain(|item| {
            item.sector
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case(sector))
                .unwrap_or(false)
        });
    }
    if let Some(action) = normalized_query(query.action.as_deref()) {
        items.retain(|item| item.action.eq_ignore_ascii_case(action));
    }
    Ok(Json(items))
}

pub(super) async fn get_recommendation_contracts(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<OntologyObjectQuery>,
) -> Result<Json<Vec<RecommendationContract>>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let mut items = snapshot.recommendations;
    if let Some(symbol) = normalized_query(query.symbol.as_deref()) {
        items.retain(|item| item.recommendation.symbol.eq_ignore_ascii_case(symbol));
    }
    if let Some(sector) = normalized_query(query.sector.as_deref()) {
        items.retain(|item| {
            item.recommendation
                .sector
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case(sector))
                .unwrap_or(false)
        });
    }
    if let Some(action) = normalized_query(query.action.as_deref()) {
        items.retain(|item| item.recommendation.best_action.eq_ignore_ascii_case(action));
    }
    Ok(Json(items))
}

pub(super) async fn get_macro_event_contracts(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<OntologyObjectQuery>,
) -> Result<Json<Vec<MacroEventContract>>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let mut items = snapshot.macro_events;
    if let Some(symbol) = normalized_query(query.symbol.as_deref()) {
        items.retain(|item| {
            item.event
                .impact
                .affected_symbols
                .iter()
                .any(|value| value.eq_ignore_ascii_case(symbol))
        });
    }
    if let Some(sector) = normalized_query(query.sector.as_deref()) {
        items.retain(|item| {
            item.event
                .impact
                .affected_sectors
                .iter()
                .any(|value| value.eq_ignore_ascii_case(sector))
        });
    }
    Ok(Json(items))
}

pub(super) async fn get_thread_contracts(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<OntologyObjectQuery>,
) -> Result<Json<Vec<ThreadContract>>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let mut items = snapshot.threads;
    if let Some(symbol) = normalized_query(query.symbol.as_deref()) {
        items.retain(|item| item.thread.symbol.eq_ignore_ascii_case(symbol));
    }
    Ok(Json(items))
}

pub(super) async fn get_workflow_contracts(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path(market): Path<String>,
) -> Result<Json<Vec<WorkflowContract>>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    Ok(Json(snapshot.workflows))
}

pub(super) async fn get_workflow_contract(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, workflow_id)): Path<(String, String)>,
) -> Result<Json<WorkflowContract>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let item = snapshot
        .workflow(&workflow_id)
        .cloned()
        .ok_or_else(|| ApiError::not_found(format!("workflow contract `{workflow_id}` not found")))?;
    Ok(Json(item))
}

pub(super) async fn get_symbol_state_contract(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, symbol)): Path<(String, String)>,
) -> Result<Json<SymbolStateContract>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let item = snapshot
        .symbol(&symbol)
        .cloned()
        .ok_or_else(|| ApiError::not_found(format!("symbol contract `{symbol}` not found")))?;
    Ok(Json(item))
}

pub(super) async fn get_case_contract(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, case_id)): Path<(String, String)>,
) -> Result<Json<CaseContract>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let item = snapshot
        .case(&case_id)
        .cloned()
        .ok_or_else(|| ApiError::not_found(format!("case contract `{case_id}` not found")))?;
    Ok(Json(item))
}

pub(super) async fn get_recommendation_contract(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, recommendation_id)): Path<(String, String)>,
) -> Result<Json<RecommendationContract>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let item = snapshot
        .recommendation(&recommendation_id)
        .cloned()
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "recommendation contract `{recommendation_id}` not found"
            ))
        })?;
    Ok(Json(item))
}

pub(super) async fn get_macro_event_contract(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, event_id)): Path<(String, String)>,
) -> Result<Json<MacroEventContract>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let item = snapshot
        .macro_event(&event_id)
        .cloned()
        .ok_or_else(|| ApiError::not_found(format!("macro event contract `{event_id}` not found")))?;
    Ok(Json(item))
}

pub(super) async fn get_thread_contract(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, thread_id)): Path<(String, String)>,
) -> Result<Json<ThreadContract>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let item = snapshot
        .thread(&thread_id)
        .cloned()
        .ok_or_else(|| ApiError::not_found(format!("thread contract `{thread_id}` not found")))?;
    Ok(Json(item))
}

pub(super) async fn get_sector_flow_sidecars(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<OntologyObjectQuery>,
) -> Result<Json<Vec<AgentSectorFlow>>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let mut items = snapshot.sidecars.sector_flows;
    if let Some(sector) = normalized_query(query.sector.as_deref()) {
        items.retain(|item| item.sector.eq_ignore_ascii_case(sector));
    }
    Ok(Json(items))
}

pub(super) async fn get_backward_investigation_sidecar(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, symbol)): Path<(String, String)>,
) -> Result<Json<BackwardInvestigation>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let item = snapshot
        .backward_investigation(&symbol)
        .cloned()
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "backward investigation sidecar for `{symbol}` not found"
            ))
        })?;
    Ok(Json(item))
}

fn normalized_query(raw: Option<&str>) -> Option<&str> {
    raw.map(str::trim).filter(|value| !value.is_empty())
}
