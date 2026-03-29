//! Curated action/governance doctrine surface.
//!
//! This keeps the operational contract explicit without changing the underlying
//! workflow or narrative logic. The doctrine is grouped into three families:
//! - `actions`: workflow payloads and stage-state records
//! - `governance`: stage progression and workflow-state traits
//! - `narratives`: regime and directional framing used to justify action policy

pub mod actions {
    pub use super::super::workflow::{
        ActionDescriptor, ConfirmedAction, ExecutedAction, MonitoredAction, ReviewedAction,
        SuggestedAction,
    };
}

pub mod governance {
    pub use super::super::workflow::{
        allowed_transition_targets, governance_reason, governance_reason_code,
        validate_assignment_update, validate_queue_pin_update, validate_transition,
        workflow_governance, ActionExecutionPolicy, ActionGovernanceContract,
        ActionGovernanceReasonCode, ActionStage, ActionWorkflowState, WorkflowGovernanceError,
    };
}

pub mod narratives {
    pub use super::super::narrative::{
        Direction, DimensionReading, NarrativeSnapshot, Regime, SymbolNarrative,
    };
}
