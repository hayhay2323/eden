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
use rust_decimal::Decimal;
use tokio::sync::mpsc;

use crate::ontology::reasoning::TacticalDirection;
use crate::pipeline::event_driven_bp::BeliefSubstrate;
use crate::pipeline::loopy_bp::{self, STATE_BEAR, STATE_BULL};

use super::channel_state::SharedChannelStates;
use super::setup_registry::SharedSetupRegistry;

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
    setup_registry: SharedSetupRegistry,
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
            let (cf_value, mo_value, vol_value) = {
                let map = states.tradeflow.read();
                map.get(&symbol)
                    .map(|s| (s.capital_flow_value, s.momentum_value, s.volume_value))
                    .unwrap_or((0.0, 0.0, 0.0))
            };
            let prior = loopy_bp::prior_from_pressure_channels(
                Some(ob_value.to_f64().unwrap_or(0.0)),
                Some(cf_value),
                None, // Institutional — HK-only (broker queue)
                Some(mo_value),
                Some(vol_value),
                Some(st_value.to_f64().unwrap_or(0.0)),
            );
            // Skip if sub-tick channels can't produce a confident prior —
            // overwriting the tick-bound prior with uniform/unobserved
            // would erase real evidence between ticks. Only update BP
            // when the sub-tick path has something material to add.
            if !prior.observed {
                continue;
            }
            let prior_snap = prior.clone();
            substrate.observe_symbol(&symbol, prior, &[]);
            observe_count = observe_count.wrapping_add(1);

            // Sub-tick setup confidence trace. Pull the latest posterior
            // (already refreshed by the substrate's publisher) and, for
            // every setup the tactical pipeline has registered for this
            // symbol, compute what its confidence WOULD be right now and
            // print a delta vs the tick-bound confidence. Mutation of
            // the running setup objects stays at tick boundary; this is
            // observability only.
            if let Some(setups) = setup_registry.get(&symbol) {
                let snap = substrate.posterior_snapshot();
                if let Some(post) = snap.beliefs.get(&symbol) {
                    for s in &setups {
                        let p_target = match s.direction {
                            TacticalDirection::Long => post[STATE_BULL],
                            TacticalDirection::Short => post[STATE_BEAR],
                        };
                        if let Ok(sub_conf) = Decimal::try_from(p_target.clamp(0.0, 1.0)) {
                            let delta = sub_conf - s.tick_confidence;
                            // Print on any meaningful drift (≥0.01).
                            // 0.01 is barely-noisy; 0.05 was missing
                            // most of the actual sub-tick evolution.
                            if delta.abs() >= Decimal::new(1, 2) {
                                eprintln!(
                                    "[setup-trace] sym={} dir={:?} tick_conf={:.3} sub_conf={:.3} delta={:+.3} hyp={} gen={}",
                                    symbol,
                                    s.direction,
                                    s.tick_confidence,
                                    sub_conf,
                                    delta,
                                    s.hypothesis_id,
                                    snap.generation,
                                );
                            }
                        }
                    }
                }
            }
            if observe_count == 1 || observe_count % 25 == 0 {
                let snap = substrate.posterior_snapshot();
                eprintln!(
                    "[pressure-agg] obs={} sym={} prior=[{:.3},{:.3},{:.3}] obs?={} cf={:.3} mo={:.3} vol={:.3} ob={:.3} st={:.3} gen={}",
                    observe_count,
                    symbol,
                    prior_snap.belief[0],
                    prior_snap.belief[1],
                    prior_snap.belief[2],
                    prior_snap.observed,
                    cf_value,
                    mo_value,
                    vol_value,
                    ob_value.to_f64().unwrap_or(0.0),
                    st_value.to_f64().unwrap_or(0.0),
                    snap.generation,
                );
            }
        }
    });
    AggregatorHandle { tx }
}
