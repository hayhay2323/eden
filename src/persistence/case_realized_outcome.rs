use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::temporal::lineage::CaseRealizedOutcome;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaseRealizedOutcomeRecord {
    pub setup_id: String,
    pub workflow_id: Option<String>,
    pub market: String,
    pub symbol: Option<String>,
    pub primary_lens: Option<String>,
    pub family: String,
    pub session: String,
    pub market_regime: String,
    pub entry_tick: u64,
    #[serde(with = "rfc3339")]
    pub entry_timestamp: OffsetDateTime,
    pub resolved_tick: u64,
    #[serde(with = "rfc3339")]
    pub resolved_at: OffsetDateTime,
    pub direction: i8,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub return_pct: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub net_return: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub max_favorable_excursion: Decimal,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub max_adverse_excursion: Decimal,
    pub followed_through: bool,
    pub invalidated: bool,
    pub structure_retained: bool,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub convergence_score: Decimal,
}

impl CaseRealizedOutcomeRecord {
    #[allow(deprecated)]
    pub fn from_outcome(
        outcome: &CaseRealizedOutcome,
        market: &str,
        primary_lens: Option<String>,
    ) -> Self {
        Self {
            setup_id: outcome.setup_id.clone(),
            workflow_id: outcome.workflow_id.clone(),
            market: market.to_string(),
            symbol: outcome.symbol.clone(),
            primary_lens,
            family: outcome.family.clone(),
            session: outcome.session.clone(),
            market_regime: outcome.market_regime.clone(),
            entry_tick: outcome.entry_tick,
            entry_timestamp: outcome.entry_timestamp,
            resolved_tick: outcome.resolved_tick,
            resolved_at: outcome.resolved_at,
            direction: outcome.direction,
            return_pct: outcome.return_pct,
            net_return: outcome.net_return,
            max_favorable_excursion: outcome.max_favorable_excursion,
            max_adverse_excursion: outcome.max_adverse_excursion,
            followed_through: outcome.followed_through,
            invalidated: outcome.invalidated,
            structure_retained: outcome.structure_retained,
            convergence_score: outcome.convergence_score,
        }
    }

    /// Build a record from a US resolved topology outcome.
    /// Uses the topology outcome's basic fields and fills structural fields with defaults.
    pub fn from_us_topology_outcome(
        outcome: &crate::us::temporal::lineage::UsResolvedTopologyOutcome,
        entry_tick: u64,
        entry_timestamp: OffsetDateTime,
        resolved_at: OffsetDateTime,
        family: &str,
    ) -> Self {
        let followed_through = outcome.net_return > Decimal::ZERO;
        Self {
            setup_id: outcome.setup_id.clone(),
            workflow_id: None,
            market: "us".to_string(),
            symbol: Some(outcome.symbol.0.clone()),
            primary_lens: None,
            family: family.to_string(),
            session: "live".to_string(),
            market_regime: "neutral".to_string(),
            entry_tick,
            entry_timestamp,
            resolved_tick: outcome.resolved_tick,
            resolved_at,
            direction: if outcome.net_return >= Decimal::ZERO {
                1
            } else {
                -1
            },
            return_pct: outcome.net_return * Decimal::from(100),
            net_return: outcome.net_return,
            max_favorable_excursion: if outcome.net_return > Decimal::ZERO {
                outcome.net_return
            } else {
                Decimal::ZERO
            },
            max_adverse_excursion: if outcome.net_return < Decimal::ZERO {
                outcome.net_return.abs()
            } else {
                Decimal::ZERO
            },
            followed_through,
            invalidated: !followed_through && outcome.net_return < Decimal::new(-5, 3),
            structure_retained: followed_through,
            convergence_score: outcome.convergence_detail.institutional_alignment,
        }
    }

    pub fn record_id(&self) -> &str {
        &self.setup_id
    }
}

/// Rolling ledger of resolved cases. Survives buffer pruning so the
/// long-run track record is visible on every tick regardless of how
/// many records are currently in `TickHistory`.
///
/// Records are deduplicated by `setup_id`: each setup resolves once,
/// subsequent calls on the same id are treated as corrections (last
/// write wins). Family and regime breakdowns are derived on read so
/// we don't pay the cost of re-aggregating mid-tick.
#[derive(Debug, Clone, Default)]
pub struct EdenLedgerAccumulator {
    resolved: std::collections::HashMap<String, ResolvedEntry>,
}

#[derive(Debug, Clone)]
struct ResolvedEntry {
    net_return: Decimal,
    followed_through: bool,
    invalidated: bool,
    family: String,
}

/// Aggregate ledger read — one-shot snapshot used to render the
/// `ledger:` wake.reasons line. Everything is optional: if no outcomes
/// have been accumulated the caller skips the line.
#[derive(Debug, Clone)]
pub struct EdenLedgerSummary {
    pub total: usize,
    pub wins: usize,
    pub losses: usize,
    pub flats: usize,
    pub hit_rate: Decimal,
    pub mean_net_return: Decimal,
    pub best_family: Option<(String, usize, Decimal, Decimal)>,
    pub worst_family: Option<(String, usize, Decimal, Decimal)>,
}

impl EdenLedgerAccumulator {
    pub fn record(&mut self, outcome: &CaseRealizedOutcomeRecord) {
        self.resolved.insert(
            outcome.setup_id.clone(),
            ResolvedEntry {
                net_return: outcome.net_return,
                followed_through: outcome.followed_through,
                invalidated: outcome.invalidated,
                family: outcome.family.clone(),
            },
        );
    }

    pub fn record_batch(&mut self, outcomes: &[CaseRealizedOutcomeRecord]) {
        for outcome in outcomes {
            self.record(outcome);
        }
    }

    pub fn len(&self) -> usize {
        self.resolved.len()
    }

    pub fn is_empty(&self) -> bool {
        self.resolved.is_empty()
    }

    pub fn summary(&self) -> Option<EdenLedgerSummary> {
        if self.resolved.is_empty() {
            return None;
        }
        let mut total = 0usize;
        let mut wins = 0usize;
        let mut losses = 0usize;
        let mut flats = 0usize;
        let mut sum = Decimal::ZERO;
        let mut by_family: std::collections::HashMap<String, FamilyBucket> =
            std::collections::HashMap::new();
        for entry in self.resolved.values() {
            total += 1;
            sum += entry.net_return;
            if entry.followed_through {
                wins += 1;
            } else if entry.invalidated || entry.net_return < Decimal::ZERO {
                losses += 1;
            } else {
                flats += 1;
            }
            let bucket = by_family
                .entry(entry.family.clone())
                .or_insert_with(FamilyBucket::default);
            bucket.count += 1;
            bucket.sum_return += entry.net_return;
            if entry.followed_through {
                bucket.wins += 1;
            }
        }
        let hit_rate = if total > 0 {
            Decimal::from(wins as i64) / Decimal::from(total as i64)
        } else {
            Decimal::ZERO
        };
        let mean_net_return = if total > 0 {
            sum / Decimal::from(total as i64)
        } else {
            Decimal::ZERO
        };
        // Require at least 5 observations to rank a family — single-name
        // outliers would otherwise dominate the summary.
        let mut family_stats: Vec<(String, usize, Decimal, Decimal)> = by_family
            .into_iter()
            .filter(|(_, bucket)| bucket.count >= 5)
            .map(|(name, bucket)| {
                let hit = Decimal::from(bucket.wins as i64) / Decimal::from(bucket.count as i64);
                let mean = bucket.sum_return / Decimal::from(bucket.count as i64);
                (name, bucket.count, hit, mean)
            })
            .collect();
        family_stats.sort_by(|a, b| b.3.cmp(&a.3));
        let best_family = family_stats.first().cloned();
        let worst_family = family_stats.last().cloned().filter(|worst| {
            best_family
                .as_ref()
                .map(|best| best.0 != worst.0)
                .unwrap_or(true)
        });
        Some(EdenLedgerSummary {
            total,
            wins,
            losses,
            flats,
            hit_rate,
            mean_net_return,
            best_family,
            worst_family,
        })
    }
}

#[derive(Debug, Default)]
struct FamilyBucket {
    count: usize,
    wins: usize,
    sum_return: Decimal,
}

impl EdenLedgerSummary {
    /// Render the summary as a single `ledger: ...` line for
    /// wake.reasons. Percentages are rounded to 1 dp to keep the line
    /// compact on an operator's screen.
    pub fn wake_line(&self) -> String {
        let hit_pct = (self.hit_rate * Decimal::from(100)).round_dp(1);
        let mean_bps = (self.mean_net_return * Decimal::from(10_000)).round_dp(0);
        let mut line = format!(
            "ledger: Eden track record {} resolved, {} win/{} loss/{} flat ({}% hit), mean {} bps",
            self.total, self.wins, self.losses, self.flats, hit_pct, mean_bps,
        );
        if let Some((name, n, hit, mean)) = &self.best_family {
            let hit_pct = (hit * Decimal::from(100)).round_dp(1);
            let mean_bps = (mean * Decimal::from(10_000)).round_dp(0);
            line.push_str(&format!(
                " | best={} n={} {}% {} bps",
                name, n, hit_pct, mean_bps
            ));
        }
        if let Some((name, n, hit, mean)) = &self.worst_family {
            let hit_pct = (hit * Decimal::from(100)).round_dp(1);
            let mean_bps = (mean * Decimal::from(10_000)).round_dp(0);
            line.push_str(&format!(
                " | worst={} n={} {}% {} bps",
                name, n, hit_pct, mean_bps
            ));
        }
        line
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    use super::*;

    #[allow(deprecated)]
    #[test]
    fn realized_outcome_record_preserves_key_fields() {
        let outcome = CaseRealizedOutcome {
            setup_id: "setup:1".into(),
            workflow_id: Some("wf:1".into()),
            symbol: Some("700.HK".into()),
            entry_tick: 10,
            entry_timestamp: OffsetDateTime::UNIX_EPOCH,
            resolved_tick: 25,
            resolved_at: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(30),
            family: "Flow".into(),
            session: "am".into(),
            market_regime: "risk_on".into(),
            direction: 1,
            return_pct: dec!(0.04),
            net_return: dec!(0.035),
            max_favorable_excursion: dec!(0.06),
            max_adverse_excursion: dec!(-0.02),
            followed_through: true,
            invalidated: false,
            structure_retained: true,
            convergence_score: dec!(0.5),
        };

        let record =
            CaseRealizedOutcomeRecord::from_outcome(&outcome, "hk", Some("iceberg".into()));
        assert_eq!(record.market, "hk");
        assert_eq!(record.setup_id, "setup:1");
        assert_eq!(record.primary_lens.as_deref(), Some("iceberg"));
        assert_eq!(record.net_return, dec!(0.035));
        assert!(record.followed_through);
    }

    fn mk_record(
        setup_id: &str,
        family: &str,
        net_return: Decimal,
        followed_through: bool,
    ) -> CaseRealizedOutcomeRecord {
        CaseRealizedOutcomeRecord {
            setup_id: setup_id.into(),
            workflow_id: None,
            market: "hk".into(),
            symbol: Some("700.HK".into()),
            primary_lens: None,
            family: family.into(),
            session: "am".into(),
            market_regime: "neutral".into(),
            entry_tick: 0,
            entry_timestamp: OffsetDateTime::UNIX_EPOCH,
            resolved_tick: 20,
            resolved_at: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(60),
            direction: 1,
            return_pct: net_return * Decimal::from(100),
            net_return,
            max_favorable_excursion: net_return.max(Decimal::ZERO),
            max_adverse_excursion: net_return.min(Decimal::ZERO),
            followed_through,
            invalidated: !followed_through && net_return < Decimal::ZERO,
            structure_retained: followed_through,
            convergence_score: dec!(0.5),
        }
    }

    #[test]
    fn ledger_accumulator_deduplicates_by_setup_id() {
        let mut ledger = EdenLedgerAccumulator::default();
        ledger.record(&mk_record("setup:1", "Flow", dec!(0.01), true));
        ledger.record(&mk_record("setup:1", "Flow", dec!(0.02), true));
        assert_eq!(ledger.len(), 1);
        let summary = ledger.summary().unwrap();
        // Latest write (0.02) wins — not 0.01.
        assert_eq!(summary.mean_net_return, dec!(0.02));
    }

    #[test]
    fn ledger_summary_computes_hit_rate_and_family_rankings() {
        let mut ledger = EdenLedgerAccumulator::default();
        // Flow: 5 wins / 1 loss — should be best-ranked.
        for i in 0..5 {
            ledger.record(&mk_record(
                &format!("setup:flow-win-{i}"),
                "Flow",
                dec!(0.015),
                true,
            ));
        }
        ledger.record(&mk_record("setup:flow-loss", "Flow", dec!(-0.01), false));
        // Stress: 1 win / 5 losses — should be worst-ranked.
        ledger.record(&mk_record("setup:stress-win", "Stress", dec!(0.01), true));
        for i in 0..5 {
            ledger.record(&mk_record(
                &format!("setup:stress-loss-{i}"),
                "Stress",
                dec!(-0.02),
                false,
            ));
        }
        let summary = ledger.summary().unwrap();
        assert_eq!(summary.total, 12);
        assert_eq!(summary.wins, 6);
        // 5 Flow losses invalidated? No — only 1 Flow loss + 5 Stress losses = 6 losses.
        assert_eq!(summary.losses, 6);
        let (best_name, best_n, _, _) = summary.best_family.as_ref().unwrap();
        assert_eq!(best_name, "Flow");
        assert_eq!(*best_n, 6);
        let (worst_name, worst_n, _, _) = summary.worst_family.as_ref().unwrap();
        assert_eq!(worst_name, "Stress");
        assert_eq!(*worst_n, 6);
    }

    #[test]
    fn ledger_wake_line_renders_compactly() {
        let mut ledger = EdenLedgerAccumulator::default();
        for i in 0..5 {
            ledger.record(&mk_record(
                &format!("setup:win-{i}"),
                "Flow",
                dec!(0.01),
                true,
            ));
        }
        for i in 0..5 {
            ledger.record(&mk_record(
                &format!("setup:loss-{i}"),
                "Flow",
                dec!(-0.01),
                false,
            ));
        }
        let summary = ledger.summary().unwrap();
        let line = summary.wake_line();
        assert!(line.starts_with("ledger: Eden track record 10 resolved"));
        assert!(line.contains("50.0% hit"));
        // 5×0.01 − 5×0.01 = 0, mean 0 bps.
        assert!(line.contains("mean 0 bps"));
    }
}
