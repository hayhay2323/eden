use crate::persistence::action_workflow::{ActionWorkflowEventRecord, ActionWorkflowRecord};
use crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord;
use crate::persistence::case_reasoning_assessment::CaseReasoningAssessmentRecord;
use crate::persistence::hypothesis_track::HypothesisTrackRecord;
use crate::persistence::symbol_perception_state::SymbolPerceptionStateRecord;
use crate::persistence::tactical_setup::TacticalSetupRecord;
use serde_json::Value;

use super::super::store_helpers::{
    exec_query_checked, fetch_latest_timestamp_field, fetch_market_state_records,
    fetch_optional_record_by_field, fetch_ordered_records, fetch_records_by_field_order,
    fetch_records_by_ids, sync_market_state_checked, take_records, upsert_batch_checked,
    upsert_json_query, upsert_record_checked, StoreError,
};
use super::EdenStore;

const WORKFLOW_EVENT_SCAN_LIMIT: usize = 10_000;

impl EdenStore {
    pub async fn write_action_workflows(
        &self,
        records: &[ActionWorkflowRecord],
    ) -> Result<(), StoreError> {
        upsert_batch_checked(&self.db, "action_workflow", records, |record| {
            record.record_id()
        })
        .await
    }

    pub async fn write_action_workflow(
        &self,
        record: &ActionWorkflowRecord,
    ) -> Result<(), StoreError> {
        upsert_record_checked(&self.db, "action_workflow", record.record_id(), record).await
    }

    pub async fn write_action_workflow_event(
        &self,
        record: &ActionWorkflowEventRecord,
    ) -> Result<(), StoreError> {
        upsert_record_checked(
            &self.db,
            "action_workflow_event",
            record.record_id(),
            record,
        )
        .await
    }

    pub async fn write_action_workflow_state_and_event(
        &self,
        record: &ActionWorkflowRecord,
        event: &ActionWorkflowEventRecord,
    ) -> Result<(), StoreError> {
        let mut query = String::from("BEGIN TRANSACTION;");
        query.push_str(&upsert_json_query(
            "action_workflow_event",
            event.record_id(),
            event,
        )?);
        query.push_str(&upsert_json_query(
            "action_workflow",
            record.record_id(),
            record,
        )?);
        query.push_str("COMMIT TRANSACTION;");
        exec_query_checked(&self.db, query).await
    }

    pub async fn write_tactical_setup(
        &self,
        record: &TacticalSetupRecord,
    ) -> Result<(), StoreError> {
        upsert_record_checked(&self.db, "tactical_setup", record.record_id(), record).await
    }

    pub async fn write_tactical_setups(
        &self,
        records: &[TacticalSetupRecord],
    ) -> Result<(), StoreError> {
        upsert_batch_checked(&self.db, "tactical_setup", records, |record| {
            record.record_id()
        })
        .await
    }

    pub async fn write_case_reasoning_assessment(
        &self,
        record: &CaseReasoningAssessmentRecord,
    ) -> Result<(), StoreError> {
        upsert_record_checked(
            &self.db,
            "case_reasoning_assessment",
            record.record_id(),
            record,
        )
        .await
    }

    pub async fn write_case_reasoning_assessments(
        &self,
        records: &[CaseReasoningAssessmentRecord],
    ) -> Result<(), StoreError> {
        upsert_batch_checked(&self.db, "case_reasoning_assessment", records, |record| {
            record.record_id()
        })
        .await
    }

    pub async fn write_case_realized_outcomes(
        &self,
        records: &[CaseRealizedOutcomeRecord],
    ) -> Result<(), StoreError> {
        upsert_batch_checked(&self.db, "case_realized_outcome", records, |record| {
            record.record_id()
        })
        .await
    }

    pub async fn sync_symbol_perception_states(
        &self,
        market: &str,
        records: &[SymbolPerceptionStateRecord],
    ) -> Result<(), StoreError> {
        let _guard = self.acquire_table_lock("symbol_perception_state").await;
        sync_market_state_checked(
            &self.db,
            "symbol_perception_state",
            market,
            records,
            |record| record.record_id(),
        )
        .await
    }

    pub async fn recent_symbol_perception_states_by_market(
        &self,
        market: &str,
        limit: usize,
    ) -> Result<Vec<SymbolPerceptionStateRecord>, StoreError> {
        fetch_market_state_records(
            &self.db,
            "symbol_perception_state",
            market,
            "updated_at",
            limit,
        )
        .await
    }

    pub async fn tactical_setup_by_id(
        &self,
        setup_id: &str,
    ) -> Result<Option<TacticalSetupRecord>, StoreError> {
        fetch_optional_record_by_field(&self.db, "tactical_setup", "setup_id", setup_id).await
    }

    pub async fn tactical_setups_by_ids(
        &self,
        setup_ids: &[String],
    ) -> Result<Vec<TacticalSetupRecord>, StoreError> {
        fetch_records_by_ids(&self.db, "tactical_setup", "setup_id", setup_ids).await
    }

    pub async fn recent_case_reasoning_assessments(
        &self,
        setup_id: &str,
        limit: usize,
    ) -> Result<Vec<CaseReasoningAssessmentRecord>, StoreError> {
        fetch_records_by_field_order(
            &self.db,
            "case_reasoning_assessment",
            "setup_id",
            setup_id,
            "recorded_at",
            false,
            limit,
        )
        .await
    }

    pub async fn recent_case_reasoning_assessments_by_market(
        &self,
        market: &str,
        limit: usize,
    ) -> Result<Vec<CaseReasoningAssessmentRecord>, StoreError> {
        fetch_market_state_records(
            &self.db,
            "case_reasoning_assessment",
            market,
            "recorded_at",
            limit,
        )
        .await
    }

    pub async fn recent_case_realized_outcomes_by_market(
        &self,
        market: &str,
        limit: usize,
    ) -> Result<Vec<CaseRealizedOutcomeRecord>, StoreError> {
        fetch_market_state_records(
            &self.db,
            "case_realized_outcome",
            market,
            "resolved_at",
            limit,
        )
        .await
    }

    pub async fn recent_case_realized_outcomes(
        &self,
        setup_id: &str,
        limit: usize,
    ) -> Result<Vec<CaseRealizedOutcomeRecord>, StoreError> {
        fetch_records_by_field_order(
            &self.db,
            "case_realized_outcome",
            "setup_id",
            setup_id,
            "resolved_at",
            false,
            limit,
        )
        .await
    }

    pub async fn write_hypothesis_track(
        &self,
        record: &HypothesisTrackRecord,
    ) -> Result<(), StoreError> {
        upsert_record_checked(&self.db, "hypothesis_track", record.record_id(), record).await
    }

    pub async fn write_hypothesis_tracks(
        &self,
        records: &[HypothesisTrackRecord],
    ) -> Result<(), StoreError> {
        upsert_batch_checked(&self.db, "hypothesis_track", records, |record| {
            record.record_id()
        })
        .await
    }

    pub async fn action_workflow_by_id(
        &self,
        workflow_id: &str,
    ) -> Result<Option<ActionWorkflowRecord>, StoreError> {
        fetch_optional_record_by_field(&self.db, "action_workflow", "workflow_id", workflow_id)
            .await
    }

    pub async fn action_workflows_by_ids(
        &self,
        workflow_ids: &[String],
    ) -> Result<Vec<ActionWorkflowRecord>, StoreError> {
        fetch_records_by_ids(&self.db, "action_workflow", "workflow_id", workflow_ids).await
    }

    pub async fn recent_action_workflows(
        &self,
        limit: usize,
    ) -> Result<Vec<ActionWorkflowRecord>, StoreError> {
        fetch_ordered_records(&self.db, "action_workflow", "recorded_at", false, limit).await
    }

    pub async fn recent_action_workflows_by_market(
        &self,
        market: &str,
        limit: usize,
    ) -> Result<Vec<ActionWorkflowRecord>, StoreError> {
        let result = self
            .db
            .query(
                "SELECT * FROM action_workflow WHERE payload.market = $market ORDER BY recorded_at DESC LIMIT $limit",
            )
            .bind(("market", market.to_string()))
            .bind(("limit", limit))
            .await?;
        take_records(result)
    }

    pub async fn action_workflow_events(
        &self,
        workflow_id: &str,
    ) -> Result<Vec<ActionWorkflowEventRecord>, StoreError> {
        fetch_records_by_field_order(
            &self.db,
            "action_workflow_event",
            "workflow_id",
            workflow_id,
            "recorded_at",
            true,
            WORKFLOW_EVENT_SCAN_LIMIT,
        )
        .await
    }

    pub async fn action_workflow_event_values(
        &self,
        workflow_id: &str,
    ) -> Result<Vec<Value>, StoreError> {
        fetch_records_by_field_order(
            &self.db,
            "action_workflow_event",
            "workflow_id",
            workflow_id,
            "recorded_at",
            true,
            WORKFLOW_EVENT_SCAN_LIMIT,
        )
        .await
    }

    pub async fn latest_action_workflow_recorded_at(
        &self,
    ) -> Result<Option<time::OffsetDateTime>, StoreError> {
        fetch_latest_timestamp_field(&self.db, "action_workflow", "recorded_at").await
    }
}
