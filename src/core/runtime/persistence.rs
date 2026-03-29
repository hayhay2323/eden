#[cfg(feature = "persistence")]
use std::fmt::Display;
#[cfg(feature = "persistence")]
use std::future::Future;
#[cfg(feature = "persistence")]
use std::sync::Arc;

#[cfg(feature = "persistence")]
use crate::cases::CaseSummary;
#[cfg(feature = "persistence")]
use crate::live_snapshot::LiveMarket;
#[cfg(feature = "persistence")]
use crate::ontology::microstructure::TickArchive;
#[cfg(feature = "persistence")]
use crate::ontology::{
    merged_knowledge_events, merged_knowledge_links, ActionNode,
    AgentKnowledgeEvent, AgentKnowledgeLink, AgentMacroEvent, BackwardReasoningSnapshot,
    Hypothesis, TacticalSetup, WorldStateSnapshot,
};
#[cfg(feature = "persistence")]
use crate::agent::AgentDecision;
#[cfg(feature = "persistence")]
use crate::persistence::agent_graph::{
    build_knowledge_node_records, build_runtime_knowledge_events, build_runtime_knowledge_links,
    reasoning_knowledge_events, reasoning_knowledge_links, KnowledgeEventHistoryRecord,
    KnowledgeEventStateRecord, KnowledgeLinkHistoryRecord, KnowledgeLinkStateRecord,
    KnowledgeNodeHistoryRecord, KnowledgeNodeStateRecord, MacroEventHistoryRecord,
    MacroEventStateRecord,
};
#[cfg(feature = "persistence")]
use crate::persistence::case_reasoning_assessment::CaseReasoningAssessmentRecord;
#[cfg(feature = "persistence")]
use crate::persistence::lineage_metric_row::{rows_from_lineage_stats, LineageMetricRowRecord};
#[cfg(feature = "persistence")]
use crate::persistence::lineage_snapshot::LineageSnapshotRecord;
#[cfg(feature = "persistence")]
use crate::persistence::store::EdenStore;
#[cfg(feature = "persistence")]
use crate::persistence::us_lineage_metric_row::{rows_from_us_lineage_stats, UsLineageMetricRowRecord};
#[cfg(feature = "persistence")]
use crate::persistence::us_lineage_snapshot::UsLineageSnapshotRecord;
#[cfg(feature = "persistence")]
use crate::temporal::lineage::LineageStats;
#[cfg(feature = "persistence")]
use crate::temporal::record::TickRecord;
#[cfg(feature = "persistence")]
use crate::us::temporal::lineage::UsLineageStats;
#[cfg(feature = "persistence")]
use crate::us::temporal::record::UsTickRecord;
#[cfg(feature = "persistence")]
use serde_json::json;
#[cfg(feature = "persistence")]
use tokio::sync::Semaphore;

#[cfg(feature = "persistence")]
use super::telemetry::{log_runtime_issue, RuntimeIssueLevel};
#[cfg(feature = "persistence")]
use super::RuntimeInfraConfig;

#[cfg(feature = "persistence")]
#[derive(Debug, Clone, Default)]
pub struct KnowledgePersistenceBundle {
    pub macro_event_history_records: Vec<MacroEventHistoryRecord>,
    pub macro_event_state_records: Vec<MacroEventStateRecord>,
    pub knowledge_link_history_records: Vec<KnowledgeLinkHistoryRecord>,
    pub knowledge_link_state_records: Vec<KnowledgeLinkStateRecord>,
    pub knowledge_event_history_records: Vec<KnowledgeEventHistoryRecord>,
    pub knowledge_event_state_records: Vec<KnowledgeEventStateRecord>,
    pub knowledge_node_history_records: Vec<KnowledgeNodeHistoryRecord>,
    pub knowledge_node_state_records: Vec<KnowledgeNodeStateRecord>,
}

#[cfg(feature = "persistence")]
pub fn build_market_knowledge_persistence_bundle(
    market: LiveMarket,
    tick_number: u64,
    recorded_at: time::OffsetDateTime,
    macro_events: &[AgentMacroEvent],
    decisions: &[crate::agent::AgentDecision],
    hypotheses: &[Hypothesis],
    setups: &[TacticalSetup],
    cases: &[CaseSummary],
    world_state: Option<&WorldStateSnapshot>,
    backward_reasoning: Option<&BackwardReasoningSnapshot>,
    active_positions: &[ActionNode],
    links: &[AgentKnowledgeLink],
    events: &[AgentKnowledgeEvent],
) -> KnowledgePersistenceBundle {
    let macro_event_history_records = macro_events
        .iter()
        .map(|event| MacroEventHistoryRecord::from_agent_event(event, recorded_at))
        .collect::<Vec<_>>();
    let macro_event_state_records = macro_events
        .iter()
        .map(|event| MacroEventStateRecord::from_agent_event(event, recorded_at))
        .collect::<Vec<_>>();
    let knowledge_link_history_records = links
        .iter()
        .cloned()
        .map(|link| {
            KnowledgeLinkHistoryRecord::from_agent_link(market, tick_number, recorded_at, &link)
        })
        .collect::<Vec<_>>();
    let knowledge_link_state_records = links
        .iter()
        .cloned()
        .map(|link| {
            KnowledgeLinkStateRecord::from_agent_link(market, tick_number, recorded_at, &link)
        })
        .collect::<Vec<_>>();
    let knowledge_event_history_records = events
        .iter()
        .cloned()
        .map(|event| {
            KnowledgeEventHistoryRecord::from_agent_event(market, tick_number, recorded_at, &event)
        })
        .collect::<Vec<_>>();
    let knowledge_event_state_records = events
        .iter()
        .cloned()
        .map(|event| {
            KnowledgeEventStateRecord::from_agent_event(market, tick_number, recorded_at, &event)
        })
        .collect::<Vec<_>>();
    let (knowledge_node_history_records, knowledge_node_state_records) =
        build_knowledge_node_records(
            market,
            tick_number,
            recorded_at,
            macro_events,
            decisions,
            hypotheses,
            setups,
            cases,
            world_state,
            backward_reasoning,
            active_positions,
            links,
        );

    KnowledgePersistenceBundle {
        macro_event_history_records,
        macro_event_state_records,
        knowledge_link_history_records,
        knowledge_link_state_records,
        knowledge_event_history_records,
        knowledge_event_state_records,
        knowledge_node_history_records,
        knowledge_node_state_records,
    }
}

#[cfg(feature = "persistence")]
pub fn build_market_knowledge_followup_bundle(
    market: LiveMarket,
    tick_number: u64,
    recorded_at: time::OffsetDateTime,
    snapshot_knowledge_links: &[AgentKnowledgeLink],
    recommendation_knowledge_links: &[AgentKnowledgeLink],
    macro_events: &[AgentMacroEvent],
    decisions: &[AgentDecision],
    hypotheses: &[Hypothesis],
    setups: &[TacticalSetup],
    cases: &[CaseSummary],
    world_state: Option<&WorldStateSnapshot>,
    backward_reasoning: Option<&BackwardReasoningSnapshot>,
    active_positions: &[ActionNode],
) -> KnowledgePersistenceBundle {
    let reasoning_links = reasoning_knowledge_links(hypotheses, setups, cases);
    let reasoning_events = reasoning_knowledge_events(hypotheses, setups, cases);
    let runtime_links =
        build_runtime_knowledge_links(world_state, backward_reasoning, active_positions);
    let runtime_events = build_runtime_knowledge_events(backward_reasoning, active_positions);
    let unified_knowledge_links = merged_knowledge_links(
        &merged_knowledge_links(
            &merged_knowledge_links(snapshot_knowledge_links, recommendation_knowledge_links),
            &runtime_links,
        ),
        &reasoning_links,
    );
    let unified_knowledge_events = merged_knowledge_events(&runtime_events, &reasoning_events);

    build_market_knowledge_persistence_bundle(
        market,
        tick_number,
        recorded_at,
        macro_events,
        decisions,
        hypotheses,
        setups,
        cases,
        world_state,
        backward_reasoning,
        active_positions,
        &unified_knowledge_links,
        &unified_knowledge_events,
    )
}

#[cfg(feature = "persistence")]
pub enum LineagePersistenceBundle {
    Hk {
        snapshot: LineageSnapshotRecord,
        rows: Vec<LineageMetricRowRecord>,
    },
    Us {
        snapshot: UsLineageSnapshotRecord,
        rows: Vec<UsLineageMetricRowRecord>,
    },
}

#[cfg(feature = "persistence")]
pub async fn schedule_store_operation<F, Fut, E>(
    store: &Option<EdenStore>,
    persistence_limit: &Arc<Semaphore>,
    config: &RuntimeInfraConfig,
    label: &'static str,
    issue_code: &'static str,
    error_prefix: &'static str,
    action: F,
) where
    F: FnOnce(EdenStore) -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), E>> + Send + 'static,
    E: Display + Send + 'static,
{
    let Some(store) = store.clone() else {
        return;
    };
    let Ok(permit) = persistence_limit.clone().acquire_owned().await else {
        return;
    };
    let config = config.clone();
    tokio::spawn(async move {
        let _permit = permit;
        if let Err(error) = action(store).await {
            log_runtime_issue(
                &config,
                RuntimeIssueLevel::Error,
                issue_code,
                format!("{error_prefix}: {error}"),
                json!({
                    "label": label,
                    "error": error.to_string(),
                }),
            );
        }
    });
}

#[cfg(feature = "persistence")]
pub async fn schedule_store_batch_operations<T, F, Fut, E>(
    store: &Option<EdenStore>,
    persistence_limit: &Arc<Semaphore>,
    config: &RuntimeInfraConfig,
    label: &'static str,
    issue_code: &'static str,
    error_prefix: &'static str,
    items: Vec<T>,
    action: F,
) where
    T: Send + 'static,
    F: Fn(EdenStore, T) -> Fut + Send + Sync + Copy + 'static,
    Fut: Future<Output = Result<(), E>> + Send + 'static,
    E: Display + Send + 'static,
{
    for item in items {
        schedule_store_operation(
            store,
            persistence_limit,
            config,
            label,
            issue_code,
            error_prefix,
            move |store_ref| action(store_ref, item),
        )
        .await;
    }
}

#[cfg(feature = "persistence")]
pub async fn persist_market_knowledge_projection(
    store: &Option<EdenStore>,
    persistence_limit: &Arc<Semaphore>,
    config: &RuntimeInfraConfig,
    bundle: KnowledgePersistenceBundle,
) {
    let market = config.market.slug().to_string();
    let KnowledgePersistenceBundle {
        macro_event_history_records,
        macro_event_state_records,
        knowledge_link_history_records,
        knowledge_link_state_records,
        knowledge_event_history_records,
        knowledge_event_state_records,
        knowledge_node_history_records,
        knowledge_node_state_records,
    } = bundle;
    let market_macro_state = market.clone();
    let market_link_state = market.clone();
    let market_event_state = market.clone();
    let market_node_state = market.clone();
    schedule_store_operation(
        store,
        persistence_limit,
        config,
        "macro_event_history",
        "write_macro_event_history_failed",
        "failed to write macro event history",
        move |store_ref| {
            let records = macro_event_history_records.clone();
            async move { store_ref.write_macro_event_history(&records).await }
        },
    )
    .await;
    schedule_store_operation(
        store,
        persistence_limit,
        config,
        "knowledge_link_history",
        "write_knowledge_link_history_failed",
        "failed to write knowledge link history",
        move |store_ref| {
            let records = knowledge_link_history_records.clone();
            async move { store_ref.write_knowledge_link_history(&records).await }
        },
    )
    .await;
    schedule_store_operation(
        store,
        persistence_limit,
        config,
        "knowledge_event_history",
        "write_knowledge_event_history_failed",
        "failed to write knowledge event history",
        move |store_ref| {
            let records = knowledge_event_history_records.clone();
            async move { store_ref.write_knowledge_event_history(&records).await }
        },
    )
    .await;
    schedule_store_operation(
        store,
        persistence_limit,
        config,
        "knowledge_node_history",
        "write_knowledge_node_history_failed",
        "failed to write knowledge node history",
        move |store_ref| {
            let records = knowledge_node_history_records.clone();
            async move { store_ref.write_knowledge_node_history(&records).await }
        },
    )
    .await;
    schedule_store_operation(
        store,
        persistence_limit,
        config,
        "macro_event_state",
        "sync_macro_event_state_failed",
        "failed to sync macro event state",
        move |store_ref| {
            let market = market_macro_state.clone();
            let records = macro_event_state_records.clone();
            async move { store_ref.sync_macro_event_state(&market, &records).await }
        },
    )
    .await;
    schedule_store_operation(
        store,
        persistence_limit,
        config,
        "knowledge_link_state",
        "sync_knowledge_link_state_failed",
        "failed to sync knowledge link state",
        move |store_ref| {
            let market = market_link_state.clone();
            let records = knowledge_link_state_records.clone();
            async move { store_ref.sync_knowledge_link_state(&market, &records).await }
        },
    )
    .await;
    schedule_store_operation(
        store,
        persistence_limit,
        config,
        "knowledge_event_state",
        "sync_knowledge_event_state_failed",
        "failed to sync knowledge event state",
        move |store_ref| {
            let market = market_event_state.clone();
            let records = knowledge_event_state_records.clone();
            async move { store_ref.sync_knowledge_event_state(&market, &records).await }
        },
    )
    .await;
    schedule_store_operation(
        store,
        persistence_limit,
        config,
        "knowledge_node_state",
        "sync_knowledge_node_state_failed",
        "failed to sync knowledge node state",
        move |store_ref| {
            let market = market_node_state.clone();
            let records = knowledge_node_state_records.clone();
            async move { store_ref.sync_knowledge_node_state(&market, &records).await }
        },
    )
    .await;
}

#[cfg(feature = "persistence")]
pub async fn persist_market_lineage_projection(
    store: &Option<EdenStore>,
    persistence_limit: &Arc<Semaphore>,
    config: &RuntimeInfraConfig,
    bundle: LineagePersistenceBundle,
) {
    match bundle {
        LineagePersistenceBundle::Hk { snapshot, rows } => {
            schedule_store_operation(
                store,
                persistence_limit,
                config,
                "lineage_snapshot",
                "write_lineage_snapshot_failed",
                "failed to write lineage snapshot",
                move |store_ref| async move { store_ref.write_lineage_snapshot(&snapshot).await },
            )
            .await;
            schedule_store_operation(
                store,
                persistence_limit,
                config,
                "lineage_metric_rows",
                "write_lineage_metric_rows_failed",
                "failed to write lineage metric rows",
                move |store_ref| async move { store_ref.write_lineage_metric_rows(&rows).await },
            )
            .await;
        }
        LineagePersistenceBundle::Us { snapshot, rows } => {
            schedule_store_operation(
                store,
                persistence_limit,
                config,
                "us_lineage_snapshot",
                "write_us_lineage_snapshot_failed",
                "failed to write US lineage snapshot",
                move |store_ref| async move { store_ref.write_us_lineage_snapshot(&snapshot).await },
            )
            .await;
            schedule_store_operation(
                store,
                persistence_limit,
                config,
                "us_lineage_metric_rows",
                "write_us_lineage_metric_rows_failed",
                "failed to write US lineage metric rows",
                move |store_ref| async move { store_ref.write_us_lineage_metric_rows(&rows).await },
            )
            .await;
        }
    }
}
