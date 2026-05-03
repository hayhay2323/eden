use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use super::objects::Symbol;
use super::snapshot::RawSnapshot;

// ── Enums ──

/// Candlestick period. Eden-owned mirror of longport::quote::Period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CandlePeriod {
    Min1,
    Min5,
    Min15,
    Min30,
    Min60,
    Day,
    Week,
    Month,
}

/// Trade session. Eden-owned mirror of longport::quote::TradeSession.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TradeSession {
    Normal,
    Pre,
    Post,
    Overnight,
}

impl TradeSession {
    pub fn from_longport(session: longport::quote::TradeSession) -> Self {
        match session {
            longport::quote::TradeSession::Intraday => Self::Normal,
            longport::quote::TradeSession::Pre => Self::Pre,
            longport::quote::TradeSession::Post => Self::Post,
            longport::quote::TradeSession::Overnight => Self::Overnight,
        }
    }
}

/// Trade direction. Mirrors links::TradeDirection but Serialize-able for archival.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArchivedTradeDirection {
    Up,
    Down,
    Neutral,
}

impl ArchivedTradeDirection {
    pub fn from_longport(dir: longport::quote::TradeDirection) -> Self {
        match dir {
            longport::quote::TradeDirection::Up => Self::Up,
            longport::quote::TradeDirection::Down => Self::Down,
            _ => Self::Neutral,
        }
    }
}

// ── Archived types ──

/// Single depth level with price, volume, and order count.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedDepthLevel {
    pub position: i32,
    pub price: Option<Decimal>,
    pub volume: i64,
    pub order_num: i64,
}

/// Full 10-level order book snapshot for one symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedOrderBook {
    pub symbol: Symbol,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub ask_levels: Vec<ArchivedDepthLevel>,
    pub bid_levels: Vec<ArchivedDepthLevel>,
}

/// Single candlestick bar with full OHLCV.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedCandlestick {
    pub symbol: Symbol,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub period: CandlePeriod,
    pub session: TradeSession,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: i64,
    pub turnover: Decimal,
}

/// Single trade record with session classification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedTrade {
    pub symbol: Symbol,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub price: Decimal,
    pub volume: i64,
    pub direction: ArchivedTradeDirection,
    pub session: TradeSession,
    pub trade_type: String,
}

/// Full intraday capital flow time series (all minute-level data points).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedCapitalFlowSeries {
    pub symbol: Symbol,
    pub points: Vec<ArchivedCapitalFlowPoint>,
}

/// Single capital flow data point.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedCapitalFlowPoint {
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    /// Cumulative net inflow in 萬元 (ten-thousands of yuan).
    pub inflow: Decimal,
}

/// Pre/post market quote data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExtendedQuote {
    pub last_done: Decimal,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub volume: i64,
    pub turnover: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub prev_close: Decimal,
}

/// Full quote snapshot including pre/post market.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedQuote {
    pub symbol: Symbol,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub last_done: Decimal,
    pub prev_close: Decimal,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub volume: i64,
    pub turnover: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_market: Option<ExtendedQuote>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_market: Option<ExtendedQuote>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overnight: Option<ExtendedQuote>,
}

/// Capital distribution snapshot (large/medium/small breakdown).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedCapitalDistribution {
    pub symbol: Symbol,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub large_in: Decimal,
    pub large_out: Decimal,
    pub medium_in: Decimal,
    pub medium_out: Decimal,
    pub small_in: Decimal,
    pub small_out: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedCalcIndex {
    pub symbol: Symbol,
    pub turnover_rate: Option<Decimal>,
    pub volume_ratio: Option<Decimal>,
    pub pe_ttm_ratio: Option<Decimal>,
    pub pb_ratio: Option<Decimal>,
    pub dividend_ratio_ttm: Option<Decimal>,
    pub amplitude: Option<Decimal>,
    pub five_minutes_change_rate: Option<Decimal>,
    pub change_rate: Option<Decimal>,
    pub ytd_change_rate: Option<Decimal>,
    pub five_day_change_rate: Option<Decimal>,
    pub ten_day_change_rate: Option<Decimal>,
    pub half_year_change_rate: Option<Decimal>,
    pub total_market_value: Option<Decimal>,
    pub capital_flow: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedMarketTemperature {
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub temperature: Decimal,
    pub valuation: Decimal,
    pub sentiment: Decimal,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedIntradayLine {
    pub symbol: Symbol,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub price: Decimal,
    pub volume: i64,
    pub turnover: Decimal,
    pub avg_price: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedOptionSurface {
    pub underlying: Symbol,
    pub expiry_label: String,
    pub atm_call_iv: Option<Decimal>,
    pub atm_put_iv: Option<Decimal>,
    pub put_call_skew: Option<Decimal>,
    pub total_call_oi: i64,
    pub total_put_oi: i64,
    pub put_call_oi_ratio: Option<Decimal>,
    pub atm_delta: Option<Decimal>,
    pub atm_vega: Option<Decimal>,
}

// ── Tick Archive ──

/// Single broker queue entry — who is at which position on which side.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchivedBrokerEntry {
    pub symbol: Symbol,
    pub broker_id: i32,
    pub side: String, // "bid" or "ask"
    pub position: i32,
}

/// Full-fidelity archive of one tick's market data.
/// Stored separately from TickRecord to keep the hot path (TickHistory ring buffer) lean.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickArchive {
    #[serde(default)]
    pub market: String,
    pub tick_number: u64,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub calc_indexes: Vec<ArchivedCalcIndex>,
    pub order_books: Vec<ArchivedOrderBook>,
    pub candlesticks: Vec<ArchivedCandlestick>,
    pub trades: Vec<ArchivedTrade>,
    pub capital_flows: Vec<ArchivedCapitalFlowSeries>,
    pub capital_distributions: Vec<ArchivedCapitalDistribution>,
    pub quotes: Vec<ArchivedQuote>,
    pub intraday: Vec<ArchivedIntradayLine>,
    pub option_surfaces: Vec<ArchivedOptionSurface>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_temperature: Option<ArchivedMarketTemperature>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub broker_queues: Vec<ArchivedBrokerEntry>,
}

impl TickArchive {
    /// Convert a RawSnapshot into a full-fidelity archive.
    /// Pure function — no I/O, fully testable.
    pub fn from_raw(tick_number: u64, raw: &RawSnapshot) -> Self {
        Self::from_raw_for_market("unknown", tick_number, raw)
    }

    /// Convert a RawSnapshot into a full-fidelity market-scoped archive.
    /// Production writers must use this so HK and US tick numbers cannot
    /// collide in persistence/replay storage.
    pub fn from_raw_for_market(
        market: impl Into<String>,
        tick_number: u64,
        raw: &RawSnapshot,
    ) -> Self {
        let timestamp = raw.timestamp;

        let calc_indexes = archive_calc_indexes(raw);
        let order_books = archive_order_books(raw, timestamp);
        let candlesticks = archive_candlesticks(raw, timestamp);
        let trades = archive_trades(raw);
        let capital_flows = archive_capital_flows(raw);
        let capital_distributions = archive_capital_distributions(raw, timestamp);
        let quotes = archive_quotes(raw);
        let intraday = archive_intraday_lines(raw);
        let option_surfaces = archive_option_surfaces(raw);
        let market_temperature = archive_market_temperature(raw);
        let broker_queues = archive_broker_queues(raw);

        TickArchive {
            market: market.into(),
            tick_number,
            timestamp,
            calc_indexes,
            order_books,
            candlesticks,
            trades,
            capital_flows,
            capital_distributions,
            quotes,
            intraday,
            option_surfaces,
            market_temperature,
            broker_queues,
        }
    }
}

// ── Conversion functions ──

fn archive_order_books(raw: &RawSnapshot, timestamp: OffsetDateTime) -> Vec<ArchivedOrderBook> {
    raw.depths
        .iter()
        .map(|(symbol, depth)| {
            let ask_levels = depth
                .asks
                .iter()
                .map(|d| ArchivedDepthLevel {
                    position: d.position,
                    price: d.price,
                    volume: d.volume,
                    order_num: d.order_num,
                })
                .collect();
            let bid_levels = depth
                .bids
                .iter()
                .map(|d| ArchivedDepthLevel {
                    position: d.position,
                    price: d.price,
                    volume: d.volume,
                    order_num: d.order_num,
                })
                .collect();
            ArchivedOrderBook {
                symbol: symbol.clone(),
                timestamp,
                ask_levels,
                bid_levels,
            }
        })
        .collect()
}

fn archive_candlesticks(raw: &RawSnapshot, _timestamp: OffsetDateTime) -> Vec<ArchivedCandlestick> {
    raw.candlesticks
        .iter()
        .flat_map(|(symbol, candles)| {
            candles.iter().map(move |c| ArchivedCandlestick {
                symbol: symbol.clone(),
                timestamp: c.timestamp,
                period: CandlePeriod::Min1, // WebSocket push candles are 1m by default
                session: TradeSession::from_longport(c.trade_session),
                open: c.open,
                high: c.high,
                low: c.low,
                close: c.close,
                volume: c.volume,
                turnover: c.turnover,
            })
        })
        .collect()
}

fn archive_trades(raw: &RawSnapshot) -> Vec<ArchivedTrade> {
    raw.trades
        .iter()
        .flat_map(|(symbol, trades)| {
            trades.iter().map(move |t| ArchivedTrade {
                symbol: symbol.clone(),
                timestamp: t.timestamp,
                price: t.price,
                volume: t.volume,
                direction: ArchivedTradeDirection::from_longport(t.direction),
                session: TradeSession::from_longport(t.trade_session),
                trade_type: t.trade_type.clone(),
            })
        })
        .collect()
}

fn archive_capital_flows(raw: &RawSnapshot) -> Vec<ArchivedCapitalFlowSeries> {
    raw.capital_flows
        .iter()
        .map(|(symbol, lines)| {
            let points = lines
                .iter()
                .map(|line| ArchivedCapitalFlowPoint {
                    timestamp: line.timestamp,
                    inflow: line.inflow,
                })
                .collect();
            ArchivedCapitalFlowSeries {
                symbol: symbol.clone(),
                points,
            }
        })
        .collect()
}

fn archive_capital_distributions(
    raw: &RawSnapshot,
    timestamp: OffsetDateTime,
) -> Vec<ArchivedCapitalDistribution> {
    raw.capital_distributions
        .iter()
        .map(|(symbol, dist)| ArchivedCapitalDistribution {
            symbol: symbol.clone(),
            timestamp,
            large_in: dist.capital_in.large,
            large_out: dist.capital_out.large,
            medium_in: dist.capital_in.medium,
            medium_out: dist.capital_out.medium,
            small_in: dist.capital_in.small,
            small_out: dist.capital_out.small,
        })
        .collect()
}

fn archive_quotes(raw: &RawSnapshot) -> Vec<ArchivedQuote> {
    raw.quotes
        .iter()
        .map(|(symbol, q)| {
            let convert_extended = |ppq: &longport::quote::PrePostQuote| -> ExtendedQuote {
                ExtendedQuote {
                    last_done: ppq.last_done,
                    timestamp: ppq.timestamp,
                    volume: ppq.volume,
                    turnover: ppq.turnover,
                    high: ppq.high,
                    low: ppq.low,
                    prev_close: ppq.prev_close,
                }
            };

            ArchivedQuote {
                symbol: symbol.clone(),
                timestamp: q.timestamp,
                last_done: q.last_done,
                prev_close: q.prev_close,
                open: q.open,
                high: q.high,
                low: q.low,
                volume: q.volume,
                turnover: q.turnover,
                pre_market: q.pre_market_quote.as_ref().map(convert_extended),
                post_market: q.post_market_quote.as_ref().map(convert_extended),
                overnight: q.overnight_quote.as_ref().map(convert_extended),
            }
        })
        .collect()
}

fn archive_intraday_lines(raw: &RawSnapshot) -> Vec<ArchivedIntradayLine> {
    raw.intraday_lines
        .iter()
        .flat_map(|(symbol, lines)| {
            lines.iter().map(move |line| ArchivedIntradayLine {
                symbol: symbol.clone(),
                timestamp: line.timestamp,
                price: line.price,
                volume: line.volume,
                turnover: line.turnover,
                avg_price: line.avg_price,
            })
        })
        .collect()
}

fn archive_option_surfaces(raw: &RawSnapshot) -> Vec<ArchivedOptionSurface> {
    raw.option_surfaces
        .iter()
        .map(|surface| ArchivedOptionSurface {
            underlying: surface.underlying.clone(),
            expiry_label: surface.expiry_label.clone(),
            atm_call_iv: surface.atm_call_iv,
            atm_put_iv: surface.atm_put_iv,
            put_call_skew: surface.put_call_skew,
            total_call_oi: surface.total_call_oi,
            total_put_oi: surface.total_put_oi,
            put_call_oi_ratio: surface.put_call_oi_ratio,
            atm_delta: surface.atm_delta,
            atm_vega: surface.atm_vega,
        })
        .collect()
}

fn archive_calc_indexes(raw: &RawSnapshot) -> Vec<ArchivedCalcIndex> {
    raw.calc_indexes
        .iter()
        .map(|(symbol, idx)| ArchivedCalcIndex {
            symbol: symbol.clone(),
            turnover_rate: idx.turnover_rate,
            volume_ratio: idx.volume_ratio,
            pe_ttm_ratio: idx.pe_ttm_ratio,
            pb_ratio: idx.pb_ratio,
            dividend_ratio_ttm: idx.dividend_ratio_ttm,
            amplitude: idx.amplitude,
            five_minutes_change_rate: idx.five_minutes_change_rate,
            change_rate: idx.change_rate,
            ytd_change_rate: idx.ytd_change_rate,
            five_day_change_rate: idx.five_day_change_rate,
            ten_day_change_rate: idx.ten_day_change_rate,
            half_year_change_rate: idx.half_year_change_rate,
            total_market_value: idx.total_market_value,
            capital_flow: idx.capital_flow,
        })
        .collect()
}

fn archive_market_temperature(raw: &RawSnapshot) -> Option<ArchivedMarketTemperature> {
    raw.market_temperature
        .as_ref()
        .map(|temp| ArchivedMarketTemperature {
            timestamp: temp.timestamp,
            temperature: Decimal::from(temp.temperature),
            valuation: Decimal::from(temp.valuation),
            sentiment: Decimal::from(temp.sentiment),
            description: temp.description.clone(),
        })
}

fn archive_broker_queues(raw: &RawSnapshot) -> Vec<ArchivedBrokerEntry> {
    let mut entries = Vec::new();
    for (symbol, sec_brokers) in &raw.brokers {
        for broker_group in &sec_brokers.ask_brokers {
            for &broker_id in &broker_group.broker_ids {
                entries.push(ArchivedBrokerEntry {
                    symbol: symbol.clone(),
                    broker_id,
                    side: "ask".into(),
                    position: broker_group.position,
                });
            }
        }
        for broker_group in &sec_brokers.bid_brokers {
            for &broker_id in &broker_group.broker_ids {
                entries.push(ArchivedBrokerEntry {
                    symbol: symbol.clone(),
                    broker_id,
                    side: "bid".into(),
                    position: broker_group.position,
                });
            }
        }
    }
    entries
}

// ── Diff capability ──

/// Describes how a single depth level changed between ticks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LevelChange {
    Added {
        position: i32,
        price: Option<Decimal>,
        volume: i64,
    },
    Removed {
        position: i32,
        price: Option<Decimal>,
        prev_volume: i64,
    },
    VolumeChanged {
        position: i32,
        price: Option<Decimal>,
        prev_volume: i64,
        new_volume: i64,
    },
}

/// Delta between two order book snapshots for the same symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrderBookDelta {
    pub symbol: Symbol,
    pub bid_changes: Vec<LevelChange>,
    pub ask_changes: Vec<LevelChange>,
    pub spread_change: Option<(Decimal, Decimal)>, // (old, new)
}

/// Delta in capital flow between ticks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapitalFlowDelta {
    pub symbol: Symbol,
    /// New data points since last tick
    pub new_point_count: usize,
    /// Latest cumulative inflow
    pub latest_inflow: Decimal,
    /// Inflow velocity (change per minute)
    pub velocity: Decimal,
    /// Velocity acceleration (change in velocity)
    pub acceleration: Decimal,
}

/// Container for all microstructure deltas in one tick.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MicrostructureDeltas {
    pub order_book_deltas: Vec<OrderBookDelta>,
    pub capital_flow_deltas: Vec<CapitalFlowDelta>,
    pub new_trades: Vec<ArchivedTrade>,
}

impl ArchivedOrderBook {
    /// Compute the delta between this order book and a previous one.
    pub fn diff(&self, previous: &ArchivedOrderBook) -> OrderBookDelta {
        let bid_changes = diff_levels(&previous.bid_levels, &self.bid_levels);
        let ask_changes = diff_levels(&previous.ask_levels, &self.ask_levels);

        let old_spread = compute_spread(&previous.ask_levels, &previous.bid_levels);
        let new_spread = compute_spread(&self.ask_levels, &self.bid_levels);
        let spread_change = match (old_spread, new_spread) {
            (Some(old), Some(new)) if old != new => Some((old, new)),
            _ => None,
        };

        OrderBookDelta {
            symbol: self.symbol.clone(),
            bid_changes,
            ask_changes,
            spread_change,
        }
    }
}

fn compute_spread(asks: &[ArchivedDepthLevel], bids: &[ArchivedDepthLevel]) -> Option<Decimal> {
    let best_ask = asks.iter().filter_map(|l| l.price).min()?;
    let best_bid = bids.iter().filter_map(|l| l.price).max()?;
    Some(best_ask - best_bid)
}

fn diff_levels(old: &[ArchivedDepthLevel], new: &[ArchivedDepthLevel]) -> Vec<LevelChange> {
    let mut changes = Vec::new();

    let old_map: std::collections::HashMap<i32, &ArchivedDepthLevel> =
        old.iter().map(|l| (l.position, l)).collect();
    let new_map: std::collections::HashMap<i32, &ArchivedDepthLevel> =
        new.iter().map(|l| (l.position, l)).collect();

    // Check for changes and additions
    for (pos, new_level) in &new_map {
        match old_map.get(pos) {
            Some(old_level) if old_level.volume != new_level.volume => {
                changes.push(LevelChange::VolumeChanged {
                    position: *pos,
                    price: new_level.price,
                    prev_volume: old_level.volume,
                    new_volume: new_level.volume,
                });
            }
            None => {
                changes.push(LevelChange::Added {
                    position: *pos,
                    price: new_level.price,
                    volume: new_level.volume,
                });
            }
            _ => {} // unchanged
        }
    }

    // Check for removals
    for (pos, old_level) in &old_map {
        if !new_map.contains_key(pos) {
            changes.push(LevelChange::Removed {
                position: *pos,
                price: old_level.price,
                prev_volume: old_level.volume,
            });
        }
    }

    changes.sort_by_key(|c| match c {
        LevelChange::Added { position, .. } => *position,
        LevelChange::Removed { position, .. } => *position,
        LevelChange::VolumeChanged { position, .. } => *position,
    });

    changes
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
    }

    #[test]
    fn empty_raw_snapshot_produces_empty_archive() {
        let raw = RawSnapshot::empty();
        let archive = TickArchive::from_raw(0, &raw);
        assert!(archive.order_books.is_empty());
        assert!(archive.candlesticks.is_empty());
        assert!(archive.trades.is_empty());
        assert!(archive.capital_flows.is_empty());
        assert!(archive.capital_distributions.is_empty());
        assert!(archive.quotes.is_empty());
        assert_eq!(archive.market, "unknown");
    }

    #[test]
    fn market_scoped_raw_archive_preserves_market() {
        let raw = RawSnapshot::empty();
        let archive = TickArchive::from_raw_for_market("us", 7, &raw);
        let json = serde_json::to_string(&archive).unwrap();
        let restored: TickArchive = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.market, "us");
        assert_eq!(restored.tick_number, 7);
    }

    #[test]
    fn archive_preserves_all_depth_levels() {
        let mut raw = RawSnapshot::empty();
        raw.depths.insert(
            sym("700.HK"),
            longport::quote::SecurityDepth {
                asks: vec![
                    longport::quote::Depth {
                        position: 1,
                        price: Some(dec!(500.0)),
                        volume: 1000,
                        order_num: 5,
                    },
                    longport::quote::Depth {
                        position: 2,
                        price: Some(dec!(500.2)),
                        volume: 2000,
                        order_num: 10,
                    },
                ],
                bids: vec![longport::quote::Depth {
                    position: 1,
                    price: Some(dec!(499.8)),
                    volume: 3000,
                    order_num: 15,
                }],
            },
        );

        let archive = TickArchive::from_raw(1, &raw);
        assert_eq!(archive.order_books.len(), 1);

        let ob = &archive.order_books[0];
        assert_eq!(ob.ask_levels.len(), 2);
        assert_eq!(ob.bid_levels.len(), 1);
        assert_eq!(ob.ask_levels[0].price, Some(dec!(500.0)));
        assert_eq!(ob.ask_levels[0].volume, 1000);
        assert_eq!(ob.ask_levels[1].volume, 2000);
        assert_eq!(ob.bid_levels[0].volume, 3000);
    }

    #[test]
    fn archive_preserves_all_capital_flow_points() {
        let mut raw = RawSnapshot::empty();
        let t1 = OffsetDateTime::UNIX_EPOCH;
        let t2 = t1 + time::Duration::minutes(1);
        let t3 = t1 + time::Duration::minutes(2);
        raw.capital_flows.insert(
            sym("700.HK"),
            vec![
                longport::quote::CapitalFlowLine {
                    inflow: dec!(100.5),
                    timestamp: t1,
                },
                longport::quote::CapitalFlowLine {
                    inflow: dec!(200.3),
                    timestamp: t2,
                },
                longport::quote::CapitalFlowLine {
                    inflow: dec!(-50.1),
                    timestamp: t3,
                },
            ],
        );

        let archive = TickArchive::from_raw(1, &raw);
        assert_eq!(archive.capital_flows.len(), 1);

        let cf = &archive.capital_flows[0];
        assert_eq!(cf.points.len(), 3); // all 3 points preserved, not just last()
        assert_eq!(cf.points[0].inflow, dec!(100.5));
        assert_eq!(cf.points[2].inflow, dec!(-50.1));
    }

    #[test]
    fn trade_session_round_trips() {
        assert_eq!(
            TradeSession::from_longport(longport::quote::TradeSession::Pre),
            TradeSession::Pre,
        );
        assert_eq!(
            TradeSession::from_longport(longport::quote::TradeSession::Post),
            TradeSession::Post,
        );
        assert_eq!(
            TradeSession::from_longport(longport::quote::TradeSession::Intraday),
            TradeSession::Normal,
        );
        assert_eq!(
            TradeSession::from_longport(longport::quote::TradeSession::Overnight),
            TradeSession::Overnight,
        );
    }

    #[test]
    fn trade_direction_round_trips() {
        assert_eq!(
            ArchivedTradeDirection::from_longport(longport::quote::TradeDirection::Up),
            ArchivedTradeDirection::Up,
        );
        assert_eq!(
            ArchivedTradeDirection::from_longport(longport::quote::TradeDirection::Down),
            ArchivedTradeDirection::Down,
        );
    }

    #[test]
    fn archive_serialization_round_trip() {
        let raw = RawSnapshot::empty();
        let archive = TickArchive::from_raw(42, &raw);
        let json = serde_json::to_string(&archive).unwrap();
        let restored: TickArchive = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.tick_number, 42);
    }

    // ── Diff tests ──

    fn make_level(position: i32, price: Decimal, volume: i64) -> ArchivedDepthLevel {
        ArchivedDepthLevel {
            position,
            price: Some(price),
            volume,
            order_num: 1,
        }
    }

    fn make_order_book(
        symbol: &str,
        asks: Vec<ArchivedDepthLevel>,
        bids: Vec<ArchivedDepthLevel>,
    ) -> ArchivedOrderBook {
        ArchivedOrderBook {
            symbol: sym(symbol),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            ask_levels: asks,
            bid_levels: bids,
        }
    }

    #[test]
    fn diff_identical_order_books_produces_no_changes() {
        let ob = make_order_book(
            "700.HK",
            vec![make_level(1, dec!(500.0), 1000)],
            vec![make_level(1, dec!(499.0), 2000)],
        );
        let delta = ob.diff(&ob);
        assert!(delta.bid_changes.is_empty());
        assert!(delta.ask_changes.is_empty());
        assert!(delta.spread_change.is_none());
    }

    #[test]
    fn diff_detects_volume_change() {
        let old = make_order_book(
            "700.HK",
            vec![make_level(1, dec!(500.0), 1000)],
            vec![make_level(1, dec!(499.0), 2000)],
        );
        let new = make_order_book(
            "700.HK",
            vec![make_level(1, dec!(500.0), 1500)],
            vec![make_level(1, dec!(499.0), 2000)],
        );
        let delta = new.diff(&old);
        assert_eq!(delta.ask_changes.len(), 1);
        assert!(delta.bid_changes.is_empty());
        match &delta.ask_changes[0] {
            LevelChange::VolumeChanged {
                prev_volume,
                new_volume,
                ..
            } => {
                assert_eq!(*prev_volume, 1000);
                assert_eq!(*new_volume, 1500);
            }
            other => panic!("expected VolumeChanged, got {:?}", other),
        }
    }

    #[test]
    fn diff_detects_addition_and_removal() {
        let old = make_order_book(
            "700.HK",
            vec![make_level(1, dec!(500.0), 1000)],
            vec![make_level(1, dec!(499.0), 2000)],
        );
        let new = make_order_book(
            "700.HK",
            vec![make_level(2, dec!(500.2), 800)],
            vec![make_level(1, dec!(499.0), 2000)],
        );
        let delta = new.diff(&old);
        // Position 1 removed, position 2 added
        assert_eq!(delta.ask_changes.len(), 2);
        let has_removed = delta
            .ask_changes
            .iter()
            .any(|c| matches!(c, LevelChange::Removed { position: 1, .. }));
        let has_added = delta
            .ask_changes
            .iter()
            .any(|c| matches!(c, LevelChange::Added { position: 2, .. }));
        assert!(has_removed, "expected a Removed change for position 1");
        assert!(has_added, "expected an Added change for position 2");
    }
}
