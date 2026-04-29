//! Per-symbol per-channel incremental state.
//!
//! Each channel keeps the latest input slices it needs to recompute
//! its single-symbol value without rebuilding the full LinkSnapshot.
//! Phase C1 ships OrderBook + Structure (both depth-driven, share
//! their input state). Other channels join this struct as their
//! workers are added.

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use rust_decimal::Decimal;

#[derive(Debug, Clone, Default)]
pub struct OrderBookState {
    pub bids: Vec<(Decimal, Decimal)>,
    pub asks: Vec<(Decimal, Decimal)>,
    pub last_updated: Option<DateTime<Utc>>,
    /// Latest computed OrderBook channel value (signed [-1, 1]).
    pub orderbook_value: Decimal,
    /// Latest computed Structure channel value (signed [-1, 1]).
    pub structure_value: Decimal,
}

/// Per-symbol trade-flow incremental state. Drives 3 sub-tick channels:
/// CapitalFlow (signed-volume EMA), Momentum (price-flow EMA), Volume
/// (trade-size z-score).
///
/// Uses simple per-trade EMA with constant α — push rate ~1-10 Hz
/// per active US symbol means α=0.05 gives a ~20-trade half-life,
/// roughly 5-30 seconds of memory which mirrors the tick window
/// without needing timestamp-aware decay (added later if needed).
#[derive(Debug, Clone, Default)]
pub struct TradeFlowState {
    pub last_price: Option<Decimal>,
    pub last_updated: Option<DateTime<Utc>>,
    /// EMA of signed-volume (direction sign × volume). Bull-tinted when
    /// trades print on the offer (Up), bear-tinted on bid (Down). Signs
    /// out unclassified trades (Neutral).
    pub ema_signed_volume: f64,
    /// EMA of (Δprice × volume). Captures momentum: large bullish prints
    /// move the EMA up; bearish prints pull it down.
    pub ema_price_flow: f64,
    /// EMA of trade volume (unsigned). Used as denominator for the
    /// volume-divergence ratio (current trade volume / EMA).
    pub ema_volume: f64,
    /// Latest computed CapitalFlow channel value (signed [-1, 1]).
    pub capital_flow_value: f64,
    /// Latest computed Momentum channel value (signed [-1, 1]).
    pub momentum_value: f64,
    /// Latest computed Volume channel value (signed [-1, 1]).
    pub volume_value: f64,
}

#[derive(Debug, Default)]
pub struct ChannelStates {
    pub orderbook: RwLock<HashMap<String, OrderBookState>>,
    pub tradeflow: RwLock<HashMap<String, TradeFlowState>>,
}

pub type SharedChannelStates = Arc<ChannelStates>;
