use super::*;

pub(crate) fn summarize_hk_scorecard(scorecard: &SignalScorecard) -> LiveScorecard {
    let stats = scorecard.stats();
    let total_signals = stats.iter().map(|item| item.total).sum::<usize>();
    let resolved_signals = stats.iter().map(|item| item.resolved).sum::<usize>();
    let hits = stats.iter().map(|item| item.hits).sum::<usize>();
    let misses = resolved_signals.saturating_sub(hits);
    let hit_rate = if resolved_signals == 0 {
        Decimal::ZERO
    } else {
        Decimal::from(hits as i64) / Decimal::from(resolved_signals as i64)
    };
    let mean_return = if resolved_signals == 0 {
        Decimal::ZERO
    } else {
        stats
            .iter()
            .map(|item| item.mean_return * Decimal::from(item.resolved as i64))
            .sum::<Decimal>()
            / Decimal::from(resolved_signals as i64)
    };

    LiveScorecard {
        total_signals,
        resolved_signals,
        hits,
        misses,
        hit_rate,
        mean_return,
        // HK runtime does not yet track a separate `requires_confirmation`
        // tier for its scorecard; the fields exist for schema compatibility
        // with the US runtime and will stay at zero until HK plumbs through
        // an actionable-tier flag per signal.
        ..LiveScorecard::default()
    }
}

pub(crate) fn build_hk_lineage_metrics(history: &TickHistory) -> Vec<LiveLineageMetric> {
    eden::temporal::lineage::compute_multi_horizon_lineage_metrics(
        history,
        super::LINEAGE_WINDOW,
        330,
    )
    .iter()
    .map(|item| LiveLineageMetric {
        horizon: Some(item.horizon.clone()),
        template: item.template.clone(),
        total: item.total,
        resolved: item.resolved,
        hits: item.hits,
        hit_rate: item.hit_rate,
        mean_return: item.mean_return,
    })
    .collect()
}

pub(crate) fn extract_symbol_scope(scope: &eden::ReasoningScope) -> Option<&Symbol> {
    match scope {
        eden::ReasoningScope::Symbol(symbol) => Some(symbol),
        _ => None,
    }
}

pub(crate) fn symbol_string_from_scope(scope: &eden::ReasoningScope) -> String {
    extract_symbol_scope(scope)
        .map(|symbol| symbol.0.clone())
        .unwrap_or_default()
}

pub(crate) fn hk_scope_label(scope: &eden::ReasoningScope) -> String {
    match scope {
        eden::ReasoningScope::Market(_) => "market".into(),
        eden::ReasoningScope::Symbol(symbol) => symbol.0.clone(),
        eden::ReasoningScope::Sector(sector) => format!("sector:{}", sector),
        eden::ReasoningScope::Institution(institution) => {
            format!("institution:{}", institution)
        }
        eden::ReasoningScope::Theme(theme) => format!("theme:{}", theme),
        eden::ReasoningScope::Region(region) => format!("region:{}", region),
        eden::ReasoningScope::Custom(value) => value.to_string(),
    }
}

pub(crate) fn sector_name_for_symbol(
    store: &std::sync::Arc<eden::ontology::store::ObjectStore>,
    symbol: &Symbol,
) -> Option<String> {
    let sector_id = store.stocks.get(symbol)?.sector_id.as_ref()?;
    store
        .sectors
        .get(sector_id)
        .map(|sector| sector.name.clone())
}

pub(crate) fn hk_market_phase(now: time::OffsetDateTime) -> &'static str {
    let hong_kong =
        now.to_offset(time::UtcOffset::from_hms(8, 0, 0).expect("valid Asia/Hong_Kong offset"));
    let minutes = u16::from(hong_kong.hour()) * 60 + u16::from(hong_kong.minute());
    const PRE_MARKET_START: u16 = 9 * 60;
    const PRE_MARKET_END: u16 = 9 * 60 + 29;
    const MORNING_SESSION_START: u16 = 9 * 60 + 30;
    const MORNING_SESSION_END: u16 = 11 * 60 + 59;
    const LUNCH_BREAK_START: u16 = 12 * 60;
    const LUNCH_BREAK_END: u16 = 12 * 60 + 59;
    const AFTERNOON_SESSION_START: u16 = 13 * 60;
    const AFTERNOON_SESSION_END: u16 = 15 * 60 + 59;
    match minutes {
        PRE_MARKET_START..=PRE_MARKET_END => "pre_market",
        MORNING_SESSION_START..=MORNING_SESSION_END => "cash_session",
        LUNCH_BREAK_START..=LUNCH_BREAK_END => "lunch_break",
        AFTERNOON_SESSION_START..=AFTERNOON_SESSION_END => "cash_session",
        _ => "closed",
    }
}

pub(crate) fn hk_market_active(now: time::OffsetDateTime) -> bool {
    hk_market_phase(now) == "cash_session"
}
