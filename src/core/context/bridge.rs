//! Bridge from [`LiveSnapshot`] to the context layer types.
//!
//! This module provides zero-copy-friendly conversions so that existing
//! snapshot-producing code can feed into the new [`LayeredContext`] system
//! without modifying `live_snapshot.rs`.

use std::collections::HashMap;

use rust_decimal::prelude::ToPrimitive;

use super::layered::LayeredContext;
use super::live_context::LiveContext;
use super::reasoning_state::ReasoningState;
use super::session_memory::SessionMemory;
use super::static_context::StaticContext;
use crate::live_snapshot::LiveSnapshot;

// ---------------------------------------------------------------------------
// From<&LiveSnapshot> for LiveContext
// ---------------------------------------------------------------------------

impl From<&LiveSnapshot> for LiveContext {
    fn from(snap: &LiveSnapshot) -> Self {
        // Derive a mood label from market-regime bias when it carries meaning.
        let market_mood = if snap.market_regime.bias.is_empty() {
            None
        } else {
            Some(snap.market_regime.bias.clone())
        };

        // Regime mirrors the bias string (e.g. "bullish", "neutral", "bearish").
        let regime = Some(snap.market_regime.bias.clone());

        // Map composite stress (Decimal in [0,1]) to f64.
        let stress_level = snap.stress.composite_stress.to_f64();

        // Active signal count: use top_signals length as the best proxy.
        let active_signal_count = snap.top_signals.len();

        LiveContext {
            tick_count: snap.tick,
            timestamp: snap.timestamp.clone(),
            market_mood,
            regime,
            stress_level,
            active_signal_count,
        }
    }
}

// ---------------------------------------------------------------------------
// Convenience builder
// ---------------------------------------------------------------------------

/// Build a full [`LayeredContext`] from a [`LiveSnapshot`] and session metadata.
///
/// - `market` — market identifier string (e.g. `"HK"`, `"US"`).
/// - `session_date` — ISO-8601 date for the trading session.
/// - `symbols` — the symbol universe for this session.
///
/// Session memory and reasoning state are initialised empty; callers can
/// populate them afterwards via the mutable accessors on [`LayeredContext`].
pub fn build_layered_from_snapshot(
    snapshot: &LiveSnapshot,
    market: &str,
    session_date: &str,
    symbols: Vec<String>,
) -> LayeredContext {
    let static_ctx = StaticContext::new(
        market.to_string(),
        symbols,
        HashMap::new(),
        session_date.to_string(),
    );

    let live = LiveContext::from(snapshot);
    let memory = SessionMemory::new();
    let reasoning = ReasoningState::new();

    LayeredContext::new(static_ctx, live, memory, reasoning)
}
