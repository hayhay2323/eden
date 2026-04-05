use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentBriefing {
    pub tick: u64,
    pub timestamp: String,
    pub market: LiveMarket,
    pub should_speak: bool,
    pub priority: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spoken_message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus_symbols: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub current_investigations: Vec<AgentInvestigation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub current_judgments: Vec<AgentOperationalJudgment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub executed_tools: Vec<AgentExecutedTool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentExecutedTool {
    pub tool: String,
    pub args: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
    pub result: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTurn {
    pub tick: u64,
    pub timestamp: String,
    pub should_speak: bool,
    pub priority: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spoken_message: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus_symbols: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggered_notice_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub triggered_transition_summaries: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub investigations: Vec<AgentInvestigation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub judgments: Vec<AgentOperationalJudgment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub executed_tools: Vec<AgentExecutedTool>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentThread {
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    pub status: String,
    pub first_tick: u64,
    pub last_tick: u64,
    pub idle_ticks: u64,
    pub turns_observed: u64,
    pub priority: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_transition: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_leader: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalidation_status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_stage: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_next_step: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_policy: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub blocked_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub unlock_condition: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSession {
    pub tick: u64,
    pub timestamp: String,
    pub market: LiveMarket,
    pub should_speak: bool,
    pub active_thread_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus_symbols: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_threads: Vec<AgentThread>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub current_investigations: Vec<AgentInvestigation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub current_judgments: Vec<AgentOperationalJudgment>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_turns: Vec<AgentTurn>,
}
