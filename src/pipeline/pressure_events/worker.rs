//! Per-channel worker tasks. The dispatcher drains the global event
//! bus and fans events out to per-channel tokio mpsc channels. Each
//! channel worker takes its own subset, recomputes its symbol value,
//! and notifies the aggregator.
//!
//! Phase C1: dispatcher + OrderBook/Structure worker (depth events).

use std::sync::Arc;

use rust_decimal::Decimal;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::pipeline::dimensions;

use super::aggregator::AggregatorHandle;
use super::bus::EventBusHandle;
use super::channel_state::SharedChannelStates;
use super::event::PressureEvent;

/// Capacity of each per-channel sub-bus. The dispatcher uses
/// `try_send` so when a worker falls behind the event is dropped on
/// that channel — pressure freshness is recoverable on the next
/// matching event, and we never let one slow worker block the
/// dispatcher.
const SUB_BUS_CAP: usize = 20_000;

pub struct WorkerPoolHandles {
    pub dispatcher: JoinHandle<()>,
    pub orderbook: JoinHandle<()>,
}

pub fn spawn_worker_pool(
    bus: Arc<EventBusHandle>,
    states: SharedChannelStates,
    aggregator: AggregatorHandle,
) -> WorkerPoolHandles {
    let (depth_tx, depth_rx) = mpsc::channel::<PressureEvent>(SUB_BUS_CAP);
    // Trade/Broker/Quote sub-buses are reserved for Phases C2..C6;
    // create them now so the dispatcher's pattern is fixed and adding
    // a worker later is a one-line change.
    let (trade_tx, _trade_rx) = mpsc::channel::<PressureEvent>(SUB_BUS_CAP);
    let (broker_tx, _broker_rx) = mpsc::channel::<PressureEvent>(SUB_BUS_CAP);
    let (quote_tx, _quote_rx) = mpsc::channel::<PressureEvent>(SUB_BUS_CAP);

    let dispatcher = tokio::spawn(async move {
        let mut counts = [0u64; 4]; // depth, trade, broker, quote
        loop {
            let evt = match bus.pop().await {
                Some(e) => e,
                None => break,
            };
            let (target, idx) = match &evt {
                PressureEvent::Depth { .. } => (&depth_tx, 0),
                PressureEvent::Trade { .. } => (&trade_tx, 1),
                PressureEvent::Broker { .. } => (&broker_tx, 2),
                PressureEvent::Quote { .. } => (&quote_tx, 3),
            };
            counts[idx] += 1;
            let total: u64 = counts.iter().sum();
            if total == 1 || total % 50 == 0 {
                eprintln!(
                    "[pressure-dispatch] total={} depth={} trade={} broker={} quote={}",
                    total, counts[0], counts[1], counts[2], counts[3],
                );
            }
            let _ = target.try_send(evt);
        }
    });

    let orderbook = spawn_orderbook_worker(depth_rx, states, aggregator);

    WorkerPoolHandles {
        dispatcher,
        orderbook,
    }
}

fn spawn_orderbook_worker(
    mut rx: mpsc::Receiver<PressureEvent>,
    states: SharedChannelStates,
    aggregator: AggregatorHandle,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(evt) = rx.recv().await {
            let PressureEvent::Depth {
                symbol,
                bids,
                asks,
                ts,
            } = evt
            else {
                continue;
            };
            let new_ob = dimensions::compute_order_book_pressure_from_depth(&bids, &asks);
            let new_st = dimensions::compute_depth_structure_imbalance_from_depth(&bids, &asks);
            let changed = {
                let mut map = states.orderbook.write();
                let s = map.entry(symbol.clone()).or_default();
                let ob_changed = (s.orderbook_value - new_ob).abs() > Decimal::new(1, 3);
                let st_changed = (s.structure_value - new_st).abs() > Decimal::new(1, 3);
                s.bids = bids;
                s.asks = asks;
                s.last_updated = Some(ts);
                s.orderbook_value = new_ob;
                s.structure_value = new_st;
                ob_changed || st_changed
            };
            if changed {
                aggregator.notify_symbol_changed(symbol);
            }
        }
    })
}
