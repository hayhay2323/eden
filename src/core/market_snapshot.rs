use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::core::market::MarketId;
use crate::ontology::links::convert_pre_post_quote;
use crate::ontology::links::OptionSurfaceObservation;
use crate::ontology::objects::Symbol;
use crate::ontology::snapshot::RawSnapshot;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalMarketStatus {
    Normal,
    Halted,
    SuspendTrade,
    ToBeOpened,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalTradeDirection {
    Up,
    Down,
    Neutral,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CanonicalTradeSession {
    Normal,
    Pre,
    Post,
    Overnight,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalExtendedSessionQuote {
    pub last_done: Decimal,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub volume: i64,
    pub turnover: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub prev_close: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalQuote {
    pub symbol: Symbol,
    pub last_done: Decimal,
    pub prev_close: Decimal,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub volume: i64,
    pub turnover: Decimal,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub market_status: CanonicalMarketStatus,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_market: Option<CanonicalExtendedSessionQuote>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_market: Option<CanonicalExtendedSessionQuote>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalTrade {
    pub price: Decimal,
    pub volume: i64,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub direction: CanonicalTradeDirection,
    pub session: CanonicalTradeSession,
    pub trade_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalCandle {
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub open: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub close: Decimal,
    pub volume: i64,
    pub turnover: Decimal,
    pub session: CanonicalTradeSession,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalDepthLevel {
    pub position: i32,
    pub price: Option<Decimal>,
    pub volume: i64,
    pub order_num: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalOrderBook {
    pub symbol: Symbol,
    pub ask_levels: Vec<CanonicalDepthLevel>,
    pub bid_levels: Vec<CanonicalDepthLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalBrokerLevel {
    pub position: i32,
    pub broker_ids: Vec<i32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalBrokerQueues {
    pub symbol: Symbol,
    pub ask_levels: Vec<CanonicalBrokerLevel>,
    pub bid_levels: Vec<CanonicalBrokerLevel>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalCalcIndex {
    pub turnover_rate: Option<Decimal>,
    pub volume_ratio: Option<Decimal>,
    pub pe_ttm_ratio: Option<Decimal>,
    pub pb_ratio: Option<Decimal>,
    pub dividend_ratio_ttm: Option<Decimal>,
    pub amplitude: Option<Decimal>,
    pub five_minutes_change_rate: Option<Decimal>,
    pub ytd_change_rate: Option<Decimal>,
    pub five_day_change_rate: Option<Decimal>,
    pub ten_day_change_rate: Option<Decimal>,
    pub half_year_change_rate: Option<Decimal>,
    pub total_market_value: Option<Decimal>,
    pub capital_flow: Option<Decimal>,
    pub change_rate: Option<Decimal>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalCapitalFlowPoint {
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub inflow: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalCapitalDistribution {
    pub large_in: Decimal,
    pub large_out: Decimal,
    pub medium_in: Decimal,
    pub medium_out: Decimal,
    pub small_in: Decimal,
    pub small_out: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalIntradayPoint {
    pub price: Decimal,
    pub avg_price: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalOptionSurface {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalMarketTemperature {
    pub temperature: Decimal,
    pub valuation: Decimal,
    pub sentiment: Decimal,
    pub description: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CanonicalMarketSnapshot {
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub market: MarketId,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub quotes: HashMap<Symbol, CanonicalQuote>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub trades: HashMap<Symbol, Vec<CanonicalTrade>>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub candlesticks: HashMap<Symbol, Vec<CanonicalCandle>>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub order_books: HashMap<Symbol, CanonicalOrderBook>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub broker_queues: HashMap<Symbol, CanonicalBrokerQueues>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub calc_indexes: HashMap<Symbol, CanonicalCalcIndex>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub capital_flow_series: HashMap<Symbol, Vec<CanonicalCapitalFlowPoint>>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub capital_distributions: HashMap<Symbol, CanonicalCapitalDistribution>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub intraday: HashMap<Symbol, Vec<CanonicalIntradayPoint>>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub option_surfaces: Vec<CanonicalOptionSurface>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub market_temperature: Option<CanonicalMarketTemperature>,
}

impl CanonicalMarketSnapshot {
    pub fn with_option_surfaces(mut self, option_surfaces: &[OptionSurfaceObservation]) -> Self {
        self.option_surfaces = option_surfaces
            .iter()
            .map(CanonicalOptionSurface::from_observation)
            .collect();
        self
    }

    pub fn option_surface_observations(&self) -> Vec<OptionSurfaceObservation> {
        self.option_surfaces
            .iter()
            .map(|surface| OptionSurfaceObservation {
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
}

impl RawSnapshot {
    pub fn to_canonical_snapshot(
        &self,
        market: MarketId,
        intraday_lines: &HashMap<Symbol, Vec<longport::quote::IntradayLine>>,
    ) -> CanonicalMarketSnapshot {
        CanonicalMarketSnapshot {
            timestamp: self.timestamp,
            market,
            quotes: self
                .quotes
                .iter()
                .map(|(symbol, quote)| {
                    (
                        symbol.clone(),
                        CanonicalQuote::from_security_quote(symbol.clone(), quote),
                    )
                })
                .collect(),
            trades: self
                .trades
                .iter()
                .map(|(symbol, trades)| {
                    (
                        symbol.clone(),
                        trades.iter().map(CanonicalTrade::from_longport).collect(),
                    )
                })
                .collect(),
            candlesticks: self
                .candlesticks
                .iter()
                .map(|(symbol, candles)| {
                    (
                        symbol.clone(),
                        candles.iter().map(CanonicalCandle::from_longport).collect(),
                    )
                })
                .collect(),
            order_books: self
                .depths
                .iter()
                .map(|(symbol, depth)| {
                    (
                        symbol.clone(),
                        CanonicalOrderBook {
                            symbol: symbol.clone(),
                            ask_levels: depth
                                .asks
                                .iter()
                                .map(|level| CanonicalDepthLevel {
                                    position: level.position,
                                    price: level.price,
                                    volume: level.volume,
                                    order_num: level.order_num,
                                })
                                .collect(),
                            bid_levels: depth
                                .bids
                                .iter()
                                .map(|level| CanonicalDepthLevel {
                                    position: level.position,
                                    price: level.price,
                                    volume: level.volume,
                                    order_num: level.order_num,
                                })
                                .collect(),
                        },
                    )
                })
                .collect(),
            broker_queues: self
                .brokers
                .iter()
                .map(|(symbol, brokers)| {
                    (
                        symbol.clone(),
                        CanonicalBrokerQueues {
                            symbol: symbol.clone(),
                            ask_levels: brokers
                                .ask_brokers
                                .iter()
                                .map(|level| CanonicalBrokerLevel {
                                    position: level.position,
                                    broker_ids: level.broker_ids.clone(),
                                })
                                .collect(),
                            bid_levels: brokers
                                .bid_brokers
                                .iter()
                                .map(|level| CanonicalBrokerLevel {
                                    position: level.position,
                                    broker_ids: level.broker_ids.clone(),
                                })
                                .collect(),
                        },
                    )
                })
                .collect(),
            calc_indexes: self
                .calc_indexes
                .iter()
                .map(|(symbol, index)| {
                    (
                        symbol.clone(),
                        CanonicalCalcIndex {
                            turnover_rate: index.turnover_rate,
                            volume_ratio: index.volume_ratio,
                            pe_ttm_ratio: index.pe_ttm_ratio,
                            pb_ratio: index.pb_ratio,
                            dividend_ratio_ttm: index.dividend_ratio_ttm,
                            amplitude: index.amplitude,
                            five_minutes_change_rate: index.five_minutes_change_rate,
                            ytd_change_rate: index.ytd_change_rate,
                            five_day_change_rate: index.five_day_change_rate,
                            ten_day_change_rate: index.ten_day_change_rate,
                            half_year_change_rate: index.half_year_change_rate,
                            total_market_value: index.total_market_value,
                            capital_flow: index.capital_flow,
                            change_rate: index.change_rate,
                        },
                    )
                })
                .collect(),
            capital_flow_series: self
                .capital_flows
                .iter()
                .map(|(symbol, lines)| {
                    (
                        symbol.clone(),
                        lines
                            .iter()
                            .map(|line| CanonicalCapitalFlowPoint {
                                timestamp: line.timestamp,
                                inflow: line.inflow,
                            })
                            .collect(),
                    )
                })
                .collect(),
            capital_distributions: self
                .capital_distributions
                .iter()
                .map(|(symbol, distribution)| {
                    (
                        symbol.clone(),
                        CanonicalCapitalDistribution {
                            large_in: distribution.capital_in.large,
                            large_out: distribution.capital_out.large,
                            medium_in: distribution.capital_in.medium,
                            medium_out: distribution.capital_out.medium,
                            small_in: distribution.capital_in.small,
                            small_out: distribution.capital_out.small,
                        },
                    )
                })
                .collect(),
            intraday: intraday_lines
                .iter()
                .map(|(symbol, lines)| {
                    (
                        symbol.clone(),
                        lines
                            .iter()
                            .map(|line| CanonicalIntradayPoint {
                                price: line.price,
                                avg_price: line.avg_price,
                            })
                            .collect(),
                    )
                })
                .collect(),
            option_surfaces: Vec::new(),
            market_temperature: self.market_temperature.as_ref().map(|temperature| {
                CanonicalMarketTemperature {
                    temperature: Decimal::from(temperature.temperature),
                    valuation: Decimal::from(temperature.valuation),
                    sentiment: Decimal::from(temperature.sentiment),
                    description: temperature.description.clone(),
                }
            }),
        }
    }
}

impl CanonicalQuote {
    fn from_security_quote(symbol: Symbol, quote: &longport::quote::SecurityQuote) -> Self {
        Self {
            symbol,
            last_done: quote.last_done,
            prev_close: quote.prev_close,
            open: quote.open,
            high: quote.high,
            low: quote.low,
            volume: quote.volume,
            turnover: quote.turnover,
            timestamp: quote.timestamp,
            market_status: canonical_market_status_from_trade_status(quote.trade_status),
            pre_market: quote
                .pre_market_quote
                .as_ref()
                .map(convert_pre_post_quote)
                .map(CanonicalExtendedSessionQuote::from_links_quote),
            post_market: quote
                .post_market_quote
                .as_ref()
                .map(convert_pre_post_quote)
                .map(CanonicalExtendedSessionQuote::from_links_quote),
        }
    }
}

impl CanonicalExtendedSessionQuote {
    fn from_links_quote(quote: crate::ontology::links::ExtendedSessionQuote) -> Self {
        Self {
            last_done: quote.last_done,
            timestamp: quote.timestamp,
            volume: quote.volume,
            turnover: quote.turnover,
            high: quote.high,
            low: quote.low,
            prev_close: quote.prev_close,
        }
    }
}

impl CanonicalTrade {
    fn from_longport(trade: &longport::quote::Trade) -> Self {
        Self {
            price: trade.price,
            volume: trade.volume,
            timestamp: trade.timestamp,
            direction: canonical_trade_direction_from_longport(trade.direction),
            session: canonical_trade_session_from_longport(trade.trade_session),
            trade_type: trade.trade_type.clone(),
        }
    }
}

impl CanonicalCandle {
    fn from_longport(candle: &longport::quote::Candlestick) -> Self {
        Self {
            timestamp: candle.timestamp,
            open: candle.open,
            high: candle.high,
            low: candle.low,
            close: candle.close,
            volume: candle.volume,
            turnover: candle.turnover,
            session: canonical_trade_session_from_longport(candle.trade_session),
        }
    }
}

impl CanonicalOptionSurface {
    fn from_observation(observation: &OptionSurfaceObservation) -> Self {
        Self {
            underlying: observation.underlying.clone(),
            expiry_label: observation.expiry_label.clone(),
            atm_call_iv: observation.atm_call_iv,
            atm_put_iv: observation.atm_put_iv,
            put_call_skew: observation.put_call_skew,
            total_call_oi: observation.total_call_oi,
            total_put_oi: observation.total_put_oi,
            put_call_oi_ratio: observation.put_call_oi_ratio,
            atm_delta: observation.atm_delta,
            atm_vega: observation.atm_vega,
        }
    }
}

fn canonical_market_status_from_trade_status(
    status: longport::quote::TradeStatus,
) -> CanonicalMarketStatus {
    use longport::quote::TradeStatus;

    #[allow(unreachable_patterns)]
    match status {
        TradeStatus::Normal => CanonicalMarketStatus::Normal,
        TradeStatus::Halted | TradeStatus::Fuse | TradeStatus::SplitStockHalts => {
            CanonicalMarketStatus::Halted
        }
        TradeStatus::SuspendTrade => CanonicalMarketStatus::SuspendTrade,
        TradeStatus::PrepareList | TradeStatus::ToBeOpened | TradeStatus::WarrantPrepareList => {
            CanonicalMarketStatus::ToBeOpened
        }
        _ => CanonicalMarketStatus::Other,
    }
}

fn canonical_trade_direction_from_longport(
    direction: longport::quote::TradeDirection,
) -> CanonicalTradeDirection {
    match direction {
        longport::quote::TradeDirection::Up => CanonicalTradeDirection::Up,
        longport::quote::TradeDirection::Down => CanonicalTradeDirection::Down,
        _ => CanonicalTradeDirection::Neutral,
    }
}

fn canonical_trade_session_from_longport(
    session: longport::quote::TradeSession,
) -> CanonicalTradeSession {
    match session {
        longport::quote::TradeSession::Intraday => CanonicalTradeSession::Normal,
        longport::quote::TradeSession::Pre => CanonicalTradeSession::Pre,
        longport::quote::TradeSession::Post => CanonicalTradeSession::Post,
        longport::quote::TradeSession::Overnight => CanonicalTradeSession::Overnight,
    }
}
