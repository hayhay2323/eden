//! Backward pass for `BrokerArchetypeBeliefField` — HK only.
//!
//! When a symbol-scoped setup is built, we snapshot which brokers
//! were on the bid vs ask for that symbol at entry time. When the
//! corresponding outcome resolves, we credit the winning-side
//! brokers with one archetype sample:
//!
//!   - Winning LONG  → bid brokers get +1 Accumulative
//!     (they were positioned on the right side of an up-move)
//!   - Winning SHORT → ask brokers get +1 Distributive
//!     (they were supplying at a level that turned out to be right)
//!
//! Losing outcomes are NOT back-propagated: a lost trade can fail
//! for reasons unrelated to broker positioning (wrong setup,
//! surprise news), so we don't want to penalize a specific broker
//! archetype on noise. Only confirmed-correct positioning credits.
//!
//! The snapshot is an in-memory `HashMap<setup_id, BrokerEntrySnapshot>`
//! maintained by the HK runtime. Lost on restart — fine because
//! horizons are typically minutes-hours within a session, and
//! orphaned outcomes simply don't credit (same as current state).
//!
//! Idempotent via the same `credited_setup_ids` HashSet used by
//! intent outcome_feedback.

use std::collections::{HashMap, HashSet};

use crate::ontology::objects::{BrokerId, Symbol};
use crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord;
use crate::pipeline::broker_archetype::{BrokerArchetype, BrokerArchetypeBeliefField};
use crate::pipeline::raw_expectation::RawBrokerPresence;

use rust_decimal::prelude::ToPrimitive;

const MIN_ABS_RETURN_PCT: f64 = 0.002;

#[derive(Debug, Clone, Default)]
pub struct BrokerEntrySnapshot {
    pub bid_brokers: Vec<i32>,
    pub ask_brokers: Vec<i32>,
}

/// Capture which brokers are on bid and ask for the setup's focal
/// symbol at setup-construction time. Idempotent-per-setup_id: if
/// the setup is rebuilt on a later tick with a different broker
/// set, we keep the ORIGINAL entry snapshot.
pub fn snapshot_setup_brokers(
    setup_id: &str,
    symbol: &Symbol,
    presence: &RawBrokerPresence,
    snapshots: &mut HashMap<String, BrokerEntrySnapshot>,
) {
    if snapshots.contains_key(setup_id) {
        return;
    }
    let Some(per_sym) = presence.for_symbol(symbol) else {
        return;
    };
    let snap = BrokerEntrySnapshot {
        bid_brokers: per_sym.bid.keys().copied().collect(),
        ask_brokers: per_sym.ask.keys().copied().collect(),
    };
    if snap.bid_brokers.is_empty() && snap.ask_brokers.is_empty() {
        return;
    }
    snapshots.insert(setup_id.to_string(), snap);
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BrokerFeedbackSummary {
    pub winning_longs_credited: usize,
    pub winning_shorts_credited: usize,
    pub broker_samples_pushed: usize,
    pub skipped_no_snapshot: usize,
    pub skipped_loss: usize,
    pub skipped_flat: usize,
    pub skipped_already_credited: usize,
}

impl BrokerFeedbackSummary {
    pub fn applied(&self) -> usize {
        self.winning_longs_credited + self.winning_shorts_credited
    }

    pub fn summary_line(&self, market: &str) -> String {
        format!(
            "broker_outcome_feedback: {} market={} longs={} shorts={} broker_samples={} skipped(no_snap={},loss={},flat={},already={})",
            self.applied(),
            market,
            self.winning_longs_credited,
            self.winning_shorts_credited,
            self.broker_samples_pushed,
            self.skipped_no_snapshot,
            self.skipped_loss,
            self.skipped_flat,
            self.skipped_already_credited,
        )
    }
}

pub fn apply_broker_outcome_batch(
    records: &[CaseRealizedOutcomeRecord],
    snapshots: &HashMap<String, BrokerEntrySnapshot>,
    broker_field: &mut BrokerArchetypeBeliefField,
    credited_setup_ids: &mut HashSet<String>,
) -> BrokerFeedbackSummary {
    let mut summary = BrokerFeedbackSummary::default();
    for record in records {
        if credited_setup_ids.contains(&record.setup_id) {
            summary.skipped_already_credited += 1;
            continue;
        }
        if record.direction == 0 {
            continue;
        }
        let return_f = record.return_pct.to_f64().unwrap_or(0.0);
        if return_f.abs() < MIN_ABS_RETURN_PCT {
            summary.skipped_flat += 1;
            continue;
        }
        let won = (return_f > 0.0) == (record.direction > 0);
        if !won {
            summary.skipped_loss += 1;
            continue;
        }
        let Some(snap) = snapshots.get(&record.setup_id) else {
            summary.skipped_no_snapshot += 1;
            continue;
        };
        let (side_brokers, archetype) = if record.direction > 0 {
            (&snap.bid_brokers, BrokerArchetype::Accumulative)
        } else {
            (&snap.ask_brokers, BrokerArchetype::Distributive)
        };
        if side_brokers.is_empty() {
            summary.skipped_no_snapshot += 1;
            continue;
        }
        for broker_id in side_brokers {
            broker_field.observe_outcome_archetype(BrokerId(*broker_id), archetype);
            summary.broker_samples_pushed += 1;
        }
        if record.direction > 0 {
            summary.winning_longs_credited += 1;
        } else {
            summary.winning_shorts_credited += 1;
        }
        credited_setup_ids.insert(record.setup_id.clone());
    }
    summary
}

/// GC helper. Call once per N ticks to evict snapshots that have
/// aged beyond what any outcome window can still credit. Simple
/// contract: remove setup_ids already in `credited_setup_ids`.
pub fn gc_credited_snapshots(
    snapshots: &mut HashMap<String, BrokerEntrySnapshot>,
    credited_setup_ids: &HashSet<String>,
) -> usize {
    let before = snapshots.len();
    snapshots.retain(|k, _| !credited_setup_ids.contains(k));
    before.saturating_sub(snapshots.len())
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    use crate::ontology::objects::Market;
    use crate::pipeline::raw_expectation::{BrokerPresenceEntry, PerSymbolBrokerPresence};

    fn mk_outcome(
        setup_id: &str,
        direction: i8,
        return_pct: Decimal,
        symbol: Option<&str>,
    ) -> CaseRealizedOutcomeRecord {
        CaseRealizedOutcomeRecord {
            setup_id: setup_id.to_string(),
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

    fn presence_with(bid: &[i32], ask: &[i32]) -> RawBrokerPresence {
        let mut p = RawBrokerPresence::default();
        let entry = p
            .per_symbol
            .entry(Symbol("0700.HK".into()))
            .or_insert_with(PerSymbolBrokerPresence::default);
        for id in bid {
            let mut e = BrokerPresenceEntry::default();
            for _ in 0..5 {
                e.push(true);
            }
            entry.bid.insert(*id, e);
        }
        for id in ask {
            let mut e = BrokerPresenceEntry::default();
            for _ in 0..5 {
                e.push(true);
            }
            entry.ask.insert(*id, e);
        }
        p
    }

    #[test]
    fn snapshot_captures_both_sides() {
        let presence = presence_with(&[100, 101], &[200]);
        let mut snapshots = HashMap::new();
        snapshot_setup_brokers("s1", &Symbol("0700.HK".into()), &presence, &mut snapshots);
        let snap = snapshots.get("s1").unwrap();
        let mut bid_sorted = snap.bid_brokers.clone();
        bid_sorted.sort();
        assert_eq!(bid_sorted, vec![100, 101]);
        assert_eq!(snap.ask_brokers, vec![200]);
    }

    #[test]
    fn snapshot_is_entry_time_only() {
        let mut snapshots = HashMap::new();
        let initial = presence_with(&[100], &[]);
        snapshot_setup_brokers("s1", &Symbol("0700.HK".into()), &initial, &mut snapshots);
        // Second call with different presence must NOT overwrite.
        let later = presence_with(&[999], &[888]);
        snapshot_setup_brokers("s1", &Symbol("0700.HK".into()), &later, &mut snapshots);
        let snap = snapshots.get("s1").unwrap();
        assert_eq!(snap.bid_brokers, vec![100]);
    }

    #[test]
    fn winning_long_credits_bid_brokers_accumulative() {
        let mut broker_field = BrokerArchetypeBeliefField::new(Market::Hk);
        let presence = presence_with(&[111, 222], &[333]);
        let mut snapshots = HashMap::new();
        snapshot_setup_brokers(
            "win_long",
            &Symbol("0700.HK".into()),
            &presence,
            &mut snapshots,
        );

        let mut seen = HashSet::new();
        let records = vec![mk_outcome("win_long", 1, dec!(0.02), Some("0700.HK"))];
        let summary =
            apply_broker_outcome_batch(&records, &snapshots, &mut broker_field, &mut seen);
        assert_eq!(summary.winning_longs_credited, 1);
        assert_eq!(summary.broker_samples_pushed, 2);
        let b111 = broker_field.query(BrokerId(111)).unwrap();
        assert_eq!(b111.sample_count, 1);
        let b333 = broker_field.query(BrokerId(333));
        assert!(
            b333.is_none(),
            "ask broker should NOT be credited on long win"
        );
    }

    #[test]
    fn winning_short_credits_ask_brokers_distributive() {
        let mut broker_field = BrokerArchetypeBeliefField::new(Market::Hk);
        let presence = presence_with(&[111], &[333, 444]);
        let mut snapshots = HashMap::new();
        snapshot_setup_brokers(
            "win_short",
            &Symbol("0700.HK".into()),
            &presence,
            &mut snapshots,
        );
        let mut seen = HashSet::new();
        let records = vec![mk_outcome("win_short", -1, dec!(-0.02), Some("0700.HK"))];
        let summary =
            apply_broker_outcome_batch(&records, &snapshots, &mut broker_field, &mut seen);
        assert_eq!(summary.winning_shorts_credited, 1);
        assert_eq!(summary.broker_samples_pushed, 2);
        let b111 = broker_field.query(BrokerId(111));
        assert!(
            b111.is_none(),
            "bid broker should NOT be credited on short win"
        );
        let b333 = broker_field.query(BrokerId(333)).unwrap();
        assert_eq!(b333.sample_count, 1);
    }

    #[test]
    fn losing_trade_is_not_credited() {
        let mut broker_field = BrokerArchetypeBeliefField::new(Market::Hk);
        let presence = presence_with(&[111], &[222]);
        let mut snapshots = HashMap::new();
        snapshot_setup_brokers(
            "lose_long",
            &Symbol("0700.HK".into()),
            &presence,
            &mut snapshots,
        );
        let mut seen = HashSet::new();
        let records = vec![mk_outcome("lose_long", 1, dec!(-0.02), Some("0700.HK"))];
        let summary =
            apply_broker_outcome_batch(&records, &snapshots, &mut broker_field, &mut seen);
        assert_eq!(summary.skipped_loss, 1);
        assert_eq!(summary.broker_samples_pushed, 0);
    }

    #[test]
    fn idempotent_on_repeat() {
        let mut broker_field = BrokerArchetypeBeliefField::new(Market::Hk);
        let presence = presence_with(&[111, 222], &[]);
        let mut snapshots = HashMap::new();
        snapshot_setup_brokers(
            "repeat",
            &Symbol("0700.HK".into()),
            &presence,
            &mut snapshots,
        );
        let mut seen = HashSet::new();
        let records = vec![mk_outcome("repeat", 1, dec!(0.02), Some("0700.HK"))];

        apply_broker_outcome_batch(&records, &snapshots, &mut broker_field, &mut seen);
        apply_broker_outcome_batch(&records, &snapshots, &mut broker_field, &mut seen);
        apply_broker_outcome_batch(&records, &snapshots, &mut broker_field, &mut seen);

        let b = broker_field.query(BrokerId(111)).unwrap();
        assert_eq!(
            b.sample_count, 1,
            "idempotency: sample_count must stay at 1"
        );
    }

    #[test]
    fn missing_snapshot_is_skipped() {
        let mut broker_field = BrokerArchetypeBeliefField::new(Market::Hk);
        let snapshots = HashMap::new();
        let mut seen = HashSet::new();
        let records = vec![mk_outcome("orphan", 1, dec!(0.02), Some("0700.HK"))];
        let summary =
            apply_broker_outcome_batch(&records, &snapshots, &mut broker_field, &mut seen);
        assert_eq!(summary.skipped_no_snapshot, 1);
        assert_eq!(summary.broker_samples_pushed, 0);
    }

    #[test]
    fn gc_removes_credited_entries() {
        let mut snapshots = HashMap::new();
        snapshots.insert(
            "done".to_string(),
            BrokerEntrySnapshot {
                bid_brokers: vec![1],
                ask_brokers: vec![],
            },
        );
        snapshots.insert(
            "still_open".to_string(),
            BrokerEntrySnapshot {
                bid_brokers: vec![2],
                ask_brokers: vec![],
            },
        );
        let mut credited = HashSet::new();
        credited.insert("done".to_string());
        let dropped = gc_credited_snapshots(&mut snapshots, &credited);
        assert_eq!(dropped, 1);
        assert!(snapshots.contains_key("still_open"));
        assert!(!snapshots.contains_key("done"));
    }
}
