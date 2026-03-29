use std::sync::Arc;

use tokio::sync::Semaphore;

use crate::agent::{AgentBriefing, AgentSession, AgentSnapshot};
use crate::agent_llm::spawn_analysis_if_enabled;
use crate::cases::CaseMarket;

pub trait AnalystService {
    fn trigger_runtime_analysis(
        &self,
        market: CaseMarket,
        snapshot: AgentSnapshot,
        briefing: AgentBriefing,
        session: AgentSession,
        limit: &Arc<Semaphore>,
    );
}

#[derive(Debug, Default, Clone, Copy)]
pub struct DefaultAnalystService;

impl AnalystService for DefaultAnalystService {
    fn trigger_runtime_analysis(
        &self,
        market: CaseMarket,
        snapshot: AgentSnapshot,
        briefing: AgentBriefing,
        session: AgentSession,
        limit: &Arc<Semaphore>,
    ) {
        spawn_analysis_if_enabled(market, snapshot, briefing, session, limit);
    }
}
