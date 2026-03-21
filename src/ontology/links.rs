use std::collections::HashMap;

use rust_decimal::Decimal;
use time::OffsetDateTime;

use super::objects::{BrokerId, InstitutionId, Symbol};
use super::snapshot::RawSnapshot;
use super::store::ObjectStore;

// ── Link types ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Ask,
    Bid,
}

#[derive(Debug, Clone)]
pub struct BrokerQueueEntry {
    pub symbol: Symbol,
    pub broker_id: BrokerId,
    pub side: Side,
    pub position: i32,
}

#[derive(Debug, Clone)]
pub struct InstitutionActivity {
    pub symbol: Symbol,
    pub institution_id: InstitutionId,
    pub ask_positions: Vec<i32>,
    pub bid_positions: Vec<i32>,
    pub seat_count: usize,
}

#[derive(Debug, Clone)]
pub struct CrossStockPresence {
    pub institution_id: InstitutionId,
    pub symbols: Vec<Symbol>,
    pub ask_symbols: Vec<Symbol>,
    pub bid_symbols: Vec<Symbol>,
}

#[derive(Debug, Clone)]
pub struct CapitalFlow {
    pub symbol: Symbol,
    pub net_inflow: Decimal,
}

#[derive(Debug, Clone)]
pub struct CapitalBreakdown {
    pub symbol: Symbol,
    pub large_net: Decimal,
    pub medium_net: Decimal,
    pub small_net: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketStatus {
    Normal,
    Halted,
    SuspendTrade,
    ToBeOpened,
    Other,
}

#[derive(Debug, Clone)]
pub struct DepthLevel {
    pub position: i32,
    pub price: Option<Decimal>,
    pub volume: i64,
    pub order_num: i64,
}

#[derive(Debug, Clone)]
pub struct DepthProfile {
    /// Volume concentration in top 3 levels vs total (0..1). High = wall at top, low = distributed.
    pub top3_volume_ratio: Decimal,
    /// Weighted average distance from best price (volume-weighted), in price units.
    pub volume_weighted_distance: Decimal,
    /// Volume at best level / total volume (0..1). High = large wall at best.
    pub best_level_ratio: Decimal,
    /// Number of levels with nonzero volume.
    pub active_levels: usize,
}

impl DepthProfile {
    pub fn empty() -> Self {
        DepthProfile {
            top3_volume_ratio: Decimal::ZERO,
            volume_weighted_distance: Decimal::ZERO,
            best_level_ratio: Decimal::ZERO,
            active_levels: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OrderBookObservation {
    pub symbol: Symbol,
    pub ask_levels: Vec<DepthLevel>,
    pub bid_levels: Vec<DepthLevel>,
    pub total_ask_volume: i64,
    pub total_bid_volume: i64,
    pub total_ask_orders: i64,
    pub total_bid_orders: i64,
    pub spread: Option<Decimal>,
    pub ask_level_count: usize,
    pub bid_level_count: usize,
    pub bid_profile: DepthProfile,
    pub ask_profile: DepthProfile,
}

#[derive(Debug, Clone)]
pub struct QuoteObservation {
    pub symbol: Symbol,
    pub last_done: Decimal,
    pub prev_close: Decimal,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub volume: i64,
    pub turnover: Decimal,
    pub timestamp: OffsetDateTime,
    pub market_status: MarketStatus,
}

#[derive(Debug, Clone)]
pub struct CalcIndexObservation {
    pub symbol: Symbol,
    pub turnover_rate: Option<Decimal>,
    pub volume_ratio: Option<Decimal>,
    pub pe_ttm_ratio: Option<Decimal>,
    pub pb_ratio: Option<Decimal>,
    pub dividend_ratio_ttm: Option<Decimal>,
    pub amplitude: Option<Decimal>,
    pub five_minutes_change_rate: Option<Decimal>,
}

#[derive(Debug, Clone)]
pub struct CandlestickObservation {
    pub symbol: Symbol,
    pub candle_count: usize,
    pub window_return: Decimal,
    pub body_bias: Decimal,
    pub volume_ratio: Decimal,
    pub range_ratio: Decimal,
}

#[derive(Debug, Clone)]
pub struct MarketTemperatureObservation {
    pub temperature: Decimal,
    pub valuation: Decimal,
    pub sentiment: Decimal,
    pub description: String,
    pub timestamp: OffsetDateTime,
}

// ── Trade types ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeDirection {
    Up,
    Down,
    Neutral,
}

#[derive(Debug, Clone)]
pub struct TradeRecord {
    pub price: Decimal,
    pub volume: i64,
    pub timestamp: OffsetDateTime,
    pub direction: TradeDirection,
}

/// Aggregated trade activity for a symbol from recent tick data.
#[derive(Debug, Clone)]
pub struct TradeActivity {
    pub symbol: Symbol,
    pub trade_count: usize,
    pub total_volume: i64,
    pub buy_volume: i64,
    pub sell_volume: i64,
    pub neutral_volume: i64,
    pub vwap: Decimal,
    pub last_price: Option<Decimal>,
    pub trades: Vec<TradeRecord>,
}

// ── LinkSnapshot ──

#[derive(Debug)]
pub struct LinkSnapshot {
    pub timestamp: OffsetDateTime,
    pub broker_queues: Vec<BrokerQueueEntry>,
    pub calc_indexes: Vec<CalcIndexObservation>,
    pub candlesticks: Vec<CandlestickObservation>,
    pub institution_activities: Vec<InstitutionActivity>,
    pub cross_stock_presences: Vec<CrossStockPresence>,
    pub capital_flows: Vec<CapitalFlow>,
    pub capital_breakdowns: Vec<CapitalBreakdown>,
    pub market_temperature: Option<MarketTemperatureObservation>,
    pub order_books: Vec<OrderBookObservation>,
    pub quotes: Vec<QuoteObservation>,
    pub trade_activities: Vec<TradeActivity>,
}

impl LinkSnapshot {
    /// Compute all links from a raw API snapshot + the object store.
    /// Pure synchronous function — no I/O, fully testable with synthetic data.
    pub fn compute(raw: &RawSnapshot, store: &ObjectStore) -> Self {
        let broker_queues = compute_broker_queues(raw);
        let calc_indexes = compute_calc_indexes(raw);
        let candlesticks = compute_candlesticks(raw);
        let institution_activities = compute_institution_activities(&broker_queues, store);
        let cross_stock_presences = compute_cross_stock_presences(&institution_activities);
        let capital_flows = compute_capital_flows(raw);
        let capital_breakdowns = compute_capital_breakdowns(raw);
        let market_temperature = compute_market_temperature(raw);
        let order_books = compute_order_books(raw);
        let quotes = compute_quotes(raw);
        let trade_activities = compute_trade_activities(raw);

        LinkSnapshot {
            timestamp: raw.timestamp,
            broker_queues,
            calc_indexes,
            candlesticks,
            institution_activities,
            cross_stock_presences,
            capital_flows,
            capital_breakdowns,
            market_temperature,
            order_books,
            quotes,
            trade_activities,
        }
    }
}

// ── Computation functions ──

/// Expand SecurityBrokers into flat BrokerQueueEntry records.
/// Each (symbol, broker_id, side, position) is one entry.
fn compute_broker_queues(raw: &RawSnapshot) -> Vec<BrokerQueueEntry> {
    let mut entries = Vec::new();

    for (symbol, sec_brokers) in &raw.brokers {
        for broker_group in &sec_brokers.ask_brokers {
            for &broker_id in &broker_group.broker_ids {
                entries.push(BrokerQueueEntry {
                    symbol: symbol.clone(),
                    broker_id: BrokerId(broker_id),
                    side: Side::Ask,
                    position: broker_group.position,
                });
            }
        }
        for broker_group in &sec_brokers.bid_brokers {
            for &broker_id in &broker_group.broker_ids {
                entries.push(BrokerQueueEntry {
                    symbol: symbol.clone(),
                    broker_id: BrokerId(broker_id),
                    side: Side::Bid,
                    position: broker_group.position,
                });
            }
        }
    }

    entries
}

/// Group broker queue entries by (symbol, institution) and aggregate positions.
/// Unknown brokers (not in the object store) are silently skipped.
fn compute_institution_activities(
    broker_queues: &[BrokerQueueEntry],
    store: &ObjectStore,
) -> Vec<InstitutionActivity> {
    // Key: (Symbol, InstitutionId) → (ask_positions, bid_positions, unique broker_ids)
    let mut map: HashMap<
        (Symbol, InstitutionId),
        (Vec<i32>, Vec<i32>, std::collections::HashSet<BrokerId>),
    > = HashMap::new();

    for entry in broker_queues {
        let institution_id = match store.broker_to_institution.get(&entry.broker_id) {
            Some(&iid) => iid,
            None => continue, // unknown broker, skip
        };

        let key = (entry.symbol.clone(), institution_id);
        let record = map
            .entry(key)
            .or_insert_with(|| (Vec::new(), Vec::new(), std::collections::HashSet::new()));

        match entry.side {
            Side::Ask => record.0.push(entry.position),
            Side::Bid => record.1.push(entry.position),
        }
        record.2.insert(entry.broker_id);
    }

    map.into_iter()
        .map(
            |((symbol, institution_id), (ask_positions, bid_positions, broker_ids))| {
                InstitutionActivity {
                    symbol,
                    institution_id,
                    ask_positions,
                    bid_positions,
                    seat_count: broker_ids.len(),
                }
            },
        )
        .collect()
}

/// Find institutions present in ≥2 stocks.
fn compute_cross_stock_presences(activities: &[InstitutionActivity]) -> Vec<CrossStockPresence> {
    // Group by institution_id
    let mut map: HashMap<InstitutionId, (Vec<Symbol>, Vec<Symbol>, Vec<Symbol>)> = HashMap::new();

    for act in activities {
        let entry = map
            .entry(act.institution_id)
            .or_insert_with(|| (Vec::new(), Vec::new(), Vec::new()));
        // Add to overall symbols list (deduplicated below)
        if !entry.0.contains(&act.symbol) {
            entry.0.push(act.symbol.clone());
        }
        if !act.ask_positions.is_empty() && !entry.1.contains(&act.symbol) {
            entry.1.push(act.symbol.clone());
        }
        if !act.bid_positions.is_empty() && !entry.2.contains(&act.symbol) {
            entry.2.push(act.symbol.clone());
        }
    }

    map.into_iter()
        .filter(|(_, (symbols, _, _))| symbols.len() >= 2)
        .map(
            |(institution_id, (symbols, ask_symbols, bid_symbols))| CrossStockPresence {
                institution_id,
                symbols,
                ask_symbols,
                bid_symbols,
            },
        )
        .collect()
}

/// Extract the latest capital flow entry for each symbol.
/// Longport inflow is in 萬元, turnover is in 元 — multiply by 10000 to align.
fn compute_capital_flows(raw: &RawSnapshot) -> Vec<CapitalFlow> {
    let scale = rust_decimal::Decimal::from(10000);
    raw.capital_flows
        .iter()
        .filter_map(|(symbol, lines)| {
            lines.last().map(|line| CapitalFlow {
                symbol: symbol.clone(),
                net_inflow: line.inflow * scale,
            })
        })
        .collect()
}

fn compute_calc_indexes(raw: &RawSnapshot) -> Vec<CalcIndexObservation> {
    raw.calc_indexes
        .iter()
        .map(|(symbol, idx)| CalcIndexObservation {
            symbol: symbol.clone(),
            turnover_rate: idx.turnover_rate,
            volume_ratio: idx.volume_ratio,
            pe_ttm_ratio: idx.pe_ttm_ratio,
            pb_ratio: idx.pb_ratio,
            dividend_ratio_ttm: idx.dividend_ratio_ttm,
            amplitude: idx.amplitude,
            five_minutes_change_rate: idx.five_minutes_change_rate,
        })
        .collect()
}

/// Compute net capital by size tier for each symbol.
fn compute_capital_breakdowns(raw: &RawSnapshot) -> Vec<CapitalBreakdown> {
    raw.capital_distributions
        .iter()
        .map(|(symbol, dist)| CapitalBreakdown {
            symbol: symbol.clone(),
            large_net: dist.capital_in.large - dist.capital_out.large,
            medium_net: dist.capital_in.medium - dist.capital_out.medium,
            small_net: dist.capital_in.small - dist.capital_out.small,
        })
        .collect()
}

fn clamp_unit(value: Decimal) -> Decimal {
    value.clamp(-Decimal::ONE, Decimal::ONE)
}

fn compute_candlesticks(raw: &RawSnapshot) -> Vec<CandlestickObservation> {
    raw.candlesticks
        .iter()
        .filter_map(|(symbol, candles)| {
            let latest = candles.last()?;
            let first = candles
                .iter()
                .rev()
                .take(5)
                .last()
                .copied()
                .unwrap_or(*latest);

            let window_high = candles
                .iter()
                .rev()
                .take(5)
                .map(|c| c.high)
                .max()
                .unwrap_or(latest.high);
            let window_low = candles
                .iter()
                .rev()
                .take(5)
                .map(|c| c.low)
                .min()
                .unwrap_or(latest.low);

            let window_return = if first.open > Decimal::ZERO {
                clamp_unit((latest.close - first.open) / first.open / Decimal::new(2, 2))
            } else {
                Decimal::ZERO
            };

            let latest_range = latest.high - latest.low;
            let body_bias = if latest_range > Decimal::ZERO {
                clamp_unit((latest.close - latest.open) / latest_range)
            } else {
                Decimal::ZERO
            };

            let recent = candles.iter().rev().take(5).collect::<Vec<_>>();
            let average_volume = if recent.is_empty() {
                Decimal::ZERO
            } else {
                Decimal::from(recent.iter().map(|c| c.volume).sum::<i64>())
                    / Decimal::from(recent.len() as i64)
            };
            let volume_ratio = if average_volume > Decimal::ZERO {
                Decimal::from(latest.volume) / average_volume
            } else {
                Decimal::ZERO
            };

            let range_ratio = if first.open > Decimal::ZERO {
                clamp_unit((window_high - window_low) / first.open / Decimal::new(8, 2))
            } else {
                Decimal::ZERO
            };

            Some(CandlestickObservation {
                symbol: symbol.clone(),
                candle_count: candles.len(),
                window_return,
                body_bias,
                volume_ratio,
                range_ratio,
            })
        })
        .collect()
}

/// Compute a structural profile of one side (bid or ask) of the order book.
fn compute_depth_profile(levels: &[DepthLevel], best_price: Option<Decimal>) -> DepthProfile {
    let active_levels = levels.iter().filter(|l| l.volume > 0).count();
    let total_vol: i64 = levels.iter().map(|l| l.volume).sum();

    if total_vol == 0 || levels.is_empty() {
        return DepthProfile {
            top3_volume_ratio: Decimal::ZERO,
            volume_weighted_distance: Decimal::ZERO,
            best_level_ratio: Decimal::ZERO,
            active_levels,
        };
    }

    let total_dec = Decimal::from(total_vol);

    // Top 3 volume ratio
    let top3_vol: i64 = levels.iter().take(3).map(|l| l.volume).sum();
    let top3_volume_ratio = Decimal::from(top3_vol) / total_dec;

    // Best level ratio
    let best_vol = levels.first().map(|l| l.volume).unwrap_or(0);
    let best_level_ratio = Decimal::from(best_vol) / total_dec;

    // Volume-weighted distance from best price
    let volume_weighted_distance = if let Some(bp) = best_price {
        let mut weighted_sum = Decimal::ZERO;
        for l in levels {
            if let Some(price) = l.price {
                let dist = (price - bp).abs();
                weighted_sum += dist * Decimal::from(l.volume);
            }
        }
        weighted_sum / total_dec
    } else {
        Decimal::ZERO
    };

    DepthProfile {
        top3_volume_ratio,
        volume_weighted_distance,
        best_level_ratio,
        active_levels,
    }
}

/// Convert SecurityDepth → OrderBookObservation for each symbol.
fn compute_order_books(raw: &RawSnapshot) -> Vec<OrderBookObservation> {
    raw.depths
        .iter()
        .map(|(symbol, depth)| {
            let ask_levels: Vec<DepthLevel> = depth
                .asks
                .iter()
                .map(|d| DepthLevel {
                    position: d.position,
                    price: d.price,
                    volume: d.volume,
                    order_num: d.order_num,
                })
                .collect();
            let bid_levels: Vec<DepthLevel> = depth
                .bids
                .iter()
                .map(|d| DepthLevel {
                    position: d.position,
                    price: d.price,
                    volume: d.volume,
                    order_num: d.order_num,
                })
                .collect();

            let total_ask_volume: i64 = ask_levels.iter().map(|l| l.volume).sum();
            let total_bid_volume: i64 = bid_levels.iter().map(|l| l.volume).sum();
            let total_ask_orders: i64 = ask_levels.iter().map(|l| l.order_num).sum();
            let total_bid_orders: i64 = bid_levels.iter().map(|l| l.order_num).sum();

            let best_ask = ask_levels.iter().filter_map(|l| l.price).min();
            let best_bid = bid_levels.iter().filter_map(|l| l.price).max();
            let spread = match (best_ask, best_bid) {
                (Some(a), Some(b)) => Some(a - b),
                _ => None,
            };

            let bid_profile = compute_depth_profile(&bid_levels, best_bid);
            let ask_profile = compute_depth_profile(&ask_levels, best_ask);

            OrderBookObservation {
                symbol: symbol.clone(),
                ask_levels,
                bid_levels,
                total_ask_volume,
                total_bid_volume,
                total_ask_orders,
                total_bid_orders,
                spread,
                ask_level_count: depth.asks.len(),
                bid_level_count: depth.bids.len(),
                bid_profile,
                ask_profile,
            }
        })
        .collect()
}

/// Aggregate trade ticks into TradeActivity per symbol.
fn compute_trade_activities(raw: &RawSnapshot) -> Vec<TradeActivity> {
    raw.trades
        .iter()
        .map(|(symbol, trades)| {
            let mut buy_volume: i64 = 0;
            let mut sell_volume: i64 = 0;
            let mut neutral_volume: i64 = 0;
            let mut price_volume_sum = Decimal::ZERO;
            let mut total_volume: i64 = 0;
            let mut records = Vec::with_capacity(trades.len());
            let mut last_price = None;

            for t in trades {
                total_volume += t.volume;
                price_volume_sum += t.price * Decimal::from(t.volume);

                let dir = match t.direction {
                    longport::quote::TradeDirection::Up => {
                        buy_volume += t.volume;
                        TradeDirection::Up
                    }
                    longport::quote::TradeDirection::Down => {
                        sell_volume += t.volume;
                        TradeDirection::Down
                    }
                    _ => {
                        neutral_volume += t.volume;
                        TradeDirection::Neutral
                    }
                };

                last_price = Some(t.price);
                records.push(TradeRecord {
                    price: t.price,
                    volume: t.volume,
                    timestamp: t.timestamp,
                    direction: dir,
                });
            }

            let vwap = if total_volume > 0 {
                price_volume_sum / Decimal::from(total_volume)
            } else {
                Decimal::ZERO
            };

            TradeActivity {
                symbol: symbol.clone(),
                trade_count: trades.len(),
                total_volume,
                buy_volume,
                sell_volume,
                neutral_volume,
                vwap,
                last_price,
                trades: records,
            }
        })
        .collect()
}

fn market_status_from_trade_status(status: longport::quote::TradeStatus) -> MarketStatus {
    use longport::quote::TradeStatus;
    #[allow(unreachable_patterns)] // forward-compat: new SDK variants map to Other
    match status {
        TradeStatus::Normal => MarketStatus::Normal,
        TradeStatus::Halted => MarketStatus::Halted,
        TradeStatus::Delisted => MarketStatus::Other,
        TradeStatus::Fuse => MarketStatus::Halted,
        TradeStatus::PrepareList => MarketStatus::ToBeOpened,
        TradeStatus::CodeMoved => MarketStatus::Other,
        TradeStatus::ToBeOpened => MarketStatus::ToBeOpened,
        TradeStatus::SplitStockHalts => MarketStatus::Halted,
        TradeStatus::Expired => MarketStatus::Other,
        TradeStatus::WarrantPrepareList => MarketStatus::ToBeOpened,
        TradeStatus::SuspendTrade => MarketStatus::SuspendTrade,
        _ => MarketStatus::Other,
    }
}

/// Convert SecurityQuote → QuoteObservation for each symbol.
fn compute_quotes(raw: &RawSnapshot) -> Vec<QuoteObservation> {
    raw.quotes
        .iter()
        .map(|(symbol, q)| QuoteObservation {
            symbol: symbol.clone(),
            last_done: q.last_done,
            prev_close: q.prev_close,
            open: q.open,
            high: q.high,
            low: q.low,
            volume: q.volume,
            turnover: q.turnover,
            timestamp: q.timestamp,
            market_status: market_status_from_trade_status(q.trade_status),
        })
        .collect()
}

fn compute_market_temperature(raw: &RawSnapshot) -> Option<MarketTemperatureObservation> {
    raw.market_temperature
        .as_ref()
        .map(|temp| MarketTemperatureObservation {
            temperature: Decimal::from(temp.temperature),
            valuation: Decimal::from(temp.valuation),
            sentiment: Decimal::from(temp.sentiment),
            description: temp.description.clone(),
            timestamp: temp.timestamp,
        })
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::super::objects::{Institution, InstitutionClass};
    use super::*;
    use longport::quote::{
        Brokers, CapitalDistribution, CapitalDistributionResponse, CapitalFlowLine,
        Depth as LPDepth, SecurityBrokers, SecurityDepth, SecurityQuote, TradeStatus,
    };

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
    }

    fn make_raw_with_brokers(data: Vec<(Symbol, SecurityBrokers)>) -> RawSnapshot {
        RawSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            brokers: data.into_iter().collect(),
            calc_indexes: HashMap::new(),
            candlesticks: HashMap::new(),
            capital_flows: HashMap::new(),
            capital_distributions: HashMap::new(),
            depths: HashMap::new(),
            market_temperature: None,
            quotes: HashMap::new(),
            trades: HashMap::new(),
        }
    }

    fn make_store_with_institutions(institutions: Vec<(i32, &[i32])>) -> ObjectStore {
        let insts: Vec<Institution> = institutions
            .into_iter()
            .map(|(min_id, broker_ids)| Institution {
                id: InstitutionId(min_id),
                name_en: format!("Inst{}", min_id),
                name_cn: String::new(),
                name_hk: String::new(),
                broker_ids: broker_ids.iter().map(|&i| BrokerId(i)).collect(),
                class: InstitutionClass::Unknown,
            })
            .collect();

        ObjectStore::from_parts(insts, vec![], vec![])
    }

    // ── broker_queue tests ──

    #[test]
    fn broker_queue_basic() {
        let raw = make_raw_with_brokers(vec![(
            sym("700.HK"),
            SecurityBrokers {
                ask_brokers: vec![Brokers {
                    position: 1,
                    broker_ids: vec![100],
                }],
                bid_brokers: vec![],
            },
        )]);

        let entries = compute_broker_queues(&raw);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].broker_id, BrokerId(100));
        assert_eq!(entries[0].side, Side::Ask);
        assert_eq!(entries[0].position, 1);
        assert_eq!(entries[0].symbol, sym("700.HK"));
    }

    #[test]
    fn broker_queue_multiple_at_same_position() {
        let raw = make_raw_with_brokers(vec![(
            sym("700.HK"),
            SecurityBrokers {
                ask_brokers: vec![Brokers {
                    position: 1,
                    broker_ids: vec![100, 200, 300],
                }],
                bid_brokers: vec![],
            },
        )]);

        let entries = compute_broker_queues(&raw);
        assert_eq!(entries.len(), 3);
        assert!(entries
            .iter()
            .all(|e| e.position == 1 && e.side == Side::Ask));
    }

    #[test]
    fn broker_queue_both_sides() {
        let raw = make_raw_with_brokers(vec![(
            sym("700.HK"),
            SecurityBrokers {
                ask_brokers: vec![Brokers {
                    position: 1,
                    broker_ids: vec![100],
                }],
                bid_brokers: vec![Brokers {
                    position: 2,
                    broker_ids: vec![100],
                }],
            },
        )]);

        let entries = compute_broker_queues(&raw);
        assert_eq!(entries.len(), 2);
        let ask = entries.iter().find(|e| e.side == Side::Ask).unwrap();
        let bid = entries.iter().find(|e| e.side == Side::Bid).unwrap();
        assert_eq!(ask.broker_id, BrokerId(100));
        assert_eq!(bid.broker_id, BrokerId(100));
        assert_eq!(ask.position, 1);
        assert_eq!(bid.position, 2);
    }

    #[test]
    fn broker_queue_empty() {
        let raw = make_raw_with_brokers(vec![(
            sym("700.HK"),
            SecurityBrokers {
                ask_brokers: vec![],
                bid_brokers: vec![],
            },
        )]);

        let entries = compute_broker_queues(&raw);
        assert!(entries.is_empty());
    }

    // ── institution_activity tests ──

    #[test]
    fn institution_activity_aggregation() {
        // Institution 100 has 3 seats: 100, 101, 102
        let store = make_store_with_institutions(vec![(100, &[100, 101, 102])]);

        let raw = make_raw_with_brokers(vec![(
            sym("700.HK"),
            SecurityBrokers {
                ask_brokers: vec![
                    Brokers {
                        position: 1,
                        broker_ids: vec![100],
                    },
                    Brokers {
                        position: 3,
                        broker_ids: vec![101],
                    },
                ],
                bid_brokers: vec![Brokers {
                    position: 2,
                    broker_ids: vec![102],
                }],
            },
        )]);

        let queues = compute_broker_queues(&raw);
        let activities = compute_institution_activities(&queues, &store);
        assert_eq!(activities.len(), 1);

        let act = &activities[0];
        assert_eq!(act.institution_id, InstitutionId(100));
        assert_eq!(act.seat_count, 3);
        assert_eq!(act.ask_positions.len(), 2);
        assert_eq!(act.bid_positions.len(), 1);
    }

    #[test]
    fn institution_activity_unknown_broker() {
        // Store only knows broker 100, not 999
        let store = make_store_with_institutions(vec![(100, &[100])]);

        let raw = make_raw_with_brokers(vec![(
            sym("700.HK"),
            SecurityBrokers {
                ask_brokers: vec![Brokers {
                    position: 1,
                    broker_ids: vec![100, 999],
                }],
                bid_brokers: vec![],
            },
        )]);

        let queues = compute_broker_queues(&raw);
        let activities = compute_institution_activities(&queues, &store);
        // Only broker 100 is recognized → 1 activity with seat_count=1
        assert_eq!(activities.len(), 1);
        assert_eq!(activities[0].seat_count, 1);
    }

    // ── cross_stock tests ──

    #[test]
    fn cross_stock_two_stocks() {
        let store = make_store_with_institutions(vec![(100, &[100])]);

        let raw = make_raw_with_brokers(vec![
            (
                sym("700.HK"),
                SecurityBrokers {
                    ask_brokers: vec![Brokers {
                        position: 1,
                        broker_ids: vec![100],
                    }],
                    bid_brokers: vec![],
                },
            ),
            (
                sym("9988.HK"),
                SecurityBrokers {
                    ask_brokers: vec![],
                    bid_brokers: vec![Brokers {
                        position: 1,
                        broker_ids: vec![100],
                    }],
                },
            ),
        ]);

        let queues = compute_broker_queues(&raw);
        let activities = compute_institution_activities(&queues, &store);
        let cross = compute_cross_stock_presences(&activities);
        assert_eq!(cross.len(), 1);
        assert_eq!(cross[0].institution_id, InstitutionId(100));
        assert_eq!(cross[0].symbols.len(), 2);
    }

    #[test]
    fn cross_stock_single_stock() {
        let store = make_store_with_institutions(vec![(100, &[100])]);

        let raw = make_raw_with_brokers(vec![(
            sym("700.HK"),
            SecurityBrokers {
                ask_brokers: vec![Brokers {
                    position: 1,
                    broker_ids: vec![100],
                }],
                bid_brokers: vec![],
            },
        )]);

        let queues = compute_broker_queues(&raw);
        let activities = compute_institution_activities(&queues, &store);
        let cross = compute_cross_stock_presences(&activities);
        assert!(cross.is_empty());
    }

    // ── capital_flow tests ──

    #[test]
    fn capital_flow_latest() {
        let raw = RawSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            brokers: HashMap::new(),
            calc_indexes: HashMap::new(),
            candlesticks: HashMap::new(),
            capital_flows: HashMap::from([(
                sym("700.HK"),
                vec![
                    CapitalFlowLine {
                        inflow: Decimal::new(100, 0),
                        timestamp: OffsetDateTime::UNIX_EPOCH,
                    },
                    CapitalFlowLine {
                        inflow: Decimal::new(200, 0),
                        timestamp: OffsetDateTime::UNIX_EPOCH,
                    },
                    CapitalFlowLine {
                        inflow: Decimal::new(300, 0),
                        timestamp: OffsetDateTime::UNIX_EPOCH,
                    },
                ],
            )]),
            capital_distributions: HashMap::new(),
            depths: HashMap::new(),
            market_temperature: None,
            quotes: HashMap::new(),
            trades: HashMap::new(),
        };

        let flows = compute_capital_flows(&raw);
        assert_eq!(flows.len(), 1);
        assert_eq!(flows[0].net_inflow, Decimal::new(300, 0));
    }

    #[test]
    fn capital_flow_empty() {
        let raw = RawSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            brokers: HashMap::new(),
            calc_indexes: HashMap::new(),
            candlesticks: HashMap::new(),
            capital_flows: HashMap::from([(sym("700.HK"), vec![])]),
            capital_distributions: HashMap::new(),
            depths: HashMap::new(),
            market_temperature: None,
            quotes: HashMap::new(),
            trades: HashMap::new(),
        };

        let flows = compute_capital_flows(&raw);
        assert!(flows.is_empty());
    }

    // ── capital_breakdown tests ──

    #[test]
    fn capital_breakdown_net() {
        let raw = RawSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            brokers: HashMap::new(),
            calc_indexes: HashMap::new(),
            candlesticks: HashMap::new(),
            capital_flows: HashMap::new(),
            capital_distributions: HashMap::from([(
                sym("700.HK"),
                CapitalDistributionResponse {
                    timestamp: OffsetDateTime::UNIX_EPOCH,
                    capital_in: CapitalDistribution {
                        large: Decimal::new(100, 0),
                        medium: Decimal::new(50, 0),
                        small: Decimal::new(20, 0),
                    },
                    capital_out: CapitalDistribution {
                        large: Decimal::new(30, 0),
                        medium: Decimal::new(10, 0),
                        small: Decimal::new(5, 0),
                    },
                },
            )]),
            depths: HashMap::new(),
            market_temperature: None,
            quotes: HashMap::new(),
            trades: HashMap::new(),
        };

        let breakdowns = compute_capital_breakdowns(&raw);
        assert_eq!(breakdowns.len(), 1);
        assert_eq!(breakdowns[0].large_net, Decimal::new(70, 0));
        assert_eq!(breakdowns[0].medium_net, Decimal::new(40, 0));
        assert_eq!(breakdowns[0].small_net, Decimal::new(15, 0));
    }

    // ── full integration ──

    #[test]
    fn full_snapshot_integration() {
        let store = make_store_with_institutions(vec![(100, &[100, 101]), (200, &[200])]);

        let raw = RawSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            brokers: HashMap::from([
                (
                    sym("700.HK"),
                    SecurityBrokers {
                        ask_brokers: vec![Brokers {
                            position: 1,
                            broker_ids: vec![100, 200],
                        }],
                        bid_brokers: vec![Brokers {
                            position: 1,
                            broker_ids: vec![101],
                        }],
                    },
                ),
                (
                    sym("9988.HK"),
                    SecurityBrokers {
                        ask_brokers: vec![Brokers {
                            position: 2,
                            broker_ids: vec![100],
                        }],
                        bid_brokers: vec![],
                    },
                ),
            ]),
            calc_indexes: HashMap::new(),
            candlesticks: HashMap::new(),
            capital_flows: HashMap::from([
                (
                    sym("700.HK"),
                    vec![CapitalFlowLine {
                        inflow: Decimal::new(500, 0),
                        timestamp: OffsetDateTime::UNIX_EPOCH,
                    }],
                ),
                (
                    sym("9988.HK"),
                    vec![CapitalFlowLine {
                        inflow: Decimal::new(-200, 0),
                        timestamp: OffsetDateTime::UNIX_EPOCH,
                    }],
                ),
            ]),
            capital_distributions: HashMap::from([(
                sym("700.HK"),
                CapitalDistributionResponse {
                    timestamp: OffsetDateTime::UNIX_EPOCH,
                    capital_in: CapitalDistribution {
                        large: Decimal::new(1000, 0),
                        medium: Decimal::new(500, 0),
                        small: Decimal::new(200, 0),
                    },
                    capital_out: CapitalDistribution {
                        large: Decimal::new(400, 0),
                        medium: Decimal::new(200, 0),
                        small: Decimal::new(100, 0),
                    },
                },
            )]),
            depths: HashMap::from([(
                sym("700.HK"),
                SecurityDepth {
                    asks: vec![LPDepth {
                        position: 1,
                        price: Some(Decimal::new(35000, 2)),
                        volume: 1000,
                        order_num: 5,
                    }],
                    bids: vec![LPDepth {
                        position: 1,
                        price: Some(Decimal::new(34980, 2)),
                        volume: 800,
                        order_num: 3,
                    }],
                },
            )]),
            market_temperature: None,
            quotes: HashMap::from([(
                sym("700.HK"),
                SecurityQuote {
                    symbol: "700.HK".into(),
                    last_done: Decimal::new(35000, 2),
                    prev_close: Decimal::new(34800, 2),
                    open: Decimal::new(34900, 2),
                    high: Decimal::new(35200, 2),
                    low: Decimal::new(34700, 2),
                    timestamp: OffsetDateTime::UNIX_EPOCH,
                    volume: 10_000_000,
                    turnover: Decimal::new(35_000_000_00, 2),
                    trade_status: TradeStatus::Normal,
                    pre_market_quote: None,
                    post_market_quote: None,
                    overnight_quote: None,
                },
            )]),
            trades: HashMap::new(),
        };

        let snapshot = LinkSnapshot::compute(&raw, &store);

        // Broker queues: 700.HK has 3 entries (100 ask, 200 ask, 101 bid) + 9988.HK has 1 (100 ask) = 4
        assert_eq!(snapshot.broker_queues.len(), 4);

        // Institution activities: inst 100 in 700.HK + 9988.HK, inst 200 in 700.HK = 3
        assert_eq!(snapshot.institution_activities.len(), 3);

        // Cross-stock: inst 100 appears in 2 stocks
        assert_eq!(snapshot.cross_stock_presences.len(), 1);
        assert_eq!(
            snapshot.cross_stock_presences[0].institution_id,
            InstitutionId(100)
        );

        // Capital flows: 2 symbols
        assert_eq!(snapshot.capital_flows.len(), 2);

        // Capital breakdowns: 1 symbol (only 700.HK has distribution data)
        assert_eq!(snapshot.capital_breakdowns.len(), 1);
        assert_eq!(
            snapshot.capital_breakdowns[0].large_net,
            Decimal::new(600, 0)
        );

        // Order books: 1 symbol with depth data
        assert_eq!(snapshot.order_books.len(), 1);
        assert_eq!(snapshot.order_books[0].spread, Some(Decimal::new(20, 2)));

        // Quotes: 1 symbol
        assert_eq!(snapshot.quotes.len(), 1);
        assert_eq!(snapshot.quotes[0].market_status, MarketStatus::Normal);
        assert_eq!(snapshot.quotes[0].last_done, Decimal::new(35000, 2));
    }

    // ── order_book tests ──

    fn make_raw_with_depths(data: Vec<(Symbol, SecurityDepth)>) -> RawSnapshot {
        RawSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            brokers: HashMap::new(),
            calc_indexes: HashMap::new(),
            candlesticks: HashMap::new(),
            capital_flows: HashMap::new(),
            capital_distributions: HashMap::new(),
            depths: data.into_iter().collect(),
            market_temperature: None,
            quotes: HashMap::new(),
            trades: HashMap::new(),
        }
    }

    fn make_raw_with_quotes(data: Vec<(Symbol, SecurityQuote)>) -> RawSnapshot {
        RawSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            brokers: HashMap::new(),
            calc_indexes: HashMap::new(),
            candlesticks: HashMap::new(),
            capital_flows: HashMap::new(),
            capital_distributions: HashMap::new(),
            depths: HashMap::new(),
            market_temperature: None,
            quotes: data.into_iter().collect(),
            trades: HashMap::new(),
        }
    }

    fn make_quote(symbol: &str, trade_status: TradeStatus) -> SecurityQuote {
        SecurityQuote {
            symbol: symbol.into(),
            last_done: Decimal::new(35000, 2),
            prev_close: Decimal::new(34800, 2),
            open: Decimal::new(34900, 2),
            high: Decimal::new(35200, 2),
            low: Decimal::new(34700, 2),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            volume: 10_000_000,
            turnover: Decimal::new(35_000_000_00, 2),
            trade_status,
            pre_market_quote: None,
            post_market_quote: None,
            overnight_quote: None,
        }
    }

    #[test]
    fn order_book_basic_spread() {
        let raw = make_raw_with_depths(vec![(
            sym("700.HK"),
            SecurityDepth {
                asks: vec![LPDepth {
                    position: 1,
                    price: Some(Decimal::new(35000, 2)),
                    volume: 500,
                    order_num: 3,
                }],
                bids: vec![LPDepth {
                    position: 1,
                    price: Some(Decimal::new(34980, 2)),
                    volume: 400,
                    order_num: 2,
                }],
            },
        )]);

        let books = compute_order_books(&raw);
        assert_eq!(books.len(), 1);
        assert_eq!(books[0].spread, Some(Decimal::new(20, 2)));
        assert_eq!(books[0].total_ask_volume, 500);
        assert_eq!(books[0].total_bid_volume, 400);
    }

    #[test]
    fn order_book_multiple_levels() {
        let raw = make_raw_with_depths(vec![(
            sym("700.HK"),
            SecurityDepth {
                asks: vec![
                    LPDepth {
                        position: 1,
                        price: Some(Decimal::new(35000, 2)),
                        volume: 100,
                        order_num: 1,
                    },
                    LPDepth {
                        position: 2,
                        price: Some(Decimal::new(35020, 2)),
                        volume: 200,
                        order_num: 2,
                    },
                    LPDepth {
                        position: 3,
                        price: Some(Decimal::new(35040, 2)),
                        volume: 300,
                        order_num: 3,
                    },
                ],
                bids: vec![
                    LPDepth {
                        position: 1,
                        price: Some(Decimal::new(34980, 2)),
                        volume: 150,
                        order_num: 1,
                    },
                    LPDepth {
                        position: 2,
                        price: Some(Decimal::new(34960, 2)),
                        volume: 250,
                        order_num: 4,
                    },
                ],
            },
        )]);

        let books = compute_order_books(&raw);
        assert_eq!(books[0].total_ask_volume, 600);
        assert_eq!(books[0].total_bid_volume, 400);
        assert_eq!(books[0].total_ask_orders, 6);
        assert_eq!(books[0].total_bid_orders, 5);
        assert_eq!(books[0].ask_level_count, 3);
        assert_eq!(books[0].bid_level_count, 2);
    }

    #[test]
    fn order_book_empty_one_side() {
        let raw = make_raw_with_depths(vec![(
            sym("700.HK"),
            SecurityDepth {
                asks: vec![LPDepth {
                    position: 1,
                    price: Some(Decimal::new(35000, 2)),
                    volume: 100,
                    order_num: 1,
                }],
                bids: vec![],
            },
        )]);

        let books = compute_order_books(&raw);
        assert_eq!(books[0].spread, None);
        assert_eq!(books[0].total_bid_volume, 0);
    }

    #[test]
    fn order_book_empty_depth() {
        let raw = make_raw_with_depths(vec![(
            sym("700.HK"),
            SecurityDepth {
                asks: vec![],
                bids: vec![],
            },
        )]);

        let books = compute_order_books(&raw);
        assert_eq!(books[0].total_ask_volume, 0);
        assert_eq!(books[0].total_bid_volume, 0);
        assert_eq!(books[0].spread, None);
    }

    #[test]
    fn order_book_no_symbols() {
        let raw = make_raw_with_depths(vec![]);
        let books = compute_order_books(&raw);
        assert!(books.is_empty());
    }

    #[test]
    fn quote_basic() {
        let raw = make_raw_with_quotes(vec![(
            sym("700.HK"),
            make_quote("700.HK", TradeStatus::Normal),
        )]);
        let quotes = compute_quotes(&raw);
        assert_eq!(quotes.len(), 1);
        assert_eq!(quotes[0].last_done, Decimal::new(35000, 2));
        assert_eq!(quotes[0].prev_close, Decimal::new(34800, 2));
        assert_eq!(quotes[0].market_status, MarketStatus::Normal);
    }

    #[test]
    fn quote_halted_status() {
        let raw = make_raw_with_quotes(vec![(
            sym("700.HK"),
            make_quote("700.HK", TradeStatus::Halted),
        )]);
        let quotes = compute_quotes(&raw);
        assert_eq!(quotes[0].market_status, MarketStatus::Halted);
    }

    #[test]
    fn quote_suspended_status() {
        let raw = make_raw_with_quotes(vec![(
            sym("700.HK"),
            make_quote("700.HK", TradeStatus::SuspendTrade),
        )]);
        let quotes = compute_quotes(&raw);
        assert_eq!(quotes[0].market_status, MarketStatus::SuspendTrade);
    }

    #[test]
    fn quote_unknown_status() {
        let raw = make_raw_with_quotes(vec![(
            sym("700.HK"),
            make_quote("700.HK", TradeStatus::Expired),
        )]);
        let quotes = compute_quotes(&raw);
        assert_eq!(quotes[0].market_status, MarketStatus::Other);
    }

    #[test]
    fn quote_multiple_symbols() {
        let raw = make_raw_with_quotes(vec![
            (sym("700.HK"), make_quote("700.HK", TradeStatus::Normal)),
            (sym("9988.HK"), make_quote("9988.HK", TradeStatus::Normal)),
        ]);
        let quotes = compute_quotes(&raw);
        assert_eq!(quotes.len(), 2);
    }

    #[test]
    fn quote_empty() {
        let raw = make_raw_with_quotes(vec![]);
        let quotes = compute_quotes(&raw);
        assert!(quotes.is_empty());
    }
}
