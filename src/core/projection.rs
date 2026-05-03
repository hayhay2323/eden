use crate::agent::llm::{build_narration, AgentNarration};
use crate::agent::{
    build_alert_scoreboard, build_briefing, build_eod_review, build_hk_agent_snapshot,
    build_perception_report, build_session, build_us_agent_snapshot, build_watchlist,
    AgentAlertScoreboard, AgentBriefing, AgentEodReview, AgentPerceptionReport,
    AgentRecommendations, AgentSession, AgentSnapshot, AgentWatchlist,
};
use crate::live_snapshot::LiveSnapshot;
use crate::ontology::links::LinkSnapshot;
use crate::ontology::store::ObjectStore;
use crate::ontology::{
    build_operational_snapshot, derive_agent_briefing, derive_agent_eod_review,
    derive_agent_narration, derive_agent_recommendations, derive_agent_scoreboard,
    derive_agent_session, derive_agent_watchlist,
};
use crate::temporal::buffer::TickHistory;
use crate::temporal::lineage::FamilyContextLineageOutcome;
use crate::us::pipeline::reasoning::UsReasoningSnapshot;
use crate::us::pipeline::world::UsBackwardSnapshot;
use crate::us::temporal::buffer::UsTickHistory;
use crate::us::temporal::lineage::UsLineageStats;

pub struct ProjectionBundle {
    pub live_snapshot: LiveSnapshot,
    pub agent_snapshot: AgentSnapshot,
    pub agent_briefing: AgentBriefing,
    pub agent_session: AgentSession,
    pub agent_recommendations: AgentRecommendations,
    pub agent_perception: AgentPerceptionReport,
    pub agent_watchlist: AgentWatchlist,
    pub agent_scoreboard: AgentAlertScoreboard,
    pub agent_eod_review: AgentEodReview,
    pub agent_narration: AgentNarration,
}

pub struct HkProjectionInputs<'a> {
    pub live_snapshot: LiveSnapshot,
    pub history: &'a TickHistory,
    pub links: &'a LinkSnapshot,
    pub store: &'a ObjectStore,
    pub lineage_priors: &'a [FamilyContextLineageOutcome],
    pub previous_agent_snapshot: Option<&'a AgentSnapshot>,
    pub previous_agent_session: Option<&'a AgentSession>,
    pub previous_agent_scoreboard: Option<&'a AgentAlertScoreboard>,
    /// HK-specific signal momentum tracker. When present, `project_hk`
    /// appends `institutional_flow` / `depth_imbalance` / `trade_aggression`
    /// peak/collapse narratives onto the generated agent snapshot's
    /// `wake.reasons` so operator-facing surfaces can see the
    /// second-derivative health of HK microstructure signals.
    pub hk_momentum: Option<&'a crate::temporal::lineage::HkSignalMomentumTracker>,
    pub perception_graph: &'a std::sync::RwLock<crate::perception::PerceptionGraph>,
    }

    pub struct UsProjectionInputs<'a> {
    pub live_snapshot: LiveSnapshot,
    pub history: &'a UsTickHistory,
    pub store: &'a ObjectStore,
    pub reasoning: &'a UsReasoningSnapshot,
    pub backward: &'a UsBackwardSnapshot,
    pub lineage_stats: &'a UsLineageStats,
    pub previous_agent_snapshot: Option<&'a AgentSnapshot>,
    pub previous_agent_session: Option<&'a AgentSession>,
    pub previous_agent_scoreboard: Option<&'a AgentAlertScoreboard>,
    pub perception_graph: &'a std::sync::RwLock<crate::perception::PerceptionGraph>,
    }

struct AgentProjectionSurfaces {
    briefing: AgentBriefing,
    session: AgentSession,
    recommendations: AgentRecommendations,
    perception: AgentPerceptionReport,
    watchlist: AgentWatchlist,
    scoreboard: AgentAlertScoreboard,
    eod_review: AgentEodReview,
    narration: AgentNarration,
}

fn empty_agent_recommendations(snapshot: &AgentSnapshot) -> AgentRecommendations {
    AgentRecommendations {
        tick: snapshot.tick,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        regime_bias: snapshot.market_regime.bias.clone(),
        total: 0,
        market_recommendation: None,
        decisions: Vec::new(),
        items: Vec::new(),
        knowledge_links: Vec::new(),
    }
}

fn project_agent_surfaces(
    live_snapshot: &LiveSnapshot,
    agent_snapshot: &AgentSnapshot,
    previous_agent_session: Option<&AgentSession>,
    previous_agent_scoreboard: Option<&AgentAlertScoreboard>,
) -> AgentProjectionSurfaces {
    let bootstrap_briefing = build_briefing(agent_snapshot);
    let bootstrap_session =
        build_session(agent_snapshot, &bootstrap_briefing, previous_agent_session);
    let bootstrap_recommendations = empty_agent_recommendations(agent_snapshot);
    let perception = build_perception_report(agent_snapshot);

    if let Ok(operational) = build_operational_snapshot(
        live_snapshot,
        agent_snapshot,
        &bootstrap_session,
        &bootstrap_recommendations,
        None,
    ) {
        let scoreboard = derive_agent_scoreboard(&operational, previous_agent_scoreboard);
        let eod_review = derive_agent_eod_review(&operational, &scoreboard);
        return AgentProjectionSurfaces {
            briefing: derive_agent_briefing(&operational),
            session: derive_agent_session(&operational),
            recommendations: derive_agent_recommendations(&operational),
            perception,
            watchlist: derive_agent_watchlist(&operational, 8),
            scoreboard,
            eod_review,
            narration: derive_agent_narration(&operational, None),
        };
    }

    let watchlist = build_watchlist(
        agent_snapshot,
        Some(&bootstrap_session),
        Some(&bootstrap_recommendations),
        8,
    );
    let scoreboard = build_alert_scoreboard(
        agent_snapshot,
        Some(&bootstrap_recommendations),
        previous_agent_scoreboard,
    );
    let eod_review = build_eod_review(agent_snapshot, &scoreboard);
    let narration = build_narration(
        agent_snapshot,
        &bootstrap_briefing,
        &bootstrap_session,
        Some(&watchlist),
        Some(&bootstrap_recommendations),
        None,
    );

    AgentProjectionSurfaces {
        briefing: bootstrap_briefing,
        session: bootstrap_session,
        recommendations: bootstrap_recommendations,
        perception,
        watchlist,
        scoreboard,
        eod_review,
        narration,
    }
}

pub fn project_hk(inputs: HkProjectionInputs<'_>) -> ProjectionBundle {
    let mut agent_snapshot = build_hk_agent_snapshot(
        &inputs.live_snapshot,
        inputs.history,
        inputs.links,
        inputs.store,
        inputs.lineage_priors,
        inputs.previous_agent_snapshot,
        inputs.perception_graph,
    );
    if let Some(hk_momentum) = inputs.hk_momentum {
        let momentum_reasons = crate::agent::attention::describe_momentum_health(
            [
                ("institutional_flow", &hk_momentum.institutional_flow),
                ("depth_imbalance", &hk_momentum.depth_imbalance),
                ("trade_aggression", &hk_momentum.trade_aggression),
            ],
            5,
        );
        agent_snapshot.wake.reasons.extend(momentum_reasons);
    }
    let surfaces = project_agent_surfaces(
        &inputs.live_snapshot,
        &agent_snapshot,
        inputs.previous_agent_session,
        inputs.previous_agent_scoreboard,
    );

    ProjectionBundle {
        live_snapshot: inputs.live_snapshot,
        agent_snapshot,
        agent_briefing: surfaces.briefing,
        agent_session: surfaces.session,
        agent_recommendations: surfaces.recommendations,
        agent_perception: surfaces.perception,
        agent_watchlist: surfaces.watchlist,
        agent_scoreboard: surfaces.scoreboard,
        agent_eod_review: surfaces.eod_review,
        agent_narration: surfaces.narration,
    }
}

pub fn project_us(inputs: UsProjectionInputs<'_>) -> ProjectionBundle {
    let agent_snapshot = build_us_agent_snapshot(
        &inputs.live_snapshot,
        inputs.history,
        inputs.reasoning,
        inputs.backward,
        inputs.store,
        inputs.lineage_stats,
        inputs.previous_agent_snapshot,
        inputs.perception_graph,
    );
    let surfaces = project_agent_surfaces(
        &inputs.live_snapshot,
        &agent_snapshot,
        inputs.previous_agent_session,
        inputs.previous_agent_scoreboard,
    );

    ProjectionBundle {
        live_snapshot: inputs.live_snapshot,
        agent_snapshot,
        agent_briefing: surfaces.briefing,
        agent_session: surfaces.session,
        agent_recommendations: surfaces.recommendations,
        agent_perception: surfaces.perception,
        agent_watchlist: surfaces.watchlist,
        agent_scoreboard: surfaces.scoreboard,
        agent_eod_review: surfaces.eod_review,
        agent_narration: surfaces.narration,
    }
}
