use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::action::workflow::{
    governance_reason, governance_reason_code, workflow_governance, ActionExecutionPolicy,
    ActionGovernanceContract, ActionGovernanceReasonCode, ActionStage, ActionWorkflowSnapshot,
    ActionWorkflowState,
};

/// Persistence row for the latest known workflow state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionWorkflowRecord {
    pub workflow_id: String,
    pub title: String,
    pub payload: Value,
    pub current_stage: ActionStage,
    pub execution_policy: ActionExecutionPolicy,
    pub governance_reason_code: ActionGovernanceReasonCode,
    #[serde(with = "rfc3339")]
    pub recorded_at: OffsetDateTime,
    pub actor: Option<String>,
    pub owner: Option<String>,
    pub reviewer: Option<String>,
    pub queue_pin: Option<String>,
    pub note: Option<String>,
}

impl ActionWorkflowRecord {
    pub fn from_state<S: ActionWorkflowState>(state: &S) -> Self {
        Self {
            workflow_id: state.descriptor().workflow_id.clone(),
            title: state.descriptor().title.clone(),
            payload: state.descriptor().payload.clone(),
            current_stage: state.stage(),
            execution_policy: ActionExecutionPolicy::ReviewRequired,
            governance_reason_code: governance_reason_code(
                Some(state.stage()),
                ActionExecutionPolicy::ReviewRequired,
            ),
            recorded_at: state.timestamp(),
            actor: state.actor().map(str::to_owned),
            owner: None,
            reviewer: None,
            queue_pin: None,
            note: state.note().map(str::to_owned),
        }
    }

    pub fn record_id(&self) -> &str {
        &self.workflow_id
    }

    pub fn governance_summary(&self) -> String {
        let contract = self.governance_contract();
        let allowed = contract
            .allowed_transitions
            .iter()
            .map(|stage| stage.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "policy={} code={} review_required={} auto_execute={} allowed=[{}]",
            contract.execution_policy,
            self.governance_reason_code,
            contract.review_required,
            contract.auto_execute_eligible,
            allowed,
        )
    }
}

/// Persistence row for the append-only transition log.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionWorkflowEventRecord {
    pub event_id: String,
    pub workflow_id: String,
    pub title: String,
    pub payload: Value,
    pub from_stage: Option<ActionStage>,
    pub to_stage: ActionStage,
    pub execution_policy: ActionExecutionPolicy,
    pub governance_reason_code: ActionGovernanceReasonCode,
    #[serde(with = "rfc3339")]
    pub recorded_at: OffsetDateTime,
    pub actor: Option<String>,
    pub owner: Option<String>,
    pub reviewer: Option<String>,
    pub queue_pin: Option<String>,
    pub note: Option<String>,
}

impl ActionWorkflowEventRecord {
    pub fn from_snapshot(snapshot: &ActionWorkflowSnapshot) -> Self {
        Self {
            event_id: event_id_for(&snapshot.workflow_id, snapshot.stage, snapshot.timestamp),
            workflow_id: snapshot.workflow_id.clone(),
            title: snapshot.title.clone(),
            payload: snapshot.payload.clone(),
            from_stage: None,
            to_stage: snapshot.stage,
            execution_policy: snapshot.governance.execution_policy,
            governance_reason_code: governance_reason_code(
                Some(snapshot.stage),
                snapshot.governance.execution_policy,
            ),
            recorded_at: snapshot.timestamp,
            actor: snapshot.actor.clone(),
            owner: None,
            reviewer: None,
            queue_pin: None,
            note: snapshot.note.clone(),
        }
    }

    pub fn from_transition<F: ActionWorkflowState, T: ActionWorkflowState>(
        from: &F,
        to: &T,
    ) -> Self {
        Self {
            event_id: event_id_for(&to.descriptor().workflow_id, to.stage(), to.timestamp()),
            workflow_id: to.descriptor().workflow_id.clone(),
            title: to.descriptor().title.clone(),
            payload: to.descriptor().payload.clone(),
            from_stage: Some(from.stage()),
            to_stage: to.stage(),
            execution_policy: workflow_governance(Some(to.stage())).execution_policy,
            governance_reason_code: governance_reason_code(
                Some(to.stage()),
                workflow_governance(Some(to.stage())).execution_policy,
            ),
            recorded_at: to.timestamp(),
            actor: to.actor().map(str::to_owned),
            owner: None,
            reviewer: None,
            queue_pin: None,
            note: to.note().map(str::to_owned),
        }
    }

    pub fn record_id(&self) -> &str {
        &self.event_id
    }

    pub fn governance_summary(&self) -> String {
        let contract = ActionGovernanceContract::for_workflow(Some(self.to_stage), self.execution_policy);
        let allowed = contract
            .allowed_transitions
            .iter()
            .map(|stage| stage.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        format!(
            "policy={} code={} review_required={} auto_execute={} allowed=[{}]",
            contract.execution_policy,
            self.governance_reason_code,
            contract.review_required,
            contract.auto_execute_eligible,
            allowed,
        )
    }

    pub fn governance_reason(&self) -> String {
        governance_reason(Some(self.to_stage), self.execution_policy)
    }
}

pub fn workflow_record_id(workflow_id: &str) -> String {
    workflow_id.to_string()
}

pub fn event_id_for(workflow_id: &str, stage: ActionStage, recorded_at: OffsetDateTime) -> String {
    format!(
        "{}:{}:{}",
        workflow_id,
        stage.as_str(),
        recorded_at.unix_timestamp_nanos()
    )
}

impl ActionWorkflowRecord {
    pub fn governance_contract(&self) -> ActionGovernanceContract {
        ActionGovernanceContract::for_workflow(Some(self.current_stage), self.execution_policy)
    }

    pub fn governance_reason(&self) -> String {
        governance_reason(Some(self.current_stage), self.execution_policy)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::workflow::{ActionDescriptor, SuggestedAction};
    use serde_json::json;

    fn ts(seconds: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(seconds).expect("valid timestamp")
    }

    #[test]
    fn record_helpers_are_stable() {
        assert_eq!(workflow_record_id("wf-1"), "wf-1");
        assert_eq!(
            event_id_for("wf-1", ActionStage::Review, ts(1_773_914_400)),
            "wf-1:review:1773914400000000000"
        );
    }

    #[test]
    fn snapshot_converts_into_event_record() {
        let descriptor = ActionDescriptor::new(
            "wf-2",
            "Demo",
            json!({
                "k": "v",
                "decision_lineage": {
                    "based_on": ["hyp:700.HK:flow"],
                    "blocked_by": [],
                    "promoted_by": ["review -> enter"],
                    "falsified_by": ["local flow flips negative"]
                }
            }),
        );
        let suggested = SuggestedAction::new(
            descriptor,
            ts(1_773_914_400),
            Some("system".to_string()),
            Some("note".to_string()),
        );
        let snapshot = ActionWorkflowSnapshot::from_state(&suggested);
        let event = ActionWorkflowEventRecord::from_snapshot(&snapshot);

        assert_eq!(event.workflow_id, "wf-2");
        assert_eq!(event.to_stage, ActionStage::Suggest);
        assert_eq!(event.actor.as_deref(), Some("system"));
        assert_eq!(
            event.payload["decision_lineage"]["promoted_by"][0],
            json!("review -> enter")
        );
    }
}
