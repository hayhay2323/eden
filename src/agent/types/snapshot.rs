use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSnapshot {
    pub tick: u64,
    pub timestamp: String,
    pub market: LiveMarket,
    pub market_regime: LiveMarketRegime,
    pub stress: LiveStressSnapshot,
    pub wake: AgentWakeState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub world_state: Option<WorldStateSnapshot>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub backward_reasoning: Option<BackwardReasoningSnapshot>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notices: Vec<AgentNotice>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub active_structures: Vec<AgentStructureState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub recent_transitions: Vec<AgentTransition>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sector_flows: Vec<AgentSectorFlow>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub symbols: Vec<AgentSymbolState>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub events: Vec<LiveEvent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cross_market_signals: Vec<LiveCrossMarketSignal>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub context_priors: Vec<AgentContextPrior>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub macro_event_candidates: Vec<AgentMacroEventCandidate>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub macro_events: Vec<AgentMacroEvent>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub knowledge_links: Vec<AgentKnowledgeLink>,
}

impl AgentSnapshot {
    pub fn symbol(&self, symbol: &str) -> Option<&AgentSymbolState> {
        self.symbols
            .iter()
            .find(|item| item.symbol.eq_ignore_ascii_case(symbol))
    }

    pub fn backward_investigation(&self, symbol: &str) -> Option<&BackwardInvestigation> {
        self.backward_reasoning
            .as_ref()?
            .investigations
            .iter()
            .find(|item| {
                scope_symbol(&item.leaf_scope)
                    .map(|candidate| candidate.0.eq_ignore_ascii_case(symbol))
                    .unwrap_or(false)
            })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentWakeState {
    pub should_speak: bool,
    pub priority: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub headline: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub summary: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub focus_symbols: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub suggested_tools: Vec<AgentSuggestedToolCall>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentSuggestedToolCall {
    pub tool: String,
    pub args: Value,
    pub reason: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentToolCategory {
    DerivedView,
    Feed,
    ObjectQuery,
    Microstructure,
    GraphQuery,
    CompatQuery,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolSpec {
    pub name: String,
    pub category: AgentToolCategory,
    pub route: String,
    pub method: String,
    pub description: String,
    #[serde(default)]
    pub deprecated: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replacement: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub args: Vec<AgentToolArgSpec>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolArgSpec {
    pub name: String,
    pub required: bool,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentToolRequest {
    pub tool: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub since_tick: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", content = "data", rename_all = "snake_case")]
pub enum AgentToolOutput {
    Wake(AgentWakeState),
    MarketSessionContract(crate::ontology::MarketSessionContract),
    Tools(Vec<AgentToolSpec>),
    Session(AgentSession),
    SymbolContract(crate::ontology::SymbolStateContract),
    Watchlist(AgentWatchlist),
    Recommendations(AgentRecommendations),
    Scoreboard(AgentAlertScoreboard),
    EodReview(AgentEodReview),
    Threads(Vec<AgentThread>),
    Thread(AgentThread),
    Turns(Vec<AgentTurn>),
    Notices(Vec<AgentNotice>),
    Transitions(Vec<AgentTransition>),
    Structures(Vec<AgentStructureState>),
    Structure(AgentStructureState),
    Symbol(AgentSymbolState),
    Depth(AgentDepthState),
    Brokers(AgentBrokerState),
    Invalidation(AgentInvalidationState),
    SectorFlow(Vec<AgentSectorFlow>),
    MacroEventCandidates(Vec<AgentMacroEventCandidate>),
    MacroEvents(Vec<AgentMacroEvent>),
    KnowledgeLinks(Vec<AgentKnowledgeLink>),
    World(WorldStateSnapshot),
    Backward(BackwardInvestigation),
}

impl AgentToolOutput {
    pub fn preview(&self) -> Option<String> {
        match self {
            Self::Wake(wake) => wake
                .headline
                .clone()
                .or_else(|| wake.summary.first().cloned()),
            Self::MarketSessionContract(session) => session
                .wake_headline
                .clone()
                .or_else(|| session.market_summary.clone())
                .or_else(|| session.focus_symbols.first().cloned()),
            Self::Tools(_) => None,
            Self::Session(session) => session
                .recent_turns
                .last()
                .and_then(|turn| turn.headline.clone())
                .or_else(|| session.focus_symbols.first().cloned()),
            Self::SymbolContract(item) => item
                .state
                .structure
                .as_ref()
                .map(|structure| {
                    format!(
                        "{} {} conf={:+}",
                        item.symbol,
                        structure.action,
                        structure.confidence.round_dp(3)
                    )
                })
                .or_else(|| Some(item.symbol.clone())),
            Self::Watchlist(watchlist) => watchlist.entries.first().map(|entry| {
                format!(
                    "{} {} {}",
                    entry.symbol,
                    entry.action,
                    entry.score.round_dp(3)
                )
            }),
            Self::Recommendations(recommendations) => {
                recommendations
                    .decisions
                    .first()
                    .map(|decision| match decision {
                        AgentDecision::Market(item) => format!(
                            "{} {} {}",
                            market_scope_symbol(item.market),
                            item.best_action,
                            item.market_impulse_score.round_dp(3)
                        ),
                        AgentDecision::Sector(item) => format!(
                            "{} {} {}",
                            item.sector,
                            item.best_action,
                            item.sector_impulse_score.round_dp(3)
                        ),
                        AgentDecision::Symbol(item) => format!(
                            "{} {} {}",
                            item.symbol,
                            item.action,
                            item.confidence.round_dp(3)
                        ),
                    })
            }
            Self::Scoreboard(scoreboard) => Some(format!(
                "alerts={} resolved={} hit_rate={}",
                scoreboard.stats.total_alerts,
                scoreboard.stats.resolved_alerts,
                scoreboard.stats.hit_rate.round_dp(3)
            )),
            Self::EodReview(review) => Some(format!(
                "resolved={} hit_rate={} mean_return={}",
                review.resolved_alerts,
                review.hit_rate.round_dp(3),
                review.mean_oriented_return.round_dp(3)
            )),
            Self::Threads(items) => items.first().and_then(|item| item.latest_summary.clone()),
            Self::Thread(item) => item.latest_summary.clone(),
            Self::Turns(items) => items.first().and_then(|item| item.headline.clone()),
            Self::Notices(items) => items.first().map(|item| item.summary.clone()),
            Self::Transitions(items) => items.first().map(|item| item.summary.clone()),
            Self::Structures(items) => items.first().map(|item| {
                format!(
                    "{} {} conf={:+}",
                    item.symbol,
                    item.action,
                    item.confidence.round_dp(3)
                )
            }),
            Self::Structure(item) => Some(format!(
                "{} {} conf={:+}",
                item.symbol,
                item.action,
                item.confidence.round_dp(3)
            )),
            Self::Symbol(item) => item
                .structure
                .as_ref()
                .map(|structure| {
                    format!(
                        "{} {} conf={:+}",
                        item.symbol,
                        structure.action,
                        structure.confidence.round_dp(3)
                    )
                })
                .or_else(|| {
                    item.signal.as_ref().map(|signal| {
                        format!(
                            "{} composite={:+}",
                            item.symbol,
                            signal.composite.round_dp(3)
                        )
                    })
                }),
            Self::Depth(item) => Some(item.summary.clone()),
            Self::Brokers(item) => Some(format!(
                "entered=[{}] exited=[{}]",
                item.entered.join(", "),
                item.exited.join(", ")
            )),
            Self::Invalidation(item) => Some(format!(
                "status={} invalidated={} falsifier={}",
                item.status,
                item.invalidated,
                item.leading_falsifier.as_deref().unwrap_or("-")
            )),
            Self::SectorFlow(items) => items.first().map(|item| item.summary.clone()),
            Self::MacroEventCandidates(items) => items.first().map(|item| item.headline.clone()),
            Self::MacroEvents(items) => items.first().map(|item| item.headline.clone()),
            Self::KnowledgeLinks(items) => items
                .first()
                .map(|item| format!("{} -> {}", item.source.label, item.target.label)),
            Self::World(world) => world
                .entities
                .first()
                .map(|item| format!("{} layer={} regime={}", item.label, item.layer, item.regime)),
            Self::Backward(item) => item.leading_cause.as_ref().map(|cause| {
                format!(
                    "{} lead={} streak={}",
                    item.leaf_label, cause.explanation, item.leading_cause_streak
                )
            }),
        }
    }

    pub fn as_json(&self) -> Value {
        serde_json::to_value(self).unwrap_or(Value::Null)
    }
}
