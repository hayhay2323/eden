#[cfg(feature = "persistence")]
use axum::extract::{Path, State};
use axum::Json;
#[cfg(feature = "persistence")]
use serde::Deserialize;
#[cfg(feature = "persistence")]
use time::OffsetDateTime;

#[cfg(feature = "persistence")]
use crate::action::workflow as action_workflow;
#[cfg(feature = "persistence")]
use crate::action::workflow::{ActionExecutionPolicy, ActionStage};
#[cfg(feature = "persistence")]
use crate::cases::enrich_case_detail;
#[cfg(feature = "persistence")]
use crate::cases::{
    build_case_detail, build_case_summaries, load_snapshot, CaseMarket, CaseSummary,
};
#[cfg(feature = "persistence")]
use crate::cases::{workflow_record_payload, CaseWorkflowState};
#[cfg(feature = "persistence")]
use crate::ontology::contracts::market_slug;
#[cfg(feature = "persistence")]
use crate::persistence::case_reasoning_assessment::CaseReasoningAssessmentRecord;
#[cfg(feature = "persistence")]
use crate::persistence::store::EdenStore;

#[cfg(feature = "persistence")]
use super::core::parse_case_market;
use super::foundation::ApiError;
#[cfg(feature = "persistence")]
use super::foundation::ApiState;

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize)]
pub(super) struct CaseTransitionBody {
    pub(super) target_stage: String,
    pub(super) actor: Option<String>,
    pub(super) note: Option<String>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize)]
pub(super) struct CaseAssignBody {
    #[serde(default)]
    pub(super) owner: Option<Option<String>>,
    #[serde(default)]
    pub(super) reviewer: Option<Option<String>>,
    #[serde(default)]
    pub(super) queue_pin: Option<Option<String>>,
    pub(super) actor: Option<String>,
    pub(super) note: Option<String>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Deserialize)]
pub(super) struct CaseQueuePinBody {
    pub(super) pinned: bool,
    #[serde(default)]
    pub(super) label: Option<String>,
    pub(super) actor: Option<String>,
    pub(super) note: Option<String>,
}

#[cfg(feature = "persistence")]
fn normalize_optional_string(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
}

#[cfg(feature = "persistence")]
fn normalize_assignment_update(value: Option<Option<String>>) -> Option<Option<String>> {
    value.map(|next| next.and_then(|value| normalize_optional_string(Some(value))))
}

#[cfg(feature = "persistence")]
fn assignment_note(
    owner: Option<&Option<String>>,
    reviewer: Option<&Option<String>>,
    queue_pin: Option<&Option<String>>,
) -> Option<String> {
    let mut parts = Vec::new();

    match owner {
        Some(Some(owner)) => parts.push(format!("assigned owner -> {owner}")),
        Some(None) => parts.push("owner cleared".to_string()),
        None => {}
    }

    match reviewer {
        Some(Some(reviewer)) => parts.push(format!("assigned reviewer -> {reviewer}")),
        Some(None) => parts.push("reviewer cleared".to_string()),
        None => {}
    }

    match queue_pin {
        Some(Some(queue_pin)) => parts.push(format!("queue pin -> {queue_pin}")),
        Some(None) => parts.push("queue pin cleared".to_string()),
        None => {}
    }

    if parts.is_empty() {
        None
    } else {
        Some(parts.join(", "))
    }
}

#[cfg(feature = "persistence")]
fn default_queue_pin_label(label: Option<String>) -> Option<String> {
    normalize_optional_string(label).or(Some("frontend-review-list".into()))
}

#[cfg(feature = "persistence")]
fn operator_decision_kind_for_assignment(
    owner: Option<&Option<String>>,
    reviewer: Option<&Option<String>>,
    queue_pin: Option<&Option<String>>,
) -> &'static str {
    if queue_pin.is_some() {
        "queue_pin"
    } else if owner.is_some() || reviewer.is_some() {
        "assign"
    } else {
        "annotate"
    }
}

#[cfg(feature = "persistence")]
fn operator_decision_kind_for_transition(target_stage: ActionStage) -> &'static str {
    match target_stage {
        ActionStage::Suggest => "defer",
        ActionStage::Confirm | ActionStage::Execute | ActionStage::Monitor => "accept",
        ActionStage::Review => "reject",
    }
}

#[cfg(feature = "persistence")]
fn payload_with_operator_decision(
    mut payload: serde_json::Value,
    kind: &'static str,
    actor: Option<&str>,
    note: Option<&str>,
) -> serde_json::Value {
    if let Some(object) = payload.as_object_mut() {
        object.insert(
            "operator_decision".into(),
            serde_json::json!({
                "kind": kind,
                "actor": actor,
                "note": note,
            }),
        );
    }
    payload
}

#[cfg(feature = "persistence")]
#[derive(Debug, Clone)]
struct CaseWorkflowSeed {
    setup_id: String,
    workflow_id: String,
    title: String,
    payload: serde_json::Value,
    execution_policy: ActionExecutionPolicy,
}

#[cfg(feature = "persistence")]
async fn resolve_case_workflow_seed(
    store: &EdenStore,
    market: CaseMarket,
    setup_id: &str,
) -> Result<CaseWorkflowSeed, ApiError> {
    if let Some(setup) = store
        .tactical_setup_by_id(setup_id)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load tactical setup: {error}")))?
    {
        return Ok(CaseWorkflowSeed {
            setup_id: setup.setup_id.clone(),
            workflow_id: setup.workflow_id.clone().unwrap_or_else(|| {
                crate::persistence::action_workflow::synthetic_workflow_id_for_setup(
                    &setup.setup_id,
                )
            }),
            title: setup.title.clone(),
            payload: workflow_record_payload(&setup),
            execution_policy: ActionExecutionPolicy::ReviewRequired,
        });
    }

    let live_snapshot = load_snapshot(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load case snapshot: {error}")))?;
    let case = build_case_summaries(&live_snapshot)
        .into_iter()
        .find(|item| item.setup_id == setup_id)
        .ok_or_else(|| ApiError::not_found(format!("case `{setup_id}` not found")))?;
    Ok(case_workflow_seed_from_summary(case))
}

#[cfg(feature = "persistence")]
fn case_workflow_seed_from_summary(case: CaseSummary) -> CaseWorkflowSeed {
    let workflow_id = case.workflow_id.clone().unwrap_or_else(|| {
        crate::persistence::action_workflow::synthetic_workflow_id_for_setup(&case.setup_id)
    });
    let payload = serde_json::json!({
        "setup_id": case.setup_id,
        "case_id": case.case_id,
        "symbol": case.symbol,
        "title": case.title,
        "market": market_slug(case.market),
        "recommended_action": case.recommended_action,
        "workflow_state": case.workflow_state,
        "governance_reason_code": case.governance_reason_code.map(|value| value.as_str().to_string()),
        "governance_reason": case.governance_reason,
        "why_now": case.why_now,
        "primary_lens": case.primary_lens,
        "family_label": case.family_label,
    });

    CaseWorkflowSeed {
        setup_id: case.setup_id,
        workflow_id,
        title: case.title,
        payload,
        execution_policy: case
            .execution_policy
            .unwrap_or(ActionExecutionPolicy::ReviewRequired),
    }
}

#[cfg(feature = "persistence")]
async fn apply_case_assign_update(
    state: &ApiState,
    market: CaseMarket,
    setup_id: &str,
    body: CaseAssignBody,
) -> Result<Json<CaseWorkflowState>, ApiError> {
    let store = &state.store;
    let seed = resolve_case_workflow_seed(store, market, setup_id).await?;
    let workflow_id = seed.workflow_id.clone();
    let current = store
        .action_workflow_by_id(&workflow_id)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load workflow: {error}")))?;
    action_workflow::validate_assignment_update(
        current.as_ref().map(|record| record.current_stage),
    )
    .map_err(|error| ApiError::bad_request(error.to_string()))?;

    let timestamp = OffsetDateTime::now_utc();
    let actor = normalize_optional_string(body.actor.clone()).or(Some("frontend".into()));
    let requested_owner = normalize_assignment_update(body.owner.clone());
    let requested_reviewer = normalize_assignment_update(body.reviewer.clone());
    let requested_queue_pin = normalize_assignment_update(body.queue_pin.clone());
    action_workflow::validate_queue_pin_update(
        current
            .as_ref()
            .and_then(|record| record.queue_pin.as_deref()),
        requested_queue_pin.as_ref(),
        actor.as_deref(),
    )
    .map_err(|error| ApiError::bad_request(error.to_string()))?;
    let owner = match requested_owner.as_ref() {
        Some(next) => next.clone(),
        None => current.as_ref().and_then(|record| record.owner.clone()),
    };
    let reviewer = match requested_reviewer.as_ref() {
        Some(next) => next.clone(),
        None => current.as_ref().and_then(|record| record.reviewer.clone()),
    };
    let queue_pin = match requested_queue_pin.as_ref() {
        Some(next) => next.clone(),
        None => current.as_ref().and_then(|record| record.queue_pin.clone()),
    };
    let note = body.note.clone().or_else(|| {
        assignment_note(
            requested_owner.as_ref(),
            requested_reviewer.as_ref(),
            requested_queue_pin.as_ref(),
        )
    });
    let stage = current
        .as_ref()
        .map(|record| record.current_stage)
        .unwrap_or(ActionStage::Suggest);
    let title = current
        .as_ref()
        .map(|record| record.title.clone())
        .unwrap_or_else(|| seed.title.clone());
    let payload = current
        .as_ref()
        .map(|record| record.payload.clone())
        .unwrap_or_else(|| seed.payload.clone());
    let payload = payload_with_operator_decision(
        payload,
        operator_decision_kind_for_assignment(
            requested_owner.as_ref(),
            requested_reviewer.as_ref(),
            requested_queue_pin.as_ref(),
        ),
        actor.as_deref(),
        note.as_deref(),
    );
    let execution_policy = current
        .as_ref()
        .map(|item| item.execution_policy)
        .unwrap_or(seed.execution_policy);

    let record = crate::persistence::action_workflow::ActionWorkflowRecord {
        workflow_id: workflow_id.clone(),
        title: title.clone(),
        payload: payload.clone(),
        current_stage: stage,
        execution_policy,
        governance_reason_code: crate::action::workflow::governance_reason_code(
            Some(stage),
            execution_policy,
        ),
        recorded_at: timestamp,
        actor: actor.clone(),
        owner: owner.clone(),
        reviewer: reviewer.clone(),
        queue_pin: queue_pin.clone(),
        note: note.clone(),
    };
    let event = crate::persistence::action_workflow::ActionWorkflowEventRecord {
        event_id: crate::persistence::action_workflow::event_id_for(&workflow_id, stage, timestamp),
        workflow_id: workflow_id.clone(),
        title,
        payload,
        from_stage: current.as_ref().map(|item| item.current_stage),
        to_stage: stage,
        execution_policy,
        governance_reason_code: crate::action::workflow::governance_reason_code(
            Some(stage),
            execution_policy,
        ),
        recorded_at: timestamp,
        actor: actor.clone(),
        owner: owner.clone(),
        reviewer: reviewer.clone(),
        queue_pin: queue_pin.clone(),
        note: note.clone(),
    };

    store
        .write_action_workflow_state_and_event(&record, &event)
        .await
        .map_err(|error| {
            ApiError::internal(format!(
                "failed to write assignment workflow update: {error}"
            ))
        })?;
    persist_reasoning_assessment_snapshot(
        store,
        market,
        &seed.setup_id,
        timestamp,
        "workflow_update",
    )
    .await;

    Ok(Json(CaseWorkflowState {
        workflow_id,
        stage: stage.as_str().to_string(),
        execution_policy: record.execution_policy,
        governance: record.governance_contract(),
        governance_reason_code: record.governance_reason_code,
        governance_reason: record.governance_reason(),
        timestamp,
        actor,
        owner,
        reviewer,
        queue_pin,
        note,
    }))
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn post_case_assign() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "case assignment requires building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
pub(super) async fn post_case_assign(
    State(state): State<ApiState>,
    Path((market, setup_id)): Path<(String, String)>,
    Json(body): Json<CaseAssignBody>,
) -> Result<Json<CaseWorkflowState>, ApiError> {
    let market = parse_case_market(&market)?;
    apply_case_assign_update(&state, market, &setup_id, body).await
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn post_case_queue_pin() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "queue pin updates require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
pub(super) async fn post_case_queue_pin(
    State(state): State<ApiState>,
    Path((market, setup_id)): Path<(String, String)>,
    Json(body): Json<CaseQueuePinBody>,
) -> Result<Json<CaseWorkflowState>, ApiError> {
    let market = parse_case_market(&market)?;
    let queue_pin = if body.pinned {
        Some(default_queue_pin_label(body.label.clone()))
    } else {
        Some(None)
    };
    let note = body.note.clone().or_else(|| {
        if body.pinned {
            default_queue_pin_label(body.label.clone()).map(|label| format!("queue pin -> {label}"))
        } else {
            Some("queue pin cleared".into())
        }
    });
    apply_case_assign_update(
        &state,
        market,
        &setup_id,
        CaseAssignBody {
            owner: None,
            reviewer: None,
            queue_pin,
            actor: body.actor,
            note,
        },
    )
    .await
}

#[cfg(feature = "persistence")]
pub(super) async fn post_case_transition(
    State(state): State<ApiState>,
    Path((market, setup_id)): Path<(String, String)>,
    Json(body): Json<CaseTransitionBody>,
) -> Result<Json<CaseWorkflowState>, ApiError> {
    let target_stage = parse_action_stage(&body.target_stage)?;
    let market = parse_case_market(&market)?;
    let store = &state.store;
    let seed = resolve_case_workflow_seed(store, market, &setup_id).await?;
    let workflow_id = seed.workflow_id.clone();
    let current = store
        .action_workflow_by_id(&workflow_id)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load workflow: {error}")))?;
    validate_transition(
        current.as_ref().map(|record| record.current_stage),
        target_stage,
    )?;

    let timestamp = OffsetDateTime::now_utc();
    let actor = normalize_optional_string(body.actor.clone()).or(Some("frontend".into()));
    let note = body.note.clone();
    let title = current
        .as_ref()
        .map(|record| record.title.clone())
        .unwrap_or_else(|| seed.title.clone());
    let owner = current.as_ref().and_then(|record| record.owner.clone());
    let reviewer = current.as_ref().and_then(|record| record.reviewer.clone());
    let queue_pin = current.as_ref().and_then(|record| record.queue_pin.clone());
    let payload = current
        .as_ref()
        .map(|record| record.payload.clone())
        .unwrap_or_else(|| seed.payload.clone());
    let payload = payload_with_operator_decision(
        payload,
        operator_decision_kind_for_transition(target_stage),
        actor.as_deref(),
        note.as_deref(),
    );
    let execution_policy = current
        .as_ref()
        .map(|item| item.execution_policy)
        .unwrap_or(seed.execution_policy);

    let record = crate::persistence::action_workflow::ActionWorkflowRecord {
        workflow_id: workflow_id.clone(),
        title: title.clone(),
        payload: payload.clone(),
        current_stage: target_stage,
        execution_policy,
        governance_reason_code: crate::action::workflow::governance_reason_code(
            Some(target_stage),
            execution_policy,
        ),
        recorded_at: timestamp,
        actor: actor.clone(),
        owner: owner.clone(),
        reviewer: reviewer.clone(),
        queue_pin: queue_pin.clone(),
        note: note.clone(),
    };
    let event = crate::persistence::action_workflow::ActionWorkflowEventRecord {
        event_id: crate::persistence::action_workflow::event_id_for(
            &workflow_id,
            target_stage,
            timestamp,
        ),
        workflow_id: workflow_id.clone(),
        title,
        payload,
        from_stage: current.as_ref().map(|item| item.current_stage),
        to_stage: target_stage,
        execution_policy,
        governance_reason_code: crate::action::workflow::governance_reason_code(
            Some(target_stage),
            execution_policy,
        ),
        recorded_at: timestamp,
        actor: actor.clone(),
        owner: owner.clone(),
        reviewer: reviewer.clone(),
        queue_pin: queue_pin.clone(),
        note: note.clone(),
    };

    store
        .write_action_workflow_state_and_event(&record, &event)
        .await
        .map_err(|error| {
            ApiError::internal(format!("failed to write workflow transition: {error}"))
        })?;
    persist_reasoning_assessment_snapshot(
        store,
        market,
        &seed.setup_id,
        timestamp,
        "workflow_update",
    )
    .await;

    Ok(Json(CaseWorkflowState {
        workflow_id,
        stage: target_stage.as_str().to_string(),
        execution_policy: record.execution_policy,
        governance: record.governance_contract(),
        governance_reason_code: record.governance_reason_code,
        governance_reason: record.governance_reason(),
        timestamp,
        actor,
        owner,
        reviewer,
        queue_pin,
        note,
    }))
}

#[cfg(not(feature = "persistence"))]
pub(super) async fn post_case_transition() -> Result<Json<serde_json::Value>, ApiError> {
    Err(ApiError::not_implemented(
        "case transitions require building with `--features persistence`",
    ))
}

#[cfg(feature = "persistence")]
async fn persist_reasoning_assessment_snapshot(
    store: &EdenStore,
    market: CaseMarket,
    setup_id: &str,
    recorded_at: OffsetDateTime,
    source: &str,
) {
    let Ok(snapshot) = load_snapshot(market).await else {
        eprintln!(
            "Warning: failed to reload snapshot for reasoning assessment {}",
            setup_id
        );
        return;
    };
    let Some(mut detail) = build_case_detail(&snapshot, setup_id) else {
        eprintln!(
            "Warning: failed to rebuild case detail for reasoning assessment {}",
            setup_id
        );
        return;
    };
    if let Err(error) = enrich_case_detail(store, &mut detail).await {
        eprintln!(
            "Warning: failed to enrich case detail for reasoning assessment {}: {}",
            setup_id, error
        );
        return;
    }

    let mut record =
        CaseReasoningAssessmentRecord::from_case_summary(&detail.summary, recorded_at, source);
    record.operator_decision_kind = detail
        .workflow_history
        .last()
        .and_then(|event| event.operator_decision_kind.clone());
    if let Err(error) = store.write_case_reasoning_assessment(&record).await {
        eprintln!(
            "Warning: failed to write reasoning assessment for {}: {}",
            setup_id, error
        );
    }
}

#[cfg(feature = "persistence")]
pub(super) fn parse_action_stage(raw: &str) -> Result<ActionStage, ApiError> {
    match raw {
        "suggest" => Ok(ActionStage::Suggest),
        "confirm" => Ok(ActionStage::Confirm),
        "execute" => Ok(ActionStage::Execute),
        "monitor" => Ok(ActionStage::Monitor),
        "review" => Ok(ActionStage::Review),
        _ => Err(ApiError::bad_request(format!(
            "unsupported action stage `{raw}`"
        ))),
    }
}

#[cfg(feature = "persistence")]
pub(super) fn validate_transition(
    current: Option<ActionStage>,
    target: ActionStage,
) -> Result<(), ApiError> {
    action_workflow::validate_transition(current, target)
        .map_err(|error| ApiError::bad_request(error.to_string()))
}
