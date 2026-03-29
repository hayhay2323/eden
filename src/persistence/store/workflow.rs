use crate::ontology::links::CrossStockPresence;
use crate::persistence::action_workflow::{ActionWorkflowEventRecord, ActionWorkflowRecord};
use crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord;
use crate::persistence::case_reasoning_assessment::CaseReasoningAssessmentRecord;
use crate::persistence::hypothesis_track::HypothesisTrackRecord;
use crate::persistence::tactical_setup::TacticalSetupRecord;

use super::super::store_helpers::{
    fetch_latest_timestamp_field, fetch_market_state_records, fetch_optional_record_by_field,
    fetch_records_by_field_order, fetch_records_by_ids, upsert_batch_checked,
    upsert_record_checked, StoreError,
};
use super::EdenStore;

impl EdenStore {
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
        upsert_record_checked(&self.db, "action_workflow_event", record.record_id(), record).await
    }

    pub async fn write_tactical_setup(
        &self,
        record: &TacticalSetupRecord,
    ) -> Result<(), StoreError> {
        upsert_record_checked(&self.db, "tactical_setup", record.record_id(), record).await
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

    pub async fn write_hypothesis_track(
        &self,
        record: &HypothesisTrackRecord,
    ) -> Result<(), StoreError> {
        upsert_record_checked(&self.db, "hypothesis_track", record.record_id(), record).await
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
            usize::MAX,
        )
        .await
    }

    pub async fn latest_action_workflow_recorded_at(
        &self,
    ) -> Result<Option<time::OffsetDateTime>, StoreError> {
        fetch_latest_timestamp_field(&self.db, "action_workflow", "recorded_at").await
    }

}
