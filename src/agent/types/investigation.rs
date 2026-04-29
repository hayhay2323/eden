use super::*;
use crate::ontology::reasoning::ReviewReasonCode;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInvestigation {
    pub rank: usize,
    pub object_kind: String,
    pub object_id: String,
    pub title: String,
    pub summary: String,
    pub priority: Decimal,
    pub attention_hint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub family_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rationale: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_reason_code: Option<ReviewReasonCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hypothesis_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub runner_up_hypothesis_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backward_investigation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recommendation_id: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reference_symbols: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentInvestigations {
    pub tick: u64,
    pub timestamp: String,
    pub market: LiveMarket,
    pub total: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub items: Vec<AgentInvestigation>,
}
