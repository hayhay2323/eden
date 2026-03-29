use super::*;
use super::routing::{
    impact_market_label, macro_event_candidate_from_headline, macro_event_candidate_from_notice,
    macro_headline_relevant, macro_market_confirmation, macro_notice_relevant,
    world_monitor_candidate_from_record,
};

pub(crate) fn build_macro_event_candidates(
    tick: u64,
    market: LiveMarket,
    market_regime: &LiveMarketRegime,
    stress: &LiveStressSnapshot,
    wake: &AgentWakeState,
    notices: &[AgentNotice],
    sectors: &[AgentSectorFlow],
    symbols: &[AgentSymbolState],
    cross_market_signals: &[LiveCrossMarketSignal],
) -> Vec<AgentMacroEventCandidate> {
    let mut items = Vec::new();
    let mut seen = HashSet::new();

    for notice in notices
        .iter()
        .filter(|notice| macro_notice_relevant(notice))
    {
        let candidate = macro_event_candidate_from_notice(
            tick,
            market,
            market_regime,
            stress,
            notice,
            sectors,
            symbols,
            cross_market_signals,
        );
        if seen.insert(candidate.candidate_id.clone()) {
            items.push(candidate);
        }
    }

    if let Some(headline) = wake.headline.as_ref() {
        if macro_headline_relevant(headline) {
            let candidate = macro_event_candidate_from_headline(
                tick,
                market,
                market_regime,
                stress,
                headline,
                sectors,
                symbols,
                cross_market_signals,
            );
            if seen.insert(candidate.candidate_id.clone()) {
                items.push(candidate);
            }
        }
    }

    items.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| b.novelty_score.cmp(&a.novelty_score))
            .then_with(|| a.candidate_id.cmp(&b.candidate_id))
    });
    items.truncate(12);
    items
}

pub(crate) fn promote_macro_events(
    market_regime: &LiveMarketRegime,
    stress: &LiveStressSnapshot,
    candidates: &[AgentMacroEventCandidate],
) -> Vec<AgentMacroEvent> {
    let confirmation = macro_market_confirmation(market_regime, stress);
    let mut items = candidates
        .iter()
        .filter_map(|candidate| {
            let passes = candidate.confidence >= Decimal::new(55, 2)
                && (!candidate.impact.requires_market_confirmation
                    || confirmation >= Decimal::new(55, 2));
            if !passes {
                return None;
            }
            let mut promotion_reasons = vec![format!(
                "confidence={:.0}% novelty={:.0}%",
                (candidate.confidence * Decimal::from(100)).round_dp(0),
                (candidate.novelty_score * Decimal::from(100)).round_dp(0)
            )];
            promotion_reasons.extend(candidate.impact.decisive_factors.iter().take(3).cloned());
            Some(AgentMacroEvent {
                event_id: candidate
                    .candidate_id
                    .replace("macro_candidate", "macro_event"),
                tick: candidate.tick,
                market: candidate.market,
                event_type: candidate.event_type.clone(),
                authority_level: candidate.authority_level.clone(),
                headline: candidate.headline.clone(),
                summary: candidate.summary.clone(),
                confidence: candidate.confidence,
                confirmation_state: if candidate.impact.requires_market_confirmation {
                    "market_confirmed".into()
                } else {
                    "promoted".into()
                },
                impact: candidate.impact.clone(),
                supporting_notice_ids: vec![candidate.candidate_id.clone()],
                promotion_reasons,
            })
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.event_id.cmp(&b.event_id))
    });
    items.truncate(6);
    items
}

pub(crate) fn merge_macro_event_candidates(
    mut internal: Vec<AgentMacroEventCandidate>,
    external: Vec<AgentMacroEventCandidate>,
) -> Vec<AgentMacroEventCandidate> {
    let mut seen = internal
        .iter()
        .map(|item| item.candidate_id.clone())
        .collect::<HashSet<_>>();
    for item in external {
        if seen.insert(item.candidate_id.clone()) {
            internal.push(item);
        }
    }
    internal.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| b.novelty_score.cmp(&a.novelty_score))
            .then_with(|| a.candidate_id.cmp(&b.candidate_id))
    });
    internal.truncate(16);
    internal
}

pub(crate) fn build_world_monitor_macro_event_candidates(
    tick: u64,
    market: LiveMarket,
    market_regime: &LiveMarketRegime,
    stress: &LiveStressSnapshot,
    sectors: &[AgentSectorFlow],
    symbols: &[AgentSymbolState],
    cross_market_signals: &[LiveCrossMarketSignal],
) -> Vec<AgentMacroEventCandidate> {
    let Ok(records) = load_world_monitor_events() else {
        return vec![];
    };
    let current_market = impact_market_label(market);
    let mut items = records
        .into_iter()
        .filter_map(|record| {
            world_monitor_candidate_from_record(
                tick,
                market,
                market_regime,
                stress,
                sectors,
                symbols,
                cross_market_signals,
                record,
            )
        })
        .filter(|item| {
            item.impact
                .affected_markets
                .iter()
                .any(|value| value == &current_market)
                || item.impact.primary_scope == "market"
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| b.novelty_score.cmp(&a.novelty_score))
            .then_with(|| a.candidate_id.cmp(&b.candidate_id))
    });
    items.truncate(8);
    items
}
