use super::*;
use crate::core::market_snapshot::CanonicalMarketSnapshot;

impl ObservationSnapshot {
    pub fn from_links(links: &LinkSnapshot) -> Self {
        let mut observations = Vec::new();

        for quote in &links.quotes {
            observations.push(Observation::new(
                ObservationRecord::Quote {
                    symbol: quote.symbol.clone(),
                    last_done: quote.last_done,
                    turnover: quote.turnover,
                    market_status: format!("{:?}", quote.market_status),
                    pre_market_last: quote.pre_market.as_ref().map(|p| p.last_done),
                    post_market_last: quote.post_market.as_ref().map(|p| p.last_done),
                },
                provenance(
                    ProvenanceSource::WebSocket,
                    quote.timestamp,
                    Some(confidence_from_turnover(quote.turnover)),
                    [format!("quote:{}", quote.symbol)],
                ),
            ));
        }

        for order_book in &links.order_books {
            observations.push(Observation::new(
                ObservationRecord::OrderBook {
                    symbol: order_book.symbol.clone(),
                    total_bid_volume: order_book.total_bid_volume,
                    total_ask_volume: order_book.total_ask_volume,
                    spread: order_book.spread,
                },
                provenance(
                    ProvenanceSource::WebSocket,
                    links.timestamp,
                    Some(Decimal::ONE),
                    [format!("depth:{}", order_book.symbol)],
                ),
            ));
        }

        for capital_flow in &links.capital_flows {
            observations.push(Observation::new(
                ObservationRecord::CapitalFlow {
                    symbol: capital_flow.symbol.clone(),
                    net_inflow: capital_flow.net_inflow.as_yuan(),
                },
                provenance(
                    ProvenanceSource::Api,
                    links.timestamp,
                    Some(confidence_from_magnitude(capital_flow.net_inflow.as_yuan())),
                    [format!("capital_flow:{}", capital_flow.symbol)],
                ),
            ));
        }

        for series in &links.capital_flow_series {
            observations.push(Observation::new(
                ObservationRecord::CapitalFlowSeries {
                    symbol: series.symbol.clone(),
                    point_count: series.points.len(),
                    latest_inflow: series.latest_inflow.as_yuan(),
                    velocity: series.velocity,
                },
                provenance(
                    ProvenanceSource::Api,
                    links.timestamp,
                    Some(confidence_from_magnitude(series.latest_inflow.as_yuan())),
                    [format!("capital_flow_series:{}", series.symbol)],
                ),
            ));
        }

        for breakdown in &links.capital_breakdowns {
            observations.push(Observation::new(
                ObservationRecord::CapitalBreakdown {
                    symbol: breakdown.symbol.clone(),
                    large_net: breakdown.large_net,
                    medium_net: breakdown.medium_net,
                    small_net: breakdown.small_net,
                },
                provenance(
                    ProvenanceSource::Api,
                    links.timestamp,
                    Some(confidence_from_magnitude(breakdown.large_net)),
                    [format!("capital_breakdown:{}", breakdown.symbol)],
                ),
            ));
        }

        for calc in &links.calc_indexes {
            observations.push(Observation::new(
                ObservationRecord::CalcIndex {
                    symbol: calc.symbol.clone(),
                    turnover_rate: calc.turnover_rate,
                    volume_ratio: calc.volume_ratio,
                    pe_ttm_ratio: calc.pe_ttm_ratio,
                    pb_ratio: calc.pb_ratio,
                    dividend_ratio_ttm: calc.dividend_ratio_ttm,
                    amplitude: calc.amplitude,
                    five_minutes_change_rate: calc.five_minutes_change_rate,
                    ytd_change_rate: calc.ytd_change_rate,
                    five_day_change_rate: calc.five_day_change_rate,
                    ten_day_change_rate: calc.ten_day_change_rate,
                    half_year_change_rate: calc.half_year_change_rate,
                    total_market_value: calc.total_market_value,
                    change_rate: calc.change_rate,
                },
                provenance(
                    ProvenanceSource::Api,
                    links.timestamp,
                    Some(
                        calc.volume_ratio
                            .unwrap_or(Decimal::ONE)
                            .min(Decimal::new(3, 0))
                            / Decimal::new(3, 0),
                    ),
                    [format!("calc_index:{}", calc.symbol)],
                ),
            ));
        }

        for candle in &links.candlesticks {
            observations.push(Observation::new(
                ObservationRecord::Candlestick {
                    symbol: candle.symbol.clone(),
                    candle_count: candle.candle_count,
                    window_return: candle.window_return,
                    body_bias: candle.body_bias,
                    volume_ratio: candle.volume_ratio,
                    range_ratio: candle.range_ratio,
                },
                provenance(
                    ProvenanceSource::Computed,
                    links.timestamp,
                    Some(Decimal::from(candle.candle_count.min(5) as i64) / Decimal::new(5, 0)),
                    [format!("candlestick:{}", candle.symbol)],
                ),
            ));
        }

        for activity in &links.institution_activities {
            observations.push(Observation::new(
                ObservationRecord::InstitutionActivity {
                    symbol: activity.symbol.clone(),
                    institution_id: activity.institution_id.to_string(),
                    seat_count: activity.seat_count,
                },
                provenance(
                    ProvenanceSource::Computed,
                    links.timestamp,
                    Some(Decimal::ONE),
                    [
                        format!("broker_queue:{}", activity.symbol),
                        format!("institution:{}", activity.institution_id),
                    ],
                ),
            ));
        }

        for trade in &links.trade_activities {
            observations.push(Observation::new(
                ObservationRecord::TradeActivity {
                    symbol: trade.symbol.clone(),
                    trade_count: trade.trade_count,
                    total_volume: trade.total_volume,
                    buy_volume: trade.buy_volume,
                    sell_volume: trade.sell_volume,
                    vwap: trade.vwap,
                    pre_market_volume: trade.pre_market_volume,
                    post_market_volume: trade.post_market_volume,
                },
                provenance(
                    ProvenanceSource::WebSocket,
                    links.timestamp,
                    Some(Decimal::ONE),
                    [format!("trade_activity:{}", trade.symbol)],
                ),
            ));
        }

        if let Some(temp) = &links.market_temperature {
            observations.push(Observation::new(
                ObservationRecord::MarketTemperature {
                    temperature: temp.temperature,
                    valuation: temp.valuation,
                    sentiment: temp.sentiment,
                    description: temp.description.clone(),
                },
                provenance(
                    ProvenanceSource::Api,
                    temp.timestamp,
                    Some(Decimal::ONE),
                    ["market_temperature:HK".to_string()],
                ),
            ));
        }

        Self {
            timestamp: links.timestamp,
            observations,
        }
    }

    pub fn from_canonical_market(snapshot: &CanonicalMarketSnapshot) -> Self {
        let mut observations = Vec::new();

        for quote in snapshot.quotes.values() {
            observations.push(Observation::new(
                ObservationRecord::Quote {
                    symbol: quote.symbol.clone(),
                    last_done: quote.last_done,
                    turnover: quote.turnover,
                    market_status: format!("{:?}", quote.market_status),
                    pre_market_last: quote.pre_market.as_ref().map(|p| p.last_done),
                    post_market_last: quote.post_market.as_ref().map(|p| p.last_done),
                },
                provenance(
                    ProvenanceSource::WebSocket,
                    quote.timestamp,
                    Some(confidence_from_turnover(quote.turnover)),
                    [format!("quote:{}", quote.symbol)],
                ),
            ));
        }

        for order_book in snapshot.order_books.values() {
            let total_bid_volume = order_book.bid_levels.iter().map(|level| level.volume).sum();
            let total_ask_volume = order_book.ask_levels.iter().map(|level| level.volume).sum();
            let best_ask = order_book
                .ask_levels
                .iter()
                .filter_map(|level| level.price)
                .min();
            let best_bid = order_book
                .bid_levels
                .iter()
                .filter_map(|level| level.price)
                .max();
            let spread = match (best_ask, best_bid) {
                (Some(ask), Some(bid)) => Some(ask - bid),
                _ => None,
            };
            observations.push(Observation::new(
                ObservationRecord::OrderBook {
                    symbol: order_book.symbol.clone(),
                    total_bid_volume,
                    total_ask_volume,
                    spread,
                },
                provenance(
                    ProvenanceSource::WebSocket,
                    snapshot.timestamp,
                    Some(Decimal::ONE),
                    [format!("depth:{}", order_book.symbol)],
                ),
            ));
        }

        for (symbol, lines) in &snapshot.capital_flow_series {
            if let Some(last) = lines.last() {
                observations.push(Observation::new(
                    ObservationRecord::CapitalFlow {
                        symbol: symbol.clone(),
                        net_inflow: last.inflow * Decimal::from(10_000),
                    },
                    provenance(
                        ProvenanceSource::Api,
                        snapshot.timestamp,
                        Some(confidence_from_magnitude(
                            last.inflow * Decimal::from(10_000),
                        )),
                        [format!("capital_flow:{}", symbol)],
                    ),
                ));

                let velocity = if lines.len() >= 2 {
                    let prev = &lines[lines.len() - 2];
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
                observations.push(Observation::new(
                    ObservationRecord::CapitalFlowSeries {
                        symbol: symbol.clone(),
                        point_count: lines.len(),
                        latest_inflow: last.inflow * Decimal::from(10_000),
                        velocity,
                    },
                    provenance(
                        ProvenanceSource::Api,
                        snapshot.timestamp,
                        Some(confidence_from_magnitude(
                            last.inflow * Decimal::from(10_000),
                        )),
                        [format!("capital_flow_series:{}", symbol)],
                    ),
                ));
            }
        }

        for (symbol, breakdown) in &snapshot.capital_distributions {
            observations.push(Observation::new(
                ObservationRecord::CapitalBreakdown {
                    symbol: symbol.clone(),
                    large_net: breakdown.large_in - breakdown.large_out,
                    medium_net: breakdown.medium_in - breakdown.medium_out,
                    small_net: breakdown.small_in - breakdown.small_out,
                },
                provenance(
                    ProvenanceSource::Api,
                    snapshot.timestamp,
                    Some(confidence_from_magnitude(
                        breakdown.large_in - breakdown.large_out,
                    )),
                    [format!("capital_breakdown:{}", symbol)],
                ),
            ));
        }

        for (symbol, calc) in &snapshot.calc_indexes {
            observations.push(Observation::new(
                ObservationRecord::CalcIndex {
                    symbol: symbol.clone(),
                    turnover_rate: calc.turnover_rate,
                    volume_ratio: calc.volume_ratio,
                    pe_ttm_ratio: calc.pe_ttm_ratio,
                    pb_ratio: calc.pb_ratio,
                    dividend_ratio_ttm: calc.dividend_ratio_ttm,
                    amplitude: calc.amplitude,
                    five_minutes_change_rate: calc.five_minutes_change_rate,
                    ytd_change_rate: calc.ytd_change_rate,
                    five_day_change_rate: calc.five_day_change_rate,
                    ten_day_change_rate: calc.ten_day_change_rate,
                    half_year_change_rate: calc.half_year_change_rate,
                    total_market_value: calc.total_market_value,
                    change_rate: calc.change_rate,
                },
                provenance(
                    ProvenanceSource::Api,
                    snapshot.timestamp,
                    Some(
                        calc.volume_ratio
                            .unwrap_or(Decimal::ONE)
                            .min(Decimal::new(3, 0))
                            / Decimal::new(3, 0),
                    ),
                    [format!("calc_index:{}", symbol)],
                ),
            ));
        }

        for (symbol, candles) in &snapshot.candlesticks {
            let Some(latest) = candles.last() else {
                continue;
            };
            let first = candles
                .iter()
                .rev()
                .take(5)
                .last()
                .cloned()
                .unwrap_or_else(|| latest.clone());

            let window_return = if first.open > Decimal::ZERO {
                crate::math::clamp_signed_unit_interval(
                    (latest.close - first.open) / first.open / Decimal::new(2, 2),
                )
            } else {
                Decimal::ZERO
            };

            let latest_range = latest.high - latest.low;
            let body_bias = if latest_range > Decimal::ZERO {
                crate::math::clamp_signed_unit_interval((latest.close - latest.open) / latest_range)
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
            let range_ratio = if first.open > Decimal::ZERO {
                crate::math::clamp_signed_unit_interval(
                    (window_high - window_low) / first.open / Decimal::new(8, 2),
                )
            } else {
                Decimal::ZERO
            };

            observations.push(Observation::new(
                ObservationRecord::Candlestick {
                    symbol: symbol.clone(),
                    candle_count: candles.len(),
                    window_return,
                    body_bias,
                    volume_ratio,
                    range_ratio,
                },
                provenance(
                    ProvenanceSource::Computed,
                    snapshot.timestamp,
                    Some(Decimal::from(candles.len().min(5) as i64) / Decimal::new(5, 0)),
                    [format!("candlestick:{}", symbol)],
                ),
            ));
        }

        for broker_queues in snapshot.broker_queues.values() {
            for level in &broker_queues.ask_levels {
                for broker_id in &level.broker_ids {
                    observations.push(Observation::new(
                        ObservationRecord::BrokerActivity {
                            symbol: broker_queues.symbol.clone(),
                            broker_id: *broker_id,
                            institution_id: None,
                            side: "ask".into(),
                            position: level.position,
                            duration_ticks: 0,
                            replenish_count: 0,
                        },
                        provenance(
                            ProvenanceSource::Computed,
                            snapshot.timestamp,
                            Some(Decimal::ONE),
                            [format!("broker_queue:{}", broker_queues.symbol)],
                        ),
                    ));
                }
            }
            for level in &broker_queues.bid_levels {
                for broker_id in &level.broker_ids {
                    observations.push(Observation::new(
                        ObservationRecord::BrokerActivity {
                            symbol: broker_queues.symbol.clone(),
                            broker_id: *broker_id,
                            institution_id: None,
                            side: "bid".into(),
                            position: level.position,
                            duration_ticks: 0,
                            replenish_count: 0,
                        },
                        provenance(
                            ProvenanceSource::Computed,
                            snapshot.timestamp,
                            Some(Decimal::ONE),
                            [format!("broker_queue:{}", broker_queues.symbol)],
                        ),
                    ));
                }
            }
        }

        for (symbol, trades) in &snapshot.trades {
            let trade_count = trades.len();
            let total_volume: i64 = trades.iter().map(|trade| trade.volume).sum();
            let buy_volume: i64 = trades
                .iter()
                .filter(|trade| {
                    matches!(
                        trade.direction,
                        crate::core::market_snapshot::CanonicalTradeDirection::Up
                    )
                })
                .map(|trade| trade.volume)
                .sum();
            let sell_volume: i64 = trades
                .iter()
                .filter(|trade| {
                    matches!(
                        trade.direction,
                        crate::core::market_snapshot::CanonicalTradeDirection::Down
                    )
                })
                .map(|trade| trade.volume)
                .sum();
            let pre_market_volume: i64 = trades
                .iter()
                .filter(|trade| {
                    matches!(
                        trade.session,
                        crate::core::market_snapshot::CanonicalTradeSession::Pre
                    )
                })
                .map(|trade| trade.volume)
                .sum();
            let post_market_volume: i64 = trades
                .iter()
                .filter(|trade| {
                    matches!(
                        trade.session,
                        crate::core::market_snapshot::CanonicalTradeSession::Post
                    )
                })
                .map(|trade| trade.volume)
                .sum();
            let price_volume_sum = trades.iter().fold(Decimal::ZERO, |acc, trade| {
                acc + trade.price * Decimal::from(trade.volume)
            });
            let vwap = if total_volume > 0 {
                price_volume_sum / Decimal::from(total_volume)
            } else {
                Decimal::ZERO
            };

            observations.push(Observation::new(
                ObservationRecord::TradeActivity {
                    symbol: symbol.clone(),
                    trade_count,
                    total_volume,
                    buy_volume,
                    sell_volume,
                    vwap,
                    pre_market_volume,
                    post_market_volume,
                },
                provenance(
                    ProvenanceSource::WebSocket,
                    snapshot.timestamp,
                    Some(Decimal::ONE),
                    [format!("trade_activity:{}", symbol)],
                ),
            ));
        }

        if let Some(temp) = &snapshot.market_temperature {
            observations.push(Observation::new(
                ObservationRecord::MarketTemperature {
                    temperature: temp.temperature,
                    valuation: temp.valuation,
                    sentiment: temp.sentiment,
                    description: temp.description.clone(),
                },
                provenance(
                    ProvenanceSource::Api,
                    snapshot.timestamp,
                    Some(Decimal::ONE),
                    [format!("market_temperature:{}", snapshot.market.slug())],
                ),
            ));
        }

        Self {
            timestamp: snapshot.timestamp,
            observations,
        }
    }
}
