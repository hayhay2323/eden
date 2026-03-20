use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::serde::rfc3339;
use time::OffsetDateTime;

/// Compile-oriented action stages in the suggested -> confirm -> execute -> monitor -> review flow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ActionStage {
    Suggest,
    Confirm,
    Execute,
    Monitor,
    Review,
}

impl ActionStage {
    pub const ALL: [Self; 5] = [
        Self::Suggest,
        Self::Confirm,
        Self::Execute,
        Self::Monitor,
        Self::Review,
    ];

    pub fn next(self) -> Option<Self> {
        match self {
            Self::Suggest => Some(Self::Confirm),
            Self::Confirm => Some(Self::Execute),
            Self::Execute => Some(Self::Monitor),
            Self::Monitor => Some(Self::Review),
            Self::Review => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Suggest => "suggest",
            Self::Confirm => "confirm",
            Self::Execute => "execute",
            Self::Monitor => "monitor",
            Self::Review => "review",
        }
    }
}

impl std::fmt::Display for ActionStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Shared payload for every stage in the workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionDescriptor {
    pub workflow_id: String,
    pub title: String,
    pub payload: Value,
}

impl ActionDescriptor {
    pub fn new(workflow_id: impl Into<String>, title: impl Into<String>, payload: Value) -> Self {
        Self {
            workflow_id: workflow_id.into(),
            title: title.into(),
            payload,
        }
    }
}

pub trait ActionWorkflowState {
    fn descriptor(&self) -> &ActionDescriptor;
    fn stage(&self) -> ActionStage;
    fn timestamp(&self) -> OffsetDateTime;
    fn actor(&self) -> Option<&str>;
    fn note(&self) -> Option<&str>;
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SuggestedAction {
    pub descriptor: ActionDescriptor,
    #[serde(with = "rfc3339")]
    pub suggested_at: OffsetDateTime,
    pub suggester: Option<String>,
    pub note: Option<String>,
}

impl SuggestedAction {
    pub fn new(
        descriptor: ActionDescriptor,
        suggested_at: OffsetDateTime,
        suggester: Option<String>,
        note: Option<String>,
    ) -> Self {
        Self {
            descriptor,
            suggested_at,
            suggester,
            note,
        }
    }

    pub fn confirm(
        self,
        confirmed_at: OffsetDateTime,
        confirmer: Option<String>,
        note: Option<String>,
    ) -> ConfirmedAction {
        ConfirmedAction {
            descriptor: self.descriptor,
            confirmed_at,
            confirmer,
            note,
        }
    }
}

impl ActionWorkflowState for SuggestedAction {
    fn descriptor(&self) -> &ActionDescriptor {
        &self.descriptor
    }

    fn stage(&self) -> ActionStage {
        ActionStage::Suggest
    }

    fn timestamp(&self) -> OffsetDateTime {
        self.suggested_at
    }

    fn actor(&self) -> Option<&str> {
        self.suggester.as_deref()
    }

    fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfirmedAction {
    pub descriptor: ActionDescriptor,
    #[serde(with = "rfc3339")]
    pub confirmed_at: OffsetDateTime,
    pub confirmer: Option<String>,
    pub note: Option<String>,
}

impl ConfirmedAction {
    pub fn execute(
        self,
        executed_at: OffsetDateTime,
        executor: Option<String>,
        note: Option<String>,
    ) -> ExecutedAction {
        ExecutedAction {
            descriptor: self.descriptor,
            executed_at,
            executor,
            note,
        }
    }
}

impl ActionWorkflowState for ConfirmedAction {
    fn descriptor(&self) -> &ActionDescriptor {
        &self.descriptor
    }

    fn stage(&self) -> ActionStage {
        ActionStage::Confirm
    }

    fn timestamp(&self) -> OffsetDateTime {
        self.confirmed_at
    }

    fn actor(&self) -> Option<&str> {
        self.confirmer.as_deref()
    }

    fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutedAction {
    pub descriptor: ActionDescriptor,
    #[serde(with = "rfc3339")]
    pub executed_at: OffsetDateTime,
    pub executor: Option<String>,
    pub note: Option<String>,
}

impl ExecutedAction {
    pub fn monitor(
        self,
        monitored_at: OffsetDateTime,
        observer: Option<String>,
        note: Option<String>,
    ) -> MonitoredAction {
        MonitoredAction {
            descriptor: self.descriptor,
            monitored_at,
            observer,
            note,
        }
    }
}

impl ActionWorkflowState for ExecutedAction {
    fn descriptor(&self) -> &ActionDescriptor {
        &self.descriptor
    }

    fn stage(&self) -> ActionStage {
        ActionStage::Execute
    }

    fn timestamp(&self) -> OffsetDateTime {
        self.executed_at
    }

    fn actor(&self) -> Option<&str> {
        self.executor.as_deref()
    }

    fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitoredAction {
    pub descriptor: ActionDescriptor,
    #[serde(with = "rfc3339")]
    pub monitored_at: OffsetDateTime,
    pub observer: Option<String>,
    pub note: Option<String>,
}

impl MonitoredAction {
    pub fn review(
        self,
        reviewed_at: OffsetDateTime,
        reviewer: Option<String>,
        note: Option<String>,
    ) -> ReviewedAction {
        ReviewedAction {
            descriptor: self.descriptor,
            reviewed_at,
            reviewer,
            note,
        }
    }
}

impl ActionWorkflowState for MonitoredAction {
    fn descriptor(&self) -> &ActionDescriptor {
        &self.descriptor
    }

    fn stage(&self) -> ActionStage {
        ActionStage::Monitor
    }

    fn timestamp(&self) -> OffsetDateTime {
        self.monitored_at
    }

    fn actor(&self) -> Option<&str> {
        self.observer.as_deref()
    }

    fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReviewedAction {
    pub descriptor: ActionDescriptor,
    #[serde(with = "rfc3339")]
    pub reviewed_at: OffsetDateTime,
    pub reviewer: Option<String>,
    pub note: Option<String>,
}

impl ActionWorkflowState for ReviewedAction {
    fn descriptor(&self) -> &ActionDescriptor {
        &self.descriptor
    }

    fn stage(&self) -> ActionStage {
        ActionStage::Review
    }

    fn timestamp(&self) -> OffsetDateTime {
        self.reviewed_at
    }

    fn actor(&self) -> Option<&str> {
        self.reviewer.as_deref()
    }

    fn note(&self) -> Option<&str> {
        self.note.as_deref()
    }
}

/// Convenience snapshot for code that only needs the current stage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionWorkflowSnapshot {
    pub workflow_id: String,
    pub title: String,
    pub payload: Value,
    pub stage: ActionStage,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub actor: Option<String>,
    pub note: Option<String>,
}

impl ActionWorkflowSnapshot {
    pub fn from_state<S: ActionWorkflowState>(state: &S) -> Self {
        Self {
            workflow_id: state.descriptor().workflow_id.clone(),
            title: state.descriptor().title.clone(),
            payload: state.descriptor().payload.clone(),
            stage: state.stage(),
            timestamp: state.timestamp(),
            actor: state.actor().map(str::to_owned),
            note: state.note().map(str::to_owned),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ts(seconds: i64) -> OffsetDateTime {
        OffsetDateTime::from_unix_timestamp(seconds).expect("valid timestamp")
    }

    #[test]
    fn stage_progression_is_linear() {
        assert_eq!(ActionStage::Suggest.next(), Some(ActionStage::Confirm));
        assert_eq!(ActionStage::Confirm.next(), Some(ActionStage::Execute));
        assert_eq!(ActionStage::Execute.next(), Some(ActionStage::Monitor));
        assert_eq!(ActionStage::Monitor.next(), Some(ActionStage::Review));
        assert_eq!(ActionStage::Review.next(), None);
    }

    #[test]
    fn state_transitions_preserve_descriptor() {
        let descriptor = ActionDescriptor::new("wf-1", "Test action", json!({"kind": "demo"}));
        let suggested = SuggestedAction::new(
            descriptor,
            ts(1_773_914_400),
            Some("system".to_string()),
            Some("initial".to_string()),
        );

        let confirmed = suggested.confirm(
            ts(1_773_914_460),
            Some("ops".to_string()),
            Some("approved".to_string()),
        );

        assert_eq!(confirmed.descriptor.workflow_id, "wf-1");
        assert_eq!(confirmed.descriptor.title, "Test action");
        assert_eq!(confirmed.stage(), ActionStage::Confirm);

        let snapshot = ActionWorkflowSnapshot::from_state(&confirmed);
        assert_eq!(snapshot.stage, ActionStage::Confirm);
        assert_eq!(snapshot.actor.as_deref(), Some("ops"));
    }
}
