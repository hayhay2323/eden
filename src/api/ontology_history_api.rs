#[cfg(feature = "persistence")]
use axum::extract::State;
use axum::extract::{Path, Query};
use axum::Json;
use serde::Deserialize;
#[cfg(feature = "persistence")]
use time::OffsetDateTime;

#[cfg(feature = "persistence")]
use crate::action::workflow::{
    governance_reason, ActionExecutionPolicy, ActionGovernanceReasonCode, ActionStage,
};
use crate::agent::AgentRecommendationJournalRecord;
#[cfg(feature = "persistence")]
use crate::cases::{CaseReasoningAssessmentSnapshot, CaseWorkflowEvent};
#[cfg(feature = "persistence")]
use crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord;
#[cfg(feature = "persistence")]
use crate::persistence::discovered_archetype::DiscoveredArchetypeRecord;

use super::constants::{DEFAULT_LIMIT, MAX_LIMIT};
use super::core::{bounded, parse_case_market};
use super::foundation::ApiError;
#[cfg(feature = "persistence")]
use super::foundation::ApiState;
#[cfg(feature = "persistence")]
use super::ontology_api::load_enriched_contract_snapshot;
use super::ontology_history_support::{
    journal_matches_recommendation, load_recommendation_journal_records,
};

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
        .action_workflow_event_values(workflow_id)
        .await
        .map_err(|error| ApiError::internal(format!("failed to query workflow history: {error}")))?
        .into_iter()
        .map(case_workflow_event_from_value)
        .collect::<Result<Vec<_>, _>>()?;
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
        .action_workflow_event_values(&workflow_id)
        .await
        .map_err(|error| ApiError::internal(format!("failed to query workflow history: {error}")))?
        .into_iter()
        .map(case_workflow_event_from_value)
        .collect::<Result<Vec<_>, _>>()?;
    if events.len() > limit {
        events = events[events.len().saturating_sub(limit)..].to_vec();
    }
    Ok(Json(events))
}

#[cfg(feature = "persistence")]
pub(super) async fn get_discovered_archetypes(
    State(state): State<ApiState>,
    Path(market): Path<String>,
    Query(query): Query<OntologyHistoryQuery>,
) -> Result<Json<Vec<DiscoveredArchetypeRecord>>, ApiError> {
    let market = parse_case_market(&market)?;
    let market_key = match market {
        crate::cases::CaseMarket::Hk => "hk",
        crate::cases::CaseMarket::Us => "us",
    };
    let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
    let mut items = state
        .store
        .load_discovered_archetypes(market_key)
        .await
        .map_err(|error| {
            ApiError::internal(format!("failed to query discovered archetypes: {error}"))
        })?;
    items.truncate(limit);
    Ok(Json(items))
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn get_discovered_archetypes(
    Path(_market): Path<String>,
    Query(_query): Query<OntologyHistoryQuery>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    Err(ApiError::bad_request(
        "discovered archetypes require building with `--features persistence`",
    ))
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

#[cfg(feature = "persistence")]
fn case_workflow_event_from_value(event: serde_json::Value) -> Result<CaseWorkflowEvent, ApiError> {
    let workflow_id = get_required_string(&event, "workflow_id")?;
    let to_stage = parse_action_stage(get_required_string(&event, "to_stage")?.as_str())?;
    let from_stage = get_optional_string(&event, "from_stage")
        .map(|value| parse_action_stage(value.as_str()))
        .transpose()?
        .map(|stage| stage.as_str().to_string());
    let execution_policy = get_optional_string(&event, "execution_policy")
        .map(|value| parse_execution_policy(value.as_str()))
        .transpose()?
        .unwrap_or(ActionExecutionPolicy::ReviewRequired);
    let governance_reason_code = get_optional_string(&event, "governance_reason_code")
        .map(|value| parse_governance_reason_code(value.as_str()))
        .transpose()?
        .unwrap_or(ActionGovernanceReasonCode::WorkflowTransitionWindow);
    let timestamp = parse_recorded_at(&event)?;
    let governance_reason = governance_reason(Some(to_stage), execution_policy);
    let operator_decision_kind = event
        .get("payload")
        .and_then(|payload| payload.get("operator_decision"))
        .and_then(|value| value.get("kind"))
        .and_then(|value| value.as_str())
        .map(|value| value.to_string());
    Ok(CaseWorkflowEvent {
        workflow_id,
        stage: to_stage.as_str().to_string(),
        from_stage,
        operator_decision_kind,
        execution_policy,
        governance_reason_code,
        governance_reason,
        timestamp,
        actor: get_optional_string(&event, "actor"),
        owner: get_optional_string(&event, "owner"),
        reviewer: get_optional_string(&event, "reviewer"),
        queue_pin: get_optional_string(&event, "queue_pin"),
        note: get_optional_string(&event, "note"),
    })
}

#[cfg(feature = "persistence")]
fn get_required_string(event: &serde_json::Value, field: &str) -> Result<String, ApiError> {
    get_optional_string(event, field).ok_or_else(|| {
        ApiError::internal(format!(
            "failed to decode workflow history: missing field `{field}`"
        ))
    })
}

#[cfg(feature = "persistence")]
fn get_optional_string(event: &serde_json::Value, field: &str) -> Option<String> {
    event
        .get(field)
        .and_then(|value| value.as_str())
        .map(|value| value.to_string())
}

#[cfg(feature = "persistence")]
fn parse_recorded_at(event: &serde_json::Value) -> Result<OffsetDateTime, ApiError> {
    let raw = get_required_string(event, "recorded_at")?;
    OffsetDateTime::parse(raw.as_str(), &time::format_description::well_known::Rfc3339).map_err(
        |error| {
            ApiError::internal(format!(
                "failed to decode workflow history timestamp: {error}"
            ))
        },
    )
}

#[cfg(feature = "persistence")]
fn parse_action_stage(value: &str) -> Result<ActionStage, ApiError> {
    match value {
        "suggest" => Ok(ActionStage::Suggest),
        "confirm" => Ok(ActionStage::Confirm),
        "execute" => Ok(ActionStage::Execute),
        "monitor" => Ok(ActionStage::Monitor),
        "review" => Ok(ActionStage::Review),
        _ => Err(ApiError::internal(format!(
            "failed to decode workflow history: unknown stage `{value}`"
        ))),
    }
}

#[cfg(feature = "persistence")]
fn parse_execution_policy(value: &str) -> Result<ActionExecutionPolicy, ApiError> {
    match value {
        "manual_only" => Ok(ActionExecutionPolicy::ManualOnly),
        "review_required" => Ok(ActionExecutionPolicy::ReviewRequired),
        "auto_eligible" => Ok(ActionExecutionPolicy::AutoEligible),
        _ => Err(ApiError::internal(format!(
            "failed to decode workflow history: unknown execution policy `{value}`"
        ))),
    }
}

#[cfg(feature = "persistence")]
fn parse_governance_reason_code(value: &str) -> Result<ActionGovernanceReasonCode, ApiError> {
    match value {
        "workflow_not_created" => Ok(ActionGovernanceReasonCode::WorkflowNotCreated),
        "workflow_transition_window" => Ok(ActionGovernanceReasonCode::WorkflowTransitionWindow),
        "assignment_locked_during_execution" => {
            Ok(ActionGovernanceReasonCode::AssignmentLockedDuringExecution)
        }
        "terminal_review_stage" => Ok(ActionGovernanceReasonCode::TerminalReviewStage),
        "advisory_action" => Ok(ActionGovernanceReasonCode::AdvisoryAction),
        "operator_action_required" => Ok(ActionGovernanceReasonCode::OperatorActionRequired),
        "severity_requires_review" => Ok(ActionGovernanceReasonCode::SeverityRequiresReview),
        "invalidation_rule_missing" => Ok(ActionGovernanceReasonCode::InvalidationRuleMissing),
        "non_positive_expected_alpha" => Ok(ActionGovernanceReasonCode::NonPositiveExpectedAlpha),
        "auto_execution_eligible" => Ok(ActionGovernanceReasonCode::AutoExecutionEligible),
        _ => Err(ApiError::internal(format!(
            "failed to decode workflow history: unknown governance code `{value}`"
        ))),
    }
}
