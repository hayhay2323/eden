use axum::extract::{Path, Query};
#[cfg(feature = "persistence")]
use axum::extract::State;
use axum::Json;
use serde::Deserialize;

use crate::agent::{self, AgentDecision, AgentRecommendationJournalRecord};
#[cfg(feature = "persistence")]
use crate::cases::{CaseReasoningAssessmentSnapshot, CaseWorkflowEvent};
#[cfg(feature = "persistence")]
use crate::persistence::action_workflow::ActionWorkflowEventRecord;
#[cfg(feature = "persistence")]
use crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord;

use super::constants::{DEFAULT_LIMIT, MAX_LIMIT};
use super::core::{bounded, parse_case_market};
#[cfg(feature = "persistence")]
use super::foundation::ApiState;
use super::foundation::ApiError;
#[cfg(feature = "persistence")]
use super::ontology_api::load_enriched_contract_snapshot;

#[derive(Debug, Deserialize, Default, Clone)]
pub(super) struct OntologyHistoryQuery {
    limit: Option<usize>,
}

pub(super) async fn get_recommendation_journal_history(
    Path((market, recommendation_id)): Path<(String, String)>,
    Query(query): Query<OntologyHistoryQuery>,
) -> Result<Json<Vec<AgentRecommendationJournalRecord>>, ApiError> {
    let market = parse_case_market(&market)?;
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let mut rows = load_recommendation_journal_records(market).await?;
    rows.retain(|item| journal_matches_recommendation(item, &recommendation_id));
    if rows.len() > limit {
        rows = rows[rows.len().saturating_sub(limit)..].to_vec();
    }
    Ok(Json(rows))
}

#[cfg(feature = "persistence")]
pub(super) async fn get_case_workflow_history(
    State(state): State<ApiState>,
    Path((market, case_id)): Path<(String, String)>,
    Query(query): Query<OntologyHistoryQuery>,
) -> Result<Json<Vec<CaseWorkflowEvent>>, ApiError> {
    let market = parse_case_market(&market)?;
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    let case = snapshot
        .case(&case_id)
        .ok_or_else(|| ApiError::not_found(format!("case contract `{case_id}` not found")))?;
    let Some(workflow_id) = case.workflow_id.as_ref() else {
        return Ok(Json(Vec::new()));
    };
    let mut events = state
        .store
        .action_workflow_events(workflow_id)
        .await
        .map_err(|error| ApiError::internal(format!("failed to query workflow history: {error}")))?
        .into_iter()
        .map(case_workflow_event_from_record)
        .collect::<Vec<_>>();
    if events.len() > limit {
        events = events[events.len().saturating_sub(limit)..].to_vec();
    }
    Ok(Json(events))
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_case_workflow_history(
    Path((_market, _case_id)): Path<(String, String)>,
    Query(_query): Query<OntologyHistoryQuery>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    Err(ApiError::bad_request(
        "case workflow history requires building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
pub(super) async fn get_case_reasoning_history(
    State(state): State<ApiState>,
    Path((market, case_id)): Path<(String, String)>,
    Query(query): Query<OntologyHistoryQuery>,
) -> Result<Json<Vec<CaseReasoningAssessmentSnapshot>>, ApiError> {
    let market = parse_case_market(&market)?;
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    let case = snapshot
        .case(&case_id)
        .ok_or_else(|| ApiError::not_found(format!("case contract `{case_id}` not found")))?;
    let items = state
        .store
        .recent_case_reasoning_assessments(&case.setup_id, limit)
        .await
        .map_err(|error| {
            ApiError::internal(format!("failed to query case reasoning history: {error}"))
        })?
        .into_iter()
        .map(CaseReasoningAssessmentSnapshot::from_record)
        .collect::<Vec<_>>();
    Ok(Json(items))
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_case_reasoning_history(
    Path((_market, _case_id)): Path<(String, String)>,
    Query(_query): Query<OntologyHistoryQuery>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    Err(ApiError::bad_request(
        "case reasoning history requires building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
pub(super) async fn get_case_outcome_history(
    State(state): State<ApiState>,
    Path((market, case_id)): Path<(String, String)>,
    Query(query): Query<OntologyHistoryQuery>,
) -> Result<Json<Vec<CaseRealizedOutcomeRecord>>, ApiError> {
    let market = parse_case_market(&market)?;
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let snapshot = load_enriched_contract_snapshot(&state, market).await?;
    let case = snapshot
        .case(&case_id)
        .ok_or_else(|| ApiError::not_found(format!("case contract `{case_id}` not found")))?;
    let items = state
        .store
        .recent_case_realized_outcomes(&case.setup_id, limit)
        .await
        .map_err(|error| {
            ApiError::internal(format!("failed to query case outcome history: {error}"))
        })?;
    Ok(Json(items))
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_case_outcome_history(
    Path((_market, _case_id)): Path<(String, String)>,
    Query(_query): Query<OntologyHistoryQuery>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    Err(ApiError::bad_request(
        "case outcome history requires building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
pub(super) async fn get_workflow_event_history(
    State(state): State<ApiState>,
    Path((market, workflow_id)): Path<(String, String)>,
    Query(query): Query<OntologyHistoryQuery>,
) -> Result<Json<Vec<CaseWorkflowEvent>>, ApiError> {
    let _ = parse_case_market(&market)?;
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let mut events = state
        .store
        .action_workflow_events(&workflow_id)
        .await
        .map_err(|error| ApiError::internal(format!("failed to query workflow history: {error}")))?
        .into_iter()
        .map(case_workflow_event_from_record)
        .collect::<Vec<_>>();
    if events.len() > limit {
        events = events[events.len().saturating_sub(limit)..].to_vec();
    }
    Ok(Json(events))
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_workflow_event_history(
    Path((_market, _workflow_id)): Path<(String, String)>,
    Query(_query): Query<OntologyHistoryQuery>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    Err(ApiError::bad_request(
        "workflow history requires building with `--features persistence`",
    ))
}

async fn load_recommendation_journal_records(
    market: crate::cases::CaseMarket,
) -> Result<Vec<AgentRecommendationJournalRecord>, ApiError> {
    let (env_var, default_path) = agent::load_recommendation_journal_path(market);
    let path = std::env::var(env_var).unwrap_or_else(|_| default_path.to_string());
    let content = match tokio::fs::read_to_string(&path).await {
        Ok(content) => content,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => {
            return Err(ApiError::internal(format!(
                "failed to load recommendation journal: {error}"
            )))
        }
    };

    Ok(content
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<AgentRecommendationJournalRecord>(line).ok())
        .collect())
}

fn journal_matches_recommendation(
    row: &AgentRecommendationJournalRecord,
    recommendation_id: &str,
) -> bool {
    row.market_recommendation
        .as_ref()
        .map(|item| item.recommendation_id.eq_ignore_ascii_case(recommendation_id))
        .unwrap_or(false)
        || row
            .decisions
            .iter()
            .any(|item| decision_matches_recommendation(item, recommendation_id))
        || row
            .items
            .iter()
            .any(|item| item.recommendation_id.eq_ignore_ascii_case(recommendation_id))
}

fn decision_matches_recommendation(
    decision: &AgentDecision,
    recommendation_id: &str,
) -> bool {
    match decision {
        AgentDecision::Market(item) => item.recommendation_id.eq_ignore_ascii_case(recommendation_id),
        AgentDecision::Sector(item) => item.recommendation_id.eq_ignore_ascii_case(recommendation_id),
        AgentDecision::Symbol(item) => item.recommendation_id.eq_ignore_ascii_case(recommendation_id),
    }
}

#[cfg(feature = "persistence")]
fn case_workflow_event_from_record(
    event: ActionWorkflowEventRecord,
) -> CaseWorkflowEvent {
    CaseWorkflowEvent {
        workflow_id: event.workflow_id,
        stage: event.to_stage.as_str().to_string(),
        from_stage: event.from_stage.map(|stage| stage.as_str().to_string()),
        execution_policy: event.execution_policy,
        governance_reason_code: event.governance_reason_code,
        governance_reason: event.governance_reason(),
        timestamp: event.recorded_at,
        actor: event.actor,
        owner: event.owner,
        reviewer: event.reviewer,
        queue_pin: event.queue_pin,
        note: event.note,
    }
}
