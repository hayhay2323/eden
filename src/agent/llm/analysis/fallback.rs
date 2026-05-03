use super::driver::run_analysis;
use super::*;
use crate::live_snapshot::spawn_write_json_snapshots_batch;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};

#[derive(Default)]
struct AnalysisDispatchSlot {
    active: bool,
    pending: Option<PendingAnalysisRequest>,
}

struct PendingAnalysisRequest {
    snapshot: AgentSnapshot,
    briefing: AgentBriefing,
    session: AgentSession,
}

fn analysis_dispatch_slots() -> &'static Mutex<HashMap<&'static str, AnalysisDispatchSlot>> {
    static SLOTS: OnceLock<Mutex<HashMap<&'static str, AnalysisDispatchSlot>>> = OnceLock::new();
    SLOTS.get_or_init(|| Mutex::new(HashMap::new()))
}

fn analysis_market_slug(market: CaseMarket) -> &'static str {
    match market {
        CaseMarket::Hk => "hk",
        CaseMarket::Us => "us",
    }
}

async fn run_pending_analysis_loop(
    market: CaseMarket,
    config: AnalystConfig,
    limit: Arc<Semaphore>,
) {
    let market_slug = analysis_market_slug(market);

    loop {
        let pending = {
            let mut slots = analysis_dispatch_slots()
                .lock()
                .expect("analysis dispatch lock poisoned");
            let Some(slot) = slots.get_mut(market_slug) else {
                return;
            };
            match slot.pending.take() {
                Some(pending) => pending,
                None => {
                    slot.active = false;
                    slots.remove(market_slug);
                    return;
                }
            }
        };

        let Ok(permit) = limit.clone().acquire_owned().await else {
            let mut slots = analysis_dispatch_slots()
                .lock()
                .expect("analysis dispatch lock poisoned");
            if let Some(slot) = slots.get_mut(market_slug) {
                slot.active = false;
                if slot.pending.is_none() {
                    slots.remove(market_slug);
                }
            }
            return;
        };

        let analysis = run_analysis(
            config.clone(),
            pending.snapshot.clone(),
            pending.briefing.clone(),
            pending.session.clone(),
        )
        .await;
        drop(permit);

        // FP2: empty recommendations shell; the LLM-fallback path
        // produces analysis + narration artifacts, but eden no longer
        // fabricates recommendations to wrap them. Downstream
        // consumers see no recommendation context for fallback runs.
        let recommendations = AgentRecommendations::empty(&pending.snapshot);
        let watchlist = build_watchlist(
            &pending.snapshot,
            Some(&pending.session),
            Some(&recommendations),
            8,
        );
        let narration = build_narration(
            &pending.snapshot,
            &pending.briefing,
            &pending.session,
            Some(&watchlist),
            Some(&recommendations),
            Some(&analysis),
        );
        let market_id = MarketId::from(market);
        let analysis_path =
            MarketRegistry::resolve_artifact_path(market_id, ArtifactKind::Analysis);
        let narration_path =
            MarketRegistry::resolve_artifact_path(market_id, ArtifactKind::Narration);
        let analysis_payload = match serde_json::to_string(&analysis) {
            Ok(payload) => payload,
            Err(error) => {
                eprintln!(
                    "Warning: failed to serialize analysis artifact for {} tick {}: {}",
                    market_slug, pending.snapshot.tick, error
                );
                continue;
            }
        };
        let narration_payload = match serde_json::to_string(&narration) {
            Ok(payload) => payload,
            Err(error) => {
                eprintln!(
                    "Warning: failed to serialize narration artifact for {} tick {}: {}",
                    market_slug, pending.snapshot.tick, error
                );
                continue;
            }
        };
        spawn_write_json_snapshots_batch(
            format!("analysis:{market_slug}"),
            pending.snapshot.tick,
            vec![
                (analysis_path, analysis_payload),
                (narration_path, narration_payload),
            ],
        );
    }
}

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

    let market_slug = analysis_market_slug(market);
    let mut should_spawn = false;
    {
        let mut slots = analysis_dispatch_slots()
            .lock()
            .expect("analysis dispatch lock poisoned");
        let slot = slots.entry(market_slug).or_default();
        let replace_pending = slot
            .pending
            .as_ref()
            .map(|pending| pending.snapshot.tick < snapshot.tick)
            .unwrap_or(true);
        if replace_pending {
            slot.pending = Some(PendingAnalysisRequest {
                snapshot,
                briefing,
                session,
            });
        }
        if !slot.active && slot.pending.is_some() {
            slot.active = true;
            should_spawn = true;
        }
    }

    if should_spawn {
        tokio::spawn(run_pending_analysis_loop(market, config, limit.clone()));
    }
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
