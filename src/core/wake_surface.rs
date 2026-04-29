use rust_decimal::Decimal;

use crate::live_snapshot::LiveCrossMarketSignal;
use crate::pipeline::state_engine::{PersistentStateKind, PersistentSymbolState};

pub fn format_labeled_decimal_fields(prefix: &str, metrics: &[(&str, Decimal)]) -> String {
    let rendered = metrics
        .iter()
        .map(|(label, value)| format!("{label}={}", value.round_dp(2)))
        .collect::<Vec<_>>()
        .join(", ");
    format!("{prefix}: {rendered}")
}

pub fn hidden_forces_reason(symbols: &[String]) -> Option<String> {
    if symbols.is_empty() {
        return None;
    }
    let shown = symbols.iter().take(5).cloned().collect::<Vec<_>>();
    let extra = if symbols.len() > 5 {
        format!(" (+{} more)", symbols.len() - 5)
    } else {
        String::new()
    };
    Some(format!(
        "hidden forces confirmed ({}): {}{}",
        symbols.len(),
        shown.join(", "),
        extra,
    ))
}

pub fn format_backward_reason(
    subject: &str,
    explanation: &str,
    streak: Option<u64>,
    confidence: Decimal,
    qualifier: Option<&str>,
) -> String {
    let rendered_explanation = match qualifier {
        Some(prefix) => format!("{prefix}`{explanation}`"),
        None => format!("`{explanation}`"),
    };
    match streak {
        Some(streak) => format!(
            "backward: {} → {} (streak={}, conf={})",
            subject,
            rendered_explanation,
            streak,
            confidence.round_dp(2),
        ),
        None => format!(
            "backward: {} → {} (conf={})",
            subject,
            rendered_explanation,
            confidence.round_dp(2),
        ),
    }
}

pub fn cross_market_reason_lines(signals: &[LiveCrossMarketSignal], limit: usize) -> Vec<String> {
    let mut ranked = signals.iter().collect::<Vec<_>>();
    ranked.sort_by(|a, b| {
        b.propagation_confidence
            .abs()
            .cmp(&a.propagation_confidence.abs())
            .then_with(|| a.us_symbol.cmp(&b.us_symbol))
            .then_with(|| a.hk_symbol.cmp(&b.hk_symbol))
    });
    ranked
        .into_iter()
        .take(limit)
        .map(|signal| {
            format!(
                "cross-market: {}/{} pair propagation_conf={}",
                signal.hk_symbol,
                signal.us_symbol,
                signal.propagation_confidence.round_dp(2),
            )
        })
        .collect()
}

pub fn format_stock_cluster_reason(
    members: &[String],
    directional_alignment: Decimal,
    stability: Decimal,
    age: u64,
) -> String {
    let sample = members.iter().take(4).cloned().collect::<Vec<_>>();
    let extra = if members.len() > 4 {
        format!(" (+{} more)", members.len() - 4)
    } else {
        String::new()
    };
    format!(
        "stock cluster: {} aligned={} stability={} age={}{}",
        sample.join(","),
        directional_alignment.round_dp(2),
        stability.round_dp(2),
        age,
        extra,
    )
}

pub fn stable_state_reason_lines(states: &[PersistentSymbolState], limit: usize) -> Vec<String> {
    let mut stable = states
        .iter()
        .filter(|state| {
            matches!(
                state.state_kind,
                PersistentStateKind::TurningPoint | PersistentStateKind::Continuation
            ) && state.state_persistence_ticks >= 3
        })
        .collect::<Vec<_>>();
    stable.sort_by(|a, b| b.confidence.cmp(&a.confidence));
    stable
        .into_iter()
        .take(limit)
        .map(|state| {
            format!(
                "stable: {} {} {} conf={} ({} ticks)",
                state.symbol,
                state.state_kind,
                state.direction.as_deref().unwrap_or("-"),
                state.confidence.round_dp(2),
                state.state_persistence_ticks,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn hidden_forces_reason_caps_visible_symbols() {
        let symbols = vec![
            "A".to_string(),
            "B".to_string(),
            "C".to_string(),
            "D".to_string(),
            "E".to_string(),
            "F".to_string(),
        ];
        let line = hidden_forces_reason(&symbols).unwrap();
        assert!(line.contains("hidden forces confirmed (6): A, B, C, D, E (+1 more)"));
    }

    #[test]
    fn cross_market_reason_lines_sort_by_abs_confidence() {
        let lines = cross_market_reason_lines(
            &[
                LiveCrossMarketSignal {
                    hk_symbol: "700.HK".into(),
                    us_symbol: "TCEHY.US".into(),
                    propagation_confidence: dec!(0.4),
                    time_since_hk_close_minutes: None,
                },
                LiveCrossMarketSignal {
                    hk_symbol: "9988.HK".into(),
                    us_symbol: "BABA.US".into(),
                    propagation_confidence: dec!(-0.8),
                    time_since_hk_close_minutes: None,
                },
            ],
            2,
        );
        assert!(lines[0].contains("9988.HK/BABA.US"));
        assert!(lines[1].contains("700.HK/TCEHY.US"));
    }
}
