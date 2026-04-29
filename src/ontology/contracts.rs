use serde::{Deserialize, Serialize};
use time::format_description::well_known::Rfc3339;
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::action::workflow::{
    ActionExecutionPolicy, ActionGovernanceContract, ActionGovernanceReasonCode,
};
use crate::agent::llm::{
    AgentAnalysis, AgentDominantLens, AgentNarration, AgentNarrationActionCard,
};
use crate::agent::{
    AgentActionExpectancies, AgentAlertOutcome, AgentAlertRecord, AgentAlertScoreboard,
    AgentAlertSliceStat, AgentAlertStats, AgentBriefing, AgentDecision, AgentDecisionAttribution,
    AgentEodReview, AgentExecutedTool, AgentJudgmentKind, AgentMarketRecommendation, AgentNotice,
    AgentOperationalJudgment, AgentRecommendation, AgentRecommendations, AgentResolvedAlertDigest,
    AgentSectorFlow, AgentSectorRecommendation, AgentSession, AgentSnapshot,
    AgentSuggestedToolCall, AgentSymbolState, AgentThread, AgentTransition, AgentTurn,
    AgentWatchlist, AgentWatchlistEntry,
};
use crate::cases::{build_case_summaries, CaseMarket, CaseSummary};
use crate::live_snapshot::{
    LiveEvent, LiveMarket, LiveMarketRegime, LiveSnapshot, LiveStressSnapshot, LiveTacticalCase,
};
use crate::ontology::world::{BackwardInvestigation, WorldStateSnapshot};
use crate::ontology::{
    AgentKnowledgeLink, AgentMacroEvent, AgentMacroEventCandidate, ArchetypeProjection,
    CaseChannel, CaseSignature, CaseTemporalShape, CaseTopology, ConflictShape, ExpectationBinding,
    ExpectationViolation, IntentHypothesis, KnowledgeNodeKind, Symbol,
};
use rust_decimal::Decimal;
use std::collections::HashMap;

#[path = "contracts/types.rs"]
mod types;
pub use types::*;

#[path = "contracts/build.rs"]
mod build;
pub use build::*;

#[path = "contracts/derive.rs"]
mod derive;
pub use derive::*;

#[path = "contracts/alerts.rs"]
mod alerts;
pub use alerts::*;

#[cfg(test)]
#[path = "contracts/tests.rs"]
mod tests;
