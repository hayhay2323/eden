use super::*;
use crate::action::workflow::{ActionExecutionPolicy, ActionGovernanceReasonCode};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentJudgmentKind {
    Investigate,
    Escalate,
    Govern,
    Execute,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentOperationalJudgment {
    pub rank: usize,
    pub kind: AgentJudgmentKind,
    pub object_kind: String,
    pub object_id: String,
    pub title: String,
    pub summary: String,
    pub priority: Decimal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub best_action: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_policy: Option<ActionExecutionPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_reason_code: Option<ActionGovernanceReasonCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference_symbols: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentJudgments {
    pub tick: u64,
    pub timestamp: String,
    pub market: LiveMarket,
    pub total: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<AgentOperationalJudgment>,
}
