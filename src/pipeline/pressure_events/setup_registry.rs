//! Per-symbol → setup registry for sub-tick confidence tracing.
//!
//! Tactical pipeline writes the latest setups at tick boundary; the
//! aggregator (sub-tick) reads, recomputes setup confidence from the
//! freshest posterior view, and emits a trace event so operators can
//! see confidence drifting between ticks. Setup mutation stays
//! tick-bound — the registry is read-only on the sub-tick side.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use rust_decimal::Decimal;

use crate::ontology::reasoning::{ReasoningScope, TacticalDirection, TacticalSetup};

#[derive(Debug, Clone)]
pub struct RegisteredSetup {
    pub hypothesis_id: String,
    pub setup_id: String,
    pub direction: TacticalDirection,
    /// Confidence at the last tick boundary (what the tactical pipeline
    /// wrote). Lets the aggregator print delta_vs_tick alongside the
    /// sub-tick confidence so the operator can see how far the prior
    /// has drifted between ticks.
    pub tick_confidence: Decimal,
}

#[derive(Debug, Default)]
pub struct SetupRegistry {
    /// Map symbol → setups affecting it. A symbol can have multiple
    /// setups (e.g. emerge:long + emerge:short) — store all of them.
    by_symbol: RwLock<HashMap<String, Vec<RegisteredSetup>>>,
}

pub type SharedSetupRegistry = Arc<SetupRegistry>;

impl SetupRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Replace the registry contents from the latest tactical setups.
    /// Called once per tick after the tactical pipeline finalises.
    pub fn refresh_from_setups(&self, setups: &[TacticalSetup]) {
        let mut by_symbol: HashMap<String, Vec<RegisteredSetup>> = HashMap::new();
        let mut total = 0usize;
        let mut skipped_scope = 0usize;
        let mut skipped_dir = 0usize;
        for setup in setups {
            let symbol = match &setup.scope {
                ReasoningScope::Symbol(s) => s.0.clone(),
                _ => {
                    skipped_scope += 1;
                    continue;
                }
            };
            let Some(direction) = setup.direction else {
                skipped_dir += 1;
                continue;
            };
            total += 1;
            by_symbol
                .entry(symbol)
                .or_default()
                .push(RegisteredSetup {
                    hypothesis_id: setup.hypothesis_id.clone(),
                    setup_id: setup.setup_id.clone(),
                    direction,
                    tick_confidence: setup.confidence,
                });
        }
        let unique_symbols = by_symbol.len();
        *self.by_symbol.write() = by_symbol;
        eprintln!(
            "[setup-registry] refresh: total_setups={} registered={} symbols={} skipped_scope={} skipped_dir={}",
            setups.len(),
            total,
            unique_symbols,
            skipped_scope,
            skipped_dir,
        );
    }

    pub fn get(&self, symbol: &str) -> Option<Vec<RegisteredSetup>> {
        self.by_symbol.read().get(symbol).cloned()
    }
}
