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
    }
}

pub(crate) fn build_hk_lineage_metrics(
    stats: &eden::temporal::lineage::LineageStats,
) -> Vec<LiveLineageMetric> {
    stats
        .promoted_outcomes
        .iter()
        .take(6)
        .map(|item| LiveLineageMetric {
            template: item.label.clone(),
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

pub(crate) fn hk_action_surface_priority(action: &str) -> i32 {
    match action {
        "enter" => 0,
        "review" => 1,
        "observe" => 2,
        _ => 3,
    }
}
