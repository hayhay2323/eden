//! Per-channel push events.
//!
//! `PressureEvent` carries just the data each pressure channel cares
//! about, so workers don't have to filter against an envelope type.
//! `demux_push_event` translates a single `longport::quote::PushEvent`
//! into zero or more `PressureEvent`s.

use chrono::{DateTime, Utc};
use longport::quote::{PushEvent, PushEventDetail, TradeDirection};
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
        bid_broker_ids: Vec<i32>,
        ask_broker_ids: Vec<i32>,
        ts: DateTime<Utc>,
    },
    /// Option surface update. Drives Option channel.
    Option {
        symbol: String,
        put_call_ratio: Option<Decimal>,
        iv_skew: Option<Decimal>,
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
                side: match t.direction {
                    TradeDirection::Up => TradeSide::Buy,
                    TradeDirection::Down => TradeSide::Sell,
                    TradeDirection::Neutral => TradeSide::Unknown,
                },
                ts,
            })
            .collect(),
        PushEventDetail::Brokers(b) => {
            vec![PressureEvent::Broker {
                symbol,
                bid_broker_ids: b
                    .bid_brokers
                    .iter()
                    .filter_map(|s| s.broker_ids.first().copied())
                    .collect(),
                ask_broker_ids: b
                    .ask_brokers
                    .iter()
                    .filter_map(|s| s.broker_ids.first().copied())
                    .collect(),
                ts,
            }]
        }
        // Candlesticks (and any future longport variants) are not
        // pressure-relevant — they're handled by the tick-bound
        // candlestick aggregator.
        _ => Vec::new(),
    }
}
