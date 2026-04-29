use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentNotice {
    pub notice_id: String,
    pub tick: u64,
    pub kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    pub title: String,
    pub summary: String,
    pub significance: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentTransition {
    pub from_tick: u64,
    pub to_tick: u64,
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_id: Option<String>,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_state: Option<String>,
    pub to_state: String,
    pub confidence: Decimal,
    pub summary: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_reason: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentStructureState {
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub setup_id: Option<String>,
    pub title: String,
    pub action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub age_ticks: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub status_streak: Option<u64>,
    pub confidence: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_change: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_gap: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contest_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_leader: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader_streak: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leader_transition_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thesis_family: Option<String>,
    #[serde(flatten)]
    pub action_expectancies: AgentActionExpectancies,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_net_alpha: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha_horizon: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalidation_rule: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSignalState {
    pub composite: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mark_price: Option<Decimal>,
    #[serde(default)]
    pub capital_flow_direction: Decimal,
    #[serde(default)]
    pub price_momentum: Decimal,
    #[serde(default)]
    pub volume_profile: Decimal,
    #[serde(default)]
    pub pre_post_market_anomaly: Decimal,
    #[serde(default)]
    pub valuation: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector_coherence: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cross_stock_correlation: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cross_market_propagation: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentDepthState {
    pub imbalance: Decimal,
    pub imbalance_change: Decimal,
    pub bid_best_ratio: Decimal,
    pub bid_best_ratio_change: Decimal,
    pub ask_best_ratio: Decimal,
    pub ask_best_ratio_change: Decimal,
    pub bid_top3_ratio: Decimal,
    pub bid_top3_ratio_change: Decimal,
    pub ask_top3_ratio: Decimal,
    pub ask_top3_ratio_change: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spread_change: Option<Decimal>,
    #[serde(default)]
    pub bid_total_volume: i64,
    #[serde(default)]
    pub ask_total_volume: i64,
    pub bid_total_volume_change: i64,
    pub ask_total_volume_change: i64,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentBrokerInstitution {
    pub institution_id: i32,
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub bid_positions: Vec<i32>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub ask_positions: Vec<i32>,
    pub seat_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentBrokerState {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub current: Vec<AgentBrokerInstitution>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub entered: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exited: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub switched_to_bid: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub switched_to_ask: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInvalidationState {
    pub status: String,
    pub invalidated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub transition_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub leading_falsifier: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub rules: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSectorFlow {
    pub sector: String,
    pub member_count: usize,
    pub average_composite: Decimal,
    pub average_capital_flow: Decimal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub leaders: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub exceptions: Vec<String>,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSymbolState {
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub structure: Option<AgentStructureState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signal: Option<AgentSignalState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth: Option<AgentDepthState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brokers: Option<AgentBrokerState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalidation: Option<AgentInvalidationState>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pressure: Option<LivePressure>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub active_position: Option<ActionNode>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub latest_events: Vec<LiveEvent>,
}

/// Compact projection of `PersistentSymbolState` for agent consumption.
///
/// The full state object carries heavy evidence arrays the LLM does not need.
/// This projection preserves the signals that drive operator-facing judgement:
/// state kind, trend direction, how long the state has persisted, and the
/// reason codes that let the analyst explain *why* the state is what it is.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPerceptionState {
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    pub state_kind: String,
    pub label: String,
    pub trend: String,
    pub confidence: Decimal,
    pub strength: Decimal,
    pub state_persistence_ticks: u16,
    pub direction_stability_rounds: u16,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reason_codes: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_transition_summary: Option<String>,
}

impl AgentPerceptionState {
    pub fn from_persistent(state: &crate::pipeline::state_engine::PersistentSymbolState) -> Self {
        Self {
            symbol: state.symbol.clone(),
            sector: state.sector.clone(),
            state_kind: state.state_kind.as_str().to_string(),
            label: state.label.clone(),
            trend: state.trend.as_str().to_string(),
            confidence: state.confidence,
            strength: state.strength,
            state_persistence_ticks: state.state_persistence_ticks,
            direction_stability_rounds: state.direction_stability_rounds,
            direction: state.direction.clone(),
            reason_codes: state.reason_codes(),
            last_transition_summary: state.last_transition_summary.clone(),
        }
    }
}
