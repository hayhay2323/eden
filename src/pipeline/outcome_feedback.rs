//! Backward pass: realized outcomes credit/debit the KG-level beliefs.
//!
//! # Idempotency
//!
//! The lineage stream (`compute_case_realized_outcomes_adaptive`) re-emits
//! the same resolved outcome every tick while it stays inside the
//! lookback window — same setup_id, same record, N ticks in a row.
//! `EdenLedgerAccumulator` absorbs that by upserting into a HashMap.
//! This module also has to dedup or it credits the same outcome N
//! times. We do it with a caller-owned `HashSet<String>` of already-
//! credited setup_ids, passed through the apply helpers.
//!
//! Every forward stage (broker_alignment, sector_alignment, intent)
//! reads an ontology-level belief and uses it to modulate a setup's
//! confidence. Until this module shipped, those modulators were
//! open-loop heuristics — a setup could win repeatedly with a
//! specific sector alignment but the sector's intent posterior never
//! learned anything from that success. The 5-stage stack was a
//! one-way funnel.
//!
//! This module closes the loop in the narrowest, safest way:
//! for each `CaseRealizedOutcomeRecord` that has a focal symbol and
//! a nonzero direction, emit one `IntentKind` observation to the
//! live `IntentBeliefField`. Direction of the observation:
//!
//!   - Winning long  → Accumulation
//!   - Winning short → Distribution
//!   - Losing long   → Distribution (premise was wrong, net flow was
//!                      against the long; one sample of the other side)
//!   - Losing short  → Accumulation
//!
//! This is deliberately a *single* sample — the horizon ticks'
//! pressure observations have already written many samples during
//! the holding period, so we're not double-counting the market's
//! reaction; we're adding a small grounded confirmation/refutation
//! that wouldn't otherwise flow to the belief field.
//!
//! Broker-archetype credit needs the entry-time broker presence
//! snapshot, which isn't currently persisted with the outcome. That
//! piece waits until we persist broker presence at entry time.

use std::collections::HashSet;

use rust_decimal::prelude::ToPrimitive;

use crate::ontology::objects::Symbol;
use crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord;
use crate::pipeline::intent_belief::{IntentBeliefField, IntentKind};

/// Minimum absolute return_pct before we treat the outcome as a
/// real signal rather than noise. Setups that come out roughly
/// flat shouldn't move the belief field in either direction.
const MIN_ABS_RETURN_PCT: f64 = 0.002;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutcomeIntentFeedback {
    Applied { intent: IntentKind },
    SkippedFlat,
    SkippedNoSymbol,
    SkippedNoDirection,
    SkippedAlreadyCredited,
}

/// Apply one outcome; skip if this setup_id has already been credited.
/// `credited_setup_ids` is caller-owned state so runtimes retain dedup
/// across the full session.
pub fn apply_outcome_to_intent(
    record: &CaseRealizedOutcomeRecord,
    intent_field: &mut IntentBeliefField,
    credited_setup_ids: &mut HashSet<String>,
) -> OutcomeIntentFeedback {
    if credited_setup_ids.contains(&record.setup_id) {
        return OutcomeIntentFeedback::SkippedAlreadyCredited;
    }
    let Some(symbol_str) = &record.symbol else {
        return OutcomeIntentFeedback::SkippedNoSymbol;
    };
    if record.direction == 0 {
        return OutcomeIntentFeedback::SkippedNoDirection;
    }
    let return_f = record.return_pct.to_f64().unwrap_or(0.0);
    if return_f.abs() < MIN_ABS_RETURN_PCT {
        return OutcomeIntentFeedback::SkippedFlat;
    }
    let won = (return_f > 0.0) == (record.direction > 0);
    let intent = match (record.direction > 0, won) {
        (true, true) => IntentKind::Accumulation,
        (true, false) => IntentKind::Distribution,
        (false, true) => IntentKind::Distribution,
        (false, false) => IntentKind::Accumulation,
    };
    intent_field.observe_outcome_intent(&Symbol(symbol_str.clone()), intent);
    credited_setup_ids.insert(record.setup_id.clone());
    OutcomeIntentFeedback::Applied { intent }
}

pub fn apply_outcome_batch(
    records: &[CaseRealizedOutcomeRecord],
    intent_field: &mut IntentBeliefField,
    credited_setup_ids: &mut HashSet<String>,
) -> BatchFeedbackSummary {
    let mut summary = BatchFeedbackSummary::default();
    for record in records {
        match apply_outcome_to_intent(record, intent_field, credited_setup_ids) {
            OutcomeIntentFeedback::Applied {
                intent: IntentKind::Accumulation,
            } => summary.accumulation += 1,
            OutcomeIntentFeedback::Applied {
                intent: IntentKind::Distribution,
            } => summary.distribution += 1,
            OutcomeIntentFeedback::Applied { .. } => summary.other += 1,
            OutcomeIntentFeedback::SkippedFlat => summary.skipped_flat += 1,
            OutcomeIntentFeedback::SkippedNoSymbol => summary.skipped_no_symbol += 1,
            OutcomeIntentFeedback::SkippedNoDirection => summary.skipped_no_direction += 1,
            OutcomeIntentFeedback::SkippedAlreadyCredited => summary.skipped_already_credited += 1,
        }
    }
    summary
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BatchFeedbackSummary {
    pub accumulation: usize,
    pub distribution: usize,
    pub other: usize,
    pub skipped_flat: usize,
    pub skipped_no_symbol: usize,
    pub skipped_no_direction: usize,
    pub skipped_already_credited: usize,
}

impl BatchFeedbackSummary {
    pub fn applied(&self) -> usize {
        self.accumulation + self.distribution + self.other
    }

    pub fn summary_line(&self, market: &str) -> String {
        format!(
            "outcome_feedback: {} market={} accumulation={} distribution={} skipped(flat={},no_sym={},no_dir={},already={})",
            self.applied(),
            market,
            self.accumulation,
            self.distribution,
            self.skipped_flat,
            self.skipped_no_symbol,
            self.skipped_no_direction,
            self.skipped_already_credited,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    use crate::ontology::objects::Market;

    fn mk_outcome(
        direction: i8,
        return_pct: Decimal,
        symbol: Option<&str>,
    ) -> CaseRealizedOutcomeRecord {
        CaseRealizedOutcomeRecord {
            setup_id: "s".to_string(),
            workflow_id: None,
            market: "hk".to_string(),
            symbol: symbol.map(str::to_string),
            primary_lens: None,
            family: "f".to_string(),
            session: "sess".to_string(),
            market_regime: "normal".to_string(),
            entry_tick: 0,
            entry_timestamp: OffsetDateTime::UNIX_EPOCH,
            resolved_tick: 0,
            resolved_at: OffsetDateTime::UNIX_EPOCH,
            direction,
            return_pct,
            net_return: Decimal::ZERO,
            max_favorable_excursion: Decimal::ZERO,
            max_adverse_excursion: Decimal::ZERO,
            followed_through: false,
            invalidated: false,
            structure_retained: false,
            convergence_score: Decimal::ZERO,
        }
    }

    fn fresh_seen() -> HashSet<String> {
        HashSet::new()
    }

    fn mk_outcome_id(
        setup_id: &str,
        direction: i8,
        return_pct: Decimal,
        symbol: Option<&str>,
    ) -> CaseRealizedOutcomeRecord {
        let mut r = mk_outcome(direction, return_pct, symbol);
        r.setup_id = setup_id.to_string();
        r
    }

    #[test]
    fn winning_long_credits_accumulation() {
        let mut field = IntentBeliefField::new(Market::Hk);
        let mut seen = fresh_seen();
        let rec = mk_outcome_id("s1", 1, dec!(0.01), Some("NVDA.HK"));
        let res = apply_outcome_to_intent(&rec, &mut field, &mut seen);
        assert_eq!(
            res,
            OutcomeIntentFeedback::Applied {
                intent: IntentKind::Accumulation
            }
        );
        let belief = field.query(&Symbol("NVDA.HK".into())).unwrap();
        assert_eq!(belief.sample_count, 1);
    }

    #[test]
    fn winning_short_credits_distribution() {
        let mut field = IntentBeliefField::new(Market::Hk);
        let mut seen = fresh_seen();
        let rec = mk_outcome_id("s2", -1, dec!(-0.012), Some("X.HK"));
        let res = apply_outcome_to_intent(&rec, &mut field, &mut seen);
        assert_eq!(
            res,
            OutcomeIntentFeedback::Applied {
                intent: IntentKind::Distribution
            }
        );
    }

    #[test]
    fn losing_long_credits_distribution() {
        let mut field = IntentBeliefField::new(Market::Hk);
        let mut seen = fresh_seen();
        let rec = mk_outcome_id("s3", 1, dec!(-0.008), Some("X.HK"));
        let res = apply_outcome_to_intent(&rec, &mut field, &mut seen);
        assert_eq!(
            res,
            OutcomeIntentFeedback::Applied {
                intent: IntentKind::Distribution
            }
        );
    }

    #[test]
    fn flat_outcome_is_skipped() {
        let mut field = IntentBeliefField::new(Market::Hk);
        let mut seen = fresh_seen();
        let rec = mk_outcome_id("s4", 1, dec!(0.001), Some("X.HK"));
        let res = apply_outcome_to_intent(&rec, &mut field, &mut seen);
        assert_eq!(res, OutcomeIntentFeedback::SkippedFlat);
    }

    #[test]
    fn no_symbol_is_skipped() {
        let mut field = IntentBeliefField::new(Market::Hk);
        let mut seen = fresh_seen();
        let rec = mk_outcome_id("s5", 1, dec!(0.05), None);
        let res = apply_outcome_to_intent(&rec, &mut field, &mut seen);
        assert_eq!(res, OutcomeIntentFeedback::SkippedNoSymbol);
    }

    #[test]
    fn repeated_same_record_is_idempotent() {
        // The lineage compute stream re-emits the same outcome every
        // tick while it's inside the lookback window. Without dedup
        // the focal gets ~LINEAGE_WINDOW spurious samples and stages
        // 3/5 over-modulate next tick.
        let mut field = IntentBeliefField::new(Market::Hk);
        let mut seen = fresh_seen();
        let rec = mk_outcome_id("same_id", 1, dec!(0.02), Some("NVDA.HK"));
        let r1 = apply_outcome_to_intent(&rec, &mut field, &mut seen);
        let r2 = apply_outcome_to_intent(&rec, &mut field, &mut seen);
        let r3 = apply_outcome_to_intent(&rec, &mut field, &mut seen);
        assert!(matches!(r1, OutcomeIntentFeedback::Applied { .. }));
        assert_eq!(r2, OutcomeIntentFeedback::SkippedAlreadyCredited);
        assert_eq!(r3, OutcomeIntentFeedback::SkippedAlreadyCredited);
        let belief = field.query(&Symbol("NVDA.HK".into())).unwrap();
        assert_eq!(belief.sample_count, 1);
    }

    #[test]
    fn two_distinct_ids_credit_separately() {
        let mut field = IntentBeliefField::new(Market::Hk);
        let mut seen = fresh_seen();
        let a = mk_outcome_id("id_a", 1, dec!(0.02), Some("X.HK"));
        let b = mk_outcome_id("id_b", 1, dec!(0.02), Some("X.HK"));
        apply_outcome_to_intent(&a, &mut field, &mut seen);
        apply_outcome_to_intent(&b, &mut field, &mut seen);
        let belief = field.query(&Symbol("X.HK".into())).unwrap();
        assert_eq!(belief.sample_count, 2);
    }

    #[test]
    fn batch_summary_reflects_counts() {
        let mut field = IntentBeliefField::new(Market::Hk);
        let mut seen = fresh_seen();
        let records = vec![
            mk_outcome_id("A", 1, dec!(0.02), Some("A.HK")),
            mk_outcome_id("B", 1, dec!(-0.015), Some("B.HK")),
            mk_outcome_id("C", -1, dec!(-0.02), Some("C.HK")),
            mk_outcome_id("D", 0, dec!(0.01), Some("D.HK")),
            mk_outcome_id("E", 1, dec!(0.0005), Some("E.HK")),
        ];
        let summary = apply_outcome_batch(&records, &mut field, &mut seen);
        assert_eq!(summary.accumulation, 1);
        assert_eq!(summary.distribution, 2);
        assert_eq!(summary.skipped_no_direction, 1);
        assert_eq!(summary.skipped_flat, 1);
    }

    #[test]
    fn batch_across_two_calls_dedups_setup_ids() {
        let mut field = IntentBeliefField::new(Market::Hk);
        let mut seen = fresh_seen();
        let records = vec![
            mk_outcome_id("A", 1, dec!(0.02), Some("A.HK")),
            mk_outcome_id("B", -1, dec!(-0.02), Some("B.HK")),
        ];
        let first = apply_outcome_batch(&records, &mut field, &mut seen);
        assert_eq!(first.applied(), 2);
        // Same records fed again — lineage window repeat. Must all skip.
        let second = apply_outcome_batch(&records, &mut field, &mut seen);
        assert_eq!(second.applied(), 0);
        assert_eq!(second.skipped_already_credited, 2);
    }
}
