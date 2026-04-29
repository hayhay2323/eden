//! Serialize/deserialize helpers for `BrokerArchetypeBeliefField`.
//!
//! HK only — brokers (stage 4 in the modulation stack) live on the
//! HK-specific broker queue data. Same shape as
//! `intent_belief_snapshot`: per-key `CategoricalBelief` with 5
//! fixed variants. Persisting closes the stage-4/stage-5 asymmetry
//! (both now carry warm priors across sessions) and is extra valuable
//! here because broker archetype is slower to learn than per-symbol
//! intent — a broker needs many ticks of bid/ask presence history
//! before its posterior dominates, so discarding that every night
//! meant stage 4 was effectively useless at session start.

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::ontology::objects::BrokerId;
use crate::pipeline::belief::CategoricalBelief;
use crate::pipeline::broker_archetype::{
    BrokerArchetype, BrokerArchetypeBeliefField, BROKER_ARCHETYPE_VARIANTS,
};

use super::belief_snapshot::{market_to_str, str_to_market, RestoreError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerArchetypeSnapshot {
    pub market: String,
    pub snapshot_ts: DateTime<Utc>,
    pub rows: Vec<BrokerArchetypeSnapshotRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerArchetypeSnapshotRow {
    pub broker_id: i32,
    /// Probabilities in canonical BROKER_ARCHETYPE_VARIANTS order. Length is 5.
    pub probs: Vec<f64>,
    pub sample_count: u32,
}

fn decimal_to_f64(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(0.0)
}

fn f64_to_decimal(v: f64) -> Result<Decimal, RestoreError> {
    Decimal::try_from(v).map_err(|_| RestoreError::BadFloat(v))
}

pub fn serialize_field(
    field: &BrokerArchetypeBeliefField,
    now: DateTime<Utc>,
) -> BrokerArchetypeSnapshot {
    let mut rows = Vec::new();
    for (broker_id, belief) in field.per_broker_iter() {
        if belief.sample_count == 0 {
            continue;
        }
        let probs: Vec<f64> = BROKER_ARCHETYPE_VARIANTS
            .iter()
            .map(|variant| {
                belief
                    .variants
                    .iter()
                    .position(|v| v == variant)
                    .and_then(|idx| belief.probs.get(idx))
                    .map(|d| decimal_to_f64(*d))
                    .unwrap_or(0.0)
            })
            .collect();
        rows.push(BrokerArchetypeSnapshotRow {
            broker_id: broker_id.0,
            probs,
            sample_count: belief.sample_count,
        });
    }
    BrokerArchetypeSnapshot {
        market: market_to_str(field.market()).to_string(),
        snapshot_ts: now,
        rows,
    }
}

pub fn restore_field(
    snap: &BrokerArchetypeSnapshot,
) -> Result<BrokerArchetypeBeliefField, RestoreError> {
    let market = str_to_market(&snap.market)
        .ok_or_else(|| RestoreError::UnknownMarket(snap.market.clone()))?;
    let mut field = BrokerArchetypeBeliefField::new(market);
    for row in &snap.rows {
        if row.probs.len() != BROKER_ARCHETYPE_VARIANTS.len() {
            return Err(RestoreError::BadCategoricalLen {
                got: row.probs.len(),
            });
        }
        let variants: Vec<BrokerArchetype> = BROKER_ARCHETYPE_VARIANTS.to_vec();
        let probs: Vec<Decimal> = row
            .probs
            .iter()
            .map(|p| f64_to_decimal(*p))
            .collect::<Result<_, _>>()?;
        let belief =
            CategoricalBelief::<BrokerArchetype>::from_raw(variants, probs, row.sample_count);
        field.insert_raw(BrokerId(row.broker_id), belief);
    }
    Ok(field)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    use crate::ontology::objects::{Market, Symbol};
    use crate::pipeline::raw_expectation::{BrokerPresenceEntry, RawBrokerPresence};

    fn seed_broker(
        field: &mut BrokerArchetypeBeliefField,
        broker_id: i32,
        accumulative_ticks: usize,
    ) {
        let mut presence = RawBrokerPresence::default();
        let entry = presence
            .per_symbol
            .entry(Symbol("0700.HK".into()))
            .or_default();
        let mut bid = BrokerPresenceEntry::default();
        for _ in 0..8 {
            bid.push(true);
        }
        let mut ask = BrokerPresenceEntry::default();
        for _ in 0..8 {
            ask.push(false);
        }
        entry.bid.insert(broker_id, bid);
        entry.ask.insert(broker_id, ask);
        for _ in 0..accumulative_ticks {
            field.observe_tick(&presence);
        }
    }

    #[test]
    fn empty_field_produces_empty_snapshot() {
        let field = BrokerArchetypeBeliefField::new(Market::Hk);
        let snap = serialize_field(&field, Utc.timestamp_opt(0, 0).unwrap());
        assert_eq!(snap.market, "hk");
        assert!(snap.rows.is_empty());
    }

    #[test]
    fn round_trip_preserves_posterior() {
        let mut field = BrokerArchetypeBeliefField::new(Market::Hk);
        seed_broker(&mut field, 2040, 30);

        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let snap = serialize_field(&field, now);
        assert_eq!(snap.market, "hk");
        assert_eq!(snap.rows.len(), 1);
        assert_eq!(snap.rows[0].broker_id, 2040);

        let restored = restore_field(&snap).expect("restore ok");
        let orig = field.query(BrokerId(2040)).unwrap();
        let again = restored.query(BrokerId(2040)).unwrap();
        assert_eq!(orig.sample_count, again.sample_count);
        for (p_orig, p_again) in orig.probs.iter().zip(again.probs.iter()) {
            let drift = (p_orig - p_again).abs().to_f64().unwrap_or(0.0);
            assert!(drift < 1e-6, "prob drift {}", drift);
        }
    }

    #[test]
    fn wrong_prob_length_errors() {
        let snap = BrokerArchetypeSnapshot {
            market: "hk".into(),
            snapshot_ts: Utc.timestamp_opt(0, 0).unwrap(),
            rows: vec![BrokerArchetypeSnapshotRow {
                broker_id: 1,
                probs: vec![0.5, 0.5, 0.0],
                sample_count: 1,
            }],
        };
        match restore_field(&snap) {
            Ok(_) => panic!("expected BadCategoricalLen"),
            Err(RestoreError::BadCategoricalLen { got: 3 }) => {}
            Err(other) => panic!("expected BadCategoricalLen, got {}", other),
        }
    }
}
