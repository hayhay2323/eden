use std::sync::Arc;

use reqwest::Client;
use rust_decimal::Decimal;
use serde_json::{json, Value};
use tokio::sync::Semaphore;

use crate::agent::{
    build_alert_scoreboard, build_recommendations, build_watchlist, execute_tool,
    AgentActionExpectancies, AgentBriefing, AgentDecision, AgentDecisionAttribution,
    AgentRecommendations, AgentSession, AgentSnapshot, AgentToolRequest, AgentWatchlist,
};
use crate::cases::CaseMarket;
use crate::core::market::{ArtifactKind, MarketId, MarketRegistry};
use crate::live_snapshot::spawn_write_json_snapshot;

mod analysis;
mod artifacts;
mod config;
mod narration;
mod protocol;
mod types;

pub use analysis::{
    deterministic_analysis, run_analysis, run_or_fallback_analysis, spawn_analysis_if_enabled,
};
pub use artifacts::{
    analysis_path, analyst_review_path, analyst_scoreboard_path,
    build_analyst_review_from_artifacts, build_analyst_scoreboard_from_review, load_analysis,
    load_analyst_review, load_analyst_scoreboard, load_final_narration, load_narration,
    load_runtime_narration, narration_path, runtime_narration_path,
};
pub use config::AnalystConfig;
pub use narration::{build_codex_stale_narration, build_narration};
pub use types::{
    AgentAnalysis, AgentAnalysisStep, AgentAnalystReview, AgentAnalystScoreboard,
    AgentDominantLens, AgentNarration, AgentNarrationActionCard,
};

#[cfg(test)]
pub(crate) use config::first_present_env;
pub(crate) use config::{newest_existing_path, resolved_path};
#[cfg(test)]
pub(crate) use protocol::parse_action;
pub(crate) use types::{ChatCompletionRequest, ChatCompletionResponse, ChatMessage, ModelAction};

const DEFAULT_TIMEOUT_MS: u64 = 15_000;
const DEFAULT_MAX_STEPS: usize = 4;
pub const CODEX_FRESH_TICK_WINDOW: u64 = 8;

#[cfg(test)]
mod tests;
