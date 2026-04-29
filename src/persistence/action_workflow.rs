use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::action::workflow::{
    governance_reason, governance_reason_code, workflow_governance, ActionExecutionPolicy,
    ActionGovernanceContract, ActionGovernanceReasonCode, ActionStage, ActionWorkflowSnapshot,
    ActionWorkflowState,
};

fn default_execution_policy() -> ActionExecutionPolicy {
    ActionExecutionPolicy::ReviewRequired
}

fn default_governance_reason_code() -> ActionGovernanceReasonCode {
    ActionGovernanceReasonCode::WorkflowTransitionWindow
}

/// Persistence row for the latest known workflow state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionWorkflowRecord {
    pub workflow_id: String,
    pub title: String,
    pub payload: Value,
    pub current_stage: ActionStage,
    #[serde(default = "default_execution_policy")]
    pub execution_policy: ActionExecutionPolicy,
    #[serde(default = "default_governance_reason_code")]
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

    pub fn from_us_workflow(workflow: &crate::us::action::workflow::UsActionWorkflow) -> Self {
        let current_stage = action_stage_from_us_stage(workflow.stage);
        let payload = us_workflow_payload(workflow);
        Self {
            workflow_id: workflow.workflow_id.clone(),
            title: format!("Position {}", workflow.symbol),
            payload,
            current_stage,
            execution_policy: ActionExecutionPolicy::ReviewRequired,
            governance_reason_code: governance_reason_code(
                Some(current_stage),
                ActionExecutionPolicy::ReviewRequired,
            ),
            recorded_at: time::OffsetDateTime::now_utc(),
            actor: Some("tracker".into()),
            owner: None,
            reviewer: None,
            queue_pin: None,
            note: workflow.notes.last().cloned(),
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

fn action_stage_from_us_stage(stage: crate::us::action::workflow::UsActionStage) -> ActionStage {
    match stage {
        crate::us::action::workflow::UsActionStage::Suggested => ActionStage::Suggest,
        crate::us::action::workflow::UsActionStage::Confirmed => ActionStage::Confirm,
        crate::us::action::workflow::UsActionStage::Executed
        | crate::us::action::workflow::UsActionStage::Monitoring => ActionStage::Monitor,
        crate::us::action::workflow::UsActionStage::Reviewed => ActionStage::Review,
    }
}

fn us_workflow_payload(workflow: &crate::us::action::workflow::UsActionWorkflow) -> Value {
    serde_json::json!({
        "market": "us",
        "setup_id": workflow.setup_id,
        "symbol": workflow.symbol.0,
        "entry_tick": workflow.entry_tick,
        "stage_entered_tick": workflow.stage_entered_tick,
        "entry_price": workflow.entry_price,
        "confidence_at_entry": workflow.confidence_at_entry,
        "current_confidence": workflow.current_confidence,
        "pnl": workflow.pnl,
        "notes": workflow.notes,
    })
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
    #[serde(default = "default_execution_policy")]
    pub execution_policy: ActionExecutionPolicy,
    #[serde(default = "default_governance_reason_code")]
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
    pub fn from_us_workflow_stage(
        workflow: &crate::us::action::workflow::UsActionWorkflow,
        from_stage: Option<ActionStage>,
        to_stage: ActionStage,
        recorded_at: OffsetDateTime,
        actor: Option<String>,
        note: Option<String>,
    ) -> Self {
        Self {
            event_id: event_id_for(&workflow.workflow_id, to_stage, recorded_at),
            workflow_id: workflow.workflow_id.clone(),
            title: format!("Position {}", workflow.symbol),
            payload: us_workflow_payload(workflow),
            from_stage,
            to_stage,
            execution_policy: ActionExecutionPolicy::ReviewRequired,
            governance_reason_code: governance_reason_code(
                Some(to_stage),
                ActionExecutionPolicy::ReviewRequired,
            ),
            recorded_at,
            actor,
            owner: None,
            reviewer: None,
            queue_pin: None,
            note,
        }
    }

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
        let contract =
            ActionGovernanceContract::for_workflow(Some(self.to_stage), self.execution_policy);
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

pub fn synthetic_workflow_id_for_setup(setup_id: &str) -> String {
    format!("workflow:{setup_id}")
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
    use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
    use crate::ontology::objects::Symbol;
    use crate::ontology::reasoning::{
        default_case_horizon, DecisionLineage, ReasoningScope, TacticalSetup,
    };
    use rust_decimal_macros::dec;
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

    #[test]
    fn us_workflow_converts_into_action_workflow_record() {
        let setup = TacticalSetup {
            setup_id: "setup:AAPL.US:enter".into(),
            hypothesis_id: "hyp:AAPL.US:momentum".into(),
            runner_up_hypothesis_id: None,
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            ),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(Symbol("AAPL.US".into())),
            title: "AAPL.US Momentum Continuation".into(),
            action: "enter".into(),
            direction: None,
            horizon: default_case_horizon(),
            confidence: dec!(0.7),
            confidence_gap: dec!(0.2),
            heuristic_edge: dec!(0.14),
            convergence_score: None,
            convergence_detail: None,
            workflow_id: None,
            entry_rationale: "test".into(),
            causal_narrative: None,
            risk_notes: vec![],
            review_reason_code: None,
            policy_verdict: None,
        };
        let mut workflow =
            crate::us::action::workflow::UsActionWorkflow::from_setup(&setup, 10, Some(dec!(120)));
        workflow.confirm(11).unwrap();

        let record = ActionWorkflowRecord::from_us_workflow(&workflow);
        assert_eq!(record.workflow_id, workflow.workflow_id);
        assert_eq!(record.current_stage, ActionStage::Confirm);
        assert_eq!(record.actor.as_deref(), Some("tracker"));
        assert_eq!(record.payload["market"], json!("us"));
    }

    #[test]
    fn us_workflow_converts_into_action_workflow_event() {
        let setup = TacticalSetup {
            setup_id: "setup:AAPL.US:enter".into(),
            hypothesis_id: "hyp:AAPL.US:momentum".into(),
            runner_up_hypothesis_id: None,
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            ),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(Symbol("AAPL.US".into())),
            title: "AAPL.US Momentum Continuation".into(),
            action: "enter".into(),
            direction: None,
            horizon: default_case_horizon(),
            confidence: dec!(0.7),
            confidence_gap: dec!(0.2),
            heuristic_edge: dec!(0.14),
            convergence_score: None,
            convergence_detail: None,
            workflow_id: None,
            entry_rationale: "test".into(),
            causal_narrative: None,
            risk_notes: vec![],
            review_reason_code: None,
            policy_verdict: None,
        };
        let workflow =
            crate::us::action::workflow::UsActionWorkflow::from_setup(&setup, 10, Some(dec!(120)));
        let event = ActionWorkflowEventRecord::from_us_workflow_stage(
            &workflow,
            None,
            ActionStage::Suggest,
            OffsetDateTime::UNIX_EPOCH,
            Some("tracker".into()),
            Some("generated".into()),
        );

        assert_eq!(event.workflow_id, workflow.workflow_id);
        assert_eq!(event.to_stage, ActionStage::Suggest);
        assert_eq!(event.actor.as_deref(), Some("tracker"));
        assert_eq!(event.note.as_deref(), Some("generated"));
        assert_eq!(event.payload["market"], json!("us"));
    }
}
