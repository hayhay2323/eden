//! Cross-market signal propagation: US overnight signals -> HK next-session priors.

use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::bridges::pairs::CROSS_MARKET_PAIRS;
use crate::external::polymarket::{PolymarketBias, PolymarketPrior, PolymarketSnapshot};
use crate::math::clamp_unit_interval;
use crate::ontology::objects::Symbol;
use crate::ontology::ReasoningScope;

#[derive(Debug, Clone, Deserialize)]
pub struct UsSignalEntry {
    pub symbol: String,
    pub composite: Decimal,
    #[serde(default)]
    pub price_momentum: Decimal,
    #[serde(default)]
    pub pre_post_market_anomaly: Decimal,
    #[serde(default)]
    pub mark_price: Option<Decimal>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UsSnapshot {
    pub timestamp: String,
    #[serde(default)]
    pub top_signals: Vec<UsSignalEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsToHkSignal {
    pub us_symbol: Symbol,
    pub hk_symbol: Symbol,
    pub us_composite: Decimal,
    pub us_timestamp: String,
    pub time_since_us_close_minutes: i64,
    pub propagation_confidence: Decimal,
}

pub fn compute_us_to_hk_signals(
    us_snapshot: &UsSnapshot,
    minutes_since_us_close: i64,
) -> Vec<UsToHkSignal> {
    let us_signals: HashMap<&str, &UsSignalEntry> = us_snapshot
        .top_signals
        .iter()
        .map(|signal| (signal.symbol.as_str(), signal))
        .collect();

    let decay = time_decay(minutes_since_us_close);
    if decay == Decimal::ZERO {
        return Vec::new();
    }

    CROSS_MARKET_PAIRS
        .iter()
        .filter_map(|pair| {
            let us_signal = us_signals.get(pair.us_symbol)?;
            if us_signal.composite == Decimal::ZERO {
                return None;
            }

            Some(UsToHkSignal {
                us_symbol: Symbol(pair.us_symbol.to_string()),
                hk_symbol: Symbol(pair.hk_symbol.to_string()),
                us_composite: us_signal.composite,
                us_timestamp: us_snapshot.timestamp.clone(),
                time_since_us_close_minutes: minutes_since_us_close,
                propagation_confidence: us_signal.composite * decay,
            })
        })
        .collect()
}

pub fn compute_us_counterpart_moves(us_snapshot: &UsSnapshot) -> HashMap<Symbol, Decimal> {
    let us_signals: HashMap<&str, &UsSignalEntry> = us_snapshot
        .top_signals
        .iter()
        .map(|signal| (signal.symbol.as_str(), signal))
        .collect();

    CROSS_MARKET_PAIRS
        .iter()
        .filter_map(|pair| {
            let us_signal = us_signals.get(pair.us_symbol)?;
            let move_ratio = if us_signal.price_momentum != Decimal::ZERO {
                us_signal.price_momentum
            } else {
                us_signal.composite
            };
            Some((Symbol(pair.hk_symbol.to_string()), move_ratio))
        })
        .collect()
}

pub async fn read_us_snapshot(path: &str) -> Result<UsSnapshot, String> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|error| format!("failed to read {path}: {error}"))?;
    serde_json::from_str(&content).map_err(|error| format!("failed to parse {path}: {error}"))
}

pub fn minutes_since_us_close(now: OffsetDateTime) -> i64 {
    let utc_hour = now.hour() as i64;
    let utc_minute = now.minute() as i64;
    let total_minutes = utc_hour * 60 + utc_minute;
    let us_close_utc_minutes = 20 * 60;

    if total_minutes < us_close_utc_minutes {
        total_minutes + (24 * 60 - us_close_utc_minutes)
    } else {
        total_minutes - us_close_utc_minutes
    }
}

fn time_decay(minutes_since_close: i64) -> Decimal {
    const FULL_DECAY_MINUTES: i64 = 720;

    if minutes_since_close <= 0 {
        Decimal::ONE
    } else if minutes_since_close >= FULL_DECAY_MINUTES {
        Decimal::ZERO
    } else {
        Decimal::ONE - Decimal::from(minutes_since_close) / Decimal::from(FULL_DECAY_MINUTES)
    }
}

pub fn to_polymarket_snapshot(
    fetched_at: OffsetDateTime,
    signals: &[UsToHkSignal],
) -> PolymarketSnapshot {
    let directional = signals
        .iter()
        .filter(|signal| signal.propagation_confidence != Decimal::ZERO)
        .collect::<Vec<_>>();
    if directional.is_empty() {
        return PolymarketSnapshot {
            fetched_at,
            priors: vec![],
        };
    }

    let net = directional
        .iter()
        .map(|signal| signal.propagation_confidence)
        .sum::<Decimal>();
    if net == Decimal::ZERO {
        return PolymarketSnapshot {
            fetched_at,
            priors: vec![],
        };
    }

    let avg_abs = directional
        .iter()
        .map(|signal| signal.propagation_confidence.abs())
        .sum::<Decimal>()
        / Decimal::from(directional.len() as i64);
    let probability =
        clamp_unit_interval(((net.abs() + avg_abs) / Decimal::from(2)).min(Decimal::ONE));
    let bias = if net > Decimal::ZERO {
        PolymarketBias::RiskOn
    } else {
        PolymarketBias::RiskOff
    };

    let mut target_scopes = directional
        .iter()
        .map(|signal| format!("symbol:{}", signal.hk_symbol))
        .collect::<Vec<_>>();
    target_scopes.sort();
    target_scopes.dedup();
    target_scopes.truncate(8);

    PolymarketSnapshot {
        fetched_at,
        priors: vec![PolymarketPrior {
            slug: format!("us-overnight-bridge-{}", fetched_at.unix_timestamp()),
            label: if bias == PolymarketBias::RiskOn {
                "US Overnight Bridge Risk-On".into()
            } else {
                "US Overnight Bridge Risk-Off".into()
            },
            question: "Will HK follow the latest US overnight tape?".into(),
            scope: ReasoningScope::market(),
            target_scopes,
            bias,
            selected_outcome: if bias == PolymarketBias::RiskOn {
                "hk_up".into()
            } else {
                "hk_down".into()
            },
            probability,
            conviction_threshold: Decimal::new(55, 2),
            active: true,
            closed: false,
            category: Some("bridge".into()),
            volume: None,
            liquidity: None,
            end_date: None,
        }],
    }
}
