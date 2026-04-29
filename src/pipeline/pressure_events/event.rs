//! Per-channel push events.
//!
//! `PressureEvent` carries just the data each pressure channel cares
//! about, so workers don't have to filter against an envelope type.
//! `demux_push_event` translates a single `longport::quote::PushEvent`
//! into zero or more `PressureEvent`s.

use chrono::{DateTime, Utc};
use longport::quote::{PushEvent, PushEventDetail};
use rust_decimal::Decimal;

#[derive(Debug, Clone)]
pub enum PressureEvent {
    /// 10-level depth update. Drives OrderBook + Structure channels.
    Depth {
        symbol: String,
        bids: Vec<(Decimal, Decimal)>,
        asks: Vec<(Decimal, Decimal)>,
        ts: DateTime<Utc>,
    },
    /// Last trade. Drives CapitalFlow + Momentum + Volume channels.
    Trade {
        symbol: String,
        price: Decimal,
        volume: Decimal,
        side: TradeSide,
        ts: DateTime<Utc>,
    },
    /// Broker queue update. Drives Institutional channel.
    Broker {
        symbol: String,
        broker_id: i32,
        side: TradeSide,
        position: i32,
        ts: DateTime<Utc>,
    },
    /// Quote (last/volume/turnover). Drives CapitalFlow (turnover delta)
    /// + Volume (volume delta) when trade events are sparse.
    Quote {
        symbol: String,
        last: Decimal,
        volume: i64,
        turnover: Decimal,
        ts: DateTime<Utc>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeSide {
    Buy,
    Sell,
    Unknown,
}

/// Demultiplex a single longport `PushEvent` into zero or more
/// `PressureEvent`s. Caller publishes each result to the bus.
pub fn demux_push_event(evt: &PushEvent) -> Vec<PressureEvent> {
    let symbol = evt.symbol.clone();
    let ts = Utc::now();
    match &evt.detail {
        PushEventDetail::Quote(q) => vec![PressureEvent::Quote {
            symbol,
            last: q.last_done,
            volume: q.volume,
            turnover: q.turnover,
            ts,
        }],
        PushEventDetail::Depth(d) => vec![PressureEvent::Depth {
            symbol,
            bids: d
                .bids
                .iter()
                .map(|l| (l.price.unwrap_or_default(), Decimal::from(l.volume)))
                .collect(),
            asks: d
                .asks
                .iter()
                .map(|l| (l.price.unwrap_or_default(), Decimal::from(l.volume)))
                .collect(),
            ts,
        }],
        PushEventDetail::Trade(trades) => trades
            .trades
            .iter()
            .map(|t| PressureEvent::Trade {
                symbol: symbol.clone(),
                price: t.price,
                volume: Decimal::from(t.volume),
                side: TradeSide::Unknown,
                ts,
            })
            .collect(),
        PushEventDetail::Brokers(b) => {
            let mut out = Vec::with_capacity(b.bid_brokers.len() + b.ask_brokers.len());
            for seat in &b.bid_brokers {
                out.push(PressureEvent::Broker {
                    symbol: symbol.clone(),
                    broker_id: seat.broker_ids.first().copied().unwrap_or(0),
                    side: TradeSide::Buy,
                    position: seat.position,
                    ts,
                });
            }
            for seat in &b.ask_brokers {
                out.push(PressureEvent::Broker {
                    symbol: symbol.clone(),
                    broker_id: seat.broker_ids.first().copied().unwrap_or(0),
                    side: TradeSide::Sell,
                    position: seat.position,
                    ts,
                });
            }
            out
        }
        // Candlesticks (and any future longport variants) are not
        // pressure-relevant — they're handled by the tick-bound
        // candlestick aggregator.
        _ => Vec::new(),
    }
}
