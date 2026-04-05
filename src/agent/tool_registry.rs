use crate::agent::types::{AgentToolArgSpec, AgentToolCategory, AgentToolSpec};

/// Trait for self-contained agent tools.
/// Each tool defines its specification and execution logic.
pub trait AgentTool: Send + Sync {
    /// Returns the tool specification (name, description, category, parameters).
    fn spec(&self) -> AgentToolSpec;

    /// Returns true if this tool is currently enabled.
    fn is_enabled(&self) -> bool {
        true
    }
}

/// Registry holding all registered tools.
pub struct ToolRegistry {
    tools: Vec<Box<dyn AgentTool>>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self { tools: Vec::new() }
    }

    /// Register a tool into the registry.
    pub fn register(&mut self, tool: Box<dyn AgentTool>) {
        self.tools.push(tool);
    }

    /// Get the catalog of all enabled tools.
    pub fn catalog(&self) -> Vec<AgentToolSpec> {
        self.tools
            .iter()
            .filter(|t| t.is_enabled())
            .map(|t| t.spec())
            .collect()
    }

    /// Find a tool by name.
    pub fn find(&self, name: &str) -> Option<&dyn AgentTool> {
        self.tools
            .iter()
            .find(|t| t.spec().name == name)
            .map(|t| t.as_ref())
    }

    /// Number of registered tools.
    pub fn len(&self) -> usize {
        self.tools.len()
    }

    pub fn is_empty(&self) -> bool {
        self.tools.is_empty()
    }

    /// List all tool names.
    pub fn tool_names(&self) -> Vec<String> {
        self.tools.iter().map(|t| t.spec().name.clone()).collect()
    }

    /// Filter tools by category.
    pub fn by_category(&self, category: AgentToolCategory) -> Vec<AgentToolSpec> {
        self.tools
            .iter()
            .filter(|t| t.is_enabled() && t.spec().category == category)
            .map(|t| t.spec())
            .collect()
    }
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Arg-spec helpers
// ---------------------------------------------------------------------------

fn symbol_filter_arg() -> AgentToolArgSpec {
    AgentToolArgSpec {
        name: "symbol".into(),
        required: false,
        description: "Optional symbol filter.".into(),
    }
}

fn symbol_required_arg() -> AgentToolArgSpec {
    AgentToolArgSpec {
        name: "symbol".into(),
        required: true,
        description: "Ticker symbol.".into(),
    }
}

fn sector_filter_arg() -> AgentToolArgSpec {
    AgentToolArgSpec {
        name: "sector".into(),
        required: false,
        description: "Optional sector filter.".into(),
    }
}

fn limit_arg(description: &str) -> AgentToolArgSpec {
    AgentToolArgSpec {
        name: "limit".into(),
        required: false,
        description: description.into(),
    }
}

fn since_tick_arg(description: &str) -> AgentToolArgSpec {
    AgentToolArgSpec {
        name: "since_tick".into(),
        required: false,
        description: description.into(),
    }
}

fn impacted_symbol_filter_arg() -> AgentToolArgSpec {
    AgentToolArgSpec {
        name: "symbol".into(),
        required: false,
        description: "Optional impacted symbol filter.".into(),
    }
}

fn impacted_sector_filter_arg() -> AgentToolArgSpec {
    AgentToolArgSpec {
        name: "sector".into(),
        required: false,
        description: "Optional impacted sector filter.".into(),
    }
}

// ---------------------------------------------------------------------------
// Macro for defining tool structs that implement AgentTool
// ---------------------------------------------------------------------------

macro_rules! define_tool {
    ($struct_name:ident, $name:expr, $category:expr, $route:expr, $method:expr,
     $description:expr, $deprecated:expr, $replacement:expr, $args:expr) => {
        pub struct $struct_name;
        impl AgentTool for $struct_name {
            fn spec(&self) -> AgentToolSpec {
                AgentToolSpec {
                    name: $name.into(),
                    category: $category,
                    route: $route.into(),
                    method: $method.into(),
                    description: $description.into(),
                    deprecated: $deprecated,
                    replacement: $replacement,
                    args: $args,
                }
            }
        }
    };
}

// ---------------------------------------------------------------------------
// Tool definitions (all 29)
// ---------------------------------------------------------------------------

define_tool!(
    MarketSessionTool,
    "market_session",
    AgentToolCategory::ObjectQuery,
    "/api/ontology/:market/market-session",
    "GET",
    "Returns the canonical market-session object contract.",
    false,
    None,
    vec![]
);

define_tool!(
    WakeTool,
    "wake",
    AgentToolCategory::DerivedView,
    "/api/agent/:market/wake",
    "GET",
    "Returns the speech gate decision, focus symbols, and suggested next queries.",
    false,
    None,
    vec![]
);

define_tool!(
    SessionTool,
    "session",
    AgentToolCategory::DerivedView,
    "/api/agent/:market/session",
    "GET",
    "Returns the current analyst session state with threads and recent turns.",
    false,
    Some("market_session".into()),
    vec![]
);

define_tool!(
    InvestigationsTool,
    "investigations",
    AgentToolCategory::DerivedView,
    "/api/agent/:market/investigations",
    "GET",
    "Returns the active object-centric investigations before action judgments compress them.",
    false,
    None,
    vec![
        symbol_filter_arg(),
        sector_filter_arg(),
        limit_arg("Maximum investigations to return.")
    ]
);

define_tool!(
    JudgmentsTool,
    "judgments",
    AgentToolCategory::DerivedView,
    "/api/agent/:market/judgments",
    "GET",
    "Returns object-centric operational judgments ranked as investigate/escalate/govern/execute.",
    false,
    None,
    vec![
        symbol_filter_arg(),
        sector_filter_arg(),
        limit_arg("Maximum judgments to return.")
    ]
);

define_tool!(
    WatchlistTool,
    "watchlist",
    AgentToolCategory::DerivedView,
    "/api/agent/:market/watchlist",
    "GET",
    "Returns the top symbols to watch right now, ranked by decision relevance.",
    false,
    None,
    vec![
        symbol_filter_arg(),
        sector_filter_arg(),
        limit_arg("Maximum watchlist entries to return.")
    ]
);

define_tool!(
    RecommendationsTool,
    "recommendations",
    AgentToolCategory::DerivedView,
    "/api/agent/:market/recommendations",
    "GET",
    "Returns standardized action recommendations tied to the current regime.",
    false,
    None,
    vec![
        symbol_filter_arg(),
        sector_filter_arg(),
        limit_arg("Maximum recommendations to return.")
    ]
);

define_tool!(
    AlertScoreboardTool,
    "alert_scoreboard",
    AgentToolCategory::DerivedView,
    "/api/agent/:market/scoreboard",
    "GET",
    "Returns auditable alerts plus hit-rate and outcome statistics by slice.",
    false,
    None,
    vec![]
);

define_tool!(
    EodReviewTool,
    "eod_review",
    AgentToolCategory::DerivedView,
    "/api/agent/:market/eod-review",
    "GET",
    "Returns the latest end-of-day style review built from the alert scoreboard.",
    false,
    None,
    vec![]
);

define_tool!(
    ThreadsTool,
    "threads",
    AgentToolCategory::DerivedView,
    "/api/agent/:market/threads",
    "GET",
    "Returns the current analyst threads, optionally filtered by symbol or sector.",
    false,
    None,
    vec![
        symbol_filter_arg(),
        sector_filter_arg(),
        limit_arg("Maximum threads to return.")
    ]
);

define_tool!(
    TurnsTool,
    "turns",
    AgentToolCategory::DerivedView,
    "/api/agent/:market/turns",
    "GET",
    "Returns recent analyst turns, optionally filtered by since_tick or symbol.",
    false,
    None,
    vec![
        since_tick_arg("Only return turns newer than this tick."),
        AgentToolArgSpec {
            name: "symbol".into(),
            required: false,
            description: "Optional focus symbol filter.".into(),
        },
        limit_arg("Maximum turns to return.")
    ]
);

define_tool!(
    ActiveStructuresTool,
    "active_structures",
    AgentToolCategory::CompatQuery,
    "/api/agent/:market/structures",
    "GET",
    "Lists currently active structures ranked by confidence.",
    true,
    Some("recommendations".into()),
    vec![
        limit_arg("Maximum structures to return."),
        sector_filter_arg(),
        symbol_filter_arg()
    ]
);

define_tool!(
    StructureStateTool,
    "structure_state",
    AgentToolCategory::CompatQuery,
    "/api/agent/:market/structures/:symbol",
    "GET",
    "Returns the current structure state for one symbol, including age and leader streak.",
    true,
    Some("symbol_contract".into()),
    vec![symbol_required_arg()]
);

define_tool!(
    TransitionsSinceTool,
    "transitions_since",
    AgentToolCategory::Feed,
    "/api/feed/:market/transitions",
    "GET",
    "Returns recent structure transitions after an optional tick threshold.",
    false,
    None,
    vec![
        since_tick_arg("Only return transitions newer than this tick."),
        limit_arg("Maximum transitions to return."),
        symbol_filter_arg(),
        sector_filter_arg()
    ]
);

define_tool!(
    SymbolContractTool,
    "symbol_contract",
    AgentToolCategory::ObjectQuery,
    "/api/ontology/:market/symbols/:symbol",
    "GET",
    "Returns the canonical symbol-state object contract for one symbol.",
    false,
    None,
    vec![symbol_required_arg()]
);

define_tool!(
    MacroEventContractsTool,
    "macro_event_contracts",
    AgentToolCategory::ObjectQuery,
    "/api/ontology/:market/macro-events",
    "GET",
    "Returns canonical macro-event object contracts.",
    false,
    None,
    vec![]
);

define_tool!(
    SymbolStateTool,
    "symbol_state",
    AgentToolCategory::CompatQuery,
    "/api/agent/:market/symbol/:symbol",
    "GET",
    "Returns the full current state for one symbol.",
    true,
    Some("symbol_contract".into()),
    vec![symbol_required_arg()]
);

define_tool!(
    DepthChangeTool,
    "depth_change",
    AgentToolCategory::Microstructure,
    "/api/agent/:market/depth/:symbol",
    "GET",
    "Returns tick-to-tick depth and imbalance changes for one symbol.",
    false,
    None,
    vec![symbol_required_arg()]
);

define_tool!(
    BrokerMovementTool,
    "broker_movement",
    AgentToolCategory::Microstructure,
    "/api/agent/:market/brokers/:symbol",
    "GET",
    "Returns institution entries, exits, and side switches for one symbol.",
    false,
    None,
    vec![symbol_required_arg()]
);

define_tool!(
    InvalidationStatusTool,
    "invalidation_status",
    AgentToolCategory::CompatQuery,
    "/api/agent/:market/invalidation/:symbol",
    "GET",
    "Returns invalidation status, rules, and leading falsifier for one symbol.",
    true,
    Some("symbol_contract".into()),
    vec![symbol_required_arg()]
);

define_tool!(
    SectorFlowTool,
    "sector_flow",
    AgentToolCategory::ObjectQuery,
    "/api/ontology/:market/sector-flows",
    "GET",
    "Returns sector-level flow summaries and exceptions.",
    false,
    None,
    vec![sector_filter_arg(), limit_arg("Maximum sectors to return.")]
);

define_tool!(
    WorldStateTool,
    "world_state",
    AgentToolCategory::ObjectQuery,
    "/api/ontology/:market/world",
    "GET",
    "Returns the current world-state canopy/trunk/leaf snapshot when available.",
    false,
    None,
    vec![]
);

define_tool!(
    BackwardInvestigationTool,
    "backward_investigation",
    AgentToolCategory::ObjectQuery,
    "/api/ontology/:market/backward/:symbol",
    "GET",
    "Returns the current backward causal investigation for one symbol when available.",
    false,
    None,
    vec![symbol_required_arg()]
);

define_tool!(
    NoticesTool,
    "notices",
    AgentToolCategory::Feed,
    "/api/feed/:market/notices",
    "GET",
    "Returns the current notice feed, with optional since_tick and filters.",
    false,
    None,
    vec![
        since_tick_arg("Only return notices newer than this tick."),
        limit_arg("Maximum notices to return."),
        symbol_filter_arg(),
        sector_filter_arg()
    ]
);

define_tool!(
    MacroEventCandidatesTool,
    "macro_event_candidates",
    AgentToolCategory::CompatQuery,
    "/api/agent/:market/query?tool=macro_event_candidates",
    "GET",
    "Returns promoted-from-news/event candidates before final macro-event confirmation.",
    true,
    Some("graph_macro_event_candidates".into()),
    vec![
        since_tick_arg("Only return candidates newer than this tick."),
        limit_arg("Maximum candidates to return."),
        impacted_symbol_filter_arg(),
        impacted_sector_filter_arg()
    ]
);

define_tool!(
    GraphMacroEventCandidatesTool,
    "graph_macro_event_candidates",
    AgentToolCategory::GraphQuery,
    "/api/ontology/:market/macro-event-candidates",
    "GET",
    "Returns graph-oriented macro-event candidates before promotion.",
    false,
    None,
    vec![
        since_tick_arg("Only return candidates newer than this tick."),
        limit_arg("Maximum candidates to return."),
        impacted_symbol_filter_arg(),
        impacted_sector_filter_arg()
    ]
);

define_tool!(
    MacroEventsTool,
    "macro_events",
    AgentToolCategory::CompatQuery,
    "/api/agent/:market/query?tool=macro_events",
    "GET",
    "Returns confirmed macro events and their routed market/sector/symbol impact.",
    true,
    Some("macro_event_contracts".into()),
    vec![
        since_tick_arg("Only return events newer than this tick."),
        limit_arg("Maximum macro events to return."),
        impacted_symbol_filter_arg(),
        impacted_sector_filter_arg()
    ]
);

define_tool!(
    GraphKnowledgeLinksTool,
    "graph_knowledge_links",
    AgentToolCategory::GraphQuery,
    "/api/ontology/:market/knowledge-links",
    "GET",
    "Returns graph-oriented knowledge links filtered on the current ontology snapshot.",
    false,
    None,
    vec![
        symbol_filter_arg(),
        sector_filter_arg(),
        limit_arg("Maximum links to return.")
    ]
);

define_tool!(
    KnowledgeLinksTool,
    "knowledge_links",
    AgentToolCategory::GraphQuery,
    "/api/agent/:market/query?tool=knowledge_links",
    "GET",
    "Returns explicit event-to-market/sector/symbol/decision knowledge-graph links.",
    true,
    Some("graph_knowledge_links".into()),
    vec![
        symbol_filter_arg(),
        sector_filter_arg(),
        limit_arg("Maximum links to return.")
    ]
);

// ---------------------------------------------------------------------------
// Default registry builder
// ---------------------------------------------------------------------------

/// Builds a `ToolRegistry` pre-populated with all 29 agent tools.
pub fn build_default_registry() -> ToolRegistry {
    let mut registry = ToolRegistry::new();
    registry.register(Box::new(MarketSessionTool));
    registry.register(Box::new(WakeTool));
    registry.register(Box::new(SessionTool));
    registry.register(Box::new(InvestigationsTool));
    registry.register(Box::new(JudgmentsTool));
    registry.register(Box::new(WatchlistTool));
    registry.register(Box::new(RecommendationsTool));
    registry.register(Box::new(AlertScoreboardTool));
    registry.register(Box::new(EodReviewTool));
    registry.register(Box::new(ThreadsTool));
    registry.register(Box::new(TurnsTool));
    registry.register(Box::new(ActiveStructuresTool));
    registry.register(Box::new(StructureStateTool));
    registry.register(Box::new(TransitionsSinceTool));
    registry.register(Box::new(SymbolContractTool));
    registry.register(Box::new(MacroEventContractsTool));
    registry.register(Box::new(SymbolStateTool));
    registry.register(Box::new(DepthChangeTool));
    registry.register(Box::new(BrokerMovementTool));
    registry.register(Box::new(InvalidationStatusTool));
    registry.register(Box::new(SectorFlowTool));
    registry.register(Box::new(WorldStateTool));
    registry.register(Box::new(BackwardInvestigationTool));
    registry.register(Box::new(NoticesTool));
    registry.register(Box::new(MacroEventCandidatesTool));
    registry.register(Box::new(GraphMacroEventCandidatesTool));
    registry.register(Box::new(MacroEventsTool));
    registry.register(Box::new(GraphKnowledgeLinksTool));
    registry.register(Box::new(KnowledgeLinksTool));
    registry
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_builds_with_all_tools() {
        let registry = build_default_registry();
        assert!(
            registry.len() >= 29,
            "Expected at least 29 tools, got {}",
            registry.len()
        );
    }

    #[test]
    fn registry_catalog_matches_legacy() {
        let registry = build_default_registry();
        let registry_names: std::collections::HashSet<String> =
            registry.tool_names().into_iter().collect();

        // Build the legacy catalog inline (the non-registry path).
        let legacy_catalog = crate::agent::tools::tool_catalog();
        for spec in &legacy_catalog {
            assert!(
                registry_names.contains(&spec.name),
                "Legacy tool '{}' missing from registry",
                spec.name
            );
        }
    }

    #[test]
    fn find_tool_by_name() {
        let registry = build_default_registry();
        assert!(
            registry.find("market_session").is_some(),
            "market_session should be found"
        );
        assert!(
            registry.find("nonexistent").is_none(),
            "nonexistent should not be found"
        );
    }

    #[test]
    fn filter_by_category() {
        let registry = build_default_registry();
        let object_query_tools = registry.by_category(AgentToolCategory::ObjectQuery);
        assert!(
            !object_query_tools.is_empty(),
            "ObjectQuery category should have tools"
        );
        for tool in &object_query_tools {
            assert_eq!(
                tool.category,
                AgentToolCategory::ObjectQuery,
                "Tool '{}' has wrong category",
                tool.name
            );
        }
    }
}
