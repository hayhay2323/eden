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
pub struct OperationalHistoryRef {
    pub key: String,
    pub endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub count: Option<usize>,
    #[serde(default, skip_serializing_if = "Option::is_none", with = "rfc3339::option")]
    pub latest_at: Option<OffsetDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalGraphRef {
    pub node_id: String,
    pub node_kind: KnowledgeNodeKind,
    pub endpoint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationalObjectKind {
    MarketSession,
    SymbolState,
    Case,
    Recommendation,
    MacroEvent,
    Thread,
    Workflow,
}

impl OperationalObjectKind {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "market_session" => Some(Self::MarketSession),
            "symbol_state" => Some(Self::SymbolState),
            "case" => Some(Self::Case),
            "recommendation" => Some(Self::Recommendation),
            "macro_event" => Some(Self::MacroEvent),
            "thread" => Some(Self::Thread),
            "workflow" => Some(Self::Workflow),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalObjectRef {
    pub id: String,
    pub kind: OperationalObjectKind,
    pub endpoint: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CaseHistoryRefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<OperationalHistoryRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reasoning: Option<OperationalHistoryRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcomes: Option<OperationalHistoryRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct RecommendationHistoryRefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub journal: Option<OperationalHistoryRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<OperationalHistoryRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcomes: Option<OperationalHistoryRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowHistoryRefs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub events: Option<OperationalHistoryRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MarketSessionRelationships {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus_symbols: Vec<OperationalObjectRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct SymbolStateRelationships {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cases: Vec<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub macro_events: Vec<OperationalObjectRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseRelationships {
    pub symbol: OperationalObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<OperationalObjectRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendationRelationships {
    pub symbol: OperationalObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case: Option<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow: Option<OperationalObjectRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MacroEventRelationships {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cases: Vec<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<OperationalObjectRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowRelationships {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cases: Vec<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendations: Vec<OperationalObjectRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalRelationshipGroup {
    pub name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub refs: Vec<OperationalObjectRef>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OperationalNeighborhood {
    pub root: OperationalObjectRef,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relationships: Vec<OperationalRelationshipGroup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_ref: Option<OperationalGraphRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history_refs: Vec<OperationalHistoryRef>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OperationalNavigation {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub self_ref: Option<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph: Option<OperationalGraphRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub history: Vec<OperationalHistoryRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relationships: Vec<OperationalRelationshipGroup>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub neighborhood_endpoint: Option<String>,
}

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
    #[serde(default)]
    pub navigation: OperationalNavigation,
    #[serde(default)]
    pub relationships: MarketSessionRelationships,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus_symbol_refs: Vec<OperationalObjectRef>,
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
    #[serde(default)]
    pub navigation: OperationalNavigation,
    #[serde(default)]
    pub relationships: SymbolStateRelationships,
    pub graph_ref: OperationalGraphRef,
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
    #[serde(default)]
    pub navigation: OperationalNavigation,
    pub relationships: CaseRelationships,
    pub symbol_ref: OperationalObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_ref: Option<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendation_refs: Vec<OperationalObjectRef>,
    pub graph_ref: OperationalGraphRef,
    #[serde(default)]
    pub history_refs: CaseHistoryRefs,
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
    #[serde(default)]
    pub navigation: OperationalNavigation,
    pub relationships: RecommendationRelationships,
    pub symbol_ref: OperationalObjectRef,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub case_ref: Option<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub workflow_ref: Option<OperationalObjectRef>,
    pub graph_ref: OperationalGraphRef,
    pub recommendation: AgentRecommendation,
    #[serde(default)]
    pub history_refs: RecommendationHistoryRefs,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroEventContract {
    pub id: MacroEventContractId,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    #[serde(default)]
    pub navigation: OperationalNavigation,
    #[serde(default)]
    pub relationships: MacroEventRelationships,
    pub graph_ref: OperationalGraphRef,
    pub event: AgentMacroEvent,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThreadContract {
    pub id: ThreadContractId,
    pub market: LiveMarket,
    pub source_tick: u64,
    #[serde(with = "rfc3339")]
    pub observed_at: OffsetDateTime,
    #[serde(default)]
    pub navigation: OperationalNavigation,
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
    #[serde(default)]
    pub navigation: OperationalNavigation,
    #[serde(default)]
    pub relationships: WorkflowRelationships,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub case_refs: Vec<OperationalObjectRef>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recommendation_refs: Vec<OperationalObjectRef>,
    #[serde(default)]
    pub history_refs: WorkflowHistoryRefs,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct OperationalSidecars {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sector_flows: Vec<AgentSectorFlow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub backward_investigations: Vec<BackwardInvestigation>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_state: Option<WorldStateSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub macro_event_candidates: Vec<AgentMacroEventCandidate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub knowledge_links: Vec<AgentKnowledgeLink>,
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
    pub fn market_session_ref(&self) -> OperationalObjectRef {
        OperationalObjectRef {
            id: self.market_session.id.0.clone(),
            kind: OperationalObjectKind::MarketSession,
            endpoint: format!("/api/ontology/{}/market-session", market_slug(self.market)),
            label: self.market_session.wake_headline.clone(),
        }
    }

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

    pub fn world_state(&self) -> Option<&WorldStateSnapshot> {
        self.sidecars.world_state.as_ref()
    }

    pub fn navigation(
        &self,
        kind: OperationalObjectKind,
        id: &str,
    ) -> Option<&OperationalNavigation> {
        match kind {
            OperationalObjectKind::MarketSession => self
                .market_session
                .id
                .0
                .eq_ignore_ascii_case(id)
                .then_some(&self.market_session.navigation),
            OperationalObjectKind::SymbolState => self
                .symbols
                .iter()
                .find(|item| item.id.0.eq_ignore_ascii_case(id) || item.symbol.eq_ignore_ascii_case(id))
                .map(|item| &item.navigation),
            OperationalObjectKind::Case => self.case(id).map(|item| &item.navigation),
            OperationalObjectKind::Recommendation => {
                self.recommendation(id).map(|item| &item.navigation)
            }
            OperationalObjectKind::MacroEvent => self.macro_event(id).map(|item| &item.navigation),
            OperationalObjectKind::Thread => self.thread(id).map(|item| &item.navigation),
            OperationalObjectKind::Workflow => self.workflow(id).map(|item| &item.navigation),
        }
    }

    pub fn resolve_object_ref(&self, object_ref: &OperationalObjectRef) -> Option<OperationalObjectRef> {
        match object_ref.kind {
            OperationalObjectKind::MarketSession => Some(self.market_session_ref()),
            OperationalObjectKind::SymbolState => self
                .symbols
                .iter()
                .find(|item| item.id.0.eq_ignore_ascii_case(&object_ref.id))
                .map(|item| OperationalObjectRef {
                    id: item.id.0.clone(),
                    kind: OperationalObjectKind::SymbolState,
                    endpoint: object_ref.endpoint.clone(),
                    label: Some(item.symbol.clone()),
                }),
            OperationalObjectKind::Case => self
                .cases
                .iter()
                .find(|item| item.id.0.eq_ignore_ascii_case(&object_ref.id))
                .map(|item| OperationalObjectRef {
                    id: item.id.0.clone(),
                    kind: OperationalObjectKind::Case,
                    endpoint: object_ref.endpoint.clone(),
                    label: Some(item.title.clone()),
                }),
            OperationalObjectKind::Recommendation => self
                .recommendations
                .iter()
                .find(|item| item.id.0.eq_ignore_ascii_case(&object_ref.id))
                .map(|item| OperationalObjectRef {
                    id: item.id.0.clone(),
                    kind: OperationalObjectKind::Recommendation,
                    endpoint: object_ref.endpoint.clone(),
                    label: item.recommendation.title.clone(),
                }),
            OperationalObjectKind::MacroEvent => self
                .macro_events
                .iter()
                .find(|item| item.id.0.eq_ignore_ascii_case(&object_ref.id))
                .map(|item| OperationalObjectRef {
                    id: item.id.0.clone(),
                    kind: OperationalObjectKind::MacroEvent,
                    endpoint: object_ref.endpoint.clone(),
                    label: Some(item.event.headline.clone()),
                }),
            OperationalObjectKind::Thread => self
                .threads
                .iter()
                .find(|item| item.id.0.eq_ignore_ascii_case(&object_ref.id))
                .map(|item| OperationalObjectRef {
                    id: item.id.0.clone(),
                    kind: OperationalObjectKind::Thread,
                    endpoint: object_ref.endpoint.clone(),
                    label: item.thread.title.clone().or_else(|| Some(item.thread.symbol.clone())),
                }),
            OperationalObjectKind::Workflow => self
                .workflows
                .iter()
                .find(|item| item.id.0.eq_ignore_ascii_case(&object_ref.id))
                .map(|item| OperationalObjectRef {
                    id: item.id.0.clone(),
                    kind: OperationalObjectKind::Workflow,
                    endpoint: object_ref.endpoint.clone(),
                    label: Some(item.stage.clone()),
                }),
        }
    }

    pub fn neighborhood(
        &self,
        kind: OperationalObjectKind,
        id: &str,
    ) -> Option<OperationalNeighborhood> {
        let navigation = self.navigation(kind, id)?.clone();
        let root = navigation
            .self_ref
            .clone()
            .or_else(|| {
                self.resolve_object_ref(&OperationalObjectRef {
                    id: id.into(),
                    kind,
                    endpoint: String::new(),
                    label: None,
                })
            })?;

        Some(OperationalNeighborhood {
            root,
            relationships: navigation.relationships,
            graph_ref: navigation.graph,
            history_refs: navigation.history,
        })
    }
}

pub(crate) fn case_self_ref(market: LiveMarket, item: &CaseContract) -> OperationalObjectRef {
    OperationalObjectRef {
        id: item.id.0.clone(),
        kind: OperationalObjectKind::Case,
        endpoint: format!("/api/ontology/{}/cases/{}", market_slug(market), item.id.0),
        label: Some(item.title.clone()),
    }
}

pub(crate) fn recommendation_self_ref(
    market: LiveMarket,
    item: &RecommendationContract,
) -> OperationalObjectRef {
    OperationalObjectRef {
        id: item.id.0.clone(),
        kind: OperationalObjectKind::Recommendation,
        endpoint: format!(
            "/api/ontology/{}/recommendations/{}",
            market_slug(market),
            item.id.0
        ),
        label: item.recommendation.title.clone(),
    }
}

pub(crate) fn macro_event_self_ref(
    market: LiveMarket,
    item: &MacroEventContract,
) -> OperationalObjectRef {
    OperationalObjectRef {
        id: item.id.0.clone(),
        kind: OperationalObjectKind::MacroEvent,
        endpoint: format!(
            "/api/ontology/{}/macro-events/{}",
            market_slug(market),
            item.id.0
        ),
        label: Some(item.event.headline.clone()),
    }
}

pub(crate) fn workflow_self_ref(market: LiveMarket, item: &WorkflowContract) -> OperationalObjectRef {
    OperationalObjectRef {
        id: item.id.0.clone(),
        kind: OperationalObjectKind::Workflow,
        endpoint: format!("/api/ontology/{}/workflows/{}", market_slug(market), item.id.0),
        label: Some(item.stage.clone()),
    }
}

pub(crate) fn collect_case_history_refs(item: &CaseContract) -> Vec<OperationalHistoryRef> {
    item.history_refs
        .workflow
        .clone()
        .into_iter()
        .chain(item.history_refs.reasoning.clone())
        .chain(item.history_refs.outcomes.clone())
        .collect()
}

pub(crate) fn collect_recommendation_history_refs(
    item: &RecommendationContract,
) -> Vec<OperationalHistoryRef> {
    item.history_refs
        .journal
        .clone()
        .into_iter()
        .chain(item.history_refs.workflow.clone())
        .chain(item.history_refs.outcomes.clone())
        .collect()
}

pub(crate) fn collect_workflow_history_refs(item: &WorkflowContract) -> Vec<OperationalHistoryRef> {
    item.history_refs.events.clone().into_iter().collect()
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
