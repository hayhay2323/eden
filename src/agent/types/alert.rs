use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAlertScoreboard {
    pub tick: u64,
    pub timestamp: String,
    pub market: LiveMarket,
    pub regime_bias: String,
    pub total: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub alerts: Vec<AgentAlertRecord>,
    pub stats: AgentAlertStats,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_kind: Vec<AgentAlertSliceStat>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_action: Vec<AgentAlertSliceStat>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_scope: Vec<AgentAlertSliceStat>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_regime: Vec<AgentAlertSliceStat>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub by_sector: Vec<AgentAlertSliceStat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAlertRecord {
    pub alert_id: String,
    pub tick: u64,
    #[serde(default)]
    pub scope_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    pub kind: String,
    pub severity: String,
    pub why: String,
    pub suggested_action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_label: Option<String>,
    pub horizon_ticks: u64,
    pub regime_bias: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_at_alert: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reference_value_at_alert: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference_symbols: Vec<String>,
    pub action_bias: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolution: Option<AgentRecommendationResolution>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome_after_n_ticks: Option<AgentAlertOutcome>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAlertOutcome {
    pub resolved_tick: u64,
    pub ticks_elapsed: u64,
    pub status: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub price_return: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oriented_return: Option<Decimal>,
    pub follow_through: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAlertStats {
    pub total_alerts: usize,
    pub resolved_alerts: usize,
    pub hits: usize,
    pub misses: usize,
    pub flats: usize,
    pub hit_rate: Decimal,
    pub mean_oriented_return: Decimal,
    pub false_positive_rate: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAlertSliceStat {
    pub key: String,
    pub total_alerts: usize,
    pub resolved_alerts: usize,
    pub hits: usize,
    pub misses: usize,
    pub flats: usize,
    pub hit_rate: Decimal,
    pub mean_oriented_return: Decimal,
    pub false_positive_rate: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentEodReview {
    pub tick: u64,
    pub timestamp: String,
    pub market: LiveMarket,
    pub regime_bias: String,
    pub total_alerts: usize,
    pub resolved_alerts: usize,
    pub hits: usize,
    pub misses: usize,
    pub flats: usize,
    pub hit_rate: Decimal,
    pub mean_oriented_return: Decimal,
    pub false_positive_rate: Decimal,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effective_kinds: Vec<AgentAlertSliceStat>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub noisy_kinds: Vec<AgentAlertSliceStat>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effective_actions: Vec<AgentAlertSliceStat>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effective_sectors: Vec<AgentAlertSliceStat>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub effective_regimes: Vec<AgentAlertSliceStat>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_hits: Vec<AgentResolvedAlertDigest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub top_misses: Vec<AgentResolvedAlertDigest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub conclusions: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub analyst_lift: Option<AgentAnalystLiftSummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentResolvedAlertDigest {
    pub alert_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    pub kind: String,
    pub suggested_action: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    pub follow_through: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub oriented_return: Option<Decimal>,
    pub why: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentAnalystLiftSummary {
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
