use super::*;
use crate::action::workflow::ActionExecutionPolicy;
use crate::action::workflow::ActionStage;
use crate::ontology::links::CrossStockPresence;
use crate::ontology::objects::Symbol;
use crate::ontology::world::{BackwardReasoningSnapshot, WorldStateSnapshot};
use crate::ontology::KnowledgeRelation;
use crate::persistence::action_workflow::{
    event_id_for, ActionWorkflowEventRecord, ActionWorkflowRecord,
};
use crate::persistence::agent_graph::{
    KnowledgeLinkHistoryRecord, KnowledgeLinkStateRecord, MacroEventHistoryRecord,
    MacroEventStateRecord,
};
use crate::temporal::record::TickRecord;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::{SystemTime, UNIX_EPOCH};
use time::OffsetDateTime;

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
        hypotheses: vec![],
        propagation_paths: vec![],
        tactical_setups: vec![],
        hypothesis_tracks: vec![],
        case_clusters: vec![],
        world_state: WorldStateSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            entities: vec![],
            perceptual_states: vec![],
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
async fn open_upgrades_from_recorded_legacy_schema_version() {
    let path = temp_db_path("legacy");
    let db = Surreal::new::<RocksDb>(path.to_str().unwrap())
        .await
        .unwrap();
    db.use_ns("eden").use_db("market").await.unwrap();
    db.query(schema::SCHEMA_VERSION_TABLE)
        .await
        .unwrap()
        .check()
        .unwrap();
    db.query(schema::migrations()[0].statements)
        .await
        .unwrap()
        .check()
        .unwrap();
    EdenStore::write_schema_version(&db, 1, "bootstrap_core_schema")
        .await
        .unwrap();
    EdenStore::apply_schema_migrations(&db, path.to_str().unwrap())
        .await
        .unwrap();
    let store = EdenStore { db, sync_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())) };
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
    let loaded = store.action_workflow_by_id("wf:1").await.unwrap().unwrap();
    assert_eq!(loaded.payload["k"], serde_json::json!("v"));
    drop(store);
    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn workflow_state_and_event_write_is_atomic() {
    let path = temp_db_path("workflow-atomic");
    let store = EdenStore::open(path.to_str().unwrap()).await.unwrap();
    let record = ActionWorkflowRecord {
        workflow_id: "wf:atomic".into(),
        title: "Atomic Demo".into(),
        payload: serde_json::json!({ "market": "hk", "symbol": "700.HK" }),
        current_stage: ActionStage::Suggest,
        execution_policy: ActionExecutionPolicy::ReviewRequired,
        governance_reason_code:
            crate::action::workflow::ActionGovernanceReasonCode::WorkflowTransitionWindow,
        recorded_at: OffsetDateTime::UNIX_EPOCH,
        actor: Some("tester".into()),
        owner: None,
        reviewer: None,
        queue_pin: None,
        note: Some("atomic".into()),
    };
    let invalid_event = ActionWorkflowEventRecord {
        event_id: event_id_for(
            &record.workflow_id,
            ActionStage::Suggest,
            OffsetDateTime::UNIX_EPOCH,
        ),
        workflow_id: record.workflow_id.clone(),
        title: record.title.clone(),
        payload: serde_json::json!(["not-an-object"]),
        from_stage: None,
        to_stage: ActionStage::Suggest,
        execution_policy: ActionExecutionPolicy::ReviewRequired,
        governance_reason_code:
            crate::action::workflow::ActionGovernanceReasonCode::WorkflowTransitionWindow,
        recorded_at: OffsetDateTime::UNIX_EPOCH,
        actor: Some("tester".into()),
        owner: None,
        reviewer: None,
        queue_pin: None,
        note: Some("atomic".into()),
    };

    let error = store
        .write_action_workflow_state_and_event(&record, &invalid_event)
        .await
        .expect_err("invalid event payload should fail transaction");
    assert!(
        error.to_string().contains("payload"),
        "unexpected error: {error}"
    );
    assert!(
        store
            .action_workflow_by_id("wf:atomic")
            .await
            .unwrap()
            .is_none(),
        "latest workflow state must rollback when event write fails"
    );
    assert!(
        store
            .action_workflow_events("wf:atomic")
            .await
            .unwrap()
            .is_empty(),
        "event history must remain empty after rollback"
    );

    drop(store);
    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn open_backfills_workflow_payload_market_for_legacy_rows() {
    let path = temp_db_path("workflow-market-backfill");
    let db = Surreal::new::<RocksDb>(path.to_str().unwrap())
        .await
        .unwrap();
    db.use_ns("eden").use_db("market").await.unwrap();
    db.query(schema::SCHEMA_VERSION_TABLE)
        .await
        .unwrap()
        .check()
        .unwrap();
    // Stop at v37 (pre-2026-04-20 fix wave). apply_schema_migrations then runs
    // M038 / M039 / M040; the post-migration hook fires on M038 and M039 to
    // backfill payload.market on the legacy rows written below.
    for migration in schema::migrations().iter().take(37) {
        db.query(migration.statements)
            .await
            .unwrap()
            .check()
            .unwrap();
        EdenStore::write_schema_version(&db, migration.version, migration.name)
            .await
            .unwrap();
    }

    let store = EdenStore { db: db.clone(), sync_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())) };
    let record = ActionWorkflowRecord {
        workflow_id: "workflow:setup:AAPL.US:enter".into(),
        title: "Position AAPL.US".into(),
        payload: serde_json::json!({
            "setup_id": "setup:AAPL.US:enter",
            "symbol": "AAPL.US",
            "entry_tick": 12
        }),
        current_stage: ActionStage::Monitor,
        execution_policy: ActionExecutionPolicy::ReviewRequired,
        governance_reason_code:
            crate::action::workflow::ActionGovernanceReasonCode::WorkflowTransitionWindow,
        recorded_at: OffsetDateTime::UNIX_EPOCH,
        actor: Some("tracker".into()),
        owner: None,
        reviewer: None,
        queue_pin: None,
        note: None,
    };
    let event = ActionWorkflowEventRecord {
        event_id: event_id_for(
            &record.workflow_id,
            ActionStage::Monitor,
            OffsetDateTime::UNIX_EPOCH,
        ),
        workflow_id: record.workflow_id.clone(),
        title: record.title.clone(),
        payload: record.payload.clone(),
        from_stage: Some(ActionStage::Execute),
        to_stage: ActionStage::Monitor,
        execution_policy: ActionExecutionPolicy::ReviewRequired,
        governance_reason_code:
            crate::action::workflow::ActionGovernanceReasonCode::WorkflowTransitionWindow,
        recorded_at: OffsetDateTime::UNIX_EPOCH,
        actor: Some("tracker".into()),
        owner: None,
        reviewer: None,
        queue_pin: None,
        note: Some("restored".into()),
    };
    store.write_action_workflow(&record).await.unwrap();
    store.write_action_workflow_event(&event).await.unwrap();

    EdenStore::apply_schema_migrations(&db, path.to_str().unwrap())
        .await
        .unwrap();

    let store = EdenStore { db, sync_lock: std::sync::Arc::new(tokio::sync::Mutex::new(())) };
    let updated = store
        .action_workflow_by_id("workflow:setup:AAPL.US:enter")
        .await
        .unwrap()
        .unwrap();
    assert_eq!(updated.payload["market"], serde_json::json!("us"));
    let events = store
        .action_workflow_events("workflow:setup:AAPL.US:enter")
        .await
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].payload["market"], serde_json::json!("us"));

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

    // SurrealDB 2.x wraps query results in typed enums that don't
    // deserialize cleanly to `serde_json::Value`. Use count() which
    // returns a plain u64 and is semantically what this test wants
    // ("did the second upsert replace, not duplicate").
    let mut result = store
        .db
        .query("SELECT count() FROM institution_state GROUP ALL")
        .await
        .unwrap();
    let counts: Vec<u64> = result.take("count").unwrap_or_default();
    let count = counts.into_iter().next().unwrap_or(0);
    assert_eq!(count, 1, "upsert with same id should not duplicate");

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
        confidence: rust_decimal_macros::dec!(0.8),
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
        confidence: rust_decimal_macros::dec!(0.8),
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
        confidence: rust_decimal_macros::dec!(0.8),
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
        confidence: rust_decimal_macros::dec!(0.8),
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

#[tokio::test]
async fn write_and_read_horizon_evaluation_records() {
    use crate::ontology::horizon::HorizonBucket;
    use crate::persistence::horizon_evaluation::{EvaluationStatus, HorizonEvaluationRecord};
    use time::macros::datetime;

    let path = temp_db_path("horizon-eval");
    let store = EdenStore::open(path.to_str().unwrap()).await.unwrap();

    let records = vec![HorizonEvaluationRecord {
        record_id: "horizon-eval:test-1:Fast5m".into(),
        setup_id: "test-1".into(),
        market: "us".into(),
        horizon: HorizonBucket::Fast5m,
        primary: true,
        due_at: datetime!(2026-04-13 14:05 UTC),
        status: EvaluationStatus::Pending,
        result: None,
        resolution: None,
    }];

    store
        .write_horizon_evaluations(&records)
        .await
        .expect("write");

    let loaded = store
        .load_horizon_evaluations_for_setup("test-1")
        .await
        .expect("load");

    assert_eq!(loaded.len(), 1);
    assert_eq!(loaded[0].horizon, HorizonBucket::Fast5m);
    assert!(loaded[0].primary);
    assert_eq!(loaded[0].status, EvaluationStatus::Pending);

    drop(store);
    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn sweep_pending_horizons_to_due_flips_overdue_records() {
    use crate::core::runtime::sweep_pending_horizons_to_due;
    use crate::ontology::horizon::HorizonBucket;
    use crate::persistence::horizon_evaluation::{EvaluationStatus, HorizonEvaluationRecord};
    use time::macros::datetime;

    let path = temp_db_path("horizon-sweep");
    let store = EdenStore::open(path.to_str().unwrap()).await.unwrap();

    // Two pending records: one with due_at in the past, one in the future.
    let past_record = HorizonEvaluationRecord {
        record_id: "horizon-eval:past-setup:Fast5m".into(),
        setup_id: "past-setup".into(),
        market: "hk".into(),
        horizon: HorizonBucket::Fast5m,
        primary: true,
        due_at: datetime!(2020-01-01 00:00 UTC),
        status: EvaluationStatus::Pending,
        result: None,
        resolution: None,
    };
    let future_record = HorizonEvaluationRecord {
        record_id: "horizon-eval:future-setup:Fast5m".into(),
        setup_id: "future-setup".into(),
        market: "hk".into(),
        horizon: HorizonBucket::Fast5m,
        primary: true,
        due_at: datetime!(2099-12-31 23:59 UTC),
        status: EvaluationStatus::Pending,
        result: None,
        resolution: None,
    };

    store
        .write_horizon_evaluations(&[past_record.clone(), future_record.clone()])
        .await
        .expect("write");

    let now = datetime!(2026-04-19 12:00 UTC);
    let n = sweep_pending_horizons_to_due(&store, now).await;
    assert_eq!(n, 1, "exactly one record's due_at is in the past");

    let past_after = store
        .load_horizon_evaluations_for_setup("past-setup")
        .await
        .expect("load past")
        .into_iter()
        .next()
        .expect("record exists");
    assert_eq!(past_after.status, EvaluationStatus::Due);

    let future_after = store
        .load_horizon_evaluations_for_setup("future-setup")
        .await
        .expect("load future")
        .into_iter()
        .next()
        .expect("record exists");
    assert_eq!(
        future_after.status,
        EvaluationStatus::Pending,
        "future record must remain Pending"
    );

    drop(store);
    let _ = std::fs::remove_dir_all(path);
}

#[tokio::test]
async fn sweep_pending_horizons_to_due_noop_when_nothing_overdue() {
    use crate::core::runtime::sweep_pending_horizons_to_due;
    use time::macros::datetime;

    let path = temp_db_path("horizon-sweep-empty");
    let store = EdenStore::open(path.to_str().unwrap()).await.unwrap();

    let n = sweep_pending_horizons_to_due(&store, datetime!(2026-04-19 12:00 UTC)).await;
    assert_eq!(n, 0);

    drop(store);
    let _ = std::fs::remove_dir_all(path);
}
