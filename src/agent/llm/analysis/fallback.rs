use super::driver::run_analysis;
use super::*;

pub fn spawn_analysis_if_enabled(
    market: CaseMarket,
    snapshot: AgentSnapshot,
    briefing: AgentBriefing,
    session: AgentSession,
    limit: &Arc<Semaphore>,
) {
    let Some(config) = AnalystConfig::from_env() else {
        return;
    };
    if !config.enabled {
        return;
    }
    if !briefing.should_speak && !config.run_on_silent {
        return;
    }

    let Ok(permit) = limit.clone().try_acquire_owned() else {
        return;
    };

    tokio::spawn(async move {
        let _permit = permit;
        let analysis =
            run_analysis(config, snapshot.clone(), briefing.clone(), session.clone()).await;
        let recommendations = build_recommendations(&snapshot, Some(&session));
        let watchlist = build_watchlist(&snapshot, Some(&session), Some(&recommendations), 8);
        let narration = build_narration(
            &snapshot,
            &briefing,
            &session,
            Some(&watchlist),
            Some(&recommendations),
            Some(&analysis),
        );
        let market_id = MarketId::from(market);
        let analysis_path =
            MarketRegistry::resolve_artifact_path(market_id, ArtifactKind::Analysis);
        let narration_path =
            MarketRegistry::resolve_artifact_path(market_id, ArtifactKind::Narration);
        spawn_write_json_snapshot(analysis_path, analysis);
        spawn_write_json_snapshot(narration_path, narration);
    });
}

pub async fn run_or_fallback_analysis(
    snapshot: AgentSnapshot,
    briefing: AgentBriefing,
    session: AgentSession,
) -> AgentAnalysis {
    match AnalystConfig::from_env() {
        Some(config) if config.enabled => run_analysis(config, snapshot, briefing, session).await,
        _ => deterministic_analysis(&snapshot, &briefing),
    }
}

pub fn deterministic_analysis(snapshot: &AgentSnapshot, briefing: &AgentBriefing) -> AgentAnalysis {
    AgentAnalysis {
        tick: snapshot.tick,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        status: "deterministic".into(),
        should_speak: briefing.should_speak,
        provider: "local".into(),
        model: "deterministic".into(),
        message: briefing.spoken_message.clone(),
        final_action: Some(
            if briefing.should_speak {
                "speak"
            } else {
                "silent"
            }
            .into(),
        ),
        steps: briefing
            .executed_tools
            .iter()
            .enumerate()
            .map(|(index, item)| AgentAnalysisStep {
                step: index + 1,
                action: "tool".into(),
                tool: Some(item.tool.clone()),
                args: Some(item.args.clone()),
                preview: item.preview.clone(),
            })
            .collect(),
        error: None,
    }
}
