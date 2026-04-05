use super::*;

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
}
