//! Replay-based outcome learning — V5.6 graph-native hit rate validator.
//!
//! Eden already records every emerge:* setup with `(symbol, tick,
//! direction)` to ndjson snapshots. Combined with Longport historical
//! price, that's enough to compute hit rate at any horizon **without
//! forward trading** — the graph-temporal state IS the backtest
//! material.
//!
//! This module gives the pure math + aggregation primitives. The
//! Longport price fetch + binary entry point are deliberately out of
//! scope (the binary belongs to its own subagent). Anyone can wire in
//! a price provider via the `PriceProvider` trait.
//!
//! Subsumes task #85 (Codex Epic C: Backtest framework via replay.rs)
//! — that task's framing was rule-based ("replay each tick, run rules");
//! V5.6 is graph-native ("the snapshots ARE the trades").

use std::collections::HashMap;

use chrono::{DateTime, Duration, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use crate::ontology::reasoning::{ReasoningScope, TacticalDirection, TacticalSetup};

/// Horizons at which outcome is evaluated. All values are minutes.
/// 5m / 30m / 1h / 1d gives a typical intraday → end-of-day fan.
pub const REPLAY_HORIZONS_MIN: &[i64] = &[5, 30, 60, 240, 1440];

/// One emerge:* setup record waiting for outcome evaluation.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplaySetup {
    pub setup_id: String,
    pub symbol: String,
    pub tick: u64,
    pub ts: DateTime<Utc>,
    pub direction: TacticalDirection,
    pub entry_price: Decimal,
}

/// One outcome for a (setup, horizon) pair.
#[derive(Debug, Clone)]
pub struct ReplayOutcome {
    pub setup_id: String,
    pub symbol: String,
    pub direction: TacticalDirection,
    pub horizon_min: i64,
    pub entry_price: Decimal,
    pub exit_price: Decimal,
    /// Signed return — positive = setup direction was correct.
    pub signed_return_pct: f64,
    /// True when signed_return_pct > 0.
    pub hit: bool,
}

/// Per-bucket hit rate aggregation.
#[derive(Debug, Clone, Default)]
pub struct ReplayStats {
    pub n: usize,
    pub n_hits: usize,
    pub mean_return_pct: f64,
    pub median_return_pct: f64,
}

impl ReplayStats {
    pub fn hit_rate(&self) -> f64 {
        if self.n == 0 {
            0.0
        } else {
            self.n_hits as f64 / self.n as f64
        }
    }
}

/// Trait so callers can plug Longport historical price (or any other
/// price source — useful for tests).
pub trait PriceProvider {
    fn price_at(&self, symbol: &str, ts: DateTime<Utc>) -> Option<Decimal>;
}

/// Extract `ReplaySetup` records from a list of `TacticalSetup`. Only
/// emerge:* setups with a Symbol scope and concrete direction qualify;
/// vortex-derived and cluster-scope setups are skipped.
pub fn extract_emerge_setups(
    setups: &[TacticalSetup],
    timestamp: DateTime<Utc>,
    tick: u64,
) -> Vec<ReplaySetup> {
    let mut out = Vec::new();
    for s in setups {
        if !s.setup_id.starts_with("emerge:") {
            continue;
        }
        let symbol = match &s.scope {
            ReasoningScope::Symbol(sym) => sym.0.clone(),
            _ => continue,
        };
        let Some(direction) = s.direction else {
            continue;
        };
        // Entry price defaults to confidence-weighted unit; in real use
        // the binary should overwrite via the price provider before
        // computing outcomes.
        out.push(ReplaySetup {
            setup_id: s.setup_id.clone(),
            symbol,
            tick,
            ts: timestamp,
            direction,
            entry_price: Decimal::ZERO,
        });
    }
    out
}

/// Evaluate every `(setup, horizon)` pair using the supplied price
/// provider. Setups without an entry price (or whose entry price
/// falls back to 0) are pre-filled by querying the provider for the
/// setup's own timestamp.
pub fn compute_outcomes<P: PriceProvider>(
    setups: &[ReplaySetup],
    horizons_min: &[i64],
    provider: &P,
) -> Vec<ReplayOutcome> {
    let mut out = Vec::new();
    for s in setups {
        // Resolve entry price if absent.
        let entry = if s.entry_price > Decimal::ZERO {
            s.entry_price
        } else {
            match provider.price_at(&s.symbol, s.ts) {
                Some(p) => p,
                None => continue,
            }
        };
        if entry <= Decimal::ZERO {
            continue;
        }
        for &h in horizons_min {
            let exit_ts = s.ts + Duration::minutes(h);
            let Some(exit) = provider.price_at(&s.symbol, exit_ts) else {
                continue;
            };
            if exit <= Decimal::ZERO {
                continue;
            }
            let entry_f = entry.to_f64().unwrap_or(1.0).max(1e-9);
            let exit_f = exit.to_f64().unwrap_or(entry_f);
            let raw_pct = (exit_f - entry_f) / entry_f * 100.0;
            let dir_sign = match s.direction {
                TacticalDirection::Long => 1.0,
                TacticalDirection::Short => -1.0,
            };
            let signed = raw_pct * dir_sign;
            out.push(ReplayOutcome {
                setup_id: s.setup_id.clone(),
                symbol: s.symbol.clone(),
                direction: s.direction,
                horizon_min: h,
                entry_price: entry,
                exit_price: exit,
                signed_return_pct: signed,
                hit: signed > 0.0,
            });
        }
    }
    out
}

/// Aggregate outcomes by `(direction, horizon_min)` into hit rate stats.
pub fn aggregate_by_direction_horizon(
    outcomes: &[ReplayOutcome],
) -> HashMap<(TacticalDirection, i64), ReplayStats> {
    let mut by_bucket: HashMap<(TacticalDirection, i64), Vec<f64>> = HashMap::new();
    for o in outcomes {
        by_bucket
            .entry((o.direction, o.horizon_min))
            .or_default()
            .push(o.signed_return_pct);
    }
    by_bucket
        .into_iter()
        .map(|(bucket, mut returns)| {
            returns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let n = returns.len();
            let n_hits = returns.iter().filter(|r| **r > 0.0).count();
            let mean = if n > 0 {
                returns.iter().sum::<f64>() / n as f64
            } else {
                0.0
            };
            let median = if n == 0 {
                0.0
            } else if n % 2 == 1 {
                returns[n / 2]
            } else {
                (returns[n / 2 - 1] + returns[n / 2]) / 2.0
            };
            (
                bucket,
                ReplayStats {
                    n,
                    n_hits,
                    mean_return_pct: mean,
                    median_return_pct: median,
                },
            )
        })
        .collect()
}

/// Aggregate outcomes by `symbol` (irrespective of direction / horizon).
/// Useful for "which symbols Eden has the most edge on" reports.
pub fn aggregate_by_symbol(outcomes: &[ReplayOutcome]) -> HashMap<String, ReplayStats> {
    let mut by_bucket: HashMap<String, Vec<f64>> = HashMap::new();
    for o in outcomes {
        by_bucket
            .entry(o.symbol.clone())
            .or_default()
            .push(o.signed_return_pct);
    }
    by_bucket
        .into_iter()
        .map(|(symbol, mut returns)| {
            returns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
            let n = returns.len();
            let n_hits = returns.iter().filter(|r| **r > 0.0).count();
            let mean = if n > 0 {
                returns.iter().sum::<f64>() / n as f64
            } else {
                0.0
            };
            let median = if n == 0 {
                0.0
            } else if n % 2 == 1 {
                returns[n / 2]
            } else {
                (returns[n / 2 - 1] + returns[n / 2]) / 2.0
            };
            (
                symbol,
                ReplayStats {
                    n,
                    n_hits,
                    mean_return_pct: mean,
                    median_return_pct: median,
                },
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
    use crate::ontology::objects::Symbol;
    use crate::ontology::reasoning::{
        default_case_horizon, DecisionLineage, TacticalAction, TacticalDirection,
    };
    use rust_decimal_macros::dec;
    use std::collections::HashMap;
    use time::OffsetDateTime;

    /// Fake price provider keyed on (symbol, ts.timestamp()).
    struct FakeProvider(HashMap<(String, i64), Decimal>);
    impl PriceProvider for FakeProvider {
        fn price_at(&self, symbol: &str, ts: DateTime<Utc>) -> Option<Decimal> {
            self.0.get(&(symbol.to_string(), ts.timestamp())).copied()
        }
    }

    fn fake_setup(setup_id: &str, symbol: &str, dir: TacticalDirection) -> TacticalSetup {
        TacticalSetup {
            setup_id: setup_id.to_string(),
            hypothesis_id: "hyp".to_string(),
            runner_up_hypothesis_id: None,
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            ),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(Symbol(symbol.to_string())),
            title: "test".to_string(),
            action: TacticalAction::Observe,
            direction: Some(dir),
            horizon: default_case_horizon(),
            confidence: dec!(0.5),
            confidence_gap: Decimal::ZERO,
            heuristic_edge: Decimal::ZERO,
            convergence_score: None,
            convergence_detail: None,
            workflow_id: None,
            entry_rationale: String::new(),
            causal_narrative: None,
            risk_notes: Vec::new(),
            review_reason_code: None,
            policy_verdict: None,
        }
    }

    #[test]
    fn extract_only_emerge_setups() {
        let setups = vec![
            fake_setup("emerge:X.HK:1", "X.HK", TacticalDirection::Long),
            fake_setup("vortex:Y.HK:1", "Y.HK", TacticalDirection::Long),
        ];
        let extracted = extract_emerge_setups(&setups, Utc::now(), 1);
        assert_eq!(extracted.len(), 1);
        assert_eq!(extracted[0].setup_id, "emerge:X.HK:1");
    }

    #[test]
    fn compute_outcomes_signs_long_correctly() {
        let ts = Utc::now();
        let mut prices = HashMap::new();
        prices.insert(("X.HK".to_string(), ts.timestamp()), dec!(100));
        prices.insert(
            ("X.HK".to_string(), ts.timestamp() + 5 * 60),
            dec!(110), // up 10% at +5 min
        );
        let provider = FakeProvider(prices);
        let setups = vec![ReplaySetup {
            setup_id: "emerge:X.HK:1".to_string(),
            symbol: "X.HK".to_string(),
            tick: 1,
            ts,
            direction: TacticalDirection::Long,
            entry_price: Decimal::ZERO,
        }];
        let outs = compute_outcomes(&setups, &[5], &provider);
        assert_eq!(outs.len(), 1);
        assert!((outs[0].signed_return_pct - 10.0).abs() < 1e-6);
        assert!(outs[0].hit);
    }

    #[test]
    fn compute_outcomes_inverts_short_sign() {
        let ts = Utc::now();
        let mut prices = HashMap::new();
        prices.insert(("X.HK".to_string(), ts.timestamp()), dec!(100));
        prices.insert(("X.HK".to_string(), ts.timestamp() + 5 * 60), dec!(90));
        let provider = FakeProvider(prices);
        let setups = vec![ReplaySetup {
            setup_id: "emerge:X.HK:1".to_string(),
            symbol: "X.HK".to_string(),
            tick: 1,
            ts,
            direction: TacticalDirection::Short,
            entry_price: Decimal::ZERO,
        }];
        let outs = compute_outcomes(&setups, &[5], &provider);
        assert_eq!(outs.len(), 1);
        assert!((outs[0].signed_return_pct - 10.0).abs() < 1e-6);
        assert!(outs[0].hit);
    }

    #[test]
    fn aggregate_by_direction_horizon_buckets_correctly() {
        let outcomes = vec![
            ReplayOutcome {
                setup_id: "a".to_string(),
                symbol: "X".to_string(),
                direction: TacticalDirection::Long,
                horizon_min: 5,
                entry_price: dec!(100),
                exit_price: dec!(105),
                signed_return_pct: 5.0,
                hit: true,
            },
            ReplayOutcome {
                setup_id: "b".to_string(),
                symbol: "Y".to_string(),
                direction: TacticalDirection::Long,
                horizon_min: 5,
                entry_price: dec!(100),
                exit_price: dec!(99),
                signed_return_pct: -1.0,
                hit: false,
            },
            ReplayOutcome {
                setup_id: "c".to_string(),
                symbol: "Z".to_string(),
                direction: TacticalDirection::Short,
                horizon_min: 5,
                entry_price: dec!(100),
                exit_price: dec!(98),
                signed_return_pct: 2.0,
                hit: true,
            },
        ];
        let stats = aggregate_by_direction_horizon(&outcomes);
        let long_5 = &stats[&(TacticalDirection::Long, 5)];
        assert_eq!(long_5.n, 2);
        assert_eq!(long_5.n_hits, 1);
        assert_eq!(long_5.hit_rate(), 0.5);
        let short_5 = &stats[&(TacticalDirection::Short, 5)];
        assert_eq!(short_5.n, 1);
        assert_eq!(short_5.hit_rate(), 1.0);
    }

    #[test]
    fn aggregate_by_symbol_picks_per_sym_alpha() {
        let outcomes = vec![
            ReplayOutcome {
                setup_id: "a".to_string(),
                symbol: "X".to_string(),
                direction: TacticalDirection::Long,
                horizon_min: 5,
                entry_price: dec!(100),
                exit_price: dec!(105),
                signed_return_pct: 5.0,
                hit: true,
            },
            ReplayOutcome {
                setup_id: "b".to_string(),
                symbol: "X".to_string(),
                direction: TacticalDirection::Long,
                horizon_min: 30,
                entry_price: dec!(100),
                exit_price: dec!(110),
                signed_return_pct: 10.0,
                hit: true,
            },
            ReplayOutcome {
                setup_id: "c".to_string(),
                symbol: "Y".to_string(),
                direction: TacticalDirection::Short,
                horizon_min: 5,
                entry_price: dec!(100),
                exit_price: dec!(101),
                signed_return_pct: -1.0,
                hit: false,
            },
        ];
        let stats = aggregate_by_symbol(&outcomes);
        let x = &stats["X"];
        assert_eq!(x.n, 2);
        assert_eq!(x.n_hits, 2);
        assert_eq!(x.hit_rate(), 1.0);
        assert!((x.mean_return_pct - 7.5).abs() < 1e-6);
        let y = &stats["Y"];
        assert_eq!(y.n, 1);
        assert_eq!(y.hit_rate(), 0.0);
    }
}
