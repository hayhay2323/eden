//! Cross-market signal propagation: HK institutional signals -> US dual-listed stocks.

use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::bridges::pairs::CROSS_MARKET_PAIRS;
use crate::ontology::objects::Symbol;

fn hk_price_momentum_return_proxy() -> Decimal {
    Decimal::new(5, 2)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HkSignalEntry {
    pub symbol: String,
    pub composite: Decimal,
    pub institutional_alignment: Decimal,
    #[serde(default)]
    pub price_momentum: Decimal,
    #[serde(default)]
    pub sector_coherence: Option<Decimal>,
    #[serde(default)]
    pub cross_stock_correlation: Decimal,
    #[serde(default)]
    pub mark_price: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HkSnapshot {
    pub timestamp: String,
    #[serde(default)]
    pub top_signals: Vec<HkSignalEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossMarketSignal {
    pub hk_symbol: Symbol,
    pub us_symbol: Symbol,
    pub hk_composite: Decimal,
    pub hk_inst_alignment: Decimal,
    pub hk_timestamp: String,
    pub time_since_hk_close_minutes: i64,
    pub propagation_confidence: Decimal,
}

pub fn compute_cross_market_signals(
    hk_snapshot: &HkSnapshot,
    minutes_since_hk_close: i64,
) -> Vec<CrossMarketSignal> {
    let hk_signals: HashMap<&str, &HkSignalEntry> = hk_snapshot
        .top_signals
        .iter()
        .map(|signal| (signal.symbol.as_str(), signal))
        .collect();

    let decay = time_decay(minutes_since_hk_close);
    if decay == Decimal::ZERO {
        return Vec::new();
    }

    CROSS_MARKET_PAIRS
        .iter()
        .filter_map(|pair| {
            let hk_signal = hk_signals.get(pair.hk_symbol)?;
            if hk_signal.composite == Decimal::ZERO {
                return None;
            }

            Some(CrossMarketSignal {
                hk_symbol: Symbol(pair.hk_symbol.to_string()),
                us_symbol: Symbol(pair.us_symbol.to_string()),
                hk_composite: hk_signal.composite,
                hk_inst_alignment: hk_signal.institutional_alignment,
                hk_timestamp: hk_snapshot.timestamp.clone(),
                time_since_hk_close_minutes: minutes_since_hk_close,
                propagation_confidence: hk_signal.composite * decay,
            })
        })
        .collect()
}

pub fn compute_hk_counterpart_moves(hk_snapshot: &HkSnapshot) -> HashMap<Symbol, Decimal> {
    let hk_signals: HashMap<&str, &HkSignalEntry> = hk_snapshot
        .top_signals
        .iter()
        .map(|signal| (signal.symbol.as_str(), signal))
        .collect();

    CROSS_MARKET_PAIRS
        .iter()
        .filter_map(|pair| {
            let hk_signal = hk_signals.get(pair.hk_symbol)?;
            let move_ratio = if hk_signal.price_momentum != Decimal::ZERO {
                hk_signal.price_momentum * hk_price_momentum_return_proxy()
            } else {
                hk_signal.composite
            };
            Some((Symbol(pair.us_symbol.to_string()), move_ratio))
        })
        .collect()
}

pub async fn read_hk_snapshot(path: &str) -> Result<HkSnapshot, String> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|error| format!("failed to read {path}: {error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("failed to parse {path}: {error}"))
}

pub fn minutes_since_hk_close(now: OffsetDateTime) -> i64 {
    let utc_hour = now.hour() as i64;
    let utc_minute = now.minute() as i64;
    let total_minutes = utc_hour * 60 + utc_minute;
    let hk_close_utc_minutes = 8 * 60;

    if total_minutes < hk_close_utc_minutes {
        total_minutes + (24 * 60 - hk_close_utc_minutes)
    } else {
        total_minutes - hk_close_utc_minutes
    }
}

fn time_decay(minutes_since_close: i64) -> Decimal {
    // Keep HK priors alive through the bulk of the US cash session instead of
    // zeroing them out before the move has had a chance to propagate.
    const FULL_DECAY_MINUTES: i64 = 960;

    if minutes_since_close <= 0 {
        Decimal::ONE
    } else if minutes_since_close >= FULL_DECAY_MINUTES {
        Decimal::ZERO
    } else {
        Decimal::ONE - Decimal::from(minutes_since_close) / Decimal::from(FULL_DECAY_MINUTES)
    }
}
