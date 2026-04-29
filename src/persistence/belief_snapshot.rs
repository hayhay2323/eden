//! Serialize/deserialize helpers for `PressureBeliefField` snapshots.
//!
//! The types here are always compiled; the SurrealDB save/load methods
//! that reference them live in `persistence::store::belief` under the
//! `persistence` feature.
//!
//! Invariants:
//!   - Only informed beliefs (sample_count >= 1) are written
//!   - Market / channel / state enums round-trip via explicit string tags
//!   - CategoricalBelief distributions round-trip as the 5 variants in
//!     canonical order (PERSISTENT_STATE_VARIANTS)
//!
//! See docs/superpowers/specs/2026-04-19-belief-persistence-design.md.

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::ontology::objects::{Market, Symbol};
use crate::pipeline::belief::{CategoricalBelief, GaussianBelief};
use crate::pipeline::belief_field::{PressureBeliefField, PERSISTENT_STATE_VARIANTS};
use crate::pipeline::pressure::PressureChannel;
use crate::pipeline::state_engine::PersistentStateKind;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BeliefSnapshot {
    pub market: String,
    pub snapshot_ts: DateTime<Utc>,
    pub tick: u64,
    pub gaussian: Vec<GaussianSnapshotRow>,
    pub categorical: Vec<CategoricalSnapshotRow>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GaussianSnapshotRow {
    pub symbol: String,
    pub channel: String,
    pub mean: f64,
    pub variance: f64,
    pub m2: f64,
    pub sample_count: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoricalSnapshotRow {
    pub symbol: String,
    /// Probabilities in the canonical state-variant order
    /// (PERSISTENT_STATE_VARIANTS). Length is always 5.
    pub probs: Vec<f64>,
    pub sample_count: u32,
}

#[derive(Debug)]
pub enum RestoreError {
    UnknownMarket(String),
    UnknownChannel(String),
    BadCategoricalLen { got: usize },
    BadFloat(f64),
}

impl std::fmt::Display for RestoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RestoreError::UnknownMarket(s) => write!(f, "unknown market: {}", s),
            RestoreError::UnknownChannel(s) => write!(f, "unknown channel: {}", s),
            RestoreError::BadCategoricalLen { got } => {
                write!(f, "categorical probs length {}, expected 5", got)
            }
            RestoreError::BadFloat(v) => write!(f, "f64 could not be converted to Decimal: {}", v),
        }
    }
}

impl std::error::Error for RestoreError {}

// ---------------------------------------------------------------------------
// String encoders / decoders
// ---------------------------------------------------------------------------

pub fn market_to_str(m: Market) -> &'static str {
    match m {
        Market::Hk => "hk",
        Market::Us => "us",
    }
}

pub fn str_to_market(s: &str) -> Option<Market> {
    match s {
        "hk" => Some(Market::Hk),
        "us" => Some(Market::Us),
        _ => None,
    }
}

pub fn channel_to_str(c: PressureChannel) -> &'static str {
    match c {
        PressureChannel::OrderBook => "order_book",
        PressureChannel::CapitalFlow => "capital_flow",
        PressureChannel::Institutional => "institutional",
        PressureChannel::Momentum => "momentum",
        PressureChannel::Volume => "volume",
        PressureChannel::Structure => "structure",
    }
}

pub fn str_to_channel(s: &str) -> Option<PressureChannel> {
    match s {
        "order_book" => Some(PressureChannel::OrderBook),
        "capital_flow" => Some(PressureChannel::CapitalFlow),
        "institutional" => Some(PressureChannel::Institutional),
        "momentum" => Some(PressureChannel::Momentum),
        "volume" => Some(PressureChannel::Volume),
        "structure" => Some(PressureChannel::Structure),
        _ => None,
    }
}

fn decimal_to_f64(d: Decimal) -> f64 {
    d.to_f64().unwrap_or(0.0)
}

fn f64_to_decimal(v: f64) -> Result<Decimal, RestoreError> {
    Decimal::try_from(v).map_err(|_| RestoreError::BadFloat(v))
}

// ---------------------------------------------------------------------------
// Serialize: field → snapshot
// ---------------------------------------------------------------------------

/// Serialize a PressureBeliefField to a snapshot. Only informed
/// (sample_count >= 1) beliefs are written.
pub fn serialize_field(field: &PressureBeliefField, now: DateTime<Utc>) -> BeliefSnapshot {
    let mut gaussian = Vec::new();
    for ((symbol, channel), belief) in field.gaussian_iter() {
        if belief.sample_count == 0 {
            continue;
        }
        gaussian.push(GaussianSnapshotRow {
            symbol: symbol.0.clone(),
            channel: channel_to_str(*channel).to_string(),
            mean: decimal_to_f64(belief.mean),
            variance: decimal_to_f64(belief.variance),
            m2: decimal_to_f64(belief.m2_internal()),
            sample_count: belief.sample_count,
        });
    }

    let mut categorical = Vec::new();
    for (symbol, cat) in field.categorical_iter() {
        if cat.sample_count == 0 {
            continue;
        }
        // Write probs in canonical variant order so restore can match
        // without parsing variant names.
        let probs: Vec<f64> = PERSISTENT_STATE_VARIANTS
            .iter()
            .map(|variant| {
                cat.variants
                    .iter()
                    .position(|v| v == variant)
                    .and_then(|idx| cat.probs.get(idx))
                    .map(|d| decimal_to_f64(*d))
                    .unwrap_or(0.0)
            })
            .collect();

        categorical.push(CategoricalSnapshotRow {
            symbol: symbol.0.clone(),
            probs,
            sample_count: cat.sample_count,
        });
    }

    BeliefSnapshot {
        market: market_to_str(field.market()).to_string(),
        snapshot_ts: now,
        tick: field.last_tick(),
        gaussian,
        categorical,
    }
}

// ---------------------------------------------------------------------------
// Deserialize: snapshot → field
// ---------------------------------------------------------------------------

/// Reconstruct a PressureBeliefField from a snapshot.
pub fn restore_field(snap: &BeliefSnapshot) -> Result<PressureBeliefField, RestoreError> {
    let market = str_to_market(&snap.market)
        .ok_or_else(|| RestoreError::UnknownMarket(snap.market.clone()))?;

    let mut field = PressureBeliefField::new(market);

    for row in &snap.gaussian {
        let channel = str_to_channel(&row.channel)
            .ok_or_else(|| RestoreError::UnknownChannel(row.channel.clone()))?;
        let belief = GaussianBelief::from_raw(
            f64_to_decimal(row.mean)?,
            f64_to_decimal(row.variance)?,
            f64_to_decimal(row.m2)?,
            row.sample_count,
        );
        field.insert_gaussian_raw(Symbol(row.symbol.clone()), channel, belief);
    }

    for row in &snap.categorical {
        if row.probs.len() != PERSISTENT_STATE_VARIANTS.len() {
            return Err(RestoreError::BadCategoricalLen {
                got: row.probs.len(),
            });
        }
        let variants: Vec<PersistentStateKind> = PERSISTENT_STATE_VARIANTS.to_vec();
        let probs: Vec<Decimal> = row
            .probs
            .iter()
            .map(|p| f64_to_decimal(*p))
            .collect::<Result<_, _>>()?;
        let cat =
            CategoricalBelief::<PersistentStateKind>::from_raw(variants, probs, row.sample_count);
        field.insert_categorical_raw(Symbol(row.symbol.clone()), cat);
    }

    field.set_last_tick(snap.tick);
    field.set_last_snapshot_ts(snap.snapshot_ts);

    Ok(field)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use rust_decimal_macros::dec;

    #[test]
    fn serialize_skips_uninformed() {
        let field = PressureBeliefField::new(Market::Hk);
        let snap = serialize_field(&field, Utc.timestamp_opt(0, 0).unwrap());
        assert!(snap.gaussian.is_empty());
        assert!(snap.categorical.is_empty());
        assert_eq!(snap.market, "hk");
    }

    #[test]
    fn serialize_writes_informed_gaussians_and_categoricals() {
        let mut field = PressureBeliefField::new(Market::Us);
        let s = Symbol("NVDA.US".to_string());
        for _ in 0..5 {
            field.record_gaussian_sample(&s, PressureChannel::Volume, dec!(2.0), 1);
        }
        field.record_state_sample(&s, PersistentStateKind::Continuation);

        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let snap = serialize_field(&field, now);

        assert_eq!(snap.market, "us");
        assert_eq!(snap.snapshot_ts, now);
        assert_eq!(snap.tick, 1);
        assert_eq!(snap.gaussian.len(), 1);
        assert_eq!(snap.gaussian[0].symbol, "NVDA.US");
        assert_eq!(snap.gaussian[0].channel, "volume");
        assert_eq!(snap.gaussian[0].sample_count, 5);
        assert_eq!(snap.categorical.len(), 1);
        assert_eq!(snap.categorical[0].symbol, "NVDA.US");
        assert_eq!(snap.categorical[0].probs.len(), 5);
    }

    #[test]
    fn roundtrip_preserves_gaussian_welford() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let s = Symbol("0700.HK".to_string());

        for v in [dec!(1.0), dec!(2.0), dec!(3.0), dec!(4.0), dec!(5.0)] {
            field.record_gaussian_sample(&s, PressureChannel::OrderBook, v, 1);
        }

        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let snap = serialize_field(&field, now);
        let restored = restore_field(&snap).expect("restore ok");

        let orig = field
            .query_gaussian(&s, PressureChannel::OrderBook)
            .unwrap();
        let again = restored
            .query_gaussian(&s, PressureChannel::OrderBook)
            .unwrap();

        assert_eq!(orig.sample_count, again.sample_count);
        // Mean may differ in the last decimal due to f64↔Decimal rounding.
        let mean_drift = (orig.mean - again.mean).abs().to_f64().unwrap_or(0.0);
        assert!(mean_drift < 1e-6, "mean drift {}", mean_drift);
        let var_drift = (orig.variance - again.variance)
            .abs()
            .to_f64()
            .unwrap_or(0.0);
        assert!(var_drift < 1e-6, "variance drift {}", var_drift);
    }

    #[test]
    fn roundtrip_preserves_categorical_distribution() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let s = Symbol("0700.HK".to_string());

        for _ in 0..10 {
            field.record_state_sample(&s, PersistentStateKind::Continuation);
        }

        let now = Utc.timestamp_opt(1_700_000_000, 0).unwrap();
        let snap = serialize_field(&field, now);
        let restored = restore_field(&snap).expect("restore ok");

        let orig = field.query_state_posterior(&s).unwrap();
        let again = restored.query_state_posterior(&s).unwrap();
        assert_eq!(orig.sample_count, again.sample_count);
        assert_eq!(orig.variants, again.variants);
        // Probability drift bound: f64↔Decimal rounding.
        for (p, q) in orig.probs.iter().zip(again.probs.iter()) {
            let drift = (*p - *q).abs().to_f64().unwrap_or(0.0);
            assert!(drift < 1e-6, "prob drift {}", drift);
        }
    }

    #[test]
    fn restore_on_bad_market_returns_err() {
        let snap = BeliefSnapshot {
            market: "bad".to_string(),
            snapshot_ts: Utc.timestamp_opt(0, 0).unwrap(),
            tick: 0,
            gaussian: vec![],
            categorical: vec![],
        };
        assert!(matches!(
            restore_field(&snap),
            Err(RestoreError::UnknownMarket(_))
        ));
    }

    #[test]
    fn restore_on_bad_channel_returns_err() {
        let snap = BeliefSnapshot {
            market: "hk".to_string(),
            snapshot_ts: Utc.timestamp_opt(0, 0).unwrap(),
            tick: 0,
            gaussian: vec![GaussianSnapshotRow {
                symbol: "X".to_string(),
                channel: "not_a_channel".to_string(),
                mean: 0.0,
                variance: 0.0,
                m2: 0.0,
                sample_count: 1,
            }],
            categorical: vec![],
        };
        assert!(matches!(
            restore_field(&snap),
            Err(RestoreError::UnknownChannel(_))
        ));
    }

    #[test]
    fn restore_on_wrong_categorical_len_returns_err() {
        let snap = BeliefSnapshot {
            market: "hk".to_string(),
            snapshot_ts: Utc.timestamp_opt(0, 0).unwrap(),
            tick: 0,
            gaussian: vec![],
            categorical: vec![CategoricalSnapshotRow {
                symbol: "X".to_string(),
                probs: vec![0.5, 0.5], // only 2, not 5
                sample_count: 1,
            }],
        };
        assert!(matches!(
            restore_field(&snap),
            Err(RestoreError::BadCategoricalLen { got: 2 })
        ));
    }
}
