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

#[derive(Debug, Default)]
pub struct ChannelStates {
    pub orderbook: RwLock<HashMap<String, OrderBookState>>,
}

pub type SharedChannelStates = Arc<ChannelStates>;
