use super::market_data::compute_depth_profile;
use super::*;

pub(super) fn compute_order_books(raw: &RawSnapshot) -> Vec<OrderBookObservation> {
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

pub(super) fn compute_trade_activities(raw: &RawSnapshot) -> Vec<TradeActivity> {
    raw.trades
        .iter()
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

            for t in trades {
                total_volume += t.volume;
                price_volume_sum += t.price * Decimal::from(t.volume);

                let session = TradeSession::from_longport(t.trade_session);
                match session {
                    TradeSession::Pre => pre_market_volume += t.volume,
                    TradeSession::Post => post_market_volume += t.volume,
                    _ => {}
                }

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
                    session,
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
                pre_market_volume,
                post_market_volume,
            }
        })
        .collect()
}

pub(super) fn market_status_from_trade_status(
    status: longport::quote::TradeStatus,
) -> MarketStatus {
    use longport::quote::TradeStatus;
    #[allow(unreachable_patterns)]
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

pub fn convert_pre_post_quote(ppq: &longport::quote::PrePostQuote) -> ExtendedSessionQuote {
    ExtendedSessionQuote {
        last_done: ppq.last_done,
        timestamp: ppq.timestamp,
        volume: ppq.volume,
        turnover: ppq.turnover,
        high: ppq.high,
        low: ppq.low,
        prev_close: ppq.prev_close,
    }
}

pub(super) fn compute_quotes(raw: &RawSnapshot) -> Vec<QuoteObservation> {
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
            pre_market: q.pre_market_quote.as_ref().map(convert_pre_post_quote),
            post_market: q.post_market_quote.as_ref().map(convert_pre_post_quote),
        })
        .collect()
}
