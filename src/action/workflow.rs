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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionExecutionPolicy {
    ManualOnly,
    ReviewRequired,
    AutoEligible,
}

impl ActionExecutionPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ManualOnly => "manual_only",
            Self::ReviewRequired => "review_required",
            Self::AutoEligible => "auto_eligible",
        }
    }
}

impl std::fmt::Display for ActionExecutionPolicy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionGovernanceReasonCode {
    WorkflowNotCreated,
    WorkflowTransitionWindow,
    AssignmentLockedDuringExecution,
    TerminalReviewStage,
    AdvisoryAction,
    OperatorActionRequired,
    SeverityRequiresReview,
    InvalidationRuleMissing,
    NonPositiveExpectedAlpha,
    AutoExecutionEligible,
}

impl ActionGovernanceReasonCode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorkflowNotCreated => "workflow_not_created",
            Self::WorkflowTransitionWindow => "workflow_transition_window",
            Self::AssignmentLockedDuringExecution => "assignment_locked_during_execution",
            Self::TerminalReviewStage => "terminal_review_stage",
            Self::AdvisoryAction => "advisory_action",
            Self::OperatorActionRequired => "operator_action_required",
            Self::SeverityRequiresReview => "severity_requires_review",
            Self::InvalidationRuleMissing => "invalidation_rule_missing",
            Self::NonPositiveExpectedAlpha => "non_positive_expected_alpha",
            Self::AutoExecutionEligible => "auto_execution_eligible",
        }
    }
}

impl std::fmt::Display for ActionGovernanceReasonCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionGovernanceContract {
    pub current_stage: Option<ActionStage>,
    pub allowed_transitions: Vec<ActionStage>,
    pub execution_policy: ActionExecutionPolicy,
    pub review_required: bool,
    pub auto_execute_eligible: bool,
    pub human_override_supported: bool,
    pub assignment_locked: bool,
    pub terminal: bool,
}

impl ActionGovernanceContract {
    pub fn for_workflow(
        current_stage: Option<ActionStage>,
        execution_policy: ActionExecutionPolicy,
    ) -> Self {
        let allowed_transitions = allowed_transition_targets(current_stage);
        Self {
            current_stage,
            allowed_transitions: allowed_transitions.to_vec(),
            execution_policy,
            review_required: !matches!(execution_policy, ActionExecutionPolicy::AutoEligible),
            auto_execute_eligible: matches!(execution_policy, ActionExecutionPolicy::AutoEligible),
            human_override_supported: current_stage.is_some(),
            assignment_locked: matches!(current_stage, Some(ActionStage::Execute)),
            terminal: matches!(current_stage, Some(ActionStage::Review)),
        }
    }

    pub fn for_recommendation(execution_policy: ActionExecutionPolicy) -> Self {
        Self {
            current_stage: None,
            allowed_transitions: vec![ActionStage::Suggest],
            execution_policy,
            review_required: !matches!(execution_policy, ActionExecutionPolicy::AutoEligible),
            auto_execute_eligible: matches!(execution_policy, ActionExecutionPolicy::AutoEligible),
            human_override_supported: true,
            assignment_locked: false,
            terminal: false,
        }
    }

    pub fn allows_transition(&self, target: ActionStage) -> bool {
        self.allowed_transitions
            .iter()
            .any(|stage| *stage == target)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WorkflowGovernanceError {
    MustStartFromSuggest {
        attempted: ActionStage,
    },
    AlreadyInStage(ActionStage),
    TransitionNotAllowed {
        current: ActionStage,
        target: ActionStage,
        allowed: Vec<ActionStage>,
    },
    QueuePinOwnedByAnotherActor {
        current_owner: String,
        actor: Option<String>,
    },
}

impl std::fmt::Display for WorkflowGovernanceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MustStartFromSuggest { attempted } => write!(
                f,
                "workflow does not exist yet; first transition must be `suggest`, got `{}`",
                attempted.as_str()
            ),
            Self::AlreadyInStage(stage) => {
                write!(f, "workflow is already in the `{}` stage", stage.as_str())
            }
            Self::TransitionNotAllowed {
                current,
                target,
                allowed,
            } => {
                let allowed = allowed
                    .iter()
                    .map(|stage| stage.as_str())
                    .collect::<Vec<_>>()
                    .join(", ");
                write!(
                    f,
                    "invalid transition from `{}` to `{}`; allowed next stages: [{}]",
                    current.as_str(),
                    target.as_str(),
                    allowed
                )
            }
            Self::QueuePinOwnedByAnotherActor {
                current_owner,
                actor,
            } => {
                if let Some(actor) = actor {
                    write!(
                        f,
                        "queue pin is owned by `{}` and cannot be changed by `{}`",
                        current_owner, actor
                    )
                } else {
                    write!(
                        f,
                        "queue pin is owned by `{}` and cannot be changed without an actor",
                        current_owner
                    )
                }
            }
        }
    }
}

impl std::error::Error for WorkflowGovernanceError {}

pub fn allowed_transition_targets(current_stage: Option<ActionStage>) -> &'static [ActionStage] {
    match current_stage {
        None => &[ActionStage::Suggest],
        Some(ActionStage::Suggest) => &[ActionStage::Confirm, ActionStage::Review],
        Some(ActionStage::Confirm) => &[ActionStage::Execute, ActionStage::Review],
        Some(ActionStage::Execute) => &[ActionStage::Monitor, ActionStage::Review],
        Some(ActionStage::Monitor) => &[ActionStage::Review],
        Some(ActionStage::Review) => &[],
    }
}

pub fn workflow_governance(current_stage: Option<ActionStage>) -> ActionGovernanceContract {
    ActionGovernanceContract::for_workflow(current_stage, ActionExecutionPolicy::ReviewRequired)
}

pub fn governance_reason_code(
    current_stage: Option<ActionStage>,
    _execution_policy: ActionExecutionPolicy,
) -> ActionGovernanceReasonCode {
    match current_stage {
        None => ActionGovernanceReasonCode::WorkflowNotCreated,
        Some(ActionStage::Execute) => ActionGovernanceReasonCode::AssignmentLockedDuringExecution,
        Some(ActionStage::Review) => ActionGovernanceReasonCode::TerminalReviewStage,
        Some(_) => ActionGovernanceReasonCode::WorkflowTransitionWindow,
    }
}

pub fn governance_reason(
    current_stage: Option<ActionStage>,
    execution_policy: ActionExecutionPolicy,
) -> String {
    match governance_reason_code(current_stage, execution_policy) {
        ActionGovernanceReasonCode::WorkflowNotCreated => format!(
            "policy={} and workflow has not been created yet; first allowed stage is `suggest`",
            execution_policy
        ),
        ActionGovernanceReasonCode::AssignmentLockedDuringExecution => format!(
            "policy={} and assignment is locked while execution is in progress",
            execution_policy
        ),
        ActionGovernanceReasonCode::TerminalReviewStage => format!(
            "policy={} and workflow is already in terminal review stage",
            execution_policy
        ),
        ActionGovernanceReasonCode::WorkflowTransitionWindow => {
            let stage = current_stage.expect("transition window requires current stage");
            format!(
                "policy={} with current stage `{}` and next allowed transitions [{}]",
                execution_policy,
                stage.as_str(),
                allowed_transition_targets(Some(stage))
                    .iter()
                    .map(|item| item.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        }
        ActionGovernanceReasonCode::AdvisoryAction
        | ActionGovernanceReasonCode::OperatorActionRequired
        | ActionGovernanceReasonCode::SeverityRequiresReview
        | ActionGovernanceReasonCode::InvalidationRuleMissing
        | ActionGovernanceReasonCode::NonPositiveExpectedAlpha
        | ActionGovernanceReasonCode::AutoExecutionEligible => {
            format!("policy={} governs this action", execution_policy)
        }
    }
}

pub fn validate_transition(
    current_stage: Option<ActionStage>,
    target: ActionStage,
) -> Result<(), WorkflowGovernanceError> {
    let contract = workflow_governance(current_stage);
    match current_stage {
        None if target == ActionStage::Suggest => Ok(()),
        None => Err(WorkflowGovernanceError::MustStartFromSuggest { attempted: target }),
        Some(stage) if stage == target => Err(WorkflowGovernanceError::AlreadyInStage(stage)),
        Some(_stage) if contract.allows_transition(target) => Ok(()),
        Some(stage) => Err(WorkflowGovernanceError::TransitionNotAllowed {
            current: stage,
            target,
            allowed: contract.allowed_transitions,
        }),
    }
}

pub fn validate_assignment_update(
    current_stage: Option<ActionStage>,
) -> Result<(), WorkflowGovernanceError> {
    let contract = workflow_governance(current_stage);
    if contract.assignment_locked {
        return Err(WorkflowGovernanceError::TransitionNotAllowed {
            current: current_stage.expect("locked stage must exist"),
            target: ActionStage::Execute,
            allowed: contract.allowed_transitions,
        });
    }
    Ok(())
}

pub fn validate_queue_pin_update(
    current_queue_pin: Option<&str>,
    requested_queue_pin: Option<&Option<String>>,
    actor: Option<&str>,
) -> Result<(), WorkflowGovernanceError> {
    let Some(requested_queue_pin) = requested_queue_pin else {
        return Ok(());
    };
    let current_owner = current_queue_pin
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let requested_owner = requested_queue_pin
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty());

    match (current_owner, requested_owner) {
        (None, _) => Ok(()),
        (Some(current_owner), Some(requested_owner)) if current_owner == requested_owner => Ok(()),
        (Some(current_owner), _) if actor.map(str::trim) == Some(current_owner) => Ok(()),
        (Some(current_owner), _) => Err(WorkflowGovernanceError::QueuePinOwnedByAnotherActor {
            current_owner: current_owner.to_string(),
            actor: actor.map(str::to_owned),
        }),
    }
}

/// Shared payload for every stage in the workflow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActionDescriptor {
    pub workflow_id: String,
    pub title: String,
    pub payload: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_label: Option<String>,
}

impl ActionDescriptor {
    pub fn new(workflow_id: impl Into<String>, title: impl Into<String>, payload: Value) -> Self {
        Self {
            workflow_id: workflow_id.into(),
            title: title.into(),
            payload,
            governance_label: None,
        }
    }

    pub fn with_governance_label(mut self, governance_label: impl Into<String>) -> Self {
        self.governance_label = Some(governance_label.into());
        self
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
    pub governance: ActionGovernanceContract,
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
            governance: workflow_governance(Some(state.stage())),
            timestamp: state.timestamp(),
            actor: state.actor().map(str::to_owned),
            note: state.note().map(str::to_owned),
        }
    }
}

#[cfg(test)]
#[path = "workflow_tests.rs"]
mod tests;
