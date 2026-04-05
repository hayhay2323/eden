use super::*;

#[path = "narration/decision.rs"]
mod decision;
#[path = "narration/stale.rs"]
mod stale;

use decision::*;
pub use stale::build_codex_stale_narration;

use std::collections::HashMap;

pub fn build_narration(
    snapshot: &AgentSnapshot,
    briefing: &AgentBriefing,
    session: &AgentSession,
    watchlist: Option<&AgentWatchlist>,
    recommendations: Option<&AgentRecommendations>,
    analysis: Option<&AgentAnalysis>,
) -> AgentNarration {
    let recommendations = recommendations
        .cloned()
        .unwrap_or_else(|| build_recommendations(snapshot, Some(session)));
    let watchlist = watchlist
        .cloned()
        .unwrap_or_else(|| build_watchlist(snapshot, Some(session), Some(&recommendations), 8));
    let top_decision = recommendations.decisions.first();
    let market_recommendation = recommendations.decisions.iter().find_map(|decision| {
        if let AgentDecision::Market(item) = decision {
            Some(item.clone())
        } else {
            None
        }
    });
    let message = analysis
        .and_then(|analysis| analysis.message.clone())
        .or_else(|| briefing.spoken_message.clone());
    let mut bullets = watchlist
        .entries
        .iter()
        .take(3)
        .map(|entry| format!("{}: {} | {}", entry.symbol, entry.action, entry.why))
        .collect::<Vec<_>>();
    for summary in briefing.summary.iter().take(2) {
        if !bullets.iter().any(|item| item == summary) {
            bullets.push(summary.clone());
        }
    }
    if let Some(market_call) = &market_recommendation {
        bullets.insert(
            0,
            format!(
                "Market: {} via {} | {}",
                market_call.best_action, market_call.preferred_expression, market_call.summary
            ),
        );
    }
    bullets.truncate(5);

    let mut tags = snapshot
        .notices
        .iter()
        .map(|item| item.kind.clone())
        .collect::<Vec<_>>();
    for decision in recommendations.decisions.iter().take(3) {
        let tag = narration_decision_tag(decision);
        if !tags.iter().any(|existing| existing == &tag) {
            tags.push(tag);
        }
    }
    tags.sort();
    tags.dedup();
    tags.truncate(6);

    let alert_level = top_decision
        .map(narration_decision_alert_level)
        .unwrap_or_else(|| {
            if briefing.should_speak || session.active_thread_count > 0 {
                "high".into()
            } else {
                "normal".into()
            }
        });
    let what_changed = snapshot
        .recent_transitions
        .iter()
        .filter(|item| item.to_tick == snapshot.tick)
        .take(3)
        .map(|item| item.summary.clone())
        .collect::<Vec<_>>();
    let watch_next = top_decision
        .map(narration_decision_watch_next)
        .unwrap_or_default();
    let what_not_to_do = top_decision
        .map(narration_decision_do_not)
        .unwrap_or_default();
    let fragility = top_decision
        .map(narration_decision_fragility)
        .unwrap_or_default();
    let confidence_band = top_decision.and_then(narration_decision_confidence_band);
    let market_summary_5m = Some(
        analysis
            .and_then(|item| item.message.clone())
            .or_else(|| {
                if let Some(market_call) = &market_recommendation {
                    return Some(format!(
                        "{}；market impulse {} / discontinuity {}",
                        market_call.summary,
                        (market_call.market_impulse_score * rust_decimal::Decimal::from(100))
                            .round_dp(0),
                        (market_call.macro_regime_discontinuity * rust_decimal::Decimal::from(100))
                            .round_dp(0)
                    ));
                }
                let first = what_changed.first()?.clone();
                let regime = snapshot.market_regime.bias.clone();
                Some(format!(
                    "{}；大市 {}，breadth_down={}、stress={}",
                    first,
                    regime,
                    (snapshot.market_regime.breadth_down * rust_decimal::Decimal::from(100))
                        .round_dp(0),
                    snapshot.stress.composite_stress.round_dp(2)
                ))
            })
            .unwrap_or_else(|| briefing.summary.join(" ")),
    );
    let action_cards = recommendations
        .decisions
        .iter()
        .take(12)
        .map(narration_action_card)
        .collect::<Vec<_>>();
    let dominant_lenses = aggregate_dominant_lenses(&action_cards);
    if let Some(summary) = dominant_lens_summary(&dominant_lenses) {
        if !bullets.iter().any(|item| item == &summary) {
            bullets.push(summary);
            bullets.truncate(5);
        }
    }

    AgentNarration {
        tick: snapshot.tick,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        should_alert: briefing.should_speak
            || session.active_thread_count > 0
            || top_decision
                .map(narration_decision_should_alert)
                .unwrap_or(false)
            || market_recommendation
                .as_ref()
                .map(|item| item.best_action != "wait")
                .unwrap_or(false),
        alert_level: alert_level.into(),
        source: analysis
            .map(|analysis| analysis.provider.clone())
            .unwrap_or_else(|| "deterministic".into()),
        headline: analysis
            .and_then(|analysis| analysis.message.clone())
            .or_else(|| {
                if let Some(market_call) = &market_recommendation {
                    if market_call.best_action != "wait" {
                        return Some(format!(
                            "{} {} via {}",
                            market_label(snapshot.market),
                            market_call.best_action,
                            market_call.preferred_expression
                        ));
                    }
                }
                top_decision.map(narration_decision_headline)
            })
            .or_else(|| briefing.headline.clone()),
        message,
        bullets,
        focus_symbols: if watchlist.entries.is_empty() {
            briefing.focus_symbols.clone()
        } else {
            let mut focus = Vec::new();
            for entry in watchlist.entries.iter().take(4) {
                if entry.reference_symbols.is_empty() {
                    if entry.scope_kind == "symbol" {
                        focus.push(entry.symbol.clone());
                    }
                } else {
                    for symbol in &entry.reference_symbols {
                        if !focus.iter().any(|value| value == symbol) {
                            focus.push(symbol.clone());
                        }
                    }
                }
                if focus.len() >= 4 {
                    break;
                }
            }
            if focus.is_empty() {
                briefing.focus_symbols.clone()
            } else {
                focus
            }
        },
        tags,
        primary_action: top_decision
            .and_then(narration_decision_primary_action)
            .or_else(|| {
                market_recommendation.as_ref().and_then(|item| {
                    (item.best_action != "wait").then(|| format!("market_{}", item.best_action))
                })
            }),
        confidence_band,
        what_changed,
        why_it_matters: top_decision.map(narration_decision_why).or_else(|| {
            market_recommendation
                .as_ref()
                .map(|item| item.summary.clone())
        }),
        watch_next,
        what_not_to_do,
        fragility,
        recommendation_ids: recommendations
            .decisions
            .iter()
            .take(3)
            .map(narration_decision_id)
            .collect(),
        market_summary_5m,
        market_recommendation,
        dominant_lenses,
        action_cards,
    }
}

fn aggregate_dominant_lenses(cards: &[AgentNarrationActionCard]) -> Vec<AgentDominantLens> {
    #[derive(Default)]
    struct LensAccumulator {
        card_count: usize,
        total_confidence: Decimal,
        max_confidence: Decimal,
    }

    let mut aggregates: HashMap<String, LensAccumulator> = HashMap::new();
    for card in cards {
        let mut per_card: HashMap<String, Decimal> = HashMap::new();
        for component in card
            .why_components
            .iter()
            .chain(card.invalidation_components.iter())
        {
            let lens_name = component.lens_name.trim();
            if lens_name.is_empty() {
                continue;
            }
            let confidence = component.confidence.abs();
            per_card
                .entry(lens_name.to_string())
                .and_modify(|value| {
                    if confidence > *value {
                        *value = confidence;
                    }
                })
                .or_insert(confidence);
        }

        for (lens_name, confidence) in per_card {
            let entry = aggregates.entry(lens_name).or_default();
            entry.card_count += 1;
            entry.total_confidence += confidence;
            if confidence > entry.max_confidence {
                entry.max_confidence = confidence;
            }
        }
    }

    let mut lenses = aggregates
        .into_iter()
        .map(|(lens_name, item)| AgentDominantLens {
            lens_name,
            card_count: item.card_count,
            max_confidence: item.max_confidence.round_dp(4),
            mean_confidence: if item.card_count == 0 {
                Decimal::ZERO
            } else {
                (item.total_confidence / Decimal::from(item.card_count as i64)).round_dp(4)
            },
        })
        .collect::<Vec<_>>();
    lenses.sort_by(|left, right| {
        right
            .card_count
            .cmp(&left.card_count)
            .then_with(|| right.max_confidence.cmp(&left.max_confidence))
            .then_with(|| left.lens_name.cmp(&right.lens_name))
    });
    lenses.truncate(6);
    lenses
}

fn dominant_lens_summary(lenses: &[AgentDominantLens]) -> Option<String> {
    if lenses.is_empty() {
        return None;
    }
    let summary = lenses
        .iter()
        .take(3)
        .map(|item| {
            format!(
                "{} {}",
                render_lens_label(&item.lens_name),
                (item.max_confidence * Decimal::from(100)).round_dp(0)
            )
        })
        .collect::<Vec<_>>()
        .join(" • ");
    Some(format!("Dominant lenses: {summary}"))
}

fn render_lens_label(name: &str) -> String {
    name.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => format!("{}{}", first.to_ascii_uppercase(), chars.as_str()),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
