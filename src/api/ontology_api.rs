#[cfg(feature = "persistence")]
use axum::extract::State;
use axum::extract::{Path, Query};
use axum::Json;
use serde::Deserialize;

use crate::agent;
use crate::agent::llm;
use crate::agent::AgentSectorFlow;
use crate::cases;
#[cfg(feature = "persistence")]
use crate::cases::enrich_case_summaries;
#[cfg(feature = "persistence")]
use crate::ontology::rebuild_operational_case_graph;
use crate::ontology::world::BackwardInvestigation;
use crate::ontology::{
    build_operational_snapshot, load_operational_snapshot, AttentionAllocationContract,
    CaseContract, MacroEventContract, MarketSessionContract, OperationalNavigation,
    OperationalNeighborhood, OperationalObjectKind, OperationalSnapshot, OperatorWorkItem,
    OrganOverviewContract, PerceptualEvidenceContract, PerceptualExpectationContract,
    PerceptualStateContract, PerceptualUncertaintyContract, RecommendationContract,
    SymbolStateContract, ThreadContract, WorkflowContract,
};

use super::core::parse_case_market;
use super::foundation::ApiError;
#[cfg(feature = "persistence")]
use super::foundation::ApiState;
#[cfg(feature = "persistence")]
use super::ontology_history_enrichment::enrich_history_refs;

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
    let narration = llm::load_narration(market).await.ok();

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
        .map_err(|error| {
            ApiError::internal(format!("failed to enrich operational cases: {error}"))
        })?;
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
    enrich_history_refs(state, market, &mut snapshot).await?;
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

pub(super) async fn get_organ_overview(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path(market): Path<String>,
) -> Result<Json<OrganOverviewContract>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    Ok(Json(snapshot.organ_overview()))
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

pub(super) async fn get_perceptual_states(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<OntologyObjectQuery>,
) -> Result<Json<Vec<PerceptualStateContract>>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let mut items = snapshot.perceptual_states;
    if let Some(symbol) = normalized_query(query.symbol.as_deref()) {
        items.retain(|item| item.symbol.eq_ignore_ascii_case(symbol));
    }
    if let Some(sector) = normalized_query(query.sector.as_deref()) {
        items.retain(|item| {
            item.sector
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case(sector))
                .unwrap_or(false)
                || item.state.cluster_label.eq_ignore_ascii_case(sector)
        });
    }
    Ok(Json(items))
}

pub(super) async fn get_perceptual_evidence(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<OntologyObjectQuery>,
) -> Result<Json<Vec<PerceptualEvidenceContract>>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let mut items = all_perceptual_evidence(&snapshot);
    if let Some(symbol) = normalized_query(query.symbol.as_deref()) {
        items.retain(|item| item.symbol.eq_ignore_ascii_case(symbol));
    }
    Ok(Json(items))
}

pub(super) async fn get_perceptual_expectations(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<OntologyObjectQuery>,
) -> Result<Json<Vec<PerceptualExpectationContract>>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let mut items = all_perceptual_expectations(&snapshot);
    if let Some(symbol) = normalized_query(query.symbol.as_deref()) {
        items.retain(|item| item.symbol.eq_ignore_ascii_case(symbol));
    }
    Ok(Json(items))
}

pub(super) async fn get_attention_allocations(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<OntologyObjectQuery>,
) -> Result<Json<Vec<AttentionAllocationContract>>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let mut items = all_attention_allocations(&snapshot);
    if let Some(symbol) = normalized_query(query.symbol.as_deref()) {
        items.retain(|item| item.symbol.eq_ignore_ascii_case(symbol));
    }
    Ok(Json(items))
}

pub(super) async fn get_perceptual_uncertainties(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<OntologyObjectQuery>,
) -> Result<Json<Vec<PerceptualUncertaintyContract>>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let mut items = all_perceptual_uncertainties(&snapshot);
    if let Some(symbol) = normalized_query(query.symbol.as_deref()) {
        items.retain(|item| item.symbol.eq_ignore_ascii_case(symbol));
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

pub(super) async fn get_workflow_contract(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, workflow_id)): Path<(String, String)>,
) -> Result<Json<WorkflowContract>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let item = snapshot.workflow(&workflow_id).cloned().ok_or_else(|| {
        ApiError::not_found(format!("workflow contract `{workflow_id}` not found"))
    })?;
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

pub(super) async fn get_perceptual_state(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, state_id)): Path<(String, String)>,
) -> Result<Json<PerceptualStateContract>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let item = snapshot
        .perceptual_state(&state_id)
        .cloned()
        .ok_or_else(|| ApiError::not_found(format!("perceptual state `{state_id}` not found")))?;
    Ok(Json(item))
}

pub(super) async fn get_perceptual_evidence_detail(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, evidence_id)): Path<(String, String)>,
) -> Result<Json<PerceptualEvidenceContract>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    snapshot
        .perceptual_evidence(&evidence_id)
        .cloned()
        .map(Json)
        .ok_or_else(|| {
            ApiError::not_found(format!("perceptual evidence `{evidence_id}` not found"))
        })
}

pub(super) async fn get_perceptual_expectation_detail(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, expectation_id)): Path<(String, String)>,
) -> Result<Json<PerceptualExpectationContract>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    snapshot
        .perceptual_expectation(&expectation_id)
        .cloned()
        .map(Json)
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "perceptual expectation `{expectation_id}` not found"
            ))
        })
}

pub(super) async fn get_attention_allocation_detail(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, allocation_id)): Path<(String, String)>,
) -> Result<Json<AttentionAllocationContract>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    snapshot
        .attention_allocation(&allocation_id)
        .cloned()
        .map(Json)
        .ok_or_else(|| {
            ApiError::not_found(format!("attention allocation `{allocation_id}` not found"))
        })
}

pub(super) async fn get_perceptual_uncertainty_detail(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, uncertainty_id)): Path<(String, String)>,
) -> Result<Json<PerceptualUncertaintyContract>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    snapshot
        .perceptual_uncertainty(&uncertainty_id)
        .cloned()
        .map(Json)
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "perceptual uncertainty `{uncertainty_id}` not found"
            ))
        })
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
    let item = snapshot.macro_event(&event_id).cloned().ok_or_else(|| {
        ApiError::not_found(format!("macro event contract `{event_id}` not found"))
    })?;
    Ok(Json(item))
}

pub(super) async fn get_symbol_perceptual_state(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, symbol)): Path<(String, String)>,
) -> Result<Json<PerceptualStateContract>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let item = snapshot
        .perceptual_states
        .iter()
        .find(|item| item.symbol.eq_ignore_ascii_case(&symbol))
        .cloned()
        .ok_or_else(|| ApiError::not_found(format!("perceptual state `{symbol}` not found")))?;
    Ok(Json(item))
}

pub(super) async fn get_symbol_perceptual_evidence(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, symbol)): Path<(String, String)>,
) -> Result<Json<Vec<PerceptualEvidenceContract>>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let mut items = all_perceptual_evidence(&snapshot);
    items.retain(|item| item.symbol.eq_ignore_ascii_case(&symbol));
    Ok(Json(items))
}

pub(super) async fn get_symbol_perceptual_evidence_detail(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, symbol, evidence_id)): Path<(String, String, String)>,
) -> Result<Json<PerceptualEvidenceContract>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    all_perceptual_evidence(&snapshot)
        .into_iter()
        .find(|item| {
            item.symbol.eq_ignore_ascii_case(&symbol) && item.id.eq_ignore_ascii_case(&evidence_id)
        })
        .map(Json)
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "perceptual evidence `{evidence_id}` for `{symbol}` not found"
            ))
        })
}

pub(super) async fn get_symbol_perceptual_expectations(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, symbol)): Path<(String, String)>,
) -> Result<Json<Vec<PerceptualExpectationContract>>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let mut items = all_perceptual_expectations(&snapshot);
    items.retain(|item| item.symbol.eq_ignore_ascii_case(&symbol));
    Ok(Json(items))
}

pub(super) async fn get_symbol_perceptual_expectation_detail(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, symbol, expectation_id)): Path<(String, String, String)>,
) -> Result<Json<PerceptualExpectationContract>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    all_perceptual_expectations(&snapshot)
        .into_iter()
        .find(|item| {
            item.symbol.eq_ignore_ascii_case(&symbol)
                && item.id.eq_ignore_ascii_case(&expectation_id)
        })
        .map(Json)
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "perceptual expectation `{expectation_id}` for `{symbol}` not found"
            ))
        })
}

pub(super) async fn get_symbol_perceptual_attention(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, symbol)): Path<(String, String)>,
) -> Result<Json<Vec<AttentionAllocationContract>>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let mut items = all_attention_allocations(&snapshot);
    items.retain(|item| item.symbol.eq_ignore_ascii_case(&symbol));
    Ok(Json(items))
}

pub(super) async fn get_symbol_perceptual_attention_detail(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, symbol, allocation_id)): Path<(String, String, String)>,
) -> Result<Json<AttentionAllocationContract>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    all_attention_allocations(&snapshot)
        .into_iter()
        .find(|item| {
            item.symbol.eq_ignore_ascii_case(&symbol)
                && item.id.eq_ignore_ascii_case(&allocation_id)
        })
        .map(Json)
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "attention allocation `{allocation_id}` for `{symbol}` not found"
            ))
        })
}

pub(super) async fn get_symbol_perceptual_uncertainty(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, symbol)): Path<(String, String)>,
) -> Result<Json<Vec<PerceptualUncertaintyContract>>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let mut items = all_perceptual_uncertainties(&snapshot);
    items.retain(|item| item.symbol.eq_ignore_ascii_case(&symbol));
    Ok(Json(items))
}

pub(super) async fn get_symbol_perceptual_uncertainty_detail(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, symbol, uncertainty_id)): Path<(String, String, String)>,
) -> Result<Json<PerceptualUncertaintyContract>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    all_perceptual_uncertainties(&snapshot)
        .into_iter()
        .find(|item| {
            item.symbol.eq_ignore_ascii_case(&symbol)
                && item.id.eq_ignore_ascii_case(&uncertainty_id)
        })
        .map(Json)
        .ok_or_else(|| {
            ApiError::not_found(format!(
                "perceptual uncertainty `{uncertainty_id}` for `{symbol}` not found"
            ))
        })
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

pub(super) async fn get_operational_neighborhood(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, kind, id)): Path<(String, String, String)>,
) -> Result<Json<OperationalNeighborhood>, ApiError> {
    let market = parse_case_market(&market)?;
    let kind = OperationalObjectKind::parse(&kind)
        .ok_or_else(|| ApiError::bad_request(format!("unsupported object kind `{kind}`")))?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let neighborhood = snapshot.neighborhood(kind, &id).ok_or_else(|| {
        ApiError::not_found(format!("object neighborhood `{kind:?}:{id}` not found"))
    })?;
    Ok(Json(neighborhood))
}

pub(super) async fn get_operational_navigation(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path((market, kind, id)): Path<(String, String, String)>,
) -> Result<Json<OperationalNavigation>, ApiError> {
    let market = parse_case_market(&market)?;
    let kind = OperationalObjectKind::parse(&kind)
        .ok_or_else(|| ApiError::bad_request(format!("unsupported object kind `{kind}`")))?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let navigation = snapshot.navigation(kind, &id).cloned().ok_or_else(|| {
        ApiError::not_found(format!("object navigation `{kind:?}:{id}` not found"))
    })?;
    Ok(Json(navigation))
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

pub(super) async fn get_operator_work_item_sidecars(
    #[cfg(feature = "persistence")] State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<OntologyObjectQuery>,
) -> Result<Json<Vec<OperatorWorkItem>>, ApiError> {
    let market = parse_case_market(&market)?;
    #[cfg(feature = "persistence")]
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    #[cfg(not(feature = "persistence"))]
    let snapshot = load_contract_snapshot(market).await?;
    let mut items = snapshot.sidecars.operator_work_items;
    if let Some(symbol) = normalized_query(query.symbol.as_deref()) {
        items.retain(|item| {
            item.symbol
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case(symbol))
                .unwrap_or(false)
        });
    }
    if let Some(action) = normalized_query(query.action.as_deref()) {
        items.retain(|item| {
            item.best_action
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case(action))
                .unwrap_or(false)
        });
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

fn all_perceptual_evidence(snapshot: &OperationalSnapshot) -> Vec<PerceptualEvidenceContract> {
    snapshot.perceptual_evidence.clone()
}

fn all_perceptual_expectations(
    snapshot: &OperationalSnapshot,
) -> Vec<PerceptualExpectationContract> {
    snapshot.perceptual_expectations.clone()
}

fn all_attention_allocations(snapshot: &OperationalSnapshot) -> Vec<AttentionAllocationContract> {
    snapshot.attention_allocations.clone()
}

fn all_perceptual_uncertainties(
    snapshot: &OperationalSnapshot,
) -> Vec<PerceptualUncertaintyContract> {
    snapshot.perceptual_uncertainties.clone()
}
