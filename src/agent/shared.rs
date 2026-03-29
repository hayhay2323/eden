use super::*;

pub(crate) fn sort_symbol_states(symbols: &mut [AgentSymbolState]) {
    symbols.sort_by(|a, b| {
        symbol_rank(b)
            .cmp(&symbol_rank(a))
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
}

pub(crate) fn symbol_rank(item: &AgentSymbolState) -> Decimal {
    item.structure
        .as_ref()
        .map(|structure| structure.confidence.abs())
        .or_else(|| item.signal.as_ref().map(|signal| signal.composite.abs()))
        .unwrap_or(Decimal::ZERO)
}

pub(crate) fn render_track_state(track: &HypothesisTrack) -> String {
    format!("{}:{}", track.action, track.status.as_str())
}

pub(crate) fn render_hk_transition_summary(
    previous: Option<&HypothesisTrack>,
    current: &HypothesisTrack,
) -> String {
    match previous {
        None => format!(
            "{} entered as {}",
            current.title,
            render_track_state(current)
        ),
        Some(previous) if previous.hypothesis_id != current.hypothesis_id => format!(
            "{} rotated from {} to {}",
            current.title, previous.hypothesis_id, current.hypothesis_id
        ),
        Some(previous) if previous.action != current.action => format!(
            "{} action {} -> {}",
            current.title, previous.action, current.action
        ),
        Some(previous) if previous.status != current.status => format!(
            "{} status {} -> {}",
            current.title,
            previous.status.as_str(),
            current.status.as_str()
        ),
        Some(_) => current
            .transition_reason
            .clone()
            .unwrap_or_else(|| format!("{} changed", current.title)),
    }
}

pub(crate) fn previous_agent_symbol_map(
    previous_agent: Option<&AgentSnapshot>,
) -> HashMap<&str, &AgentSymbolState> {
    previous_agent
        .map(|snapshot| {
            snapshot
                .symbols
                .iter()
                .map(|item| (item.symbol.as_str(), item))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default()
}

pub(crate) fn decimal_mean(sum: Decimal, count: usize) -> Decimal {
    if count == 0 {
        Decimal::ZERO
    } else {
        sum / Decimal::from(count as i64)
    }
}

pub(crate) fn symbol_priority(item: &AgentSymbolState) -> Option<Decimal> {
    item.structure
        .as_ref()
        .map(|structure| structure.confidence.abs())
        .or_else(|| item.signal.as_ref().map(|signal| signal.composite.abs()))
}

pub(crate) fn scope_symbol(scope: &ReasoningScope) -> Option<&Symbol> {
    match scope {
        ReasoningScope::Symbol(symbol) => Some(symbol),
        _ => None,
    }
}

pub(crate) fn dedupe_strings(values: &mut Vec<String>) {
    let mut seen = HashSet::new();
    values.retain(|value| seen.insert(value.clone()));
}

pub(crate) fn push_unique(values: &mut Vec<String>, value: String) {
    if !values.iter().any(|item| item == &value) {
        values.push(value);
    }
}

pub(crate) fn decimal_sign(value: Decimal) -> i8 {
    if value > Decimal::ZERO {
        1
    } else if value < Decimal::ZERO {
        -1
    } else {
        0
    }
}

pub(crate) fn extract_symbols(text: &str) -> Vec<String> {
    text.split(|ch: char| {
        ch.is_whitespace() || ch == ',' || ch == ';' || ch == ':' || ch == '(' || ch == ')'
    })
    .filter(|token| token.ends_with(".HK") || token.ends_with(".US"))
    .map(|token| {
        token
            .trim_matches(|ch: char| ch == '.' || ch == '"' || ch == '\'')
            .to_string()
    })
    .collect()
}
