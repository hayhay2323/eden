#[cfg(feature = "persistence")]
use std::collections::{HashMap, HashSet};

#[cfg(feature = "persistence")]
use time::OffsetDateTime;

#[cfg(feature = "persistence")]
use crate::cases::CaseMarket;
#[cfg(feature = "persistence")]
use crate::ontology::{parse_timestamp, OperationalHistoryRef, OperationalSnapshot};

#[cfg(feature = "persistence")]
use super::foundation::{ApiError, ApiState};
#[cfg(feature = "persistence")]
use super::ontology_history_support::load_recommendation_journal_records;

#[cfg(feature = "persistence")]
#[derive(Debug, Clone, Copy, Default)]
struct HistoryStats {
    count: usize,
    latest_at: Option<OffsetDateTime>,
}

#[cfg(feature = "persistence")]
const HISTORY_REF_ENRICH_LIMIT: usize = 10_000;

#[cfg(feature = "persistence")]
pub(in crate::api) async fn enrich_history_refs(
    state: &ApiState,
    market: CaseMarket,
    snapshot: &mut OperationalSnapshot,
) -> Result<(), ApiError> {
    let workflow_ids = snapshot
        .workflows
        .iter()
        .map(|item| item.id.0.clone())
        .chain(
            snapshot
                .cases
                .iter()
                .filter_map(|item| item.workflow_id.clone()),
        )
        .chain(
            snapshot
                .recommendations
                .iter()
                .filter_map(|item| item.related_workflow_id.clone()),
        )
        .collect::<HashSet<_>>();
    let setup_ids = snapshot
        .cases
        .iter()
        .map(|item| item.setup_id.clone())
        .collect::<HashSet<_>>();

    let mut workflow_stats = HashMap::new();
    for workflow_id in workflow_ids {
        let events = state
            .store
            .action_workflow_event_values(&workflow_id)
            .await
            .map_err(|error| {
                ApiError::internal(format!("failed to query workflow history: {error}"))
            })?;
        workflow_stats.insert(
            workflow_id,
            HistoryStats {
                count: events.len(),
                latest_at: events.iter().filter_map(workflow_event_recorded_at).max(),
            },
        );
    }

    let mut reasoning_stats = HashMap::new();
    let mut outcome_stats = HashMap::new();
    for setup_id in setup_ids {
        let assessments = state
            .store
            .recent_case_reasoning_assessments(&setup_id, HISTORY_REF_ENRICH_LIMIT)
            .await
            .map_err(|error| {
                ApiError::internal(format!("failed to query case reasoning history: {error}"))
            })?;
        reasoning_stats.insert(
            setup_id.clone(),
            HistoryStats {
                count: assessments.len(),
                latest_at: assessments.iter().map(|item| item.recorded_at).max(),
            },
        );

        let outcomes = state
            .store
            .recent_case_realized_outcomes(&setup_id, HISTORY_REF_ENRICH_LIMIT)
            .await
            .map_err(|error| {
                ApiError::internal(format!("failed to query case outcome history: {error}"))
            })?;
        outcome_stats.insert(
            setup_id,
            HistoryStats {
                count: outcomes.len(),
                latest_at: outcomes.iter().map(|item| item.resolved_at).max(),
            },
        );
    }

    let journal_rows = load_recommendation_journal_records(market).await?;
    let mut journal_stats = HashMap::<String, HistoryStats>::new();
    for row in journal_rows {
        let latest_at = parse_timestamp(&row.timestamp).ok();
        for recommendation_id in recommendation_ids_for_journal_row(&row) {
            merge_history_stat(
                journal_stats.entry(recommendation_id).or_default(),
                HistoryStats {
                    count: 1,
                    latest_at,
                },
            );
        }
    }

    let case_setup_by_id = snapshot
        .cases
        .iter()
        .map(|item| (item.id.0.clone(), item.setup_id.clone()))
        .collect::<HashMap<_, _>>();

    for case in &mut snapshot.cases {
        apply_history_stats(
            &mut case.history_refs.workflow,
            case.workflow_id
                .as_deref()
                .and_then(|workflow_id| workflow_stats.get(workflow_id)),
        );
        apply_history_stats(
            &mut case.history_refs.reasoning,
            reasoning_stats.get(&case.setup_id),
        );
        apply_history_stats(
            &mut case.history_refs.outcomes,
            outcome_stats.get(&case.setup_id),
        );
    }

    for workflow in &mut snapshot.workflows {
        apply_history_stats(
            &mut workflow.history_refs.events,
            workflow_stats.get(&workflow.id.0),
        );
    }

    for recommendation in &mut snapshot.recommendations {
        apply_history_stats(
            &mut recommendation.history_refs.journal,
            journal_stats.get(&recommendation.id.0),
        );
        apply_history_stats(
            &mut recommendation.history_refs.workflow,
            recommendation
                .related_workflow_id
                .as_deref()
                .and_then(|workflow_id| workflow_stats.get(workflow_id)),
        );
        apply_history_stats(
            &mut recommendation.history_refs.outcomes,
            recommendation
                .related_case_id
                .as_deref()
                .and_then(|case_id| case_setup_by_id.get(case_id))
                .and_then(|setup_id| outcome_stats.get(setup_id)),
        );
    }

    Ok(())
}

#[cfg(feature = "persistence")]
fn workflow_event_recorded_at(event: &serde_json::Value) -> Option<OffsetDateTime> {
    let raw = event.get("recorded_at")?.as_str()?;
    OffsetDateTime::parse(raw, &time::format_description::well_known::Rfc3339).ok()
}

#[cfg(feature = "persistence")]
fn recommendation_ids_for_journal_row(
    row: &crate::agent::AgentRecommendationJournalRecord,
) -> Vec<String> {
    let mut ids = Vec::new();
    if let Some(item) = row.market_recommendation.as_ref() {
        ids.push(item.recommendation_id.clone());
    }
    for item in &row.decisions {
        let recommendation_id = match item {
            crate::agent::AgentDecision::Market(item) => &item.recommendation_id,
            crate::agent::AgentDecision::Sector(item) => &item.recommendation_id,
            crate::agent::AgentDecision::Symbol(item) => &item.recommendation_id,
        };
        if !ids
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(recommendation_id))
        {
            ids.push(recommendation_id.clone());
        }
    }
    for item in &row.items {
        if !ids
            .iter()
            .any(|candidate| candidate.eq_ignore_ascii_case(&item.recommendation_id))
        {
            ids.push(item.recommendation_id.clone());
        }
    }
    ids
}

#[cfg(feature = "persistence")]
fn apply_history_stats(
    history_ref: &mut Option<OperationalHistoryRef>,
    stats: Option<&HistoryStats>,
) {
    if let Some(history_ref) = history_ref {
        history_ref.count = Some(stats.map(|item| item.count).unwrap_or(0));
        history_ref.latest_at = stats.and_then(|item| item.latest_at);
    }
}

#[cfg(feature = "persistence")]
fn merge_history_stat(target: &mut HistoryStats, incoming: HistoryStats) {
    target.count += incoming.count;
    target.latest_at = match (target.latest_at, incoming.latest_at) {
        (Some(left), Some(right)) => Some(left.max(right)),
        (Some(left), None) => Some(left),
        (None, Some(right)) => Some(right),
        (None, None) => None,
    };
}
