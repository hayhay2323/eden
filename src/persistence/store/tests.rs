use super::*;
use crate::action::workflow::ActionExecutionPolicy;
use crate::action::workflow::ActionStage;
use crate::ontology::world::{BackwardReasoningSnapshot, WorldStateSnapshot};
use crate::ontology::KnowledgeRelation;
use crate::persistence::action_workflow::ActionWorkflowRecord;
use crate::persistence::agent_graph::{
    KnowledgeLinkHistoryRecord, KnowledgeLinkStateRecord, MacroEventHistoryRecord,
    MacroEventStateRecord,
};
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};

fn temp_db_path(label: &str) -> PathBuf {
    let unique = format!(
        "eden-schema-test-{}-{}-{}",
        label,
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    );
    std::env::temp_dir().join(unique)
}

fn sample_tick_record() -> TickRecord {
    TickRecord {
        tick_number: 1,
        timestamp: OffsetDateTime::UNIX_EPOCH,
        signals: HashMap::new(),
        observations: vec![],
        events: vec![],
        derived_signals: vec![],
        action_workflows: vec![],
        polymarket_priors: vec![],
        hypotheses: vec![],
        propagation_paths: vec![],
        tactical_setups: vec![],
        hypothesis_tracks: vec![],
        case_clusters: vec![],
        world_state: WorldStateSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            entities: vec![],
            vortices: vec![],
        },
        backward_reasoning: BackwardReasoningSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            investigations: vec![],
        },
        graph_edge_transitions: vec![],
        graph_node_transitions: vec![],
        microstructure_deltas: None,
    }
}

#[tokio::test]
async fn open_initializes_schema_version_to_latest() {
    let path = temp_db_path("fresh");
    let store = EdenStore::open(path.to_str().unwrap()).await.unwrap();
    let version = EdenStore::stored_schema_version(&store.db).await.unwrap();
    assert_eq!(version, Some(schema::LATEST_SCHEMA_VERSION));
    store.write_tick(&sample_tick_record()).await.unwrap();
    drop(store);
    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn open_upgrades_legacy_schema_without_version_record() {
    let path = temp_db_path("legacy");
    let db = Surreal::new::<RocksDb>(path.to_str().unwrap())
        .await
        .unwrap();
    db.use_ns("eden").use_db("market").await.unwrap();
    db.query(schema::migrations()[0].statements).await.unwrap();
    EdenStore::apply_schema_migrations(&db).await.unwrap();
    let store = EdenStore { db };
    let version = EdenStore::stored_schema_version(&store.db).await.unwrap();
    assert_eq!(version, Some(schema::LATEST_SCHEMA_VERSION));
    store.write_tick(&sample_tick_record()).await.unwrap();
    drop(store);
    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn workflow_record_timestamps_round_trip_through_string_schema() {
    let path = temp_db_path("workflow-ts");
    let store = EdenStore::open(path.to_str().unwrap()).await.unwrap();
    let record = ActionWorkflowRecord {
        workflow_id: "wf:1".into(),
        title: "Demo".into(),
        payload: serde_json::json!({ "k": "v" }),
        current_stage: ActionStage::Review,
        execution_policy: ActionExecutionPolicy::ReviewRequired,
        governance_reason_code:
            crate::action::workflow::ActionGovernanceReasonCode::TerminalReviewStage,
        recorded_at: OffsetDateTime::UNIX_EPOCH,
        actor: Some("tester".into()),
        owner: None,
        reviewer: None,
        queue_pin: None,
        note: None,
    };

    store.write_action_workflow(&record).await.unwrap();
    let latest = store.latest_action_workflow_recorded_at().await.unwrap();
    assert_eq!(latest, Some(OffsetDateTime::UNIX_EPOCH));
    drop(store);
    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn institution_states_upsert_same_tick_instead_of_duplicating() {
    let path = temp_db_path("institution-state");
    let store = EdenStore::open(path.to_str().unwrap()).await.unwrap();
    let presences = vec![CrossStockPresence {
        institution_id: crate::ontology::InstitutionId(8120),
        symbols: vec![Symbol("700.HK".into()), Symbol("9988.HK".into())],
        ask_symbols: vec![Symbol("9988.HK".into())],
        bid_symbols: vec![Symbol("700.HK".into())],
    }];

    store
        .write_institution_states(&presences, OffsetDateTime::UNIX_EPOCH)
        .await
        .unwrap();
    store
        .write_institution_states(&presences, OffsetDateTime::UNIX_EPOCH)
        .await
        .unwrap();

    let mut result = store
        .db
        .query("SELECT * FROM institution_state")
        .await
        .unwrap();
    let rows: Vec<serde_json::Value> = result.take(0).unwrap();
    assert_eq!(rows.len(), 1);

    drop(store);
    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn macro_event_and_knowledge_link_history_round_trip() {
    let path = temp_db_path("agent-graph");
    let store = EdenStore::open(path.to_str().unwrap()).await.unwrap();

    let macro_record = MacroEventHistoryRecord {
        record_id: "hk:10:macro_event:1".into(),
        event_id: "macro_event:1".into(),
        tick_number: 10,
        market: "hk".into(),
        recorded_at: OffsetDateTime::UNIX_EPOCH,
        event_type: "rates_macro".into(),
        authority_level: "high".into(),
        headline: "Fed repricing".into(),
        summary: "rates higher".into(),
        confidence: "0.8".into(),
        confirmation_state: "confirmed".into(),
        primary_scope: "market".into(),
        affected_markets: vec!["hk".into()],
        affected_sectors: vec!["Property".into()],
        affected_symbols: vec!["700.HK".into()],
        preferred_expression: "risk_off".into(),
        requires_market_confirmation: true,
        decisive_factors: vec!["yield shock".into()],
        supporting_notice_ids: vec!["notice:1".into()],
        promotion_reasons: vec!["high authority".into()],
    };
    let link_record = KnowledgeLinkHistoryRecord {
        record_id: "hk:10:link:1".into(),
        link_id: "link:1".into(),
        tick_number: 10,
        market: "hk".into(),
        recorded_at: OffsetDateTime::UNIX_EPOCH,
        relation: KnowledgeRelation::ImpactsSymbol,
        source_node_kind: "macro_event".into(),
        source_node_id: "macro_event:1".into(),
        source_label: "Fed repricing".into(),
        target_node_kind: "symbol".into(),
        target_node_id: "symbol:700.HK".into(),
        target_label: "700.HK".into(),
        confidence: "0.8".into(),
        attributes: crate::ontology::KnowledgeLinkAttributes::ImpactsSymbol {
            event_type: "rates_macro".into(),
            authority_level: "high".into(),
            primary_scope: "market".into(),
            preferred_expression: "risk_off".into(),
        },
        rationale: Some("rates hit property".into()),
    };

    store
        .write_macro_event_history(std::slice::from_ref(&macro_record))
        .await
        .unwrap();
    store
        .write_knowledge_link_history(std::slice::from_ref(&link_record))
        .await
        .unwrap();

    let macro_records = store
        .recent_macro_event_history("hk", Some(10), 8)
        .await
        .unwrap();
    let link_records = store
        .recent_knowledge_link_history("hk", Some(10), 8)
        .await
        .unwrap();

    assert_eq!(macro_records.len(), 1);
    assert_eq!(macro_records[0].event_id, macro_record.event_id);
    assert_eq!(link_records.len(), 1);
    assert_eq!(link_records[0].link_id, link_record.link_id);

    drop(store);
    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn macro_event_and_knowledge_link_state_sync_replaces_previous_snapshot() {
    let path = temp_db_path("agent-graph-state");
    let store = EdenStore::open(path.to_str().unwrap()).await.unwrap();

    let first_macro = MacroEventStateRecord {
        state_id: "hk:macro_event:1".into(),
        event_id: "macro_event:1".into(),
        market: "hk".into(),
        latest_tick_number: 10,
        last_seen_at: OffsetDateTime::UNIX_EPOCH,
        event_type: "rates_macro".into(),
        authority_level: "high".into(),
        headline: "Fed repricing".into(),
        summary: "rates higher".into(),
        confidence: "0.8".into(),
        confirmation_state: "confirmed".into(),
        primary_scope: "market".into(),
        affected_markets: vec!["hk".into()],
        affected_sectors: vec!["Property".into()],
        affected_symbols: vec!["700.HK".into()],
        preferred_expression: "risk_off".into(),
        requires_market_confirmation: true,
        decisive_factors: vec!["yield shock".into()],
        supporting_notice_ids: vec![],
        promotion_reasons: vec![],
    };
    let first_link = KnowledgeLinkStateRecord {
        state_id: "hk:link:1".into(),
        link_id: "link:1".into(),
        market: "hk".into(),
        latest_tick_number: 10,
        last_seen_at: OffsetDateTime::UNIX_EPOCH,
        relation: KnowledgeRelation::ImpactsSymbol,
        source_node_kind: "macro_event".into(),
        source_node_id: "macro_event:1".into(),
        source_label: "Fed repricing".into(),
        target_node_kind: "symbol".into(),
        target_node_id: "symbol:700.HK".into(),
        target_label: "700.HK".into(),
        confidence: "0.8".into(),
        attributes: crate::ontology::KnowledgeLinkAttributes::ImpactsSymbol {
            event_type: "rates_macro".into(),
            authority_level: "high".into(),
            primary_scope: "market".into(),
            preferred_expression: "risk_off".into(),
        },
        rationale: Some("rates hit property".into()),
    };

    store
        .sync_macro_event_state("hk", std::slice::from_ref(&first_macro))
        .await
        .unwrap();
    store
        .sync_knowledge_link_state("hk", std::slice::from_ref(&first_link))
        .await
        .unwrap();

    store.sync_macro_event_state("hk", &[]).await.unwrap();
    store.sync_knowledge_link_state("hk", &[]).await.unwrap();

    assert!(store
        .current_macro_event_state("hk", 8)
        .await
        .unwrap()
        .is_empty());
    assert!(store
        .current_knowledge_link_state("hk", 8)
        .await
        .unwrap()
        .is_empty());

    drop(store);
    let _ = std::fs::remove_dir_all(path);
}
