use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;

use crate::agent::{
    AgentActionExpectancies, AgentBriefing, AgentDecision, AgentDecisionAttribution,
    AgentAlertOutcome, AgentAlertRecord, AgentAlertScoreboard, AgentAlertSliceStat,
    AgentAlertStats, AgentEodReview, AgentExecutedTool, AgentMarketRecommendation, AgentNotice,
    AgentRecommendation, AgentResolvedAlertDigest, AgentSectorFlow, AgentSectorRecommendation,
    AgentSession, AgentSnapshot, AgentSuggestedToolCall, AgentSymbolState, AgentThread,
    AgentTransition, AgentTurn, AgentWatchlist, AgentWatchlistEntry, AgentRecommendations,
};
use crate::agent_llm::{
    AgentAnalysis, AgentDominantLens, AgentNarration, AgentNarrationActionCard,
};
use crate::cases::{build_case_summaries, CaseMarket, CaseSummary};
use crate::live_snapshot::{LiveEvent, LiveMarket, LiveMarketRegime, LiveSnapshot, LiveStressSnapshot};
use crate::action::workflow::{ActionExecutionPolicy, ActionGovernanceContract, ActionGovernanceReasonCode};
use crate::ontology::{
    AgentKnowledgeLink, AgentMacroEvent, AgentMacroEventCandidate, KnowledgeNodeKind, Symbol,
};
use crate::ontology::world::{BackwardInvestigation, WorldStateSnapshot};
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
