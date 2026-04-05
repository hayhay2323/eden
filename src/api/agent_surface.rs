use axum::extract::{Path, Query};
use axum::response::sse::Sse;
use serde::Serialize;

use crate::agent::{
    self, AgentAlertScoreboard, AgentBriefing, AgentEodReview, AgentRecommendations, AgentSession,
    AgentWatchlist,
};
use crate::agent::llm::{AgentAnalysis, AgentAnalystReview, AgentAnalystScoreboard, AgentNarration};
use crate::cases::CaseMarket;
use crate::ontology::{
    derive_agent_briefing, derive_agent_eod_review, derive_agent_narration,
    derive_agent_recommendations, derive_agent_scoreboard, derive_agent_session,
    derive_agent_watchlist, derive_stale_agent_narration,
};

use super::agent_api::{AgentFeedQuery, AgentThreadsResponse, AgentTurnsResponse};
use super::constants::{DEFAULT_LIMIT, MAX_LIMIT};
use super::core::{
    bounded, case_market_slug, normalized_query_value, parse_case_market, ticks_within_window,
};
use super::foundation::{ApiError, JsonEventStream};
use super::ontology_api::load_contract_snapshot;
use super::stream_support::{json_poll_sse, latest_file_revision};

#[derive(Clone, Copy)]
enum AgentArtifact {
    Snapshot,
    Briefing,
    Analysis,
    Narration,
    AnalystReview,
    AnalystScoreboard,
    Session,
    Watchlist,
    Recommendations,
    Scoreboard,
    EodReview,
}

pub(super) async fn stream_agent_snapshot(
    Path(market): Path<String>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(agent_json_sse(
        market,
        AgentArtifact::Snapshot,
        move || async move {
            agent::load_snapshot(market).await.map_err(|error| {
                ApiError::internal(format!("failed to load agent snapshot: {error}"))
            })
        },
    ))
}

pub(super) async fn stream_agent_wake(
    Path(market): Path<String>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(agent_json_sse(
        market,
        AgentArtifact::Snapshot,
        move || async move {
            let snapshot = agent::load_snapshot(market).await.map_err(|error| {
                ApiError::internal(format!("failed to load agent snapshot: {error}"))
            })?;
            Ok(snapshot.wake)
        },
    ))
}

pub(super) async fn stream_agent_briefing(
    Path(market): Path<String>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(agent_json_sse(
        market,
        AgentArtifact::Briefing,
        move || async move { load_agent_briefing_for_market(case_market_slug(market)).await },
    ))
}

pub(super) async fn stream_agent_analysis(
    Path(market): Path<String>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(agent_json_sse(
        market,
        AgentArtifact::Analysis,
        move || async move {
            crate::agent::llm::load_analysis(market)
                .await
                .map_err(|error| {
                    ApiError::internal(format!("failed to load agent analysis: {error}"))
                })
        },
    ))
}

pub(super) async fn stream_agent_narration(
    Path(market): Path<String>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(agent_json_sse(
        market,
        AgentArtifact::Narration,
        move || async move { load_agent_narration_for_market(case_market_slug(market)).await },
    ))
}

pub(super) async fn stream_agent_analyst_review(
    Path(market): Path<String>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(agent_json_sse(
        market,
        AgentArtifact::AnalystReview,
        move || async move { load_agent_analyst_review_for_market(case_market_slug(market)).await },
    ))
}

pub(super) async fn stream_agent_analyst_scoreboard(
    Path(market): Path<String>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(agent_json_sse(
        market,
        AgentArtifact::AnalystScoreboard,
        move || async move { load_agent_analyst_scoreboard_for_market(case_market_slug(market)).await },
    ))
}

pub(super) async fn stream_agent_session(
    Path(market): Path<String>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(agent_json_sse(
        market,
        AgentArtifact::Session,
        move || async move { load_agent_session_for_market(case_market_slug(market)).await },
    ))
}

pub(super) async fn stream_agent_watchlist(
    Path(market): Path<String>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(agent_json_sse(
        market,
        AgentArtifact::Watchlist,
        move || async move { load_agent_watchlist_for_market(case_market_slug(market)).await },
    ))
}

pub(super) async fn stream_agent_recommendations(
    Path(market): Path<String>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(agent_json_sse(
        market,
        AgentArtifact::Recommendations,
        move || async move { load_agent_recommendations_for_market(case_market_slug(market)).await },
    ))
}

pub(super) async fn stream_agent_scoreboard(
    Path(market): Path<String>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(agent_json_sse(
        market,
        AgentArtifact::Scoreboard,
        move || async move { load_agent_scoreboard_for_market(case_market_slug(market)).await },
    ))
}

pub(super) async fn stream_agent_eod_review(
    Path(market): Path<String>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(agent_json_sse(
        market,
        AgentArtifact::EodReview,
        move || async move { load_agent_eod_review_for_market(case_market_slug(market)).await },
    ))
}

pub(super) async fn stream_agent_threads(
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(agent_json_sse(market, AgentArtifact::Session, move || {
        let query = query.clone();
        async move {
            let session = agent::load_session(market).await.map_err(|error| {
                ApiError::internal(format!("failed to load agent session: {error}"))
            })?;
            let mut threads = session.active_threads.clone();
            if let Some(symbol) = normalized_query_value(query.symbol.as_deref()) {
                threads.retain(|item| item.symbol.eq_ignore_ascii_case(symbol));
            }
            if let Some(sector) = normalized_query_value(query.sector.as_deref()) {
                threads.retain(|item| {
                    item.sector
                        .as_deref()
                        .map(|value| value.eq_ignore_ascii_case(sector))
                        .unwrap_or(false)
                });
            }
            let total = threads.len();
            let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
            if threads.len() > limit {
                threads.truncate(limit);
            }
            Ok(AgentThreadsResponse {
                tick: session.tick,
                total,
                threads,
            })
        }
    }))
}

pub(super) async fn stream_agent_turns(
    Path(market): Path<String>,
    Query(query): Query<AgentFeedQuery>,
) -> Result<Sse<JsonEventStream>, ApiError> {
    let market = parse_case_market(&market)?;
    Ok(agent_json_sse(market, AgentArtifact::Session, move || {
        let query = query.clone();
        async move {
            let session = agent::load_session(market).await.map_err(|error| {
                ApiError::internal(format!("failed to load agent session: {error}"))
            })?;
            let mut turns = session.recent_turns.clone();
            if let Some(since_tick) = query.since_tick {
                turns.retain(|item| item.tick > since_tick);
            }
            if let Some(symbol) = normalized_query_value(query.symbol.as_deref()) {
                turns.retain(|item| {
                    item.focus_symbols
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(symbol))
                });
            }
            let total = turns.len();
            let limit = bounded(query.limit, DEFAULT_LIMIT, MAX_LIMIT, "limit")?;
            if turns.len() > limit {
                turns = turns[turns.len().saturating_sub(limit)..].to_vec();
            }
            Ok(AgentTurnsResponse {
                tick: session.tick,
                total,
                turns,
            })
        }
    }))
}

fn agent_json_sse<T, F, Fut>(
    market: CaseMarket,
    artifact: AgentArtifact,
    loader: F,
) -> Sse<JsonEventStream>
where
    T: Serialize + Send + 'static,
    F: Fn() -> Fut + Clone + Send + 'static,
    Fut: std::future::Future<Output = Result<T, ApiError>> + Send + 'static,
{
    json_poll_sse(loader, move || async move {
        agent_stream_revision(market, artifact).await
    })
}

async fn agent_stream_revision(
    market: CaseMarket,
    artifact: AgentArtifact,
) -> Result<String, ApiError> {
    latest_file_revision(agent_stream_revision_candidates(market, artifact)).await
}

fn agent_stream_revision_candidates(market: CaseMarket, artifact: AgentArtifact) -> Vec<String> {
    let resolve = |pair: (&'static str, &'static str)| {
        std::env::var(pair.0).unwrap_or_else(|_| pair.1.to_string())
    };
    match artifact {
        AgentArtifact::Snapshot => vec![resolve(agent::load_agent_snapshot_path(market))],
        AgentArtifact::Briefing => vec![resolve(agent::load_briefing_path(market))],
        AgentArtifact::Analysis => vec![resolve(crate::agent::llm::analysis_path(market))],
        AgentArtifact::Narration => vec![
            resolve(crate::agent::llm::narration_path(market)),
            resolve(crate::agent::llm::runtime_narration_path(market)),
        ],
        AgentArtifact::AnalystReview => vec![
            resolve(crate::agent::llm::analyst_review_path(market)),
            resolve(crate::agent::llm::analysis_path(market)),
            resolve(crate::agent::llm::narration_path(market)),
            resolve(crate::agent::llm::runtime_narration_path(market)),
        ],
        AgentArtifact::AnalystScoreboard => vec![
            resolve(crate::agent::llm::analyst_scoreboard_path(market)),
            resolve(crate::agent::llm::analyst_review_path(market)),
            resolve(crate::agent::llm::analysis_path(market)),
            resolve(crate::agent::llm::narration_path(market)),
            resolve(crate::agent::llm::runtime_narration_path(market)),
        ],
        AgentArtifact::Session => vec![resolve(agent::load_session_path(market))],
        AgentArtifact::Watchlist | AgentArtifact::Recommendations => vec![
            resolve(match artifact {
                AgentArtifact::Watchlist => agent::load_watchlist_path(market),
                _ => agent::load_recommendations_path(market),
            }),
            resolve(agent::load_session_path(market)),
            resolve(agent::load_agent_snapshot_path(market)),
        ],
        AgentArtifact::Scoreboard | AgentArtifact::EodReview => vec![
            resolve(match artifact {
                AgentArtifact::Scoreboard => agent::load_scoreboard_path(market),
                _ => agent::load_eod_review_path(market),
            }),
            resolve(agent::load_agent_snapshot_path(market)),
            resolve(agent::load_session_path(market)),
        ],
    }
}

fn analysis_is_fresh_codex(snapshot_tick: u64, analysis: Option<&AgentAnalysis>) -> bool {
    analysis
        .map(|item| {
            item.provider.contains("codex")
                && ticks_within_window(
                    snapshot_tick,
                    item.tick,
                    crate::agent::llm::CODEX_FRESH_TICK_WINDOW,
                )
        })
        .unwrap_or(false)
}

fn narration_is_complete(narration: &AgentNarration) -> bool {
    narration.primary_action.is_some()
        && !narration.what_changed.is_empty()
        && !narration
            .market_summary_5m
            .as_deref()
            .unwrap_or("")
            .trim()
            .is_empty()
        && !narration.action_cards.is_empty()
}

pub(super) fn should_return_loaded_final_narration(
    snapshot_tick: u64,
    analysis: Option<&AgentAnalysis>,
    narration: Option<&AgentNarration>,
) -> bool {
    analysis_is_fresh_codex(snapshot_tick, analysis)
        && narration
            .map(|item| {
                narration_is_complete(item)
                    && ticks_within_window(
                        snapshot_tick,
                        item.tick,
                        crate::agent::llm::CODEX_FRESH_TICK_WINDOW,
                    )
            })
            .unwrap_or(false)
}

pub(super) async fn load_agent_session_for_market(raw: &str) -> Result<AgentSession, ApiError> {
    let market = parse_case_market(raw)?;
    let snapshot = load_contract_snapshot(market).await?;
    Ok(derive_agent_session(&snapshot))
}

pub(super) async fn load_agent_briefing_for_market(raw: &str) -> Result<AgentBriefing, ApiError> {
    let market = parse_case_market(raw)?;
    let snapshot = load_contract_snapshot(market).await?;
    Ok(derive_agent_briefing(&snapshot))
}

pub(super) async fn load_agent_watchlist_for_market(raw: &str) -> Result<AgentWatchlist, ApiError> {
    let market = parse_case_market(raw)?;
    let snapshot = load_contract_snapshot(market).await?;
    Ok(derive_agent_watchlist(&snapshot, 8))
}

pub(super) async fn load_agent_recommendations_for_market(
    raw: &str,
) -> Result<AgentRecommendations, ApiError> {
    let market = parse_case_market(raw)?;
    let snapshot = load_contract_snapshot(market).await?;
    Ok(derive_agent_recommendations(&snapshot))
}

pub(super) async fn load_agent_scoreboard_for_market(
    raw: &str,
) -> Result<AgentAlertScoreboard, ApiError> {
    let market = parse_case_market(raw)?;
    let previous = agent::load_scoreboard(market).await.ok();
    let snapshot = load_contract_snapshot(market).await?;
    Ok(derive_agent_scoreboard(&snapshot, previous.as_ref()))
}

pub(super) async fn load_agent_eod_review_for_market(
    raw: &str,
) -> Result<AgentEodReview, ApiError> {
    let market = parse_case_market(raw)?;
    let snapshot = load_contract_snapshot(market).await?;
    let previous = agent::load_scoreboard(market).await.ok();
    let scoreboard = derive_agent_scoreboard(&snapshot, previous.as_ref());
    Ok(derive_agent_eod_review(&snapshot, &scoreboard))
}

pub(super) async fn load_agent_analyst_review_for_market(
    raw: &str,
) -> Result<AgentAnalystReview, ApiError> {
    let market = parse_case_market(raw)?;
    if let Ok(review) = crate::agent::llm::load_analyst_review(market).await {
        return Ok(review);
    }
    let analysis = crate::agent::llm::load_analysis(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load agent analysis: {error}")))?;
    let narration = crate::agent::llm::load_final_narration(market)
        .await
        .map_err(|error| ApiError::internal(format!("failed to load agent narration: {error}")))?;
    let runtime = crate::agent::llm::load_runtime_narration(market)
        .await
        .map_err(|error| {
            ApiError::internal(format!("failed to load runtime narration: {error}"))
        })?;
    Ok(crate::agent::llm::build_analyst_review_from_artifacts(
        &analysis, &narration, &runtime,
    ))
}

pub(super) async fn load_agent_analyst_scoreboard_for_market(
    raw: &str,
) -> Result<AgentAnalystScoreboard, ApiError> {
    let market = parse_case_market(raw)?;
    if let Ok(scoreboard) = crate::agent::llm::load_analyst_scoreboard(market).await {
        return Ok(scoreboard);
    }
    let review = load_agent_analyst_review_for_market(raw).await?;
    Ok(crate::agent::llm::build_analyst_scoreboard_from_review(
        &review, None,
    ))
}

pub(super) async fn load_agent_narration_for_market(raw: &str) -> Result<AgentNarration, ApiError> {
    let market = parse_case_market(raw)?;
    let operational = load_contract_snapshot(market).await?;
    let loaded_final = crate::agent::llm::load_final_narration(market).await.ok();
    let analysis = crate::agent::llm::load_analysis(market).await.ok();
    let codex_fresh = analysis_is_fresh_codex(operational.source_tick, analysis.as_ref());
    if should_return_loaded_final_narration(
        operational.source_tick,
        analysis.as_ref(),
        loaded_final.as_ref(),
    ) {
        return Ok(loaded_final.expect("checked Some above"));
    }
    if !codex_fresh {
        return Ok(derive_stale_agent_narration(
            &operational,
            analysis.as_ref(),
        ));
    }
    Ok(derive_agent_narration(&operational, analysis.as_ref()))
}
