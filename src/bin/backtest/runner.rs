use eden::ontology::mechanisms::MechanismCandidateKind;
use eden::pipeline::mechanism_inference::build_reasoning_profile;
use eden::pipeline::predicate_engine::{derive_atomic_predicates, PredicateInputs};
use rust_decimal::Decimal;

use super::adapter::SyntheticTick;
use super::loader::Bar;

// ── types ─────────────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Judgment {
    pub timestamp: i64,
    pub symbol: String,
    pub session: &'static str,
    pub regime: String,
    pub mechanism: MechanismCandidateKind,
    pub mechanism_label: String,
    pub direction: i8,
    pub confidence: Decimal,
    pub score: Decimal,
}

#[derive(Debug, Clone)]
pub struct ValidatedJudgment {
    pub judgment: Judgment,
    pub outcomes: Vec<HorizonOutcome>,
}

#[derive(Debug, Clone, Copy)]
pub struct HorizonOutcome {
    pub horizon_bars: usize,
    pub horizon_label: &'static str,
    pub future_return: Decimal,
    pub hit: bool,
}

pub const HORIZONS: &[(usize, &str)] = &[(5, "5m"), (30, "30m"), (60, "1h"), (390, "1d")];

pub fn classify_hk_session(timestamp: i64) -> &'static str {
    let dt = time::OffsetDateTime::from_unix_timestamp(timestamp)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
        .to_offset(time::UtcOffset::from_hms(8, 0, 0).unwrap());
    let minutes = dt.hour() as i32 * 60 + dt.minute() as i32;
    match minutes {
        570..=629 => "opening", // 09:30-10:29 HKT
        630..=869 => "midday",  // 10:30-14:29 HKT (includes post-lunch continuation)
        870..=960 => "closing", // 14:30-16:00 HKT
        _ => "other",
    }
}

// ── pipeline evaluation ───────────────────────────────────────────────────────

/// Run the predicate → mechanism pipeline on a synthetic tick.
/// Returns None if the tick is neutral (direction == 0) or no primary mechanism is found.
pub fn evaluate_tick(tick: &SyntheticTick) -> Option<Judgment> {
    if tick.direction == 0 {
        return None;
    }

    let inputs = PredicateInputs {
        tactical_case: &tick.case,
        active_positions: &[],
        chain: None,
        pressure: Some(&tick.pressure),
        signal: Some(&tick.signal),
        causal: None,
        track: None,
        stress: &tick.stress,
        market_regime: &tick.regime,
        all_signals: std::slice::from_ref(&tick.signal),
        all_pressures: std::slice::from_ref(&tick.pressure),
        events: &[],
        cross_market_signals: &[],
        cross_market_anomalies: &[],
    };

    let predicates = derive_atomic_predicates(&inputs);
    let profile = build_reasoning_profile(&predicates, &[], None);

    let primary = profile.primary_mechanism?;

    Some(Judgment {
        timestamp: tick.timestamp,
        symbol: tick.case.symbol.clone(),
        session: classify_hk_session(tick.timestamp),
        regime: tick.regime.bias.clone(),
        mechanism: primary.kind,
        mechanism_label: primary.label.clone(),
        direction: tick.direction,
        confidence: tick.case.confidence,
        score: primary.score,
    })
}

// ── multi-horizon validation ──────────────────────────────────────────────────

/// Validate a judgment against future bars at each horizon.
/// `reference_price` is the close price of the bar that produced the judgment.
pub fn validate_judgment(
    judgment: &Judgment,
    future_bars: &[Bar],
    reference_price: f64,
) -> ValidatedJudgment {
    let outcomes = HORIZONS
        .iter()
        .map(|&(horizon_bars, horizon_label)| {
            // horizon_bars is 1-indexed offset (bar 5 = index 4)
            let idx = horizon_bars.saturating_sub(1);
            if let Some(bar) = future_bars.get(idx) {
                let future_return = if reference_price != 0.0 {
                    (bar.close - reference_price) / reference_price
                } else {
                    0.0
                };
                let future_return_dec = Decimal::try_from(future_return).unwrap_or(Decimal::ZERO);
                let hit = if judgment.direction > 0 {
                    future_return > 0.0
                } else {
                    future_return < 0.0
                };
                HorizonOutcome {
                    horizon_bars,
                    horizon_label,
                    future_return: future_return_dec,
                    hit,
                }
            } else {
                HorizonOutcome {
                    horizon_bars,
                    horizon_label,
                    future_return: Decimal::ZERO,
                    hit: false,
                }
            }
        })
        .collect();

    ValidatedJudgment {
        judgment: judgment.clone(),
        outcomes,
    }
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn bullish_judgment_hits_on_price_increase() {
        let judgment = Judgment {
            timestamp: 1700000000,
            symbol: "700.HK".into(),
            session: "opening",
            regime: "neutral".into(),
            mechanism: MechanismCandidateKind::MechanicalExecutionSignature,
            mechanism_label: "Mechanical Execution Signature".into(),
            direction: 1,
            confidence: dec!(0.7),
            score: dec!(0.7),
        };
        // Future bars with steadily rising prices
        let future_bars: Vec<Bar> = (0..400)
            .map(|i| Bar {
                symbol: "700.HK".into(),
                ts: 1700000000 + i * 60,
                open: 100.0 + i as f64 * 0.01,
                high: 100.5 + i as f64 * 0.01,
                low: 99.5,
                close: 100.0 + i as f64 * 0.02,
                volume: 100000,
                turnover: 10000000.0,
            })
            .collect();
        let validated = validate_judgment(&judgment, &future_bars, 100.0);
        assert!(
            validated.outcomes.iter().all(|o| o.hit),
            "all horizons should hit on rising prices"
        );
    }

    #[test]
    fn bearish_judgment_misses_on_price_increase() {
        let judgment = Judgment {
            timestamp: 1700000000,
            symbol: "700.HK".into(),
            session: "opening",
            regime: "neutral".into(),
            mechanism: MechanismCandidateKind::FragilityBuildUp,
            mechanism_label: "Fragility Build-up".into(),
            direction: -1,
            confidence: dec!(0.6),
            score: dec!(0.6),
        };
        let future_bars: Vec<Bar> = (0..400)
            .map(|i| Bar {
                symbol: "700.HK".into(),
                ts: 1700000000 + i * 60,
                open: 100.0 + i as f64 * 0.01,
                high: 100.5,
                low: 99.5,
                close: 100.0 + i as f64 * 0.02,
                volume: 100000,
                turnover: 10000000.0,
            })
            .collect();
        let validated = validate_judgment(&judgment, &future_bars, 100.0);
        assert!(
            validated.outcomes.iter().all(|o| !o.hit),
            "bearish judgment should miss on rising prices"
        );
    }
}
