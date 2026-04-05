use crate::agent::{
    build_alert_scoreboard, build_briefing, build_eod_review, build_hk_agent_snapshot,
    build_recommendations, build_session, build_us_agent_snapshot, build_watchlist,
    AgentAlertScoreboard, AgentBriefing, AgentEodReview, AgentRecommendations, AgentSession,
    AgentSnapshot, AgentWatchlist,
};
use crate::agent::llm::{build_narration, AgentNarration};
use crate::live_snapshot::LiveSnapshot;
use crate::ontology::links::LinkSnapshot;
use crate::ontology::store::ObjectStore;
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
}

pub struct UsProjectionInputs<'a> {
    pub live_snapshot: LiveSnapshot,
    pub history: &'a UsTickHistory,
    pub reasoning: &'a UsReasoningSnapshot,
    pub backward: &'a UsBackwardSnapshot,
    pub store: &'a ObjectStore,
    pub lineage_stats: &'a UsLineageStats,
    pub previous_agent_snapshot: Option<&'a AgentSnapshot>,
    pub previous_agent_session: Option<&'a AgentSession>,
    pub previous_agent_scoreboard: Option<&'a AgentAlertScoreboard>,
}

pub fn project_hk(inputs: HkProjectionInputs<'_>) -> ProjectionBundle {
    let agent_snapshot = build_hk_agent_snapshot(
        &inputs.live_snapshot,
        inputs.history,
        inputs.links,
        inputs.store,
        inputs.lineage_priors,
        inputs.previous_agent_snapshot,
    );
    let agent_briefing = build_briefing(&agent_snapshot);
    let agent_session = build_session(
        &agent_snapshot,
        &agent_briefing,
        inputs.previous_agent_session,
    );
    let agent_recommendations = build_recommendations(&agent_snapshot, Some(&agent_session));
    let agent_watchlist = build_watchlist(
        &agent_snapshot,
        Some(&agent_session),
        Some(&agent_recommendations),
        8,
    );
    let agent_scoreboard = build_alert_scoreboard(
        &agent_snapshot,
        Some(&agent_recommendations),
        inputs.previous_agent_scoreboard,
    );
    let agent_eod_review = build_eod_review(&agent_snapshot, &agent_scoreboard);
    let agent_narration = build_narration(
        &agent_snapshot,
        &agent_briefing,
        &agent_session,
        Some(&agent_watchlist),
        Some(&agent_recommendations),
        None,
    );

    ProjectionBundle {
        live_snapshot: inputs.live_snapshot,
        agent_snapshot,
        agent_briefing,
        agent_session,
        agent_recommendations,
        agent_watchlist,
        agent_scoreboard,
        agent_eod_review,
        agent_narration,
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
    );
    let agent_briefing = build_briefing(&agent_snapshot);
    let agent_session = build_session(
        &agent_snapshot,
        &agent_briefing,
        inputs.previous_agent_session,
    );
    let agent_recommendations = build_recommendations(&agent_snapshot, Some(&agent_session));
    let agent_watchlist = build_watchlist(
        &agent_snapshot,
        Some(&agent_session),
        Some(&agent_recommendations),
        8,
    );
    let agent_scoreboard = build_alert_scoreboard(
        &agent_snapshot,
        Some(&agent_recommendations),
        inputs.previous_agent_scoreboard,
    );
    let agent_eod_review = build_eod_review(&agent_snapshot, &agent_scoreboard);
    let agent_narration = build_narration(
        &agent_snapshot,
        &agent_briefing,
        &agent_session,
        Some(&agent_watchlist),
        Some(&agent_recommendations),
        None,
    );

    ProjectionBundle {
        live_snapshot: inputs.live_snapshot,
        agent_snapshot,
        agent_briefing,
        agent_session,
        agent_recommendations,
        agent_watchlist,
        agent_scoreboard,
        agent_eod_review,
        agent_narration,
    }
}
