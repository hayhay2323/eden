use crate::cases::CaseMarket;
use crate::live_snapshot::LiveMarket;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub enum MarketId {
    Hk,
    Us,
}

impl MarketId {
    pub fn slug(self) -> &'static str {
        match self {
            Self::Hk => "hk",
            Self::Us => "us",
        }
    }
}

impl From<CaseMarket> for MarketId {
    fn from(value: CaseMarket) -> Self {
        match value {
            CaseMarket::Hk => Self::Hk,
            CaseMarket::Us => Self::Us,
        }
    }
}

impl From<MarketId> for CaseMarket {
    fn from(value: MarketId) -> Self {
        match value {
            MarketId::Hk => Self::Hk,
            MarketId::Us => Self::Us,
        }
    }
}

impl From<LiveMarket> for MarketId {
    fn from(value: LiveMarket) -> Self {
        match value {
            LiveMarket::Hk => Self::Hk,
            LiveMarket::Us => Self::Us,
        }
    }
}

impl From<MarketId> for LiveMarket {
    fn from(value: MarketId) -> Self {
        match value {
            MarketId::Hk => Self::Hk,
            MarketId::Us => Self::Us,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ArtifactKind {
    LiveSnapshot,
    BridgeSnapshot,
    AgentSnapshot,
    OperationalSnapshot,
    Briefing,
    Session,
    Watchlist,
    Recommendations,
    RecommendationJournal,
    Scoreboard,
    EodReview,
    Analysis,
    Narration,
    RuntimeNarration,
    AnalystReview,
    AnalystScoreboard,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct MarketCapabilities {
    pub broker_queue: bool,
    pub depth_l2: bool,
    pub capital_flow: bool,
    pub capital_distribution: bool,
    pub pre_post_market: bool,
    pub dual_listing_bridge: bool,
    pub external_priors: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct MarketDefinition {
    pub id: MarketId,
    pub slug: &'static str,
    pub display_name: &'static str,
    pub capabilities: MarketCapabilities,
}

#[derive(Debug, Clone, Copy)]
pub struct ArtifactSpec {
    pub env_var: &'static str,
    pub default_path: &'static str,
}

const HK_DEFINITION: MarketDefinition = MarketDefinition {
    id: MarketId::Hk,
    slug: "hk",
    display_name: "Hong Kong",
    capabilities: MarketCapabilities {
        broker_queue: true,
        depth_l2: true,
        capital_flow: true,
        capital_distribution: true,
        pre_post_market: false,
        dual_listing_bridge: true,
        external_priors: true,
    },
};

const US_DEFINITION: MarketDefinition = MarketDefinition {
    id: MarketId::Us,
    slug: "us",
    display_name: "United States",
    capabilities: MarketCapabilities {
        broker_queue: false,
        depth_l2: false,
        capital_flow: true,
        capital_distribution: false,
        pre_post_market: true,
        dual_listing_bridge: true,
        external_priors: false,
    },
};

pub struct MarketRegistry;

impl MarketRegistry {
    pub fn all() -> &'static [MarketDefinition] {
        &[HK_DEFINITION, US_DEFINITION]
    }

    pub fn by_slug(slug: &str) -> Option<MarketId> {
        Self::all()
            .iter()
            .find(|definition| definition.slug == slug)
            .map(|definition| definition.id)
    }

    pub fn definition(id: MarketId) -> &'static MarketDefinition {
        match id {
            MarketId::Hk => &HK_DEFINITION,
            MarketId::Us => &US_DEFINITION,
        }
    }

    pub fn artifact_spec(market: MarketId, kind: ArtifactKind) -> ArtifactSpec {
        match (market, kind) {
            (MarketId::Hk, ArtifactKind::LiveSnapshot) => ArtifactSpec {
                env_var: "EDEN_LIVE_SNAPSHOT_PATH",
                default_path: "data/live_snapshot.json",
            },
            (MarketId::Us, ArtifactKind::LiveSnapshot) => ArtifactSpec {
                env_var: "EDEN_US_LIVE_SNAPSHOT_PATH",
                default_path: "data/us_live_snapshot.json",
            },
            (MarketId::Hk, ArtifactKind::BridgeSnapshot) => ArtifactSpec {
                env_var: "EDEN_HK_BRIDGE_SNAPSHOT_PATH",
                default_path: "data/hk_bridge_snapshot.json",
            },
            (MarketId::Us, ArtifactKind::BridgeSnapshot) => ArtifactSpec {
                env_var: "EDEN_US_BRIDGE_SNAPSHOT_PATH",
                default_path: "data/us_bridge_snapshot.json",
            },
            (MarketId::Hk, ArtifactKind::AgentSnapshot) => ArtifactSpec {
                env_var: "EDEN_AGENT_SNAPSHOT_PATH",
                default_path: "data/agent_snapshot.json",
            },
            (MarketId::Us, ArtifactKind::AgentSnapshot) => ArtifactSpec {
                env_var: "EDEN_US_AGENT_SNAPSHOT_PATH",
                default_path: "data/us_agent_snapshot.json",
            },
            (MarketId::Hk, ArtifactKind::OperationalSnapshot) => ArtifactSpec {
                env_var: "EDEN_OPERATIONAL_SNAPSHOT_PATH",
                default_path: "data/operational_snapshot.json",
            },
            (MarketId::Us, ArtifactKind::OperationalSnapshot) => ArtifactSpec {
                env_var: "EDEN_US_OPERATIONAL_SNAPSHOT_PATH",
                default_path: "data/us_operational_snapshot.json",
            },
            (MarketId::Hk, ArtifactKind::Briefing) => ArtifactSpec {
                env_var: "EDEN_AGENT_BRIEFING_PATH",
                default_path: "data/agent_briefing.json",
            },
            (MarketId::Us, ArtifactKind::Briefing) => ArtifactSpec {
                env_var: "EDEN_US_AGENT_BRIEFING_PATH",
                default_path: "data/us_agent_briefing.json",
            },
            (MarketId::Hk, ArtifactKind::Session) => ArtifactSpec {
                env_var: "EDEN_AGENT_SESSION_PATH",
                default_path: "data/agent_session.json",
            },
            (MarketId::Us, ArtifactKind::Session) => ArtifactSpec {
                env_var: "EDEN_US_AGENT_SESSION_PATH",
                default_path: "data/us_agent_session.json",
            },
            (MarketId::Hk, ArtifactKind::Watchlist) => ArtifactSpec {
                env_var: "EDEN_AGENT_WATCHLIST_PATH",
                default_path: "data/agent_watchlist.json",
            },
            (MarketId::Us, ArtifactKind::Watchlist) => ArtifactSpec {
                env_var: "EDEN_US_AGENT_WATCHLIST_PATH",
                default_path: "data/us_agent_watchlist.json",
            },
            (MarketId::Hk, ArtifactKind::Recommendations) => ArtifactSpec {
                env_var: "EDEN_AGENT_RECOMMENDATIONS_PATH",
                default_path: "data/agent_recommendations.json",
            },
            (MarketId::Us, ArtifactKind::Recommendations) => ArtifactSpec {
                env_var: "EDEN_US_AGENT_RECOMMENDATIONS_PATH",
                default_path: "data/us_agent_recommendations.json",
            },
            (MarketId::Hk, ArtifactKind::RecommendationJournal) => ArtifactSpec {
                env_var: "EDEN_AGENT_RECOMMENDATION_JOURNAL_PATH",
                default_path: "data/agent_recommendation_journal.jsonl",
            },
            (MarketId::Us, ArtifactKind::RecommendationJournal) => ArtifactSpec {
                env_var: "EDEN_US_AGENT_RECOMMENDATION_JOURNAL_PATH",
                default_path: "data/us_agent_recommendation_journal.jsonl",
            },
            (MarketId::Hk, ArtifactKind::Scoreboard) => ArtifactSpec {
                env_var: "EDEN_AGENT_SCOREBOARD_PATH",
                default_path: "data/agent_scoreboard.json",
            },
            (MarketId::Us, ArtifactKind::Scoreboard) => ArtifactSpec {
                env_var: "EDEN_US_AGENT_SCOREBOARD_PATH",
                default_path: "data/us_agent_scoreboard.json",
            },
            (MarketId::Hk, ArtifactKind::EodReview) => ArtifactSpec {
                env_var: "EDEN_AGENT_EOD_REVIEW_PATH",
                default_path: "data/agent_eod_review.json",
            },
            (MarketId::Us, ArtifactKind::EodReview) => ArtifactSpec {
                env_var: "EDEN_US_AGENT_EOD_REVIEW_PATH",
                default_path: "data/us_agent_eod_review.json",
            },
            (MarketId::Hk, ArtifactKind::Analysis) => ArtifactSpec {
                env_var: "EDEN_AGENT_ANALYSIS_PATH",
                default_path: "data/agent_analysis.json",
            },
            (MarketId::Us, ArtifactKind::Analysis) => ArtifactSpec {
                env_var: "EDEN_US_AGENT_ANALYSIS_PATH",
                default_path: "data/us_agent_analysis.json",
            },
            (MarketId::Hk, ArtifactKind::Narration) => ArtifactSpec {
                env_var: "EDEN_AGENT_NARRATION_PATH",
                default_path: "data/agent_narration.json",
            },
            (MarketId::Us, ArtifactKind::Narration) => ArtifactSpec {
                env_var: "EDEN_US_AGENT_NARRATION_PATH",
                default_path: "data/us_agent_narration.json",
            },
            (MarketId::Hk, ArtifactKind::RuntimeNarration) => ArtifactSpec {
                env_var: "EDEN_AGENT_RUNTIME_NARRATION_PATH",
                default_path: "data/agent_runtime_narration.json",
            },
            (MarketId::Us, ArtifactKind::RuntimeNarration) => ArtifactSpec {
                env_var: "EDEN_US_AGENT_RUNTIME_NARRATION_PATH",
                default_path: "data/us_agent_runtime_narration.json",
            },
            (MarketId::Hk, ArtifactKind::AnalystReview) => ArtifactSpec {
                env_var: "EDEN_AGENT_ANALYST_REVIEW_PATH",
                default_path: "data/agent_analyst_review.json",
            },
            (MarketId::Us, ArtifactKind::AnalystReview) => ArtifactSpec {
                env_var: "EDEN_US_AGENT_ANALYST_REVIEW_PATH",
                default_path: "data/us_agent_analyst_review.json",
            },
            (MarketId::Hk, ArtifactKind::AnalystScoreboard) => ArtifactSpec {
                env_var: "EDEN_AGENT_ANALYST_SCOREBOARD_PATH",
                default_path: "data/agent_analyst_scoreboard.json",
            },
            (MarketId::Us, ArtifactKind::AnalystScoreboard) => ArtifactSpec {
                env_var: "EDEN_US_AGENT_ANALYST_SCOREBOARD_PATH",
                default_path: "data/us_agent_analyst_scoreboard.json",
            },
        }
    }

    pub fn artifact_tuple(market: MarketId, kind: ArtifactKind) -> (&'static str, &'static str) {
        let spec = Self::artifact_spec(market, kind);
        (spec.env_var, spec.default_path)
    }

    pub fn resolve_artifact_path(market: MarketId, kind: ArtifactKind) -> String {
        let spec = Self::artifact_spec(market, kind);
        std::env::var(spec.env_var).unwrap_or_else(|_| spec.default_path.to_string())
    }
}
