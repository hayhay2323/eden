//! Serialize/deserialize helpers for `IntentBeliefField` snapshots.
//!
//! Separate table from `belief_snapshot`: that one persists
//! `PressureBeliefField` (Gaussian per-channel + Categorical
//! PersistentStateKind per-symbol). This one persists world-space
//! `CategoricalBelief<IntentKind>` per-symbol. Both HK and US use it
//! so 3 stages of the modulation stack — intent_modulation,
//! sector_intent wake, sector_alignment_modulation — get nonzero
//! priors at session start.
//!
//! Invariants:
//!   - Only informed beliefs (sample_count >= 1) are written.
//!   - Probabilities round-trip in INTENT_VARIANTS canonical order.

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::ontology::objects::Symbol;
use crate::pipeline::belief::CategoricalBelief;
use crate::pipeline::intent_belief::{IntentBeliefField, IntentKind, INTENT_VARIANTS};

use super::belief_snapshot::{market_to_str, str_to_market, RestoreError};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentBeliefSnapshot {
    pub market: String,
    pub snapshot_ts: DateTime<Utc>,
    pub rows: Vec<IntentSnapshotRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntentSnapshotRow {
    pub symbol: String,
    /// Probabilities in canonical INTENT_VARIANTS order. Length is 5.
    pub probs: Vec<f64>,
    pub sample_count: u32,
}

fn decimal_to_f64(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(0.0)
}

fn f64_to_decimal(v: f64) -> Result<Decimal, RestoreError> {
    Decimal::try_from(v).map_err(|_| RestoreError::BadFloat(v))
}

pub fn serialize_field(field: &IntentBeliefField, now: DateTime<Utc>) -> IntentBeliefSnapshot {
    let mut rows = Vec::new();
    for (symbol, belief) in field.per_symbol_iter() {
        if belief.sample_count == 0 {
            continue;
        }
        let probs: Vec<f64> = INTENT_VARIANTS
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
        rows.push(IntentSnapshotRow {
            symbol: symbol.0.clone(),
            probs,
            sample_count: belief.sample_count,
        });
    }
    IntentBeliefSnapshot {
        market: market_to_str(field.market()).to_string(),
        snapshot_ts: now,
        rows,
    }
}

pub fn restore_field(snap: &IntentBeliefSnapshot) -> Result<IntentBeliefField, RestoreError> {
    let market = str_to_market(&snap.market)
        .ok_or_else(|| RestoreError::UnknownMarket(snap.market.clone()))?;
    let mut field = IntentBeliefField::new(market);
    for row in &snap.rows {
        if row.probs.len() != INTENT_VARIANTS.len() {
            return Err(RestoreError::BadCategoricalLen {
                got: row.probs.len(),
            });
        }
        let variants: Vec<IntentKind> = INTENT_VARIANTS.to_vec();
        let probs: Vec<Decimal> = row
            .probs
            .iter()
            .map(|p| f64_to_decimal(*p))
            .collect::<Result<_, _>>()?;
        let belief = CategoricalBelief::<IntentKind>::from_raw(variants, probs, row.sample_count);
        field.insert_raw(Symbol(row.symbol.clone()), belief);
    }
    Ok(field)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rust_decimal_macros::dec;

    use crate::ontology::objects::Market;
    use crate::pipeline::pressure::PressureChannel;

    #[test]
    fn empty_field_produces_empty_snapshot() {
        let field = IntentBeliefField::new(Market::Hk);
        let snap = serialize_field(&field, Utc.timestamp_opt(0, 0).unwrap());
        assert_eq!(snap.market, "hk");
        assert!(snap.rows.is_empty());
    }

    #[test]
    fn round_trip_preserves_posterior() {
        let mut field = IntentBeliefField::new(Market::Us);
        let sym = Symbol("NVDA.US".into());
        for _ in 0..20 {
            field.record_channel_samples(&sym, &[(PressureChannel::OrderBook, dec!(0.5))]);
        }
        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let snap = serialize_field(&field, now);
        assert_eq!(snap.market, "us");
        assert_eq!(snap.rows.len(), 1);
        assert_eq!(snap.rows[0].symbol, "NVDA.US");
        assert_eq!(snap.rows[0].sample_count, 20);

        let restored = restore_field(&snap).expect("restore ok");
        let orig = field.query(&sym).unwrap();
        let again = restored.query(&sym).unwrap();
        assert_eq!(orig.sample_count, again.sample_count);
        for (p_orig, p_again) in orig.probs.iter().zip(again.probs.iter()) {
            let drift = (p_orig - p_again).abs().to_f64().unwrap_or(0.0);
            assert!(drift < 1e-6, "prob drift {}", drift);
        }
    }

    #[test]
    fn wrong_prob_length_errors() {
        let snap = IntentBeliefSnapshot {
            market: "hk".into(),
            snapshot_ts: Utc.timestamp_opt(0, 0).unwrap(),
            rows: vec![IntentSnapshotRow {
                symbol: "X.HK".into(),
                probs: vec![0.5, 0.5],
                sample_count: 1,
            }],
        };
        let err = match restore_field(&snap) {
            Ok(_) => panic!("expected BadCategoricalLen"),
            Err(e) => e,
        };
        assert!(matches!(err, RestoreError::BadCategoricalLen { got: 2 }));
    }

    #[test]
    fn unknown_market_errors() {
        let snap = IntentBeliefSnapshot {
            market: "jp".into(),
            snapshot_ts: Utc.timestamp_opt(0, 0).unwrap(),
            rows: Vec::new(),
        };
        match restore_field(&snap) {
            Ok(_) => panic!("expected UnknownMarket"),
            Err(RestoreError::UnknownMarket(_)) => {}
            Err(other) => panic!("expected UnknownMarket, got {}", other),
        }
    }
}
