//! Curated ontology kernel contract.
//!
//! This module does not introduce new storage or reasoning behavior. It makes
//! the platform constitution explicit by grouping the stable ontology surface
//! into five contract families:
//! - `refs`: canonical identifiers and open-ended scopes
//! - `facts`: provenance-carrying observations, events, and derived signals
//! - `events`: promoted knowledge and causal event structures
//! - `actions`: decision and setup objects that can drive workflows
//! - `memory`: durable world-state and knowledge graph projections

pub mod refs {
    pub use super::super::objects::{
        CustomScopeId, InstitutionId, Market, MarketScopeId, RegionId, SectorId, Symbol, ThemeId,
    };
    pub use super::super::reasoning::{PropagationPath, PropagationStep, ReasoningScope};
}

pub mod facts {
    pub use super::super::domain::{
        DerivedSignal, Event, Observation, ProvenanceMetadata, ProvenanceSource,
    };
}

pub mod events {
    pub use super::super::knowledge::{
        AgentEventImpact, AgentKnowledgeEvent, AgentKnowledgeLink, AgentKnowledgeNodeRef,
        AgentMacroEvent, AgentMacroEventCandidate, EvidenceRef, EvidenceRefKind,
        KnowledgeEventKind, KnowledgeLinkAttributes, KnowledgeRelation,
    };
    pub use super::super::world::{
        BackwardCause, BackwardEvidenceItem, BackwardInvestigation, BackwardReasoningSnapshot,
        CausalContestState, WorldLayer,
    };
}

pub mod actions {
    pub use super::super::reasoning::{
        DecisionLineage, Hypothesis, HypothesisTrack, HypothesisTrackStatus,
        InvestigationSelection, TacticalSetup,
    };
}

pub mod memory {
    pub use super::super::knowledge::{
        AgentKnowledgeEvent, AgentKnowledgeLink, AgentKnowledgeNodeRef, AgentMacroEvent,
    };
    pub use super::super::world::{EntityState, WorldStateSnapshot};
}
