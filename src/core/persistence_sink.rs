use crate::agent::{
    AgentAlertScoreboard, AgentBriefing, AgentEodReview, AgentRecommendations, AgentSession,
    AgentSnapshot, AgentWatchlist,
};
use crate::agent_llm::AgentNarration;
use crate::cases::CaseMarket;
use crate::core::projection::ProjectionBundle;
use crate::live_snapshot::spawn_write_json_snapshot;
use crate::live_snapshot::LiveSnapshot;
use crate::ontology::{build_operational_snapshot, operational_snapshot_path};

pub struct ProjectionPersistenceEnvelope<'a> {
    pub market: CaseMarket,
    pub live_snapshot: &'a LiveSnapshot,
    pub agent_snapshot: &'a AgentSnapshot,
    pub briefing: &'a AgentBriefing,
    pub session: &'a AgentSession,
    pub recommendations: &'a AgentRecommendations,
    pub watchlist: &'a AgentWatchlist,
    pub scoreboard: &'a AgentAlertScoreboard,
    pub eod_review: &'a AgentEodReview,
    pub narration: &'a AgentNarration,
}

pub fn persist_projection_tick(envelope: ProjectionPersistenceEnvelope<'_>) {
    let path = operational_snapshot_path(envelope.market);
    match build_operational_snapshot(
        envelope.live_snapshot,
        envelope.agent_snapshot,
        envelope.session,
        envelope.recommendations,
        Some(envelope.narration),
    ) {
        Ok(snapshot) => spawn_write_json_snapshot(path, snapshot),
        Err(error) => eprintln!(
            "Warning: failed to build operational snapshot for {}: {}",
            match envelope.market {
                CaseMarket::Hk => "hk",
                CaseMarket::Us => "us",
            },
            error
        ),
    }
}

pub fn persist_projection_bundle(market: CaseMarket, bundle: &ProjectionBundle) {
    persist_projection_tick(ProjectionPersistenceEnvelope {
        market,
        live_snapshot: &bundle.live_snapshot,
        agent_snapshot: &bundle.agent_snapshot,
        briefing: &bundle.agent_briefing,
        session: &bundle.agent_session,
        recommendations: &bundle.agent_recommendations,
        watchlist: &bundle.agent_watchlist,
        scoreboard: &bundle.agent_scoreboard,
        eod_review: &bundle.agent_eod_review,
        narration: &bundle.agent_narration,
    });
}
