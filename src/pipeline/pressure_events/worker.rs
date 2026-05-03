//! Per-channel worker tasks. The dispatcher drains the global event
//! bus and fans events out to per-channel tokio mpsc channels. Each
//! channel worker takes its own subset, recomputes its symbol value,
//! and notifies the aggregator.
//!
//! Phase C1: dispatcher + OrderBook/Structure worker (depth events).

use std::sync::Arc;

use rust_decimal::prelude::{FromPrimitive, ToPrimitive};
use rust_decimal::Decimal;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::pipeline::dimensions;

use super::aggregator::AggregatorHandle;
use super::bus::EventBusHandle;
use super::channel_state::SharedChannelStates;
use super::event::{PressureEvent, TradeSide};

/// EMA smoothing factor for trade-flow channels. α=0.05 ≈ 20-trade
/// half-life, ~5-30 s of memory for a 1-10 Hz push rate. Tunable.
const TRADE_EMA_ALPHA: f64 = 0.05;

/// Trades below this share count are odd-lot / dust opportunistic prints
/// that carry no directional information. Skipping them prevents the
/// vol channel saturating to ±1 on a single 71-share match.
const TRADE_MIN_VOLUME: f64 = 100.0;

/// Capacity of each per-channel sub-bus. The dispatcher uses
/// `try_send` so when a worker falls behind the event is dropped on
/// that channel — pressure freshness is recoverable on the next
/// matching event, and we never let one slow worker block the
/// dispatcher.
const SUB_BUS_CAP: usize = 20_000;

pub struct WorkerPoolHandles {
    pub dispatcher: JoinHandle<()>,
    pub orderbook: JoinHandle<()>,
    pub trade: JoinHandle<()>,
    pub broker: JoinHandle<()>,
    pub option: JoinHandle<()>,
}

pub fn spawn_worker_pool(
    bus: Arc<EventBusHandle>,
    states: SharedChannelStates,
    aggregator: AggregatorHandle,
    store: Arc<crate::ontology::store::ObjectStore>,
) -> WorkerPoolHandles {
    let (depth_tx, depth_rx) = mpsc::channel::<PressureEvent>(SUB_BUS_CAP);
    let (trade_tx, trade_rx) = mpsc::channel::<PressureEvent>(SUB_BUS_CAP);
    let (broker_tx, broker_rx) = mpsc::channel::<PressureEvent>(SUB_BUS_CAP);
    let (option_tx, option_rx) = mpsc::channel::<PressureEvent>(SUB_BUS_CAP);
    // Quote sub-bus reserved for future use; receiver dropped here.
    let (quote_tx, _quote_rx) = mpsc::channel::<PressureEvent>(SUB_BUS_CAP);

    let dispatcher = tokio::spawn(async move {
        let mut counts = [0u64; 5]; // depth, trade, broker, quote, option
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
                PressureEvent::Option { .. } => (&option_tx, 4),
            };
            counts[idx] += 1;
            let total: u64 = counts.iter().sum();
            // 2026-05-01: throttled 50 → 10000. Was 1000s/sec — noise.
            if total == 1 || total % 10000 == 0 {
                eprintln!(
                    "[pressure-dispatch] total={} depth={} trade={} broker={} quote={} opt={}",
                    total, counts[0], counts[1], counts[2], counts[3], counts[4],
                );
            }
            let _ = target.try_send(evt);
        }
    });

    let orderbook = spawn_orderbook_worker(depth_rx, Arc::clone(&states), aggregator.clone());
    let trade = spawn_trade_worker(trade_rx, Arc::clone(&states), aggregator.clone());
    let broker = spawn_broker_worker(broker_rx, Arc::clone(&states), aggregator.clone(), store);
    let option = spawn_option_worker(option_rx, states, aggregator);

    WorkerPoolHandles {
        dispatcher,
        orderbook,
        trade,
        broker,
        option,
    }
}

// ... spawn_broker_worker ...

/// Option-driven worker: drains Option surface events, recomputes the
/// `option_bias` scalar (weighted sum of IV skew and OI ratio).
fn spawn_option_worker(
    mut rx: mpsc::Receiver<PressureEvent>,
    states: SharedChannelStates,
    aggregator: AggregatorHandle,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(evt) = rx.recv().await {
            let PressureEvent::Option {
                symbol,
                put_call_ratio,
                iv_skew,
                ts,
            } = evt
            else {
                continue;
            };

            // Option Bias Formula:
            // High Put/Call Ratio = Bearish (-)
            // High IV Skew (Puts > Calls) = Bearish (-)
            let pc_raw = put_call_ratio.and_then(|r| r.to_f64()).unwrap_or(1.0);
            let skew_raw = iv_skew.and_then(|s| s.to_f64()).unwrap_or(0.0);

            // Put/Call component: map 1.0 -> 0.0, 0.5 -> 0.5, 2.0 -> -0.5
            let pc_bias = (2.0 / (1.0 + pc_raw)) - 1.0;
            // Skew component: high skew (0.2+) is bearish.
            let skew_bias = -(skew_raw / 0.2).clamp(-1.0, 1.0);

            let new_value =
                Decimal::from_f64(0.4 * pc_bias + 0.6 * skew_bias).unwrap_or(Decimal::ZERO);

            let changed = {
                let mut map = states.option.write();
                let s = map.entry(symbol.clone()).or_default();
                let old_value = s.option_value;
                s.last_updated = Some(ts);
                s.option_value = new_value;
                (old_value - new_value).abs() > Decimal::new(1, 3)
            };

            if changed {
                aggregator.notify_symbol_changed(symbol);
            }
        }
    })
}

// ... spawn_orderbook_worker ...

/// Broker-driven worker: drains Broker events, maps broker IDs to
/// institutions, recomputes the `institutional_direction` scalar.
fn spawn_broker_worker(
    mut rx: mpsc::Receiver<PressureEvent>,
    states: SharedChannelStates,
    aggregator: AggregatorHandle,
    store: Arc<crate::ontology::store::ObjectStore>,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(evt) = rx.recv().await {
            let PressureEvent::Broker {
                symbol,
                bid_broker_ids,
                ask_broker_ids,
                ts,
            } = evt
            else {
                continue;
            };

            let mut bid_inst_count = 0;
            let mut ask_inst_count = 0;

            for id in bid_broker_ids {
                if store
                    .broker_to_institution
                    .contains_key(&crate::ontology::objects::BrokerId(id))
                {
                    bid_inst_count += 1;
                }
            }
            for id in ask_broker_ids {
                if store
                    .broker_to_institution
                    .contains_key(&crate::ontology::objects::BrokerId(id))
                {
                    ask_inst_count += 1;
                }
            }

            let new_value = crate::math::normalized_ratio(
                Decimal::from(bid_inst_count),
                Decimal::from(ask_inst_count),
            );

            let changed = {
                let mut map = states.broker.write();
                let s = map.entry(symbol.clone()).or_default();
                let old_value = s.institutional_value;
                s.last_updated = Some(ts);
                s.institutional_value = new_value;
                (old_value - new_value).abs() > Decimal::new(1, 3)
            };

            if changed {
                aggregator.notify_symbol_changed(symbol);
            }
        }
    })
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

/// Trade-driven worker: drains Trade events, updates per-symbol EMA
/// state, recomputes 3 channels (CapitalFlow / Momentum / Volume),
/// notifies aggregator on material change.
///
/// Channel formulas (push-only approximations, NOT bit-identical to
/// REST-driven tick-bound versions):
///   - CapitalFlow ≈ tanh(ema_signed_volume / scale_norm)
///   - Momentum    ≈ tanh(ema_price_flow / price_norm)
///   - Volume      ≈ clamp((current_volume / ema_volume) - 1, -1, 1)
fn spawn_trade_worker(
    mut rx: mpsc::Receiver<PressureEvent>,
    states: SharedChannelStates,
    aggregator: AggregatorHandle,
) -> JoinHandle<()> {
    tokio::spawn(async move {
        while let Some(evt) = rx.recv().await {
            let PressureEvent::Trade {
                symbol,
                price,
                volume,
                side,
                ts,
            } = evt
            else {
                continue;
            };
            let price_f = price.to_f64().unwrap_or(0.0);
            let volume_f = volume.to_f64().unwrap_or(0.0);
            if volume_f < TRADE_MIN_VOLUME {
                continue;
            }
            let direction_sign = match side {
                TradeSide::Buy => 1.0,
                TradeSide::Sell => -1.0,
                TradeSide::Unknown => 0.0,
            };

            let changed = {
                let mut map = states.tradeflow.write();
                let s = map.entry(symbol.clone()).or_default();
                let alpha = TRADE_EMA_ALPHA;
                let prev_price = s.last_price.and_then(|p| p.to_f64()).unwrap_or(price_f);
                let dprice = price_f - prev_price;

                // Update EMAs.
                s.ema_signed_volume =
                    (1.0 - alpha) * s.ema_signed_volume + alpha * (direction_sign * volume_f);
                s.ema_price_flow = (1.0 - alpha) * s.ema_price_flow + alpha * (dprice * volume_f);
                let prev_ema_volume = if s.ema_volume > 0.0 {
                    s.ema_volume
                } else {
                    volume_f.max(1.0)
                };
                s.ema_volume = (1.0 - alpha) * s.ema_volume + alpha * volume_f;

                // Channel values.
                // CapitalFlow: scale by 10× volume_ema so a single trade
                // at the same volume direction gives a strong signal,
                // saturating via tanh.
                let cf_scale = (10.0 * prev_ema_volume).max(1.0);
                let new_cf = (s.ema_signed_volume / cf_scale).tanh();
                // Momentum: scale by ema_volume × small price (1 % of
                // price as natural unit) so a 1 % price change with one
                // ema-volume trade saturates.
                let price_norm_unit = (price_f * 0.01).max(1e-3);
                let mo_scale = (prev_ema_volume * price_norm_unit).max(1e-3);
                let new_mo = (s.ema_price_flow / mo_scale).tanh();
                // Volume: ratio (current / ema) − 1, clamped.
                let new_vol_raw = (volume_f / prev_ema_volume) - 1.0;
                let new_vol = new_vol_raw.clamp(-1.0, 1.0);

                s.last_price = Some(price);
                s.last_updated = Some(ts);
                s.capital_flow_value = new_cf;
                s.momentum_value = new_mo;
                s.volume_value = new_vol;
                // Always notify — EMA updates are inexpensive at the
                // aggregator level (single ArcSwap read + observe_symbol),
                // and substrate.observe_symbol's own prior_changed gate
                // is the authoritative no-op suppressor.
                true
            };

            if changed {
                aggregator.notify_symbol_changed(symbol);
            }
        }
    })
}
