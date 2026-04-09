use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceMetadata {
    pub trace_id: String,
    pub inputs: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsOrderDirection {
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
    pub symbol: Symbol,
    pub tick_emitted: u64,
    pub direction: UsOrderDirection,
    pub composite_at_emission: Decimal,
    pub price_at_emission: Option<Decimal>,
    pub resolved: bool,
    pub price_at_resolution: Option<Decimal>,
    pub hit: Option<bool>,
    pub realized_return: Option<Decimal>,
}

#[derive(Debug, Clone, Default)]
pub struct UsSignalScorecard {
    pub total_signals: usize,
    pub resolved_signals: usize,
    pub hits: usize,
    pub misses: usize,
    pub hit_rate: Decimal,
    pub mean_return: Decimal,
}

/// Cumulative scorecard that survives record pruning.
/// The buffer-based scorecard was always 0/0 because ~400 records/tick
/// overflowed the 4000-record cap, causing resolved records to be pruned
/// before `compute()` could count them.
#[derive(Debug, Clone, Default)]
pub struct UsSignalScorecardAccumulator {
    pub total_resolved: usize,
    pub total_hits: usize,
    pub total_return: Decimal,
}

impl UsSignalScorecardAccumulator {
    pub fn record_resolution(&mut self, hit: bool, realized_return: Decimal) {
        self.total_resolved += 1;
        if hit {
            self.total_hits += 1;
        }
        self.total_return += realized_return;
    }

    pub fn to_scorecard(&self, active_signals: usize) -> UsSignalScorecard {
        if self.total_resolved == 0 {
            return UsSignalScorecard {
                total_signals: active_signals,
                ..Default::default()
            };
        }
        let hits = self.total_hits;
        let misses = self.total_resolved - hits;
        let hit_rate = Decimal::from(hits as i64) / Decimal::from(self.total_resolved as i64);
        let mean_return = self.total_return / Decimal::from(self.total_resolved as i64);
        UsSignalScorecard {
            total_signals: active_signals,
            resolved_signals: self.total_resolved,
            hits,
            misses,
            hit_rate,
            mean_return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn accumulator_scorecard_uses_active_unresolved_count() {
        let mut accumulator = UsSignalScorecardAccumulator::default();
        accumulator.record_resolution(true, dec!(0.02));
        accumulator.record_resolution(false, dec!(-0.01));

        let scorecard = accumulator.to_scorecard(37);

        assert_eq!(scorecard.total_signals, 37);
        assert_eq!(scorecard.resolved_signals, 2);
        assert_eq!(scorecard.hits, 1);
        assert_eq!(scorecard.misses, 1);
        assert_eq!(scorecard.hit_rate, dec!(0.5));
        assert_eq!(scorecard.mean_return, dec!(0.005));
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

        UsSignalScorecard {
            total_signals,
            resolved_signals,
            hits,
            misses,
            hit_rate,
            mean_return,
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
                accumulator.record_resolution(hit, directional_return);
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
