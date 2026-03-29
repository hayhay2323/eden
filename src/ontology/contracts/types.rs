use super::*;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MarketSessionId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SymbolStateId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct CaseContractId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct RecommendationContractId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct MacroEventContractId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ThreadContractId(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkflowContractId(pub String);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketSessionContract {
    pub id: MarketSessionId,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    #[serde(with = "rfc3339")]
    pub computed_at: OffsetDateTime,
    pub market_regime: LiveMarketRegime,
    pub stress: LiveStressSnapshot,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus_symbols: Vec<String>,
    pub should_speak: bool,
    pub priority: Decimal,
    pub active_thread_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub wake_headline: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wake_summary: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub wake_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_tools: Vec<AgentSuggestedToolCall>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_summary: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolStateContract {
    pub id: SymbolStateId,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    pub state: AgentSymbolState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseContract {
    pub id: CaseContractId,
    pub setup_id: String,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    pub title: String,
    pub action: String,
    pub workflow_state: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_policy: Option<ActionExecutionPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_reason_code: Option<ActionGovernanceReasonCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_reason: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_pin: Option<String>,
    pub confidence: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence_gap: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thesis_family: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub current_leader: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub invalidation_rule: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expected_net_alpha: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alpha_horizon: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendation_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendationContract {
    pub id: RecommendationContractId,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_case_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_setup_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub related_workflow_id: Option<String>,
    pub recommendation: AgentRecommendation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroEventContract {
    pub id: MacroEventContractId,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    pub event: AgentMacroEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadContract {
    pub id: ThreadContractId,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    pub thread: AgentThread,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowContract {
    pub id: WorkflowContractId,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    pub stage: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution_policy: Option<ActionExecutionPolicy>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub governance_reason_code: Option<ActionGovernanceReasonCode>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub owner: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reviewer: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_pin: Option<String>,
    pub synthetic: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub case_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendation_ids: Vec<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OperationalSidecars {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sector_flows: Vec<AgentSectorFlow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backward_investigations: Vec<BackwardInvestigation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalSnapshot {
    pub version: u32,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    #[serde(with = "rfc3339")]
    pub computed_at: OffsetDateTime,
    pub market_session: MarketSessionContract,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_turns: Vec<AgentTurn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<AgentNotice>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_transitions: Vec<AgentTransition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<SymbolStateContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cases: Vec<CaseContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_recommendation: Option<AgentMarketRecommendation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sector_recommendations: Vec<AgentSectorRecommendation>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<RecommendationContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub macro_events: Vec<MacroEventContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub threads: Vec<ThreadContract>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workflows: Vec<WorkflowContract>,
    #[serde(default)]
    pub sidecars: OperationalSidecars,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<LiveEvent>,
}

impl OperationalSnapshot {
    pub fn symbol(&self, symbol: &str) -> Option<&SymbolStateContract> {
        self.symbols
            .iter()
            .find(|item| item.symbol.eq_ignore_ascii_case(symbol))
    }

    pub fn case(&self, case_id: &str) -> Option<&CaseContract> {
        self.cases
            .iter()
            .find(|item| item.id.0.eq_ignore_ascii_case(case_id))
    }

    pub fn recommendation(&self, recommendation_id: &str) -> Option<&RecommendationContract> {
        self.recommendations
            .iter()
            .find(|item| item.id.0.eq_ignore_ascii_case(recommendation_id))
    }

    pub fn macro_event(&self, event_id: &str) -> Option<&MacroEventContract> {
        self.macro_events
            .iter()
            .find(|item| item.id.0.eq_ignore_ascii_case(event_id))
    }

    pub fn thread(&self, thread_id: &str) -> Option<&ThreadContract> {
        self.threads
            .iter()
            .find(|item| item.id.0.eq_ignore_ascii_case(thread_id))
    }

    pub fn workflow(&self, workflow_id: &str) -> Option<&WorkflowContract> {
        self.workflows
            .iter()
            .find(|item| item.id.0.eq_ignore_ascii_case(workflow_id))
    }

    pub fn sector_flow(&self, sector: &str) -> Option<&AgentSectorFlow> {
        self.sidecars
            .sector_flows
            .iter()
            .find(|item| item.sector.eq_ignore_ascii_case(sector))
    }

    pub fn backward_investigation(&self, symbol: &str) -> Option<&BackwardInvestigation> {
        self.sidecars
            .backward_investigations
            .iter()
            .find(|item| match &item.leaf_scope {
                crate::ontology::ReasoningScope::Symbol(candidate) => {
                    candidate.0.eq_ignore_ascii_case(symbol)
                }
                _ => false,
            })
    }
}

pub fn operational_snapshot_path(market: CaseMarket) -> String {
    match market {
        CaseMarket::Hk => std::env::var("EDEN_HK_OPERATIONAL_SNAPSHOT_PATH")
            .or_else(|_| std::env::var("EDEN_OPERATIONAL_SNAPSHOT_PATH"))
            .unwrap_or_else(|_| "data/operational_snapshot.json".into()),
        CaseMarket::Us => std::env::var("EDEN_US_OPERATIONAL_SNAPSHOT_PATH")
            .unwrap_or_else(|_| "data/us_operational_snapshot.json".into()),
    }
}

pub async fn load_operational_snapshot(
    market: CaseMarket,
) -> Result<OperationalSnapshot, Box<dyn std::error::Error>> {
    let path = operational_snapshot_path(market);
    let content = tokio::fs::read_to_string(&path).await?;
    Ok(serde_json::from_str(&content)?)
}
