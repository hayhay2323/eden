use super::*;

pub fn load_agent_snapshot_path(market: CaseMarket) -> (&'static str, &'static str) {
    MarketRegistry::artifact_tuple(MarketId::from(market), ArtifactKind::AgentSnapshot)
}

pub fn load_briefing_path(market: CaseMarket) -> (&'static str, &'static str) {
    MarketRegistry::artifact_tuple(MarketId::from(market), ArtifactKind::Briefing)
}

pub fn load_session_path(market: CaseMarket) -> (&'static str, &'static str) {
    MarketRegistry::artifact_tuple(MarketId::from(market), ArtifactKind::Session)
}

pub fn load_watchlist_path(market: CaseMarket) -> (&'static str, &'static str) {
    MarketRegistry::artifact_tuple(MarketId::from(market), ArtifactKind::Watchlist)
}

pub fn load_recommendations_path(market: CaseMarket) -> (&'static str, &'static str) {
    MarketRegistry::artifact_tuple(MarketId::from(market), ArtifactKind::Recommendations)
}

pub fn load_recommendation_journal_path(market: CaseMarket) -> (&'static str, &'static str) {
    MarketRegistry::artifact_tuple(MarketId::from(market), ArtifactKind::RecommendationJournal)
}

pub fn load_scoreboard_path(market: CaseMarket) -> (&'static str, &'static str) {
    MarketRegistry::artifact_tuple(MarketId::from(market), ArtifactKind::Scoreboard)
}

pub fn load_eod_review_path(market: CaseMarket) -> (&'static str, &'static str) {
    MarketRegistry::artifact_tuple(MarketId::from(market), ArtifactKind::EodReview)
}

pub fn build_recommendation_journal_record(
    snapshot: &AgentSnapshot,
    recommendations: &AgentRecommendations,
) -> AgentRecommendationJournalRecord {
    let market_recommendation = recommendations.decisions.iter().find_map(|decision| {
        if let AgentDecision::Market(item) = decision {
            Some(item.clone())
        } else {
            None
        }
    });
    AgentRecommendationJournalRecord {
        tick: recommendations.tick,
        timestamp: recommendations.timestamp.clone(),
        market: recommendations.market,
        regime_bias: recommendations.regime_bias.clone(),
        breadth_up: snapshot.market_regime.breadth_up,
        breadth_down: snapshot.market_regime.breadth_down,
        composite_stress: snapshot.stress.composite_stress,
        wake_headline: snapshot.wake.headline.clone(),
        focus_symbols: snapshot.wake.focus_symbols.clone(),
        market_recommendation,
        decisions: recommendations.decisions.clone(),
        items: recommendations.items.clone(),
        knowledge_links: recommendations.knowledge_links.clone(),
    }
}

pub(crate) fn sync_alert_views(alert: &mut AgentAlertRecord) {
    if alert.resolution.is_none() {
        return;
    }
    alert.outcome_after_n_ticks = alert_outcome_from_resolution(alert, alert.resolution.as_ref());
}

pub(crate) fn backfill_alert_resolution_from_legacy_outcome(alert: &mut AgentAlertRecord) {
    if alert.resolution.is_some() {
        return;
    }
    let Some(outcome) = alert.outcome_after_n_ticks.as_ref() else {
        return;
    };
    let oriented_return = outcome.oriented_return.unwrap_or(Decimal::ZERO);
    let normalized_action = alert_resolution_action(&alert.suggested_action);
    let (follow_realized_return, fade_realized_return) = match normalized_action {
        "follow" => (oriented_return, -oriented_return),
        "fade" => (-oriented_return, oriented_return),
        _ => (Decimal::ZERO, Decimal::ZERO),
    };
    alert.resolution = Some(AgentRecommendationResolution {
        resolved_tick: outcome.resolved_tick,
        ticks_elapsed: outcome.ticks_elapsed,
        status: outcome.status.clone(),
        price_return: outcome.price_return.unwrap_or(Decimal::ZERO),
        follow_realized_return,
        fade_realized_return,
        wait_regret: follow_realized_return
            .max(fade_realized_return)
            .max(Decimal::ZERO),
        counterfactual_best_action: match normalized_action {
            "wait" => {
                if oriented_return > Decimal::ZERO {
                    "follow".into()
                } else {
                    "wait".into()
                }
            }
            _ => normalized_action.into(),
        },
        best_action_was_correct: outcome.status != "miss",
    });
}

pub fn update_recommendation_journal(
    existing: &str,
    snapshot: &AgentSnapshot,
    current: &AgentRecommendationJournalRecord,
) -> String {
    let mut records = existing
        .lines()
        .filter(|line| !line.trim().is_empty())
        .filter_map(|line| serde_json::from_str::<AgentRecommendationJournalRecord>(line).ok())
        .collect::<Vec<_>>();

    for record in &mut records {
        for decision in &mut record.decisions {
            match decision {
                AgentDecision::Market(item) => {
                    if item.resolution.is_none() {
                        item.resolution = resolve_market_recommendation_outcome(snapshot, item);
                    }
                }
                AgentDecision::Sector(item) => {
                    if item.resolution.is_none() {
                        item.resolution = resolve_sector_recommendation_outcome(snapshot, item);
                    }
                }
                AgentDecision::Symbol(item) => {
                    if item.resolution.is_none() {
                        item.resolution = resolve_recommendation_outcome(snapshot, item);
                    }
                }
            }
        }
        for item in &mut record.items {
            if item.resolution.is_none() {
                item.resolution = resolve_recommendation_outcome(snapshot, item);
            }
        }
    }

    let mut replaced = false;
    for record in &mut records {
        if record.market == current.market
            && record.tick == current.tick
            && record.timestamp == current.timestamp
        {
            *record = current.clone();
            replaced = true;
        }
    }
    if !replaced {
        records.push(current.clone());
    }

    records
        .into_iter()
        .map(|record| serde_json::to_string(&record).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\n")
        + "\n"
}
