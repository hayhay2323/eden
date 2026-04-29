use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::action::workflow::{ActionExecutionPolicy, ActionGovernanceReasonCode};
use crate::agent::{
    AgentActionExpectancies, AgentDecisionAttribution, AgentLensComponent,
    AgentMarketRecommendation,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAnalysis {
    pub tick: u64,
    pub timestamp: String,
    pub market: crate::live_snapshot::LiveMarket,
    pub status: String,
    pub should_speak: bool,
    pub provider: String,
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_action: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub steps: Vec<AgentAnalysisStep>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAnalysisStep {
    pub step: usize,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub args: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNarration {
    pub tick: u64,
    pub timestamp: String,
    pub market: crate::live_snapshot::LiveMarket,
    pub should_alert: bool,
    pub alert_level: String,
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bullets: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus_symbols: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_band: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub what_changed: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub why_it_matters: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub watch_next: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub what_not_to_do: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub fragility: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendation_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_summary_5m: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_recommendation: Option<AgentMarketRecommendation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dominant_lenses: Vec<AgentDominantLens>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub action_cards: Vec<AgentNarrationActionCard>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDominantLens {
    pub lens_name: String,
    pub card_count: usize,
    pub max_confidence: Decimal,
    pub mean_confidence: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNarrationActionCard {
    pub card_id: String,
    #[serde(default)]
    pub scope_kind: String,
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_layer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_id: Option<String>,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_label: Option<String>,
    pub severity: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    pub summary: String,
    pub why_now: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub why_components: Vec<AgentLensComponent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub primary_lens: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub supporting_lenses: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_lens: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_band: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub watch_next: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub do_not: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thesis_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub state_transition: Option<String>,
    pub best_action: String,
    #[serde(flatten)]
    pub action_expectancies: AgentActionExpectancies,
    #[serde(default)]
    pub decision_attribution: AgentDecisionAttribution,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_net_alpha: Option<Decimal>,
    pub alpha_horizon: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preferred_expression: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference_symbols: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalidation_rule: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub invalidation_components: Vec<AgentLensComponent>,
    pub execution_policy: ActionExecutionPolicy,
    pub governance_reason_code: ActionGovernanceReasonCode,
    pub governance_reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAnalystReview {
    pub tick: u64,
    pub timestamp: String,
    pub market: crate::live_snapshot::LiveMarket,
    pub provider: String,
    pub model: String,
    pub final_action: String,
    pub runtime_should_alert: bool,
    pub final_should_alert: bool,
    pub runtime_alert_level: String,
    pub final_alert_level: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runtime_primary_action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub final_primary_action: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_focus_symbols: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub final_focus_symbols: Vec<String>,
    pub decision_changed: bool,
    pub cosmetic_only: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changes: Vec<String>,
    pub lift_assessment: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAnalystScoreboard {
    pub tick: u64,
    pub timestamp: String,
    pub market: crate::live_snapshot::LiveMarket,
    pub total_reviews: usize,
    pub upgraded_attention: usize,
    pub decision_changed: usize,
    pub decision_framing_improved: usize,
    pub cosmetic_rewrite: usize,
    pub minor_refinement: usize,
    pub no_material_change: usize,
    pub material_change_rate: Decimal,
    pub cosmetic_only_rate: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_lift_assessment: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub latest_changes: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub latest_notes: Vec<String>,
}

#[derive(Serialize)]
pub(crate) struct ChatCompletionRequest {
    pub model: String,
    pub messages: Vec<ChatMessage>,
    pub temperature: f32,
}

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct ChatMessage {
    pub role: String,
    pub content: String,
}

#[derive(Deserialize)]
pub(crate) struct ChatCompletionResponse {
    pub choices: Vec<ChatChoice>,
}

#[derive(Deserialize)]
pub(crate) struct ChatChoice {
    pub message: ChatMessageResponse,
}

#[derive(Deserialize)]
pub(crate) struct ChatMessageResponse {
    pub content: Option<String>,
}

#[derive(Debug, Deserialize)]
pub(crate) struct ModelAction {
    pub action: String,
    #[serde(default)]
    pub message: Option<String>,
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub tool: Option<String>,
    #[serde(default)]
    pub symbol: Option<String>,
    #[serde(default)]
    pub sector: Option<String>,
    #[serde(default)]
    pub since_tick: Option<u64>,
    #[serde(default)]
    pub limit: Option<usize>,
}
