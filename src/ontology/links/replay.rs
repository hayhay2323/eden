use super::broker::{compute_cross_stock_presences, compute_institution_activities};
use super::market_data::compute_depth_profile;
use super::types::*;
use super::*;
use crate::math::clamp_signed_unit_interval;
use crate::ontology::microstructure::{ArchivedTradeDirection, TickArchive, TradeSession};

impl LinkSnapshot {
    /// Reconstruct a LinkSnapshot from a persisted TickArchive.
    /// This bypasses Longport types entirely — used by the replay binary.
    pub fn from_archive(archive: &TickArchive, store: &ObjectStore) -> Self {
        let broker_queues = replay_broker_queues(archive);
        let institution_activities = compute_institution_activities(&broker_queues, store);
        let cross_stock_presences = compute_cross_stock_presences(&institution_activities);
        let order_books = replay_order_books(archive);
        let quotes = replay_quotes(archive);
        let trade_activities = replay_trade_activities(archive);
        let capital_flows = replay_capital_flows(archive);
        let capital_flow_series = replay_capital_flow_series(archive);
        let capital_breakdowns = replay_capital_breakdowns(archive);
        let candlesticks = replay_candlesticks(archive);

        LinkSnapshot {
            timestamp: archive.timestamp,
            broker_queues,
            calc_indexes: Vec::new(),
            candlesticks,
            institution_activities,
            cross_stock_presences,
            capital_flows,
            capital_flow_series,
            capital_breakdowns,
            market_temperature: None,
            order_books,
            quotes,
            trade_activities,
            intraday: vec![],
        }
    }
}

fn replay_broker_queues(archive: &TickArchive) -> Vec<BrokerQueueEntry> {
    archive
        .broker_queues
        .iter()
        .map(|entry| BrokerQueueEntry {
            symbol: entry.symbol.clone(),
            broker_id: BrokerId(entry.broker_id),
            side: if entry.side == "bid" {
                Side::Bid
            } else {
                Side::Ask
            },
            position: entry.position,
        })
        .collect()
}

fn replay_order_books(archive: &TickArchive) -> Vec<OrderBookObservation> {
    archive
        .order_books
        .iter()
        .map(|ob| {
            let ask_levels: Vec<DepthLevel> = ob
                .ask_levels
                .iter()
                .map(|l| DepthLevel {
                    position: l.position,
                    price: l.price,
                    volume: l.volume,
                    order_num: l.order_num,
                })
                .collect();
            let bid_levels: Vec<DepthLevel> = ob
                .bid_levels
                .iter()
                .map(|l| DepthLevel {
                    position: l.position,
                    price: l.price,
                    volume: l.volume,
                    order_num: l.order_num,
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
                symbol: ob.symbol.clone(),
                ask_levels,
                bid_levels,
                total_ask_volume,
                total_bid_volume,
                total_ask_orders,
                total_bid_orders,
                spread,
                ask_level_count: ob.ask_levels.len(),
                bid_level_count: ob.bid_levels.len(),
                bid_profile,
                ask_profile,
            }
        })
        .collect()
}

fn replay_quotes(archive: &TickArchive) -> Vec<QuoteObservation> {
    archive
        .quotes
        .iter()
        .map(|q| {
            let convert_ext =
                |ext: &crate::ontology::microstructure::ExtendedQuote| ExtendedSessionQuote {
                    last_done: ext.last_done,
                    timestamp: ext.timestamp,
                    volume: ext.volume,
                    turnover: ext.turnover,
                    high: ext.high,
                    low: ext.low,
                    prev_close: ext.prev_close,
                };

            QuoteObservation {
                symbol: q.symbol.clone(),
                last_done: q.last_done,
                prev_close: q.prev_close,
                open: q.open,
                high: q.high,
                low: q.low,
                volume: q.volume,
                turnover: q.turnover,
                timestamp: q.timestamp,
                market_status: MarketStatus::Normal,
                pre_market: q.pre_market.as_ref().map(convert_ext),
                post_market: q.post_market.as_ref().map(convert_ext),
            }
        })
        .collect()
}

fn replay_trade_activities(archive: &TickArchive) -> Vec<TradeActivity> {
    let mut by_symbol: HashMap<Symbol, Vec<&crate::ontology::microstructure::ArchivedTrade>> =
        HashMap::new();
    for t in &archive.trades {
        by_symbol.entry(t.symbol.clone()).or_default().push(t);
    }

    by_symbol
        .into_iter()
        .map(|(symbol, trades)| {
            let mut buy_volume: i64 = 0;
            let mut sell_volume: i64 = 0;
            let mut neutral_volume: i64 = 0;
            let mut pre_market_volume: i64 = 0;
            let mut post_market_volume: i64 = 0;
            let mut price_volume_sum = Decimal::ZERO;
            let mut total_volume: i64 = 0;
            let mut records = Vec::with_capacity(trades.len());
            let mut last_price = None;

            for t in &trades {
                total_volume += t.volume;
                price_volume_sum += t.price * Decimal::from(t.volume);

                match t.session {
                    TradeSession::Pre => pre_market_volume += t.volume,
                    TradeSession::Post => post_market_volume += t.volume,
                    _ => {}
                }

                let dir = match t.direction {
                    ArchivedTradeDirection::Up => {
                        buy_volume += t.volume;
                        TradeDirection::Up
                    }
                    ArchivedTradeDirection::Down => {
                        sell_volume += t.volume;
                        TradeDirection::Down
                    }
                    ArchivedTradeDirection::Neutral => {
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
                    session: t.session,
                });
            }

            let vwap = if total_volume > 0 {
                price_volume_sum / Decimal::from(total_volume)
            } else {
                Decimal::ZERO
            };

            TradeActivity {
                symbol,
                trade_count: trades.len(),
                total_volume,
                buy_volume,
                sell_volume,
                neutral_volume,
                vwap,
                last_price,
                trades: records,
                pre_market_volume,
                post_market_volume,
            }
        })
        .collect()
}

fn replay_capital_flows(archive: &TickArchive) -> Vec<CapitalFlow> {
    archive
        .capital_flows
        .iter()
        .filter_map(|series| {
            series.points.last().map(|point| CapitalFlow {
                symbol: series.symbol.clone(),
                net_inflow: YuanAmount::from_ten_thousands(point.inflow),
            })
        })
        .collect()
}

fn replay_capital_flow_series(archive: &TickArchive) -> Vec<CapitalFlowTimeSeries> {
    archive
        .capital_flows
        .iter()
        .filter_map(|series| {
            if series.points.is_empty() {
                return None;
            }

            let points: Vec<CapitalFlowPoint> = series
                .points
                .iter()
                .map(|p| CapitalFlowPoint {
                    timestamp: p.timestamp,
                    inflow: p.inflow,
                })
                .collect();

            let last = series.points.last().unwrap();
            let latest_inflow = YuanAmount::from_ten_thousands(last.inflow);

            let velocity = if series.points.len() >= 2 {
                let prev = &series.points[series.points.len() - 2];
                let dt_seconds = (last.timestamp - prev.timestamp).whole_seconds();
                if dt_seconds > 0 {
                    let dt_minutes = Decimal::from(dt_seconds) / Decimal::from(60);
                    (last.inflow - prev.inflow) / dt_minutes
                } else {
                    Decimal::ZERO
                }
            } else {
                Decimal::ZERO
            };

            Some(CapitalFlowTimeSeries {
                symbol: series.symbol.clone(),
                points,
                latest_inflow,
                velocity,
            })
        })
        .collect()
}

fn replay_capital_breakdowns(archive: &TickArchive) -> Vec<CapitalBreakdown> {
    archive
        .capital_distributions
        .iter()
        .map(|dist| CapitalBreakdown {
            symbol: dist.symbol.clone(),
            large_net: dist.large_in - dist.large_out,
            medium_net: dist.medium_in - dist.medium_out,
            small_net: dist.small_in - dist.small_out,
        })
        .collect()
}

fn replay_candlesticks(archive: &TickArchive) -> Vec<CandlestickObservation> {
    let mut by_symbol: HashMap<Symbol, Vec<&crate::ontology::microstructure::ArchivedCandlestick>> =
        HashMap::new();
    for c in &archive.candlesticks {
        by_symbol.entry(c.symbol.clone()).or_default().push(c);
    }

    by_symbol
        .into_iter()
        .filter_map(|(symbol, candles)| {
            let latest = candles.last()?;
            let window: Vec<_> = candles.iter().rev().take(5).collect();
            let first = window.last().copied().unwrap_or(latest);

            let window_high = window.iter().map(|c| c.high).max().unwrap_or(latest.high);
            let window_low = window.iter().map(|c| c.low).min().unwrap_or(latest.low);

            let window_return = if first.open > Decimal::ZERO {
                clamp_signed_unit_interval(
                    (latest.close - first.open) / first.open / Decimal::new(2, 2),
                )
            } else {
                Decimal::ZERO
            };

            let latest_range = latest.high - latest.low;
            let body_bias = if latest_range > Decimal::ZERO {
                clamp_signed_unit_interval((latest.close - latest.open) / latest_range)
            } else {
                Decimal::ZERO
            };

            let average_volume = if window.is_empty() {
                Decimal::ZERO
            } else {
                Decimal::from(window.iter().map(|c| c.volume).sum::<i64>())
                    / Decimal::from(window.len() as i64)
            };
            let volume_ratio = if average_volume > Decimal::ZERO {
                Decimal::from(latest.volume) / average_volume
            } else {
                Decimal::ZERO
            };

            let range_normalizer = Decimal::new(8, 2);
            let range_ratio = if first.open > Decimal::ZERO {
                clamp_signed_unit_interval(
                    (window_high - window_low) / first.open / range_normalizer,
                )
            } else {
                Decimal::ZERO
            };

            Some(CandlestickObservation {
                symbol,
                candle_count: candles.len(),
                window_return,
                body_bias,
                volume_ratio,
                range_ratio,
            })
        })
        .collect()
}
