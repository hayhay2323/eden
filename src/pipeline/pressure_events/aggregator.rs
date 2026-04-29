//! Per-symbol aggregator. When a channel's state changes, the
//! aggregator reads the latest values across all wired channels,
//! derives a sub-tick `NodePrior`, and calls
//! `BeliefSubstrate::observe_symbol` so the BP residual queue
//! propagates the change.
//!
//! Phase C1: only OrderBook + Structure are wired; the other 4
//! channels pass `None` and contribute zero direction signal.

use std::sync::Arc;

use rust_decimal::prelude::ToPrimitive;
use tokio::sync::mpsc;

use crate::pipeline::event_driven_bp::BeliefSubstrate;
use crate::pipeline::loopy_bp;

use super::channel_state::SharedChannelStates;

#[derive(Clone)]
pub struct AggregatorHandle {
    tx: mpsc::Sender<String>,
}

impl AggregatorHandle {
    /// Non-blocking notification that a channel's state has changed
    /// for this symbol. Drops the notification if the channel is full
    /// (the aggregator catches up on the next genuine change).
    pub fn notify_symbol_changed(&self, symbol: String) {
        let _ = self.tx.try_send(symbol);
    }
}

const NOTIFY_QUEUE_CAP: usize = 50_000;

pub fn spawn_aggregator(
    states: SharedChannelStates,
    substrate: Arc<dyn BeliefSubstrate>,
) -> AggregatorHandle {
    let (tx, mut rx) = mpsc::channel::<String>(NOTIFY_QUEUE_CAP);
    tokio::spawn(async move {
        let mut observe_count: u64 = 0;
        while let Some(symbol) = rx.recv().await {
            let (ob_value, st_value) = {
                let map = states.orderbook.read();
                map.get(&symbol)
                    .map(|s| (s.orderbook_value, s.structure_value))
                    .unwrap_or((rust_decimal::Decimal::ZERO, rust_decimal::Decimal::ZERO))
            };
            let prior = loopy_bp::prior_from_pressure_channels(
                Some(ob_value.to_f64().unwrap_or(0.0)),
                None,
                None,
                None,
                None,
                Some(st_value.to_f64().unwrap_or(0.0)),
            );
            substrate.observe_symbol(&symbol, prior, &[]);
            observe_count = observe_count.wrapping_add(1);
            if observe_count == 1 || observe_count % 25 == 0 {
                let snap = substrate.posterior_snapshot();
                eprintln!(
                    "[pressure-agg] observe_symbol fired {} times (latest={}, posterior generation={})",
                    observe_count, symbol, snap.generation
                );
            }
        }
    });
    AggregatorHandle { tx }
}
