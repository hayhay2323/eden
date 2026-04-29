use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use super::objects::Symbol;

/// Snapshot of the full option chain for a symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionChainSnapshot {
    pub underlying: Symbol,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub expiries: Vec<OptionExpiry>,
}

/// All strikes for one expiry date.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionExpiry {
    pub date: String, // "yyyy-mm-dd"
    pub strikes: Vec<OptionStrike>,
}

/// One strike with call and put info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionStrike {
    pub strike_price: Decimal,
    pub call_symbol: String,
    pub put_symbol: String,
    pub standard: bool,
}

/// Margin ratio data for a symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarginRatio {
    pub symbol: Symbol,
    /// Initial margin factor (0-1)
    pub im_factor: Decimal,
    /// Maintenance margin factor (0-1)
    pub mm_factor: Decimal,
    /// Forced liquidation margin factor (0-1)
    pub fm_factor: Decimal,
}

/// Trading calendar for a market.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TradingCalendar {
    pub market: String,
    pub trading_days: Vec<String>, // "yyyy-mm-dd"
    pub half_trading_days: Vec<String>,
}

impl TradingCalendar {
    pub fn is_trading_day(&self, date: &str) -> bool {
        self.trading_days.contains(&date.to_string())
    }
}
