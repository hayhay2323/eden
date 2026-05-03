use std::collections::HashSet;

#[cfg(feature = "persistence")]
use crate::action::workflow::{governance_reason, governance_reason_code};
#[cfg(feature = "persistence")]
use crate::cases::{CaseLineageContext, CaseMarket};
#[cfg(feature = "persistence")]
use crate::persistence::store::EdenStore;
#[cfg(feature = "persistence")]
use crate::persistence::tactical_setup::TacticalSetupRecord;
#[cfg(feature = "persistence")]
use crate::pipeline::learning_loop::apply_learning_feedback;
use crate::pipeline::mechanism_inference::build_reasoning_profile as infer_reasoning_profile;
use crate::pipeline::predicate_engine::{
    augment_predicates_with_workflow, derive_human_review_context,
};

#[cfg(feature = "persistence")]
use super::io::CaseError;
#[cfg(feature = "persistence")]
use super::reasoning_story::{build_case_mechanism_story, record_invalidation_rules};
#[cfg(feature = "persistence")]
use super::review_analytics::{
    build_case_review_analytics_with_assessments, load_outcome_learning_context,
};
#[cfg(feature = "persistence")]
use super::types::{
    CaseDetail, CaseReasoningAssessmentSnapshot, CaseWorkflowEvent, CaseWorkflowState,
};
use super::types::{CaseReviewResponse, CaseSummary};
#[cfg(feature = "persistence")]
use crate::live_snapshot::LiveMarket;
#[cfg(feature = "persistence")]
use crate::pipeline::learning_loop::derive_learning_feedback;
#[cfg(feature = "persistence")]
use std::collections::HashMap;

#[cfg(feature = "persistence")]
pub async fn enrich_case_summaries(
    store: &EdenStore,
    cases: &mut [CaseSummary],
) -> Result<(), CaseError> {
    let setup_ids = cases
        .iter()
        .map(|case| case.setup_id.clone())
        .collect::<Vec<_>>();
    let setup_by_id = store
        .tactical_setups_by_ids(&setup_ids)
        .await?
        .into_iter()
        .map(|setup| (setup.setup_id.clone(), setup))
        .collect::<HashMap<_, _>>();
    let workflow_ids = setup_by_id
        .values()
        .filter_map(|setup| setup.workflow_id.clone())
        .collect::<Vec<_>>();
    let workflow_by_id = store
        .action_workflows_by_ids(&workflow_ids)
        .await?
        .into_iter()
        .map(|workflow| (workflow.workflow_id.clone(), workflow))
        .collect::<HashMap<_, _>>();

    for case in cases.iter_mut() {
        let Some(setup) = setup_by_id.get(&case.setup_id) else {
            continue;
        };

        case.workflow_id = setup.workflow_id.clone();
        let invalidation_rules = record_invalidation_rules(setup);
        if !invalidation_rules.is_empty() {
            case.invalidation_rules = invalidation_rules;
        }

        if let Some(workflow_id) = &setup.workflow_id {
            if let Some(workflow) = workflow_by_id.get(workflow_id) {
                case.workflow_state = workflow.current_stage.as_str().to_string();
                case.owner = workflow.owner.clone();
                case.reviewer = workflow.reviewer.clone();
                case.workflow_actor = workflow.actor.clone();
                case.workflow_note = workflow.note.clone();
            }
        }

        attach_resolution_summary(store, case).await;

        refresh_reasoning_profile(case);
    }

    if let Some(market) = cases.first().map(|item| item.market) {
        let market_key = match market {
            LiveMarket::Hk => "hk",
            LiveMarket::Us => "us",
        };
        // 2026-04-29: removed `let discovered_archetypes = store.load...`
        // — the only consumer (apply_discovered_archetype_memory) was a
        // case-level rogue modulator, deleted in the same sweep. The DB
        // call itself is now skipped; no caller below references it.
        let assessments = store
            .recent_case_reasoning_assessments_by_market(market_key, 240)
            .await?;
        let outcome_context = load_outcome_learning_context(store, market).await?;
        let feedback = derive_learning_feedback(&assessments, &outcome_context);
        for case in cases.iter_mut() {
            case.reasoning_profile = apply_learning_feedback(
                &case.reasoning_profile,
                &case.invalidation_rules,
                &feedback,
                None,
            );
            // 2026-04-29: removed case-level rogue modulators —
            // see review_analytics.rs for full rationale.
        }
    }

    Ok(())
}

#[cfg(feature = "persistence")]
pub async fn enrich_case_detail(
    store: &EdenStore,
    detail: &mut CaseDetail,
) -> Result<(), CaseError> {
    let Some(setup) = store.tactical_setup_by_id(&detail.summary.setup_id).await? else {
        return Ok(());
    };

    detail.summary.workflow_id = setup.workflow_id.clone();
    detail.risk_notes = setup.risk_notes.clone();
    detail.lineage_context = CaseLineageContext {
        based_on: setup.based_on.clone(),
        blocked_by: setup.blocked_by.clone(),
        promoted_by: setup.promoted_by.clone(),
        falsified_by: setup.falsified_by.clone(),
    };

    let invalidation_rules = record_invalidation_rules(&setup);
    if !invalidation_rules.is_empty() {
        detail.summary.invalidation_rules = invalidation_rules;
    }

    if let Some(workflow_id) = &setup.workflow_id {
        if let Some(workflow) = store.action_workflow_by_id(workflow_id).await? {
            detail.summary.workflow_state = workflow.current_stage.as_str().to_string();
            detail.summary.execution_policy = Some(workflow.execution_policy);
            detail.summary.governance = Some(workflow.governance_contract());
            detail.summary.governance_bucket = match workflow.execution_policy {
                crate::action::workflow::ActionExecutionPolicy::ManualOnly => "manual_only".into(),
                crate::action::workflow::ActionExecutionPolicy::ReviewRequired => {
                    "review_required".into()
                }
                crate::action::workflow::ActionExecutionPolicy::AutoEligible => {
                    "auto_eligible".into()
                }
            };
            detail.summary.governance_reason_code = Some(governance_reason_code(
                Some(workflow.current_stage),
                workflow.execution_policy,
            ));
            detail.summary.governance_reason = Some(governance_reason(
                Some(workflow.current_stage),
                workflow.execution_policy,
            ));
            detail.workflow = Some(CaseWorkflowState {
                workflow_id: workflow.workflow_id.clone(),
                stage: workflow.current_stage.as_str().to_string(),
                execution_policy: workflow.execution_policy,
                governance: workflow.governance_contract(),
                governance_reason_code: governance_reason_code(
                    Some(workflow.current_stage),
                    workflow.execution_policy,
                ),
                governance_reason: governance_reason(
                    Some(workflow.current_stage),
                    workflow.execution_policy,
                ),
                timestamp: workflow.recorded_at,
                actor: workflow.actor.clone(),
                owner: workflow.owner.clone(),
                reviewer: workflow.reviewer.clone(),
                queue_pin: workflow.queue_pin.clone(),
                note: workflow.note.clone(),
            });
            detail.summary.owner = workflow.owner.clone();
            detail.summary.reviewer = workflow.reviewer.clone();
            detail.summary.queue_pin = workflow.queue_pin.clone();
            detail.summary.workflow_actor = workflow.actor.clone();
            detail.summary.workflow_note = workflow.note.clone();
        }

        detail.workflow_history = store
            .action_workflow_events(workflow_id)
            .await?
            .into_iter()
            .map(|event| {
                let governance_reason = event.governance_reason();
                let operator_decision_kind = event
                    .payload
                    .get("operator_decision")
                    .and_then(|value| value.get("kind"))
                    .and_then(|value| value.as_str())
                    .map(|value| value.to_string());
                CaseWorkflowEvent {
                    workflow_id: event.workflow_id,
                    stage: event.to_stage.as_str().to_string(),
                    from_stage: event.from_stage.map(|stage| stage.as_str().to_string()),
                    operator_decision_kind,
                    execution_policy: event.execution_policy,
                    governance_reason_code: event.governance_reason_code,
                    governance_reason,
                    timestamp: event.recorded_at,
                    actor: event.actor,
                    owner: event.owner,
                    reviewer: event.reviewer,
                    queue_pin: event.queue_pin,
                    note: event.note,
                }
            })
            .collect();
    }

    detail.reasoning_history = store
        .recent_case_reasoning_assessments(&detail.summary.setup_id, 12)
        .await?
        .into_iter()
        .map(CaseReasoningAssessmentSnapshot::from_record)
        .collect();

    attach_resolution_summary(store, &mut detail.summary).await;

    refresh_reasoning_profile(&mut detail.summary);

    let market_key = match detail.summary.market {
        LiveMarket::Hk => "hk",
        LiveMarket::Us => "us",
    };
    let assessments = store
        .recent_case_reasoning_assessments_by_market(market_key, 240)
        .await?;
    let outcome_context = load_outcome_learning_context(store, detail.summary.market).await?;
    let feedback = derive_learning_feedback(&assessments, &outcome_context);
    detail.summary.reasoning_profile = apply_learning_feedback(
        &detail.summary.reasoning_profile,
        &detail.summary.invalidation_rules,
        &feedback,
        None,
    );
    // 2026-04-29: removed case-level rogue modulators on detail.summary.
    detail.mechanism_story = build_case_mechanism_story(detail);

    Ok(())
}

#[cfg(feature = "persistence")]
pub async fn enrich_case_review(
    store: &EdenStore,
    market: CaseMarket,
    review: &mut CaseReviewResponse,
) -> Result<(), CaseError> {
    let market_key = match market {
        CaseMarket::Hk => "hk",
        CaseMarket::Us => "us",
    };
    let assessments = store
        .recent_case_reasoning_assessments_by_market(market_key, 240)
        .await?;
    let case_outcomes = store
        .recent_case_realized_outcomes_by_market(market_key, 120)
        .await?;
    let discovered_archetypes = store
        .load_discovered_archetypes(market_key)
        .await
        .unwrap_or_default();
    let outcome_context = load_outcome_learning_context(
        store,
        match market {
            CaseMarket::Hk => LiveMarket::Hk,
            CaseMarket::Us => LiveMarket::Us,
        },
    )
    .await?;
    review.analytics = build_case_review_analytics_with_assessments(
        &review_all_cases(review),
        &assessments,
        &case_outcomes,
        &discovered_archetypes,
        outcome_context,
    );
    Ok(())
}

#[cfg(feature = "persistence")]
pub fn workflow_record_payload(setup: &TacticalSetupRecord) -> serde_json::Value {
    serde_json::json!({
        "setup_id": setup.setup_id,
        "title": setup.title,
        "action": setup.action,
        "decision_lineage": {
            "based_on": setup.based_on,
            "blocked_by": setup.blocked_by,
            "promoted_by": setup.promoted_by,
            "falsified_by": setup.falsified_by,
        }
    })
}

#[cfg_attr(not(feature = "persistence"), allow(dead_code))]
fn refresh_reasoning_profile(case: &mut CaseSummary) {
    let human_review =
        derive_human_review_context(&case.workflow_state, case.workflow_note.as_deref());
    let predicates = augment_predicates_with_workflow(
        &case.reasoning_profile.predicates,
        &case.workflow_state,
        case.workflow_note.as_deref(),
    );
    case.reasoning_profile =
        infer_reasoning_profile(&predicates, &case.invalidation_rules, human_review);
}

/// Project the latest persisted `case_resolution` + `horizon_evaluation`
/// records onto the case summary.
///
/// `case_resolution` is written progressively as horizons settle (or all at
/// once for realized-outcome bootstrap). `horizon_evaluation` gives the
/// per-horizon breakdown used to show operators how much of the case has
/// actually resolved. Failures to load are logged and ignored — this is an
/// enrichment, not a gate.
#[cfg(feature = "persistence")]
async fn attach_resolution_summary(store: &EdenStore, case: &mut CaseSummary) {
    match store.load_case_resolution_for_setup(&case.setup_id).await {
        Ok(Some(record)) => {
            case.case_resolution = Some(record.resolution);
        }
        Ok(None) => {}
        Err(error) => {
            eprintln!(
                "[cases] load_case_resolution_for_setup failed for {}: {}",
                case.setup_id, error
            );
        }
    }

    match store
        .load_horizon_evaluations_for_setup(&case.setup_id)
        .await
    {
        Ok(records) if !records.is_empty() => {
            use crate::persistence::horizon_evaluation::EvaluationStatus;
            let total = records.len();
            let settled = records
                .iter()
                .filter(|record| {
                    matches!(
                        record.status,
                        EvaluationStatus::Resolved | EvaluationStatus::EarlyExited
                    )
                })
                .count();
            let primary_kind = records
                .iter()
                .find(|record| record.primary)
                .and_then(|record| record.resolution.as_ref())
                .map(|resolution| format!("{:?}", resolution.kind).to_ascii_lowercase());
            let breakdown = match primary_kind {
                Some(kind) => format!("primary {kind}, {settled}/{total} settled"),
                None => format!("{settled}/{total} settled"),
            };
            case.horizon_breakdown = Some(breakdown);
        }
        Ok(_) => {}
        Err(error) => {
            eprintln!(
                "[cases] load_horizon_evaluations_for_setup failed for {}: {}",
                case.setup_id, error
            );
        }
    }
}

#[cfg_attr(not(feature = "persistence"), allow(dead_code))]
fn review_all_cases(review: &CaseReviewResponse) -> Vec<CaseSummary> {
    let mut cases = Vec::new();
    cases.extend(review.buckets.in_flight.clone());
    cases.extend(review.buckets.under_review.clone());
    cases.extend(review.buckets.at_risk.clone());
    cases.extend(review.buckets.high_conviction.clone());

    let mut seen = HashSet::new();
    cases.retain(|case| seen.insert(case.setup_id.clone()));
    cases
}
