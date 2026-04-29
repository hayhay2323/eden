use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceMetadata {
    pub trace_id: String,
    pub inputs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum UsOrderDirection {
    #[default]
    Buy,
    Sell,
}

#[derive(Debug, Clone)]
pub struct UsOrderSuggestion {
    pub symbol: Symbol,
    pub direction: UsOrderDirection,
    pub convergence: UsConvergenceScore,
    pub suggested_quantity: i32,
    pub estimated_cost: Decimal,
    pub heuristic_edge: Decimal,
    pub requires_confirmation: bool,
    pub provenance: ProvenanceMetadata,
}

#[derive(Debug, Clone)]
pub struct UsSignalRecord {
    pub setup_id: String,
    pub symbol: Symbol,
    pub tick_emitted: u64,
    pub direction: UsOrderDirection,
    pub composite_at_emission: Decimal,
    pub price_at_emission: Option<Decimal>,
    pub resolved: bool,
    pub price_at_resolution: Option<Decimal>,
    pub hit: Option<bool>,
    pub realized_return: Option<Decimal>,
    /// True when Eden marked the suggestion as `requires_confirmation=false` at
    /// emission time (composite magnitude >= 0.25 AND macro regime not blocking).
    /// Only these records feed the `actionable_*` fields of the scorecard, so
    /// the headline hit rate reflects the *tradable* subset, not every non-zero
    /// composite symbol.
    pub is_actionable_tier: bool,
}

/// Scorecard is reported in **two tiers** to tell operators the difference
/// between "all non-zero composites" (pure noise floor) and "Eden marked this
/// as actionable at emission time" (the tradable subset):
/// - `total_*`, `hit_rate`, `mean_return`: every signal record, unfiltered.
///   This is useful as a baseline but is dominated by low-conviction signals
///   that Eden flagged `requires_confirmation=true` and would never be traded.
/// - `actionable_*`, `actionable_hit_rate`, `actionable_mean_return`: filtered
///   to records where Eden emitted `requires_confirmation=false`. This is the
///   number an operator should watch — it answers "if I traded everything Eden
///   calls actionable, what would my win rate be?"
#[derive(Debug, Clone, Default)]
pub struct UsSignalScorecard {
    pub total_signals: usize,
    pub resolved_signals: usize,
    pub hits: usize,
    pub misses: usize,
    pub hit_rate: Decimal,
    pub mean_return: Decimal,
    pub actionable_resolved: usize,
    pub actionable_hits: usize,
    pub actionable_hit_rate: Decimal,
    pub actionable_mean_return: Decimal,
    /// `actionable_hit_rate - hit_rate`. Regime-independent measure of
    /// selectivity edge: if actionable tier has real edge, this stays positive
    /// even when baseline hit_rate drifts with market conditions.
    pub actionable_excess_hit_rate: Decimal,
}

/// Cumulative scorecard that survives record pruning.
/// The buffer-based scorecard was always 0/0 because ~400 records/tick
/// overflowed the 4000-record cap, causing resolved records to be pruned
/// before `compute()` could count them.
/// Cumulative scorecard that survives record pruning.
///
/// Buffer-based scoring was 0/0 historically because ~400 records/tick
/// overflowed the 4000-record cap, causing resolved records to be pruned
/// before `compute()` could count them. The accumulator keeps running totals
/// for both the *unfiltered* signal population and the *actionable* subset
/// (records Eden marked `requires_confirmation=false` at emission time).
#[derive(Debug, Clone, Default)]
pub struct UsSignalScorecardAccumulator {
    pub total_resolved: usize,
    pub total_hits: usize,
    pub total_return: Decimal,
    pub actionable_resolved: usize,
    pub actionable_hits: usize,
    pub actionable_return: Decimal,
}

impl UsSignalScorecardAccumulator {
    pub fn record_resolution(
        &mut self,
        hit: bool,
        realized_return: Decimal,
        is_actionable_tier: bool,
    ) {
        self.total_resolved += 1;
        if hit {
            self.total_hits += 1;
        }
        self.total_return += realized_return;
        if is_actionable_tier {
            self.actionable_resolved += 1;
            if hit {
                self.actionable_hits += 1;
            }
            self.actionable_return += realized_return;
        }
    }

    pub fn to_scorecard(&self, active_signals: usize) -> UsSignalScorecard {
        let mut scorecard = UsSignalScorecard {
            total_signals: active_signals,
            ..Default::default()
        };
        if self.total_resolved > 0 {
            scorecard.resolved_signals = self.total_resolved;
            scorecard.hits = self.total_hits;
            scorecard.misses = self.total_resolved - self.total_hits;
            scorecard.hit_rate =
                Decimal::from(self.total_hits as i64) / Decimal::from(self.total_resolved as i64);
            scorecard.mean_return = self.total_return / Decimal::from(self.total_resolved as i64);
        }
        if self.actionable_resolved > 0 {
            scorecard.actionable_resolved = self.actionable_resolved;
            scorecard.actionable_hits = self.actionable_hits;
            scorecard.actionable_hit_rate = Decimal::from(self.actionable_hits as i64)
                / Decimal::from(self.actionable_resolved as i64);
            scorecard.actionable_mean_return =
                self.actionable_return / Decimal::from(self.actionable_resolved as i64);
            scorecard.actionable_excess_hit_rate =
                scorecard.actionable_hit_rate - scorecard.hit_rate;
        }
        scorecard
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn accumulator_scorecard_uses_active_unresolved_count() {
        let mut accumulator = UsSignalScorecardAccumulator::default();
        // Both non-actionable-tier so actionable_* stays zero.
        accumulator.record_resolution(true, dec!(0.02), false);
        accumulator.record_resolution(false, dec!(-0.01), false);

        let scorecard = accumulator.to_scorecard(37);

        assert_eq!(scorecard.total_signals, 37);
        assert_eq!(scorecard.resolved_signals, 2);
        assert_eq!(scorecard.hits, 1);
        assert_eq!(scorecard.misses, 1);
        assert_eq!(scorecard.hit_rate, dec!(0.5));
        assert_eq!(scorecard.mean_return, dec!(0.005));
        assert_eq!(scorecard.actionable_resolved, 0);
        assert_eq!(scorecard.actionable_hits, 0);
        assert_eq!(scorecard.actionable_hit_rate, dec!(0));
    }

    #[test]
    fn accumulator_tracks_actionable_subset_independently() {
        // Mix of actionable and non-actionable signals: total hit rate should
        // be the full-set ratio; actionable_hit_rate should only reflect
        // flagged-actionable records.
        let mut accumulator = UsSignalScorecardAccumulator::default();
        // 6 total: 2 actionable (1 hit), 4 non-actionable (1 hit).
        accumulator.record_resolution(true, dec!(0.03), true); // actionable hit
        accumulator.record_resolution(false, dec!(-0.01), true); // actionable miss
        accumulator.record_resolution(true, dec!(0.01), false); // noise hit
        accumulator.record_resolution(false, dec!(-0.02), false);
        accumulator.record_resolution(false, dec!(-0.005), false);
        accumulator.record_resolution(false, dec!(-0.003), false);

        let scorecard = accumulator.to_scorecard(100);

        assert_eq!(scorecard.resolved_signals, 6);
        assert_eq!(scorecard.hits, 2);
        // 2/6 = 0.33...
        assert!(scorecard.hit_rate > dec!(0.33) && scorecard.hit_rate < dec!(0.34));

        assert_eq!(scorecard.actionable_resolved, 2);
        assert_eq!(scorecard.actionable_hits, 1);
        assert_eq!(scorecard.actionable_hit_rate, dec!(0.5));
    }
}

impl UsSignalScorecard {
    pub fn compute(records: &[UsSignalRecord]) -> Self {
        let resolved: Vec<&UsSignalRecord> = records.iter().filter(|r| r.resolved).collect();
        let resolved_signals = resolved.len();
        let total_signals = records.len();

        if resolved_signals == 0 {
            return UsSignalScorecard {
                total_signals,
                ..Default::default()
            };
        }

        let hits = resolved.iter().filter(|r| r.hit == Some(true)).count();
        let misses = resolved_signals - hits;
        let hit_rate = Decimal::from(hits as i64) / Decimal::from(resolved_signals as i64);
        let mean_return = resolved
            .iter()
            .filter_map(|r| r.realized_return)
            .sum::<Decimal>()
            / Decimal::from(resolved_signals as i64);

        let actionable_records: Vec<&&UsSignalRecord> =
            resolved.iter().filter(|r| r.is_actionable_tier).collect();
        let actionable_resolved = actionable_records.len();
        let (actionable_hits, actionable_hit_rate, actionable_mean_return) = if actionable_resolved
            > 0
        {
            let hits = actionable_records
                .iter()
                .filter(|r| r.hit == Some(true))
                .count();
            let hit_rate = Decimal::from(hits as i64) / Decimal::from(actionable_resolved as i64);
            let mean_return = actionable_records
                .iter()
                .filter_map(|r| r.realized_return)
                .sum::<Decimal>()
                / Decimal::from(actionable_resolved as i64);
            (hits, hit_rate, mean_return)
        } else {
            (0, Decimal::ZERO, Decimal::ZERO)
        };

        UsSignalScorecard {
            total_signals,
            resolved_signals,
            hits,
            misses,
            hit_rate,
            mean_return,
            actionable_resolved,
            actionable_hits,
            actionable_hit_rate,
            actionable_mean_return,
            actionable_excess_hit_rate: actionable_hit_rate - hit_rate,
        }
    }

    /// Resolve a record and feed the result into the accumulator so stats
    /// survive buffer pruning.
    pub fn try_resolve_with_accumulator(
        record: &mut UsSignalRecord,
        current_tick: u64,
        current_price: Option<Decimal>,
        accumulator: &mut UsSignalScorecardAccumulator,
    ) {
        if record.resolved {
            return;
        }
        if current_tick < record.tick_emitted + SIGNAL_RESOLUTION_LAG {
            return;
        }

        record.resolved = true;
        record.price_at_resolution = current_price;

        if let (Some(entry), Some(exit)) = (record.price_at_emission, current_price) {
            if entry > Decimal::ZERO {
                let ret = (exit - entry) / entry;
                let directional_return = match record.direction {
                    UsOrderDirection::Buy => ret,
                    UsOrderDirection::Sell => -ret,
                };
                record.realized_return = Some(directional_return);
                let hit = directional_return > Decimal::ZERO;
                record.hit = Some(hit);
                accumulator.record_resolution(hit, directional_return, record.is_actionable_tier);
            }
        }
    }

    pub fn try_resolve(
        record: &mut UsSignalRecord,
        current_tick: u64,
        current_price: Option<Decimal>,
    ) {
        if record.resolved {
            return;
        }
        if current_tick < record.tick_emitted + SIGNAL_RESOLUTION_LAG {
            return;
        }

        record.resolved = true;
        record.price_at_resolution = current_price;

        if let (Some(entry), Some(exit)) = (record.price_at_emission, current_price) {
            if entry > Decimal::ZERO {
                let ret = (exit - entry) / entry;
                let directional_return = match record.direction {
                    UsOrderDirection::Buy => ret,
                    UsOrderDirection::Sell => -ret,
                };
                record.realized_return = Some(directional_return);
                record.hit = Some(directional_return > Decimal::ZERO);
            }
        }
    }
}
