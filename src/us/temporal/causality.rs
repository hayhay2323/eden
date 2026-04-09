//! US causal timeline: tracks which dimension is LEADING the convergence signal
//! for each stock across ticks.
//!
//! At each tick, the dimension with the highest absolute value is the "leader".
//! When the leader changes, a UsCausalFlip is recorded. The flip history
//! reveals regime changes: "capital_flow led XPEV for 15 ticks, then momentum took over".

use std::collections::HashMap;

use crate::ontology::objects::Symbol;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::buffer::UsTickHistory;
use super::record::UsSymbolSignals;

// ── Core types ──

/// One data point in the causal timeline: which dimension is leading at this tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsCausalPoint {
    pub tick: u64,
    /// The dimension name with the highest absolute value at this tick.
    pub leader: String,
    /// The raw value of the leading dimension (signed).
    pub leader_value: Decimal,
    /// The composite convergence score at this tick.
    pub composite: Decimal,
}

/// A leadership change event: the dominant dimension flipped from one to another.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsCausalFlip {
    pub tick: u64,
    /// The dimension that was leading before the flip.
    pub from_leader: String,
    /// The dimension that took over after the flip.
    pub to_leader: String,
    /// The composite score at the moment of the flip.
    pub composite_at_flip: Decimal,
}

/// Full causal timeline for one stock.
/// Ring-buffer semantics: the caller controls how many ticks are kept
/// (via `UsTickHistory` capacity) before calling `compute_causal_timelines`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsCausalTimeline {
    pub symbol: Symbol,
    /// Causal points in chronological order (last N ticks from history).
    pub points: Vec<UsCausalPoint>,
    /// All leadership changes detected in the window.
    pub flips: Vec<UsCausalFlip>,
    /// The leader at the most-recent tick.
    pub current_leader: String,
    /// How many consecutive ticks the current leader has been dominant.
    pub leader_streak: u64,
}

impl UsCausalTimeline {
    pub fn latest_point(&self) -> Option<&UsCausalPoint> {
        self.points.last()
    }

    pub fn latest_flip(&self) -> Option<&UsCausalFlip> {
        self.flips.last()
    }

}

// ── Named dimension extraction ──

/// The five US dimension names, in the order we iterate them.
const DIMENSION_NAMES: [&str; 5] = [
    "capital_flow",
    "momentum",
    "volume_profile",
    "pre_post_market",
    "valuation",
];

/// Extract the five US dimension values from a `UsSymbolSignals` record.
fn dimension_values(signals: &UsSymbolSignals) -> [Decimal; 5] {
    [
        signals.capital_flow_direction,
        signals.price_momentum,
        signals.volume_profile,
        signals.pre_post_market_anomaly,
        signals.valuation,
    ]
}

/// Find the dimension with the highest absolute value.
/// Returns (name, signed_value). If all are zero, returns ("none", ZERO).
fn dominant_dimension(signals: &UsSymbolSignals) -> (&'static str, Decimal) {
    let values = dimension_values(signals);
    let mut best_abs = Decimal::ZERO;
    let mut best_name = "none";
    let mut best_value = Decimal::ZERO;
    for (i, &value) in values.iter().enumerate() {
        if value.abs() > best_abs {
            best_abs = value.abs();
            best_name = DIMENSION_NAMES[i];
            best_value = value;
        }
    }
    (best_name, best_value)
}

// ── Main computation ──

/// Compute causal timelines for all symbols present in the tick history.
///
/// For each symbol, iterates every tick in the history, finds the dominant
/// dimension at each tick, and records flips when the leader changes.
pub fn compute_causal_timelines(history: &UsTickHistory) -> HashMap<Symbol, UsCausalTimeline> {
    // Collect all symbols that appear across all ticks.
    let all_records = history.latest_n(history.len());
    let mut symbol_set: Vec<Symbol> = {
        let mut seen = std::collections::HashSet::new();
        for record in &all_records {
            for sym in record.signals.keys() {
                seen.insert(sym.clone());
            }
        }
        seen.into_iter().collect()
    };
    symbol_set.sort_by(|a, b| a.0.cmp(&b.0));

    let mut result = HashMap::new();

    for symbol in symbol_set {
        let mut points: Vec<UsCausalPoint> = Vec::new();
        let mut flips: Vec<UsCausalFlip> = Vec::new();

        for record in &all_records {
            let Some(signals) = record.signals.get(&symbol) else {
                continue;
            };

            let (leader, leader_value) = dominant_dimension(signals);

            // Check for leadership change vs the previous point.
            if let Some(prev) = points.last() {
                if prev.leader != leader {
                    flips.push(UsCausalFlip {
                        tick: record.tick_number,
                        from_leader: prev.leader.clone(),
                        to_leader: leader.to_string(),
                        composite_at_flip: signals.composite,
                    });
                }
            }

            points.push(UsCausalPoint {
                tick: record.tick_number,
                leader: leader.to_string(),
                leader_value,
                composite: signals.composite,
            });
        }

        if points.is_empty() {
            continue;
        }

        // Compute current_leader and leader_streak from the tail of points.
        let current_leader = points.last().unwrap().leader.clone();
        let leader_streak = points
            .iter()
            .rev()
            .take_while(|p| p.leader == current_leader)
            .count() as u64;

        result.insert(
            symbol.clone(),
            UsCausalTimeline {
                symbol,
                points,
                flips,
                current_leader,
                leader_streak,
            },
        );
    }

    result
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::us::graph::decision::UsMarketRegimeBias;
    use crate::us::temporal::record::UsTickRecord;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;
    use time::OffsetDateTime;

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
    }

    fn make_signals(
        flow: Decimal,
        momentum: Decimal,
        volume: Decimal,
        prepost: Decimal,
        valuation: Decimal,
    ) -> UsSymbolSignals {
        let composite = (flow + momentum + volume + prepost + valuation) / dec!(5);
        UsSymbolSignals {
            mark_price: None,
            composite,
            composite_delta: Decimal::ZERO,
            composite_acceleration: Decimal::ZERO,
            capital_flow_direction: flow,
            capital_flow_delta: Decimal::ZERO,
            flow_persistence: 0,
            flow_reversal: false,
            price_momentum: momentum,
            volume_profile: volume,
            pre_post_market_anomaly: prepost,
            valuation,
            pre_market_delta: Decimal::ZERO,
        }
    }

    fn make_tick(tick: u64, entries: Vec<(Symbol, UsSymbolSignals)>) -> UsTickRecord {
        let mut signals = HashMap::new();
        for (sym, sig) in entries {
            signals.insert(sym, sig);
        }
        UsTickRecord {
            tick_number: tick,
            timestamp: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(tick as i64),
            signals,
            cross_market_signals: vec![],
            events: vec![],
            derived_signals: vec![],
            hypotheses: vec![],
            tactical_setups: vec![],
            market_regime: UsMarketRegimeBias::Neutral,
        }
    }

    // ── dominant_dimension ──

    #[test]
    fn dominant_picks_highest_abs() {
        let sig = make_signals(dec!(0.1), dec!(-0.8), dec!(0.3), dec!(0.2), dec!(0.1));
        let (name, value) = dominant_dimension(&sig);
        assert_eq!(name, "momentum");
        assert_eq!(value, dec!(-0.8));
    }

    #[test]
    fn dominant_all_zero_returns_none() {
        let sig = make_signals(
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::ZERO,
        );
        let (name, _) = dominant_dimension(&sig);
        assert_eq!(name, "none");
    }

    #[test]
    fn dominant_picks_capital_flow_when_largest() {
        let sig = make_signals(dec!(0.9), dec!(0.3), dec!(0.2), dec!(0.1), dec!(0.0));
        let (name, value) = dominant_dimension(&sig);
        assert_eq!(name, "capital_flow");
        assert_eq!(value, dec!(0.9));
    }

    // ── UsCausalTimeline ──

    #[test]
    fn stable_leader_no_flips() {
        let mut history = crate::us::temporal::buffer::UsTickHistory::new(10);
        // Three ticks where momentum is always the dominant dimension.
        history.push(make_tick(
            1,
            vec![(
                sym("NVDA.US"),
                make_signals(dec!(0.1), dec!(0.8), dec!(0.2), dec!(0.1), dec!(0.0)),
            )],
        ));
        history.push(make_tick(
            2,
            vec![(
                sym("NVDA.US"),
                make_signals(dec!(0.1), dec!(0.7), dec!(0.3), dec!(0.1), dec!(0.0)),
            )],
        ));
        history.push(make_tick(
            3,
            vec![(
                sym("NVDA.US"),
                make_signals(dec!(0.2), dec!(0.9), dec!(0.1), dec!(0.0), dec!(0.0)),
            )],
        ));

        let timelines = compute_causal_timelines(&history);
        let tl = timelines.get(&sym("NVDA.US")).expect("timeline present");

        assert_eq!(tl.points.len(), 3);
        assert!(tl.flips.is_empty());
        assert_eq!(tl.current_leader, "momentum");
        assert_eq!(tl.leader_streak, 3);
    }

    #[test]
    fn leadership_flip_recorded() {
        let mut history = crate::us::temporal::buffer::UsTickHistory::new(10);
        // Tick 1: momentum leads.
        history.push(make_tick(
            1,
            vec![(
                sym("XPEV.US"),
                make_signals(dec!(0.1), dec!(0.8), dec!(0.0), dec!(0.0), dec!(0.0)),
            )],
        ));
        // Tick 2: capital_flow takes over.
        history.push(make_tick(
            2,
            vec![(
                sym("XPEV.US"),
                make_signals(dec!(0.9), dec!(0.2), dec!(0.0), dec!(0.0), dec!(0.0)),
            )],
        ));
        // Tick 3: capital_flow still leads.
        history.push(make_tick(
            3,
            vec![(
                sym("XPEV.US"),
                make_signals(dec!(0.7), dec!(0.3), dec!(0.0), dec!(0.0), dec!(0.0)),
            )],
        ));

        let timelines = compute_causal_timelines(&history);
        let tl = timelines.get(&sym("XPEV.US")).expect("timeline present");

        assert_eq!(tl.flips.len(), 1);
        assert_eq!(tl.flips[0].from_leader, "momentum");
        assert_eq!(tl.flips[0].to_leader, "capital_flow");
        assert_eq!(tl.flips[0].tick, 2);
        assert_eq!(tl.current_leader, "capital_flow");
        assert_eq!(tl.leader_streak, 2);
    }

    #[test]
    fn multiple_flips_tracked() {
        let mut history = crate::us::temporal::buffer::UsTickHistory::new(10);
        // Tick 1: capital_flow
        history.push(make_tick(
            1,
            vec![(
                sym("BABA.US"),
                make_signals(dec!(0.8), dec!(0.1), dec!(0.0), dec!(0.0), dec!(0.0)),
            )],
        ));
        // Tick 2: momentum
        history.push(make_tick(
            2,
            vec![(
                sym("BABA.US"),
                make_signals(dec!(0.1), dec!(0.9), dec!(0.0), dec!(0.0), dec!(0.0)),
            )],
        ));
        // Tick 3: pre_post_market
        history.push(make_tick(
            3,
            vec![(
                sym("BABA.US"),
                make_signals(dec!(0.1), dec!(0.2), dec!(0.0), dec!(0.8), dec!(0.0)),
            )],
        ));

        let timelines = compute_causal_timelines(&history);
        let tl = timelines.get(&sym("BABA.US")).unwrap();
        assert_eq!(tl.flips.len(), 2);
        assert_eq!(tl.current_leader, "pre_post_market");
        assert_eq!(tl.leader_streak, 1);
    }

    #[test]
    fn empty_history_produces_no_timelines() {
        let history = crate::us::temporal::buffer::UsTickHistory::new(10);
        let timelines = compute_causal_timelines(&history);
        assert!(timelines.is_empty());
    }

    #[test]
    fn symbol_absent_from_some_ticks_is_handled() {
        let mut history = crate::us::temporal::buffer::UsTickHistory::new(10);
        // Tick 1: only AAPL
        history.push(make_tick(
            1,
            vec![(
                sym("AAPL.US"),
                make_signals(dec!(0.5), dec!(0.1), dec!(0.0), dec!(0.0), dec!(0.0)),
            )],
        ));
        // Tick 2: both AAPL and NVDA
        history.push(make_tick(
            2,
            vec![
                (
                    sym("AAPL.US"),
                    make_signals(dec!(0.4), dec!(0.2), dec!(0.0), dec!(0.0), dec!(0.0)),
                ),
                (
                    sym("NVDA.US"),
                    make_signals(dec!(0.0), dec!(0.7), dec!(0.0), dec!(0.0), dec!(0.0)),
                ),
            ],
        ));

        let timelines = compute_causal_timelines(&history);
        assert!(timelines.contains_key(&sym("AAPL.US")));
        assert!(timelines.contains_key(&sym("NVDA.US")));
        // NVDA only appeared once
        assert_eq!(timelines[&sym("NVDA.US")].points.len(), 1);
        assert_eq!(timelines[&sym("NVDA.US")].leader_streak, 1);
    }

    #[test]
    fn flip_event_carries_composite_at_flip() {
        let mut history = crate::us::temporal::buffer::UsTickHistory::new(10);
        history.push(make_tick(
            1,
            vec![(
                sym("JD.US"),
                make_signals(dec!(0.8), dec!(0.1), dec!(0.0), dec!(0.0), dec!(0.0)),
            )],
        ));
        // composite at tick 2 = (0.1 + 0.7 + 0 + 0 + 0) / 5 = 0.16
        history.push(make_tick(
            2,
            vec![(
                sym("JD.US"),
                make_signals(dec!(0.1), dec!(0.7), dec!(0.0), dec!(0.0), dec!(0.0)),
            )],
        ));

        let timelines = compute_causal_timelines(&history);
        let tl = timelines.get(&sym("JD.US")).unwrap();
        let flip = tl.flips.first().unwrap();
        assert_eq!(flip.tick, 2);
        // The composite at the flip tick equals the composite stored in the signals.
        assert_eq!(flip.composite_at_flip, tl.points[1].composite);
    }
}
