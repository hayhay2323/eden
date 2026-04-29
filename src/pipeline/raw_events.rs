use std::collections::{HashMap, VecDeque};

use longport::quote::{
    Candlestick, CapitalDistributionResponse, CapitalFlowLine, IntradayLine, MarketTemperature,
    PrePostQuote, SecurityBrokers, SecurityCalcIndex, SecurityDepth, SecurityQuote, Trade,
};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use time::OffsetDateTime;

use crate::ontology::links::OptionSurfaceObservation;
use crate::ontology::links::Side;
use crate::ontology::microstructure::{
    ArchivedDepthLevel, ArchivedOrderBook, ArchivedTradeDirection, LevelChange, OrderBookDelta,
    TickArchive,
};
use crate::ontology::objects::{BrokerId, InstitutionId, Symbol};
use crate::ontology::store::ObjectStore;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawEventSource {
    Push,
    Rest,
}

#[derive(Clone, Debug)]
pub struct RawObservation<T> {
    pub observed_at: OffsetDateTime,
    pub ingested_at: OffsetDateTime,
    pub source: RawEventSource,
    pub value: T,
}

impl<T> RawObservation<T> {
    fn new(
        value: T,
        observed_at: OffsetDateTime,
        ingested_at: OffsetDateTime,
        source: RawEventSource,
    ) -> Self {
        Self {
            observed_at,
            ingested_at,
            source,
            value,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RawQueryWindow {
    TimeRange {
        start: OffsetDateTime,
        end: OffsetDateTime,
    },
    LastDuration(time::Duration),
    Recent(usize),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TradeAggressionReport {
    pub symbol: Symbol,
    pub window_start: Option<OffsetDateTime>,
    pub window_end: Option<OffsetDateTime>,
    pub trade_count: usize,
    pub buy_count: usize,
    pub sell_count: usize,
    pub neutral_count: usize,
    pub buy_volume: i64,
    pub sell_volume: i64,
    pub neutral_volume: i64,
    pub buy_notional: Decimal,
    pub sell_notional: Decimal,
    pub neutral_notional: Decimal,
    pub net_volume_imbalance: i64,
    pub net_notional_imbalance: Decimal,
    pub buy_volume_ratio: Decimal,
    pub sell_volume_ratio: Decimal,
}

impl TradeAggressionReport {
    fn empty(symbol: Symbol) -> Self {
        Self {
            symbol,
            window_start: None,
            window_end: None,
            trade_count: 0,
            buy_count: 0,
            sell_count: 0,
            neutral_count: 0,
            buy_volume: 0,
            sell_volume: 0,
            neutral_volume: 0,
            buy_notional: Decimal::ZERO,
            sell_notional: Decimal::ZERO,
            neutral_notional: Decimal::ZERO,
            net_volume_imbalance: 0,
            net_notional_imbalance: Decimal::ZERO,
            buy_volume_ratio: Decimal::ZERO,
            sell_volume_ratio: Decimal::ZERO,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimedOrderBookDelta {
    pub observed_at: OffsetDateTime,
    pub delta: OrderBookDelta,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepthEvolutionReport {
    pub symbol: Symbol,
    pub window_start: Option<OffsetDateTime>,
    pub window_end: Option<OffsetDateTime>,
    pub observation_count: usize,
    pub net_delta: Option<OrderBookDelta>,
    pub step_deltas: Vec<TimedOrderBookDelta>,
}

impl DepthEvolutionReport {
    fn empty(symbol: Symbol) -> Self {
        Self {
            symbol,
            window_start: None,
            window_end: None,
            observation_count: 0,
            net_delta: None,
            step_deltas: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerOnsetEvent {
    pub observed_at: OffsetDateTime,
    pub broker_id: BrokerId,
    pub institution_id: Option<InstitutionId>,
    pub institution_name: Option<String>,
    pub side: Side,
    pub position: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BrokerOnsetReport {
    pub symbol: Symbol,
    pub window_start: Option<OffsetDateTime>,
    pub window_end: Option<OffsetDateTime>,
    pub snapshot_count: usize,
    pub events: Vec<BrokerOnsetEvent>,
}

impl BrokerOnsetReport {
    fn empty(symbol: Symbol) -> Self {
        Self {
            symbol,
            window_start: None,
            window_end: None,
            snapshot_count: 0,
            events: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct RawMicrostructureExplanation {
    pub symbol: Symbol,
    pub window_start: Option<OffsetDateTime>,
    pub window_end: Option<OffsetDateTime>,
    pub summary: String,
    pub trade_summary: Option<String>,
    pub depth_summary: Option<String>,
    pub broker_summary: Option<String>,
}

impl RawMicrostructureExplanation {
    fn empty(symbol: Symbol) -> Self {
        Self {
            symbol,
            window_start: None,
            window_end: None,
            summary: "no recent raw observations".into(),
            trade_summary: None,
            depth_summary: None,
            broker_summary: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RawSourceExport {
    pub source: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<Symbol>,
    pub scope: String,
    pub summary: String,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub window_start: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub window_end: Option<OffsetDateTime>,
    pub payload: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteStateReport {
    pub symbol: Symbol,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub window_start: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub window_end: Option<OffsetDateTime>,
    pub observation_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_done: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub prev_close: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub low: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turnover: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pre_market_last: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub post_market_last: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overnight_last: Option<Decimal>,
}

impl QuoteStateReport {
    fn empty(symbol: Symbol) -> Self {
        Self {
            symbol,
            window_start: None,
            window_end: None,
            observation_count: 0,
            last_done: None,
            prev_close: None,
            open: None,
            high: None,
            low: None,
            volume: None,
            turnover: None,
            pre_market_last: None,
            post_market_last: None,
            overnight_last: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CandlestickStateReport {
    pub symbol: Symbol,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub window_start: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub window_end: Option<OffsetDateTime>,
    pub bar_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub high: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub low: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub close: Option<Decimal>,
    pub total_volume: i64,
    pub total_turnover: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub net_change: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub range: Option<Decimal>,
    pub bullish_bars: usize,
    pub bearish_bars: usize,
}

impl CandlestickStateReport {
    fn empty(symbol: Symbol) -> Self {
        Self {
            symbol,
            window_start: None,
            window_end: None,
            bar_count: 0,
            open: None,
            high: None,
            low: None,
            close: None,
            total_volume: 0,
            total_turnover: Decimal::ZERO,
            net_change: None,
            range: None,
            bullish_bars: 0,
            bearish_bars: 0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntradayProfileReport {
    pub symbol: Symbol,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub window_start: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub window_end: Option<OffsetDateTime>,
    pub observation_count: usize,
    pub point_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_price: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_avg_price: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub vwap_deviation: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_volume: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_turnover: Option<Decimal>,
}

impl IntradayProfileReport {
    fn empty(symbol: Symbol) -> Self {
        Self {
            symbol,
            window_start: None,
            window_end: None,
            observation_count: 0,
            point_count: 0,
            latest_price: None,
            latest_avg_price: None,
            vwap_deviation: None,
            latest_volume: None,
            latest_turnover: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapitalDistributionShiftReport {
    pub symbol: Symbol,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub window_start: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub window_end: Option<OffsetDateTime>,
    pub observation_count: usize,
    pub latest_large_net: Decimal,
    pub latest_medium_net: Decimal,
    pub latest_small_net: Decimal,
    pub delta_large_net: Decimal,
    pub delta_medium_net: Decimal,
    pub delta_small_net: Decimal,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dominant_bucket: Option<String>,
}

impl CapitalDistributionShiftReport {
    fn empty(symbol: Symbol) -> Self {
        Self {
            symbol,
            window_start: None,
            window_end: None,
            observation_count: 0,
            latest_large_net: Decimal::ZERO,
            latest_medium_net: Decimal::ZERO,
            latest_small_net: Decimal::ZERO,
            delta_large_net: Decimal::ZERO,
            delta_medium_net: Decimal::ZERO,
            delta_small_net: Decimal::ZERO,
            dominant_bucket: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapitalFlowShiftReport {
    pub symbol: Symbol,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub window_start: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub window_end: Option<OffsetDateTime>,
    pub observation_count: usize,
    pub point_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_inflow: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_inflow: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub velocity: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub acceleration: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub direction_persistence: Option<String>,
}

impl CapitalFlowShiftReport {
    fn empty(symbol: Symbol) -> Self {
        Self {
            symbol,
            window_start: None,
            window_end: None,
            observation_count: 0,
            point_count: 0,
            latest_inflow: None,
            delta_inflow: None,
            velocity: None,
            acceleration: None,
            direction_persistence: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalcIndexStateReport {
    pub symbol: Symbol,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub window_start: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub window_end: Option<OffsetDateTime>,
    pub observation_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub turnover_rate: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub volume_ratio: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capital_flow: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub change_rate: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub five_minutes_change_rate: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_volume_ratio: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_change_rate: Option<Decimal>,
}

impl CalcIndexStateReport {
    fn empty(symbol: Symbol) -> Self {
        Self {
            symbol,
            window_start: None,
            window_end: None,
            observation_count: 0,
            turnover_rate: None,
            volume_ratio: None,
            capital_flow: None,
            change_rate: None,
            five_minutes_change_rate: None,
            delta_volume_ratio: None,
            delta_change_rate: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketTemperatureStateReport {
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub window_start: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub window_end: Option<OffsetDateTime>,
    pub observation_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_temperature: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_valuation: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_sentiment: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_temperature: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delta_sentiment: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
}

impl MarketTemperatureStateReport {
    fn empty() -> Self {
        Self {
            window_start: None,
            window_end: None,
            observation_count: 0,
            latest_temperature: None,
            latest_valuation: None,
            latest_sentiment: None,
            delta_temperature: None,
            delta_sentiment: None,
            description: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OptionSurfaceStateReport {
    pub symbol: Symbol,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub window_start: Option<OffsetDateTime>,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub window_end: Option<OffsetDateTime>,
    pub observation_count: usize,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub expiry_label: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atm_call_iv: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atm_put_iv: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub put_call_skew: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_call_oi: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub total_put_oi: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub put_call_oi_ratio: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atm_delta: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub atm_vega: Option<Decimal>,
}

impl OptionSurfaceStateReport {
    fn empty(symbol: Symbol) -> Self {
        Self {
            symbol,
            window_start: None,
            window_end: None,
            observation_count: 0,
            expiry_label: None,
            atm_call_iv: None,
            atm_put_iv: None,
            put_call_skew: None,
            total_call_oi: None,
            total_put_oi: None,
            put_call_oi_ratio: None,
            atm_delta: None,
            atm_vega: None,
        }
    }
}

#[derive(Default)]
pub struct SymbolRawEvents {
    depths: VecDeque<RawObservation<SecurityDepth>>,
    brokers: VecDeque<RawObservation<SecurityBrokers>>,
    quotes: VecDeque<RawObservation<SecurityQuote>>,
    trades: VecDeque<RawObservation<Trade>>,
    candlesticks: VecDeque<RawObservation<Candlestick>>,
    intraday_lines: VecDeque<RawObservation<Vec<IntradayLine>>>,
    calc_indexes: VecDeque<RawObservation<SecurityCalcIndex>>,
    capital_flows: VecDeque<RawObservation<Vec<CapitalFlowLine>>>,
    capital_distributions: VecDeque<RawObservation<CapitalDistributionResponse>>,
    option_surfaces: VecDeque<RawObservation<OptionSurfaceObservation>>,
}

impl SymbolRawEvents {
    pub fn depths(&self) -> &VecDeque<RawObservation<SecurityDepth>> {
        &self.depths
    }

    pub fn brokers(&self) -> &VecDeque<RawObservation<SecurityBrokers>> {
        &self.brokers
    }

    pub fn quotes(&self) -> &VecDeque<RawObservation<SecurityQuote>> {
        &self.quotes
    }

    pub fn trades(&self) -> &VecDeque<RawObservation<Trade>> {
        &self.trades
    }

    pub fn candlesticks(&self) -> &VecDeque<RawObservation<Candlestick>> {
        &self.candlesticks
    }

    pub fn intraday_lines(&self) -> &VecDeque<RawObservation<Vec<IntradayLine>>> {
        &self.intraday_lines
    }

    pub fn calc_indexes(&self) -> &VecDeque<RawObservation<SecurityCalcIndex>> {
        &self.calc_indexes
    }

    pub fn capital_flows(&self) -> &VecDeque<RawObservation<Vec<CapitalFlowLine>>> {
        &self.capital_flows
    }

    pub fn capital_distributions(&self) -> &VecDeque<RawObservation<CapitalDistributionResponse>> {
        &self.capital_distributions
    }

    pub fn option_surfaces(&self) -> &VecDeque<RawObservation<OptionSurfaceObservation>> {
        &self.option_surfaces
    }
}

#[derive(Clone, Copy, Debug)]
pub struct RawEventCaps {
    pub depths_per_symbol: usize,
    pub brokers_per_symbol: usize,
    pub quotes_per_symbol: usize,
    pub trades_per_symbol: usize,
    pub candlesticks_per_symbol: usize,
    pub intraday_lines_per_symbol: usize,
    pub calc_indexes_per_symbol: usize,
    pub capital_flows_per_symbol: usize,
    pub capital_distributions_per_symbol: usize,
    pub option_surfaces_per_symbol: usize,
    pub market_temperature_events: usize,
}

impl Default for RawEventCaps {
    fn default() -> Self {
        Self {
            depths_per_symbol: 256,
            brokers_per_symbol: 256,
            quotes_per_symbol: 512,
            trades_per_symbol: 4_096,
            candlesticks_per_symbol: 240,
            intraday_lines_per_symbol: 240,
            calc_indexes_per_symbol: 240,
            capital_flows_per_symbol: 240,
            capital_distributions_per_symbol: 240,
            option_surfaces_per_symbol: 240,
            market_temperature_events: 240,
        }
    }
}

pub struct RawEventStore {
    caps: RawEventCaps,
    symbols: HashMap<Symbol, SymbolRawEvents>,
    market_temperature: VecDeque<RawObservation<MarketTemperature>>,
}

impl Default for RawEventStore {
    fn default() -> Self {
        Self::new(RawEventCaps::default())
    }
}

impl RawEventStore {
    pub fn new(caps: RawEventCaps) -> Self {
        Self {
            caps,
            symbols: HashMap::new(),
            market_temperature: VecDeque::new(),
        }
    }

    pub fn symbol_events(&self, symbol: &Symbol) -> Option<&SymbolRawEvents> {
        self.symbols.get(symbol)
    }

    pub fn market_temperature(&self) -> &VecDeque<RawObservation<MarketTemperature>> {
        &self.market_temperature
    }

    pub fn quote_state(&self, symbol: &Symbol, window: RawQueryWindow) -> QuoteStateReport {
        let Some(events) = self.symbol_events(symbol) else {
            return QuoteStateReport::empty(symbol.clone());
        };
        let indices = matching_indices(&events.quotes, window);
        if indices.is_empty() {
            return QuoteStateReport::empty(symbol.clone());
        }

        let (window_start, window_end) = window_bounds(&events.quotes, &indices);
        let latest = &events.quotes[*indices.last().unwrap()].value;
        let highs = indices
            .iter()
            .map(|index| events.quotes[*index].value.high)
            .collect::<Vec<_>>();
        let lows = indices
            .iter()
            .map(|index| events.quotes[*index].value.low)
            .collect::<Vec<_>>();

        QuoteStateReport {
            symbol: symbol.clone(),
            window_start,
            window_end,
            observation_count: indices.len(),
            last_done: Some(latest.last_done),
            prev_close: Some(latest.prev_close),
            open: Some(latest.open),
            high: highs.into_iter().max(),
            low: lows.into_iter().min(),
            volume: Some(latest.volume),
            turnover: Some(latest.turnover),
            pre_market_last: latest
                .pre_market_quote
                .as_ref()
                .map(|quote| quote.last_done),
            post_market_last: latest
                .post_market_quote
                .as_ref()
                .map(|quote| quote.last_done),
            overnight_last: latest.overnight_quote.as_ref().map(|quote| quote.last_done),
        }
    }

    pub fn candlestick_state(
        &self,
        symbol: &Symbol,
        window: RawQueryWindow,
    ) -> CandlestickStateReport {
        let Some(events) = self.symbol_events(symbol) else {
            return CandlestickStateReport::empty(symbol.clone());
        };
        let indices = matching_indices(&events.candlesticks, window);
        if indices.is_empty() {
            return CandlestickStateReport::empty(symbol.clone());
        }

        let (window_start, window_end) = window_bounds(&events.candlesticks, &indices);
        let first = &events.candlesticks[*indices.first().unwrap()].value;
        let last = &events.candlesticks[*indices.last().unwrap()].value;
        let high = indices
            .iter()
            .map(|index| events.candlesticks[*index].value.high)
            .max();
        let low = indices
            .iter()
            .map(|index| events.candlesticks[*index].value.low)
            .min();
        let total_volume = indices
            .iter()
            .map(|index| events.candlesticks[*index].value.volume)
            .sum();
        let total_turnover = indices
            .iter()
            .map(|index| events.candlesticks[*index].value.turnover)
            .sum();
        let bullish_bars = indices
            .iter()
            .filter(|index| {
                let candle = &events.candlesticks[**index].value;
                candle.close > candle.open
            })
            .count();
        let bearish_bars = indices
            .iter()
            .filter(|index| {
                let candle = &events.candlesticks[**index].value;
                candle.close < candle.open
            })
            .count();

        CandlestickStateReport {
            symbol: symbol.clone(),
            window_start,
            window_end,
            bar_count: indices.len(),
            open: Some(first.open),
            high,
            low,
            close: Some(last.close),
            total_volume,
            total_turnover,
            net_change: Some(last.close - first.open),
            range: high.zip(low).map(|(high, low)| high - low),
            bullish_bars,
            bearish_bars,
        }
    }

    pub fn intraday_profile(
        &self,
        symbol: &Symbol,
        window: RawQueryWindow,
    ) -> IntradayProfileReport {
        let Some(events) = self.symbol_events(symbol) else {
            return IntradayProfileReport::empty(symbol.clone());
        };
        let indices = matching_indices(&events.intraday_lines, window);
        if indices.is_empty() {
            return IntradayProfileReport::empty(symbol.clone());
        }

        let (window_start, window_end) = window_bounds(&events.intraday_lines, &indices);
        let latest_snapshot = &events.intraday_lines[*indices.last().unwrap()].value;
        let Some(latest_line) = latest_snapshot.last() else {
            return IntradayProfileReport::empty(symbol.clone());
        };

        IntradayProfileReport {
            symbol: symbol.clone(),
            window_start,
            window_end,
            observation_count: indices.len(),
            point_count: latest_snapshot.len(),
            latest_price: Some(latest_line.price),
            latest_avg_price: Some(latest_line.avg_price),
            vwap_deviation: (latest_line.avg_price > Decimal::ZERO)
                .then_some((latest_line.price - latest_line.avg_price) / latest_line.avg_price),
            latest_volume: Some(latest_line.volume),
            latest_turnover: Some(latest_line.turnover),
        }
    }

    pub fn capital_distribution_shift(
        &self,
        symbol: &Symbol,
        window: RawQueryWindow,
    ) -> CapitalDistributionShiftReport {
        let Some(events) = self.symbol_events(symbol) else {
            return CapitalDistributionShiftReport::empty(symbol.clone());
        };
        let indices = matching_indices(&events.capital_distributions, window);
        if indices.is_empty() {
            return CapitalDistributionShiftReport::empty(symbol.clone());
        }

        let (window_start, window_end) = window_bounds(&events.capital_distributions, &indices);
        let first = &events.capital_distributions[*indices.first().unwrap()].value;
        let latest = &events.capital_distributions[*indices.last().unwrap()].value;
        let latest_large = latest.capital_in.large - latest.capital_out.large;
        let latest_medium = latest.capital_in.medium - latest.capital_out.medium;
        let latest_small = latest.capital_in.small - latest.capital_out.small;
        let first_large = first.capital_in.large - first.capital_out.large;
        let first_medium = first.capital_in.medium - first.capital_out.medium;
        let first_small = first.capital_in.small - first.capital_out.small;
        let dominant_bucket = [
            ("large", latest_large.abs()),
            ("medium", latest_medium.abs()),
            ("small", latest_small.abs()),
        ]
        .into_iter()
        .max_by(|a, b| a.1.cmp(&b.1))
        .map(|(label, _)| label.to_string());

        CapitalDistributionShiftReport {
            symbol: symbol.clone(),
            window_start,
            window_end,
            observation_count: indices.len(),
            latest_large_net: latest_large,
            latest_medium_net: latest_medium,
            latest_small_net: latest_small,
            delta_large_net: latest_large - first_large,
            delta_medium_net: latest_medium - first_medium,
            delta_small_net: latest_small - first_small,
            dominant_bucket,
        }
    }

    pub fn capital_flow_shift(
        &self,
        symbol: &Symbol,
        window: RawQueryWindow,
    ) -> CapitalFlowShiftReport {
        let Some(events) = self.symbol_events(symbol) else {
            return CapitalFlowShiftReport::empty(symbol.clone());
        };
        let indices = matching_indices(&events.capital_flows, window);
        if indices.is_empty() {
            return CapitalFlowShiftReport::empty(symbol.clone());
        }

        let (window_start, window_end) = window_bounds(&events.capital_flows, &indices);
        let first = &events.capital_flows[*indices.first().unwrap()].value;
        let latest = &events.capital_flows[*indices.last().unwrap()].value;
        let latest_inflow = latest.last().map(|line| line.inflow);
        let first_inflow = first.last().map(|line| line.inflow);
        let velocity = flow_series_velocity(latest);
        let previous_velocity = indices
            .get(indices.len().saturating_sub(2))
            .and_then(|index| flow_series_velocity(&events.capital_flows[*index].value));
        let acceleration = velocity
            .zip(previous_velocity)
            .map(|(now, prev)| now - prev);
        let direction_persistence = match (first_inflow, latest_inflow) {
            (Some(first), Some(last)) if first > Decimal::ZERO && last > Decimal::ZERO => {
                Some("positive".into())
            }
            (Some(first), Some(last)) if first < Decimal::ZERO && last < Decimal::ZERO => {
                Some("negative".into())
            }
            (Some(_), Some(_)) => Some("mixed".into()),
            _ => None,
        };

        CapitalFlowShiftReport {
            symbol: symbol.clone(),
            window_start,
            window_end,
            observation_count: indices.len(),
            point_count: latest.len(),
            latest_inflow,
            delta_inflow: latest_inflow
                .zip(first_inflow)
                .map(|(last, first)| last - first),
            velocity,
            acceleration,
            direction_persistence,
        }
    }

    pub fn calc_index_state(
        &self,
        symbol: &Symbol,
        window: RawQueryWindow,
    ) -> CalcIndexStateReport {
        let Some(events) = self.symbol_events(symbol) else {
            return CalcIndexStateReport::empty(symbol.clone());
        };
        let indices = matching_indices(&events.calc_indexes, window);
        if indices.is_empty() {
            return CalcIndexStateReport::empty(symbol.clone());
        }

        let (window_start, window_end) = window_bounds(&events.calc_indexes, &indices);
        let first = &events.calc_indexes[*indices.first().unwrap()].value;
        let latest = &events.calc_indexes[*indices.last().unwrap()].value;

        CalcIndexStateReport {
            symbol: symbol.clone(),
            window_start,
            window_end,
            observation_count: indices.len(),
            turnover_rate: latest.turnover_rate,
            volume_ratio: latest.volume_ratio,
            capital_flow: latest.capital_flow,
            change_rate: latest.change_rate,
            five_minutes_change_rate: latest.five_minutes_change_rate,
            delta_volume_ratio: latest
                .volume_ratio
                .zip(first.volume_ratio)
                .map(|(last, first)| last - first),
            delta_change_rate: latest
                .change_rate
                .zip(first.change_rate)
                .map(|(last, first)| last - first),
        }
    }

    pub fn market_temperature_state(&self, window: RawQueryWindow) -> MarketTemperatureStateReport {
        let indices = matching_indices(&self.market_temperature, window);
        if indices.is_empty() {
            return MarketTemperatureStateReport::empty();
        }

        let (window_start, window_end) = window_bounds(&self.market_temperature, &indices);
        let first = &self.market_temperature[*indices.first().unwrap()].value;
        let latest = &self.market_temperature[*indices.last().unwrap()].value;

        MarketTemperatureStateReport {
            window_start,
            window_end,
            observation_count: indices.len(),
            latest_temperature: Some(i64::from(latest.temperature)),
            latest_valuation: Some(i64::from(latest.valuation)),
            latest_sentiment: Some(i64::from(latest.sentiment)),
            delta_temperature: Some(i64::from(latest.temperature - first.temperature)),
            delta_sentiment: Some(i64::from(latest.sentiment - first.sentiment)),
            description: Some(latest.description.clone()),
        }
    }

    pub fn option_surface_state(
        &self,
        symbol: &Symbol,
        window: RawQueryWindow,
    ) -> OptionSurfaceStateReport {
        let Some(events) = self.symbol_events(symbol) else {
            return OptionSurfaceStateReport::empty(symbol.clone());
        };
        let indices = matching_indices(&events.option_surfaces, window);
        if indices.is_empty() {
            return OptionSurfaceStateReport::empty(symbol.clone());
        }

        let (window_start, window_end) = window_bounds(&events.option_surfaces, &indices);
        let latest = &events.option_surfaces[*indices.last().unwrap()].value;

        OptionSurfaceStateReport {
            symbol: symbol.clone(),
            window_start,
            window_end,
            observation_count: indices.len(),
            expiry_label: Some(latest.expiry_label.clone()),
            atm_call_iv: latest.atm_call_iv,
            atm_put_iv: latest.atm_put_iv,
            put_call_skew: latest.put_call_skew,
            total_call_oi: Some(latest.total_call_oi),
            total_put_oi: Some(latest.total_put_oi),
            put_call_oi_ratio: latest.put_call_oi_ratio,
            atm_delta: latest.atm_delta,
            atm_vega: latest.atm_vega,
        }
    }

    pub fn trade_aggression(
        &self,
        symbol: &Symbol,
        window: RawQueryWindow,
    ) -> TradeAggressionReport {
        let Some(events) = self.symbol_events(symbol) else {
            return TradeAggressionReport::empty(symbol.clone());
        };
        let indices = matching_indices(&events.trades, window);
        if indices.is_empty() {
            return TradeAggressionReport::empty(symbol.clone());
        }

        let (window_start, window_end) = window_bounds(&events.trades, &indices);
        let mut report = TradeAggressionReport {
            symbol: symbol.clone(),
            window_start,
            window_end,
            ..TradeAggressionReport::empty(symbol.clone())
        };

        for index in indices {
            let trade = &events.trades[index].value;
            let notional = trade.price * Decimal::from(trade.volume);
            report.trade_count += 1;
            match ArchivedTradeDirection::from_longport(trade.direction) {
                ArchivedTradeDirection::Up => {
                    report.buy_count += 1;
                    report.buy_volume += trade.volume;
                    report.buy_notional += notional;
                }
                ArchivedTradeDirection::Down => {
                    report.sell_count += 1;
                    report.sell_volume += trade.volume;
                    report.sell_notional += notional;
                }
                ArchivedTradeDirection::Neutral => {
                    report.neutral_count += 1;
                    report.neutral_volume += trade.volume;
                    report.neutral_notional += notional;
                }
            }
        }

        let total_volume = report.buy_volume + report.sell_volume + report.neutral_volume;
        if total_volume > 0 {
            let total = Decimal::from(total_volume);
            report.buy_volume_ratio = Decimal::from(report.buy_volume) / total;
            report.sell_volume_ratio = Decimal::from(report.sell_volume) / total;
        }

        report.net_volume_imbalance = report.buy_volume - report.sell_volume;
        report.net_notional_imbalance = report.buy_notional - report.sell_notional;
        report
    }

    pub fn depth_evolution(&self, symbol: &Symbol, window: RawQueryWindow) -> DepthEvolutionReport {
        let Some(events) = self.symbol_events(symbol) else {
            return DepthEvolutionReport::empty(symbol.clone());
        };
        let indices = matching_indices(&events.depths, window);
        if indices.is_empty() {
            return DepthEvolutionReport::empty(symbol.clone());
        }

        let (window_start, window_end) = window_bounds(&events.depths, &indices);
        let mut report = DepthEvolutionReport {
            symbol: symbol.clone(),
            window_start,
            window_end,
            observation_count: indices.len(),
            net_delta: None,
            step_deltas: Vec::new(),
        };

        if indices.len() < 2 {
            return report;
        }

        let first = archived_order_book(symbol, &events.depths[indices[0]]);
        let last = archived_order_book(symbol, &events.depths[*indices.last().unwrap()]);
        report.net_delta = Some(last.diff(&first));

        for pair in indices.windows(2) {
            let previous = archived_order_book(symbol, &events.depths[pair[0]]);
            let current_obs = &events.depths[pair[1]];
            let current = archived_order_book(symbol, current_obs);
            report.step_deltas.push(TimedOrderBookDelta {
                observed_at: current_obs.observed_at,
                delta: current.diff(&previous),
            });
        }

        report
    }

    pub fn broker_onset(
        &self,
        symbol: &Symbol,
        window: RawQueryWindow,
        store: &ObjectStore,
    ) -> BrokerOnsetReport {
        let Some(events) = self.symbol_events(symbol) else {
            return BrokerOnsetReport::empty(symbol.clone());
        };
        let indices = matching_indices(&events.brokers, window);
        if indices.is_empty() {
            return BrokerOnsetReport::empty(symbol.clone());
        }

        let (window_start, window_end) = window_bounds(&events.brokers, &indices);
        let mut report = BrokerOnsetReport {
            symbol: symbol.clone(),
            window_start,
            window_end,
            snapshot_count: indices.len(),
            events: Vec::new(),
        };

        for index in indices {
            let current = &events.brokers[index];
            let Some(previous) = index
                .checked_sub(1)
                .and_then(|previous_index| events.brokers.get(previous_index))
            else {
                continue;
            };

            let previous_presence = broker_presence(&previous.value);
            for (broker_id, side, position) in broker_entries(&current.value) {
                if previous_presence.contains_key(&(broker_id, side)) {
                    continue;
                }
                let institution = store.institution_for_broker(&broker_id);
                report.events.push(BrokerOnsetEvent {
                    observed_at: current.observed_at,
                    broker_id,
                    institution_id: institution.map(|inst| inst.id),
                    institution_name: institution.map(|inst| inst.name_en.clone()),
                    side,
                    position,
                });
            }
        }

        report
    }

    pub fn explain_microstructure(
        &self,
        symbol: &Symbol,
        window: RawQueryWindow,
        store: &ObjectStore,
    ) -> RawMicrostructureExplanation {
        let trades = self.trade_aggression(symbol, window);
        let depth = self.depth_evolution(symbol, window);
        let brokers = self.broker_onset(symbol, window, store);

        let window_start = trades
            .window_start
            .or(depth.window_start)
            .or(brokers.window_start);
        let window_end = trades
            .window_end
            .or(depth.window_end)
            .or(brokers.window_end);

        if trades.trade_count == 0 && depth.observation_count == 0 && brokers.snapshot_count == 0 {
            return RawMicrostructureExplanation::empty(symbol.clone());
        }

        let trade_summary = build_trade_summary(&trades);
        let depth_summary = build_depth_summary(&depth);
        let broker_summary = build_broker_summary(&brokers);

        let mut parts = Vec::new();
        if let Some(summary) = &trade_summary {
            parts.push(summary.clone());
        }
        if let Some(summary) = &depth_summary {
            parts.push(summary.clone());
        }
        if let Some(summary) = &broker_summary {
            parts.push(summary.clone());
        }

        let summary = if parts.is_empty() {
            "raw tape is mixed with no clear directional dominance".into()
        } else {
            parts.join("; ")
        };

        RawMicrostructureExplanation {
            symbol: symbol.clone(),
            window_start,
            window_end,
            summary,
            trade_summary,
            depth_summary,
            broker_summary,
        }
    }

    pub fn record_depth(
        &mut self,
        symbol: Symbol,
        depth: SecurityDepth,
        ingested_at: OffsetDateTime,
        source: RawEventSource,
    ) {
        let cap = self.caps.depths_per_symbol;
        let events = self.symbol_events_mut(symbol);
        Self::push_capped(
            &mut events.depths,
            cap,
            RawObservation::new(depth, ingested_at, ingested_at, source),
        );
    }

    pub fn record_brokers(
        &mut self,
        symbol: Symbol,
        brokers: SecurityBrokers,
        ingested_at: OffsetDateTime,
        source: RawEventSource,
    ) {
        let cap = self.caps.brokers_per_symbol;
        let events = self.symbol_events_mut(symbol);
        Self::push_capped(
            &mut events.brokers,
            cap,
            RawObservation::new(brokers, ingested_at, ingested_at, source),
        );
    }

    pub fn record_quote(&mut self, symbol: Symbol, quote: SecurityQuote, source: RawEventSource) {
        let cap = self.caps.quotes_per_symbol;
        let observed_at = quote.timestamp;
        let events = self.symbol_events_mut(symbol);
        Self::push_capped(
            &mut events.quotes,
            cap,
            RawObservation::new(quote, observed_at, OffsetDateTime::now_utc(), source),
        );
    }

    pub fn record_trades(
        &mut self,
        symbol: Symbol,
        trades: &[Trade],
        ingested_at: OffsetDateTime,
        source: RawEventSource,
    ) {
        let cap = self.caps.trades_per_symbol;
        let events = self.symbol_events_mut(symbol);
        for trade in trades {
            Self::push_capped(
                &mut events.trades,
                cap,
                RawObservation::new(trade.clone(), trade.timestamp, ingested_at, source),
            );
        }
    }

    pub fn record_candlestick(
        &mut self,
        symbol: Symbol,
        candlestick: Candlestick,
        source: RawEventSource,
    ) {
        let cap = self.caps.candlesticks_per_symbol;
        let observed_at = candlestick.timestamp;
        let events = self.symbol_events_mut(symbol);
        Self::push_capped(
            &mut events.candlesticks,
            cap,
            RawObservation::new(candlestick, observed_at, OffsetDateTime::now_utc(), source),
        );
    }

    pub fn record_intraday_snapshot(
        &mut self,
        intraday_lines: &HashMap<Symbol, Vec<IntradayLine>>,
        ingested_at: OffsetDateTime,
        source: RawEventSource,
    ) {
        let cap = self.caps.intraday_lines_per_symbol;
        for (symbol, lines) in intraday_lines {
            if lines.is_empty() {
                continue;
            }
            let observed_at = lines
                .last()
                .map(|line| line.timestamp)
                .unwrap_or(ingested_at);
            let events = self.symbol_events_mut(symbol.clone());
            Self::push_capped(
                &mut events.intraday_lines,
                cap,
                RawObservation::new(lines.clone(), observed_at, ingested_at, source),
            );
        }
    }

    pub fn record_calc_index_snapshot(
        &mut self,
        calc_indexes: &HashMap<Symbol, SecurityCalcIndex>,
        ingested_at: OffsetDateTime,
        source: RawEventSource,
    ) {
        let cap = self.caps.calc_indexes_per_symbol;
        for (symbol, calc_index) in calc_indexes {
            let events = self.symbol_events_mut(symbol.clone());
            Self::push_capped(
                &mut events.calc_indexes,
                cap,
                RawObservation::new(calc_index.clone(), ingested_at, ingested_at, source),
            );
        }
    }

    pub fn record_capital_flow_snapshot(
        &mut self,
        capital_flows: &HashMap<Symbol, Vec<CapitalFlowLine>>,
        ingested_at: OffsetDateTime,
        source: RawEventSource,
    ) {
        let cap = self.caps.capital_flows_per_symbol;
        for (symbol, lines) in capital_flows {
            if lines.is_empty() {
                continue;
            }
            let observed_at = lines
                .last()
                .map(|line| line.timestamp)
                .unwrap_or(ingested_at);
            let events = self.symbol_events_mut(symbol.clone());
            Self::push_capped(
                &mut events.capital_flows,
                cap,
                RawObservation::new(lines.clone(), observed_at, ingested_at, source),
            );
        }
    }

    pub fn record_capital_distribution_snapshot(
        &mut self,
        capital_distributions: &HashMap<Symbol, CapitalDistributionResponse>,
        ingested_at: OffsetDateTime,
        source: RawEventSource,
    ) {
        let cap = self.caps.capital_distributions_per_symbol;
        for (symbol, distribution) in capital_distributions {
            let events = self.symbol_events_mut(symbol.clone());
            Self::push_capped(
                &mut events.capital_distributions,
                cap,
                RawObservation::new(distribution.clone(), ingested_at, ingested_at, source),
            );
        }
    }

    pub fn record_market_temperature(
        &mut self,
        temperature: MarketTemperature,
        source: RawEventSource,
    ) {
        Self::push_capped(
            &mut self.market_temperature,
            self.caps.market_temperature_events,
            RawObservation::new(
                temperature.clone(),
                temperature.timestamp,
                OffsetDateTime::now_utc(),
                source,
            ),
        );
    }

    pub fn record_option_surface_snapshot(
        &mut self,
        option_surfaces: &[OptionSurfaceObservation],
        ingested_at: OffsetDateTime,
        source: RawEventSource,
    ) {
        let cap = self.caps.option_surfaces_per_symbol;
        for surface in option_surfaces {
            let events = self.symbol_events_mut(surface.underlying.clone());
            Self::push_capped(
                &mut events.option_surfaces,
                cap,
                RawObservation::new(surface.clone(), ingested_at, ingested_at, source),
            );
        }
    }

    pub fn ingest_tick_archive(&mut self, archive: &TickArchive, source: RawEventSource) {
        for order_book in &archive.order_books {
            self.record_depth(
                order_book.symbol.clone(),
                SecurityDepth {
                    asks: order_book
                        .ask_levels
                        .iter()
                        .map(|level| longport::quote::Depth {
                            position: level.position,
                            price: level.price,
                            volume: level.volume,
                            order_num: level.order_num,
                        })
                        .collect(),
                    bids: order_book
                        .bid_levels
                        .iter()
                        .map(|level| longport::quote::Depth {
                            position: level.position,
                            price: level.price,
                            volume: level.volume,
                            order_num: level.order_num,
                        })
                        .collect(),
                },
                order_book.timestamp,
                source,
            );
        }

        let mut broker_snapshots: HashMap<Symbol, SecurityBrokers> = HashMap::new();
        for entry in &archive.broker_queues {
            let snapshot = broker_snapshots
                .entry(entry.symbol.clone())
                .or_insert_with(|| SecurityBrokers {
                    ask_brokers: Vec::new(),
                    bid_brokers: Vec::new(),
                });
            let groups = if entry.side.eq_ignore_ascii_case("bid") {
                &mut snapshot.bid_brokers
            } else {
                &mut snapshot.ask_brokers
            };
            if let Some(group) = groups
                .iter_mut()
                .find(|group| group.position == entry.position)
            {
                group.broker_ids.push(entry.broker_id);
            } else {
                groups.push(longport::quote::Brokers {
                    position: entry.position,
                    broker_ids: vec![entry.broker_id],
                });
            }
        }
        for (symbol, brokers) in broker_snapshots {
            self.record_brokers(symbol, brokers, archive.timestamp, source);
        }

        for quote in &archive.quotes {
            self.record_quote(
                quote.symbol.clone(),
                SecurityQuote {
                    symbol: quote.symbol.0.clone(),
                    last_done: quote.last_done,
                    prev_close: quote.prev_close,
                    open: quote.open,
                    high: quote.high,
                    low: quote.low,
                    timestamp: quote.timestamp,
                    volume: quote.volume,
                    turnover: quote.turnover,
                    trade_status: longport::quote::TradeStatus::Normal,
                    pre_market_quote: quote.pre_market.as_ref().map(|item| PrePostQuote {
                        last_done: item.last_done,
                        timestamp: item.timestamp,
                        volume: item.volume,
                        turnover: item.turnover,
                        high: item.high,
                        low: item.low,
                        prev_close: item.prev_close,
                    }),
                    post_market_quote: quote.post_market.as_ref().map(|item| PrePostQuote {
                        last_done: item.last_done,
                        timestamp: item.timestamp,
                        volume: item.volume,
                        turnover: item.turnover,
                        high: item.high,
                        low: item.low,
                        prev_close: item.prev_close,
                    }),
                    overnight_quote: quote.overnight.as_ref().map(|item| PrePostQuote {
                        last_done: item.last_done,
                        timestamp: item.timestamp,
                        volume: item.volume,
                        turnover: item.turnover,
                        high: item.high,
                        low: item.low,
                        prev_close: item.prev_close,
                    }),
                },
                source,
            );
        }

        let mut trades_by_symbol: HashMap<Symbol, Vec<Trade>> = HashMap::new();
        for trade in &archive.trades {
            trades_by_symbol
                .entry(trade.symbol.clone())
                .or_default()
                .push(Trade {
                    price: trade.price,
                    volume: trade.volume,
                    timestamp: trade.timestamp,
                    trade_type: trade.trade_type.clone(),
                    direction: match trade.direction {
                        ArchivedTradeDirection::Up => longport::quote::TradeDirection::Up,
                        ArchivedTradeDirection::Down => longport::quote::TradeDirection::Down,
                        ArchivedTradeDirection::Neutral => longport::quote::TradeDirection::Neutral,
                    },
                    trade_session: match trade.session {
                        crate::ontology::microstructure::TradeSession::Normal => {
                            longport::quote::TradeSession::Intraday
                        }
                        crate::ontology::microstructure::TradeSession::Pre => {
                            longport::quote::TradeSession::Pre
                        }
                        crate::ontology::microstructure::TradeSession::Post => {
                            longport::quote::TradeSession::Post
                        }
                        crate::ontology::microstructure::TradeSession::Overnight => {
                            longport::quote::TradeSession::Overnight
                        }
                    },
                });
        }
        for (symbol, trades) in trades_by_symbol {
            self.record_trades(symbol, &trades, archive.timestamp, source);
        }

        let mut candlesticks_by_symbol: HashMap<Symbol, Vec<Candlestick>> = HashMap::new();
        for candle in &archive.candlesticks {
            candlesticks_by_symbol
                .entry(candle.symbol.clone())
                .or_default()
                .push(serde_json::from_value(serde_json::json!({
                    "close": candle.close,
                    "open": candle.open,
                    "low": candle.low,
                    "high": candle.high,
                    "volume": candle.volume,
                    "turnover": candle.turnover,
                    "timestamp": candle.timestamp.format(&time::format_description::well_known::Rfc3339).ok(),
                    "trade_session": match candle.session {
                        crate::ontology::microstructure::TradeSession::Normal => "Intraday",
                        crate::ontology::microstructure::TradeSession::Pre => "Pre",
                        crate::ontology::microstructure::TradeSession::Post => "Post",
                        crate::ontology::microstructure::TradeSession::Overnight => "Overnight",
                    }
                })).unwrap());
        }
        for (symbol, candles) in candlesticks_by_symbol {
            for candle in candles {
                self.record_candlestick(symbol.clone(), candle, source);
            }
        }

        let mut capital_flows = HashMap::new();
        for series in &archive.capital_flows {
            capital_flows.insert(
                series.symbol.clone(),
                series
                    .points
                    .iter()
                    .map(|point| CapitalFlowLine {
                        inflow: point.inflow,
                        timestamp: point.timestamp,
                    })
                    .collect::<Vec<_>>(),
            );
        }
        self.record_capital_flow_snapshot(&capital_flows, archive.timestamp, source);

        let mut capital_distributions = HashMap::new();
        for distribution in &archive.capital_distributions {
            capital_distributions.insert(
                distribution.symbol.clone(),
                CapitalDistributionResponse {
                    timestamp: distribution.timestamp,
                    capital_in: longport::quote::CapitalDistribution {
                        large: distribution.large_in,
                        medium: distribution.medium_in,
                        small: distribution.small_in,
                    },
                    capital_out: longport::quote::CapitalDistribution {
                        large: distribution.large_out,
                        medium: distribution.medium_out,
                        small: distribution.small_out,
                    },
                },
            );
        }
        self.record_capital_distribution_snapshot(
            &capital_distributions,
            archive.timestamp,
            source,
        );

        let mut intraday_lines = HashMap::new();
        for line in &archive.intraday {
            intraday_lines
                .entry(line.symbol.clone())
                .or_insert_with(Vec::new)
                .push(serde_json::from_value(serde_json::json!({
                    "price": line.price,
                    "timestamp": line.timestamp.format(&time::format_description::well_known::Rfc3339).ok(),
                    "volume": line.volume,
                    "turnover": line.turnover,
                    "avg_price": line.avg_price
                })).unwrap());
        }
        self.record_intraday_snapshot(&intraday_lines, archive.timestamp, source);

        let calc_indexes = archive
            .calc_indexes
            .iter()
            .map(|item| {
                (
                    item.symbol.clone(),
                    serde_json::from_value(serde_json::json!({
                        "symbol": item.symbol.0,
                        "turnover_rate": item.turnover_rate,
                        "volume_ratio": item.volume_ratio,
                        "pe_ttm_ratio": item.pe_ttm_ratio,
                        "pb_ratio": item.pb_ratio,
                        "dividend_ratio_ttm": item.dividend_ratio_ttm,
                        "amplitude": item.amplitude,
                        "five_minutes_change_rate": item.five_minutes_change_rate,
                        "change_rate": item.change_rate,
                        "ytd_change_rate": item.ytd_change_rate,
                        "five_day_change_rate": item.five_day_change_rate,
                        "ten_day_change_rate": item.ten_day_change_rate,
                        "half_year_change_rate": item.half_year_change_rate,
                        "total_market_value": item.total_market_value,
                        "capital_flow": item.capital_flow
                    }))
                    .unwrap(),
                )
            })
            .collect::<HashMap<_, _>>();
        self.record_calc_index_snapshot(&calc_indexes, archive.timestamp, source);

        if let Some(temp) = &archive.market_temperature {
            self.record_market_temperature(
                MarketTemperature {
                    temperature: i32::try_from(
                        temp.temperature.round_dp(0).to_i64().unwrap_or_default(),
                    )
                    .unwrap_or_default(),
                    valuation: i32::try_from(
                        temp.valuation.round_dp(0).to_i64().unwrap_or_default(),
                    )
                    .unwrap_or_default(),
                    sentiment: i32::try_from(
                        temp.sentiment.round_dp(0).to_i64().unwrap_or_default(),
                    )
                    .unwrap_or_default(),
                    description: temp.description.clone(),
                    timestamp: temp.timestamp,
                },
                source,
            );
        }

        let option_surfaces = archive
            .option_surfaces
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
            .collect::<Vec<_>>();
        self.record_option_surface_snapshot(&option_surfaces, archive.timestamp, source);
    }

    pub fn export_longport_sources(
        &self,
        symbol: &Symbol,
        window: RawQueryWindow,
        store: &ObjectStore,
    ) -> Vec<RawSourceExport> {
        let quote = self.quote_state(symbol, window);
        let candles = self.candlestick_state(symbol, window);
        let intraday = self.intraday_profile(symbol, window);
        let trade = self.trade_aggression(symbol, window);
        let depth = self.depth_evolution(symbol, window);
        let broker = self.broker_onset(symbol, window, store);
        let capital_distribution = self.capital_distribution_shift(symbol, window);
        let capital_flow = self.capital_flow_shift(symbol, window);
        let calc_index = self.calc_index_state(symbol, window);
        let option_surface = self.option_surface_state(symbol, window);

        vec![
            RawSourceExport {
                source: "trade".into(),
                symbol: Some(symbol.clone()),
                scope: "symbol".into(),
                summary: describe_trade_flow(&trade),
                window_start: trade.window_start,
                window_end: trade.window_end,
                payload: serde_json::to_value(&trade).unwrap_or(Value::Null),
            },
            RawSourceExport {
                source: "depth".into(),
                symbol: Some(symbol.clone()),
                scope: "symbol".into(),
                summary: describe_depth(&depth),
                window_start: depth.window_start,
                window_end: depth.window_end,
                payload: serde_json::to_value(&depth).unwrap_or(Value::Null),
            },
            RawSourceExport {
                source: "broker".into(),
                symbol: Some(symbol.clone()),
                scope: "symbol".into(),
                summary: describe_broker_queue(&broker),
                window_start: broker.window_start,
                window_end: broker.window_end,
                payload: serde_json::to_value(&broker).unwrap_or(Value::Null),
            },
            RawSourceExport {
                source: "quote".into(),
                symbol: Some(symbol.clone()),
                scope: "symbol".into(),
                summary: describe_quote_state(&quote),
                window_start: quote.window_start,
                window_end: quote.window_end,
                payload: serde_json::to_value(&quote).unwrap_or(Value::Null),
            },
            RawSourceExport {
                source: "candlestick".into(),
                symbol: Some(symbol.clone()),
                scope: "symbol".into(),
                summary: describe_candles(&candles),
                window_start: candles.window_start,
                window_end: candles.window_end,
                payload: serde_json::to_value(&candles).unwrap_or(Value::Null),
            },
            RawSourceExport {
                source: "intraday".into(),
                symbol: Some(symbol.clone()),
                scope: "symbol".into(),
                summary: describe_intraday_profile(&intraday),
                window_start: intraday.window_start,
                window_end: intraday.window_end,
                payload: serde_json::to_value(&intraday).unwrap_or(Value::Null),
            },
            RawSourceExport {
                source: "capital_distribution".into(),
                symbol: Some(symbol.clone()),
                scope: "symbol".into(),
                summary: describe_capital_distribution(&capital_distribution),
                window_start: capital_distribution.window_start,
                window_end: capital_distribution.window_end,
                payload: serde_json::to_value(&capital_distribution).unwrap_or(Value::Null),
            },
            RawSourceExport {
                source: "capital_flow".into(),
                symbol: Some(symbol.clone()),
                scope: "symbol".into(),
                summary: describe_capital_flow(&capital_flow),
                window_start: capital_flow.window_start,
                window_end: capital_flow.window_end,
                payload: serde_json::to_value(&capital_flow).unwrap_or(Value::Null),
            },
            RawSourceExport {
                source: "calc_index".into(),
                symbol: Some(symbol.clone()),
                scope: "symbol".into(),
                summary: describe_calc_indexes(&calc_index),
                window_start: calc_index.window_start,
                window_end: calc_index.window_end,
                payload: serde_json::to_value(&calc_index).unwrap_or(Value::Null),
            },
            RawSourceExport {
                source: "option_surface".into(),
                symbol: Some(symbol.clone()),
                scope: "symbol".into(),
                summary: describe_option_surface(&option_surface),
                window_start: option_surface.window_start,
                window_end: option_surface.window_end,
                payload: serde_json::to_value(&option_surface).unwrap_or(Value::Null),
            },
        ]
    }

    pub fn export_market_sources(&self, window: RawQueryWindow) -> Vec<RawSourceExport> {
        let market_temperature = self.market_temperature_state(window);
        vec![RawSourceExport {
            source: "market_temperature".into(),
            symbol: None,
            scope: "market".into(),
            summary: describe_market_temperature(&market_temperature),
            window_start: market_temperature.window_start,
            window_end: market_temperature.window_end,
            payload: serde_json::to_value(&market_temperature).unwrap_or(Value::Null),
        }]
    }

    fn symbol_events_mut(&mut self, symbol: Symbol) -> &mut SymbolRawEvents {
        self.symbols.entry(symbol).or_default()
    }

    fn push_capped<T>(
        queue: &mut VecDeque<RawObservation<T>>,
        cap: usize,
        observation: RawObservation<T>,
    ) {
        queue.push_back(observation);
        if queue.len() > cap {
            queue.pop_front();
        }
    }
}

fn matching_indices<T>(
    observations: &VecDeque<RawObservation<T>>,
    window: RawQueryWindow,
) -> Vec<usize> {
    match window {
        RawQueryWindow::TimeRange { start, end } => {
            if start > end {
                return Vec::new();
            }
            observations
                .iter()
                .enumerate()
                .filter_map(|(index, observation)| {
                    (observation.observed_at >= start && observation.observed_at <= end)
                        .then_some(index)
                })
                .collect()
        }
        RawQueryWindow::LastDuration(duration) => {
            let Some(latest) = observations
                .back()
                .map(|observation| observation.observed_at)
            else {
                return Vec::new();
            };
            let start = latest - duration;
            observations
                .iter()
                .enumerate()
                .filter_map(|(index, observation)| {
                    (observation.observed_at >= start && observation.observed_at <= latest)
                        .then_some(index)
                })
                .collect()
        }
        RawQueryWindow::Recent(count) => {
            if count == 0 || observations.is_empty() {
                return Vec::new();
            }
            let start = observations.len().saturating_sub(count);
            (start..observations.len()).collect()
        }
    }
}

fn window_bounds<T>(
    observations: &VecDeque<RawObservation<T>>,
    indices: &[usize],
) -> (Option<OffsetDateTime>, Option<OffsetDateTime>) {
    let Some(first_index) = indices.first() else {
        return (None, None);
    };
    let Some(last_index) = indices.last() else {
        return (None, None);
    };
    (
        observations
            .get(*first_index)
            .map(|observation| observation.observed_at),
        observations
            .get(*last_index)
            .map(|observation| observation.observed_at),
    )
}

fn archived_order_book(
    symbol: &Symbol,
    observation: &RawObservation<SecurityDepth>,
) -> ArchivedOrderBook {
    ArchivedOrderBook {
        symbol: symbol.clone(),
        timestamp: observation.observed_at,
        ask_levels: observation
            .value
            .asks
            .iter()
            .map(|level| ArchivedDepthLevel {
                position: level.position,
                price: level.price,
                volume: level.volume,
                order_num: level.order_num,
            })
            .collect(),
        bid_levels: observation
            .value
            .bids
            .iter()
            .map(|level| ArchivedDepthLevel {
                position: level.position,
                price: level.price,
                volume: level.volume,
                order_num: level.order_num,
            })
            .collect(),
    }
}

fn broker_entries(brokers: &SecurityBrokers) -> Vec<(BrokerId, Side, i32)> {
    let mut entries = Vec::new();
    for group in &brokers.ask_brokers {
        for &broker_id in &group.broker_ids {
            entries.push((BrokerId(broker_id), Side::Ask, group.position));
        }
    }
    for group in &brokers.bid_brokers {
        for &broker_id in &group.broker_ids {
            entries.push((BrokerId(broker_id), Side::Bid, group.position));
        }
    }
    entries
}

fn broker_presence(brokers: &SecurityBrokers) -> HashMap<(BrokerId, Side), i32> {
    broker_entries(brokers)
        .into_iter()
        .map(|(broker_id, side, position)| ((broker_id, side), position))
        .collect()
}

fn flow_series_velocity(lines: &[CapitalFlowLine]) -> Option<Decimal> {
    if lines.len() < 2 {
        return None;
    }
    let latest = lines.last()?;
    let previous = &lines[lines.len() - 2];
    let dt_seconds = (latest.timestamp - previous.timestamp).whole_seconds();
    if dt_seconds <= 0 {
        return None;
    }
    let dt_minutes = Decimal::from(dt_seconds) / Decimal::from(60);
    Some((latest.inflow - previous.inflow) / dt_minutes)
}

fn describe_trade_flow(report: &TradeAggressionReport) -> String {
    build_trade_summary(report).unwrap_or_else(|| "no recent trade observations".into())
}

fn describe_depth(report: &DepthEvolutionReport) -> String {
    build_depth_summary(report).unwrap_or_else(|| "no recent depth observations".into())
}

fn describe_broker_queue(report: &BrokerOnsetReport) -> String {
    build_broker_summary(report).unwrap_or_else(|| "no recent broker observations".into())
}

fn describe_quote_state(report: &QuoteStateReport) -> String {
    if report.observation_count == 0 {
        return "no recent quote observations".into();
    }
    match (report.last_done, report.prev_close) {
        (Some(last), Some(prev)) if prev > Decimal::ZERO => {
            let change = (last - prev) / prev * Decimal::new(100, 0);
            format!(
                "last quote {} ({:+.2}% vs prev close)",
                last.round_dp(3),
                change.round_dp(2)
            )
        }
        (Some(last), _) => format!("last quote {}", last.round_dp(3)),
        _ => "no recent quote observations".into(),
    }
}

fn describe_candles(report: &CandlestickStateReport) -> String {
    if report.bar_count == 0 {
        return "no recent candlestick observations".into();
    }
    match (report.net_change, report.range) {
        (Some(change), Some(range)) => format!(
            "{} bars, net change {} across range {}",
            report.bar_count,
            change.round_dp(3),
            range.round_dp(3)
        ),
        _ => format!("{} recent bars observed", report.bar_count),
    }
}

fn describe_intraday_profile(report: &IntradayProfileReport) -> String {
    if report.observation_count == 0 {
        return "no recent intraday observations".into();
    }
    match (
        report.latest_price,
        report.latest_avg_price,
        report.vwap_deviation,
    ) {
        (Some(price), Some(avg), Some(dev)) => format!(
            "latest price {} vs avg {} (vwap deviation {:+.2}%)",
            price.round_dp(3),
            avg.round_dp(3),
            (dev * Decimal::new(100, 0)).round_dp(2)
        ),
        _ => "intraday profile available".into(),
    }
}

fn describe_capital_distribution(report: &CapitalDistributionShiftReport) -> String {
    if report.observation_count == 0 {
        return "no recent capital distribution observations".into();
    }
    let dominant = report
        .dominant_bucket
        .clone()
        .unwrap_or_else(|| "mixed".into());
    format!(
        "{} orders dominate (large={} medium={} small={})",
        dominant,
        report.latest_large_net.round_dp(0),
        report.latest_medium_net.round_dp(0),
        report.latest_small_net.round_dp(0)
    )
}

fn describe_capital_flow(report: &CapitalFlowShiftReport) -> String {
    if report.observation_count == 0 {
        return "no recent capital flow observations".into();
    }
    match (report.latest_inflow, report.velocity, report.acceleration) {
        (Some(inflow), Some(velocity), Some(acceleration)) => format!(
            "latest inflow {} with velocity {} and acceleration {}",
            inflow.round_dp(2),
            velocity.round_dp(2),
            acceleration.round_dp(2)
        ),
        (Some(inflow), _, _) => format!("latest inflow {}", inflow.round_dp(2)),
        _ => "capital flow observations available".into(),
    }
}

fn describe_calc_indexes(report: &CalcIndexStateReport) -> String {
    if report.observation_count == 0 {
        return "no recent calc index observations".into();
    }
    let mut parts = Vec::new();
    if let Some(vr) = report.volume_ratio {
        parts.push(format!("volume_ratio={}", vr.round_dp(2)));
    }
    if let Some(cr) = report.change_rate {
        parts.push(format!("change_rate={}%", cr.round_dp(2)));
    }
    if let Some(five) = report.five_minutes_change_rate {
        parts.push(format!("5m_change={}%", five.round_dp(2)));
    }
    if parts.is_empty() {
        "calc indexes available".into()
    } else {
        parts.join(", ")
    }
}

fn describe_market_temperature(report: &MarketTemperatureStateReport) -> String {
    if report.observation_count == 0 {
        return "no recent market temperature observations".into();
    }
    match (report.latest_temperature, report.latest_sentiment) {
        (Some(temp), Some(sentiment)) => {
            format!("market temperature {} with sentiment {}", temp, sentiment)
        }
        (Some(temp), None) => format!("market temperature {}", temp),
        _ => "market temperature observations available".into(),
    }
}

fn describe_option_surface(report: &OptionSurfaceStateReport) -> String {
    if report.observation_count == 0 {
        return "no recent option surface observations".into();
    }
    let expiry = report
        .expiry_label
        .clone()
        .unwrap_or_else(|| "unknown expiry".into());
    match (
        report.atm_call_iv,
        report.atm_put_iv,
        report.put_call_oi_ratio,
    ) {
        (Some(call_iv), Some(put_iv), Some(ratio)) => format!(
            "{} surface call_iv={} put_iv={} put/call OI={}",
            expiry,
            call_iv.round_dp(3),
            put_iv.round_dp(3),
            ratio.round_dp(2)
        ),
        _ => format!("{expiry} option surface available"),
    }
}

fn build_trade_summary(report: &TradeAggressionReport) -> Option<String> {
    if report.trade_count == 0 {
        return None;
    }

    let buy_ratio = report.buy_volume_ratio;
    let sell_ratio = report.sell_volume_ratio;
    if report.net_volume_imbalance > 0 && buy_ratio >= Decimal::new(55, 2) {
        Some(format!(
            "aggressive buying dominated ({:.0}% buy volume, net +{} shares)",
            (buy_ratio * Decimal::new(100, 0)).round_dp(0),
            report.net_volume_imbalance
        ))
    } else if report.net_volume_imbalance < 0 && sell_ratio >= Decimal::new(55, 2) {
        Some(format!(
            "aggressive selling dominated ({:.0}% sell volume, net {} shares)",
            (sell_ratio * Decimal::new(100, 0)).round_dp(0),
            report.net_volume_imbalance
        ))
    } else if report.trade_count > 0 {
        Some(format!(
            "trade flow was mixed (buy {:.0}% / sell {:.0}%)",
            (buy_ratio * Decimal::new(100, 0)).round_dp(0),
            (sell_ratio * Decimal::new(100, 0)).round_dp(0),
        ))
    } else {
        None
    }
}

fn build_depth_summary(report: &DepthEvolutionReport) -> Option<String> {
    let delta = report.net_delta.as_ref()?;
    let bid_delta = level_change_volume_delta(&delta.bid_changes);
    let ask_delta = level_change_volume_delta(&delta.ask_changes);

    if bid_delta == 0 && ask_delta == 0 && delta.spread_change.is_none() {
        return None;
    }

    let liquidity_summary = if bid_delta > 0 && ask_delta <= 0 {
        format!(
            "bid-side liquidity built (+{} vs ask {})",
            bid_delta, ask_delta
        )
    } else if ask_delta > 0 && bid_delta <= 0 {
        format!(
            "ask-side liquidity built (+{} vs bid {})",
            ask_delta, bid_delta
        )
    } else if bid_delta < 0 && ask_delta >= 0 {
        format!(
            "bid-side liquidity thinned ({} vs ask +{})",
            bid_delta, ask_delta
        )
    } else if ask_delta < 0 && bid_delta >= 0 {
        format!(
            "ask-side liquidity thinned ({} vs bid +{})",
            ask_delta, bid_delta
        )
    } else {
        format!("depth shifted (bid {} / ask {})", bid_delta, ask_delta)
    };

    let spread_summary = delta
        .spread_change
        .map(|(old, new)| {
            if new < old {
                format!(", spread tightened {}→{}", old.round_dp(3), new.round_dp(3))
            } else if new > old {
                format!(", spread widened {}→{}", old.round_dp(3), new.round_dp(3))
            } else {
                String::new()
            }
        })
        .unwrap_or_default();

    Some(format!("{liquidity_summary}{spread_summary}"))
}

pub fn level_change_volume_delta(changes: &[LevelChange]) -> i64 {
    changes
        .iter()
        .map(|change| match change {
            LevelChange::Added { volume, .. } => *volume,
            LevelChange::Removed { prev_volume, .. } => -*prev_volume,
            LevelChange::VolumeChanged {
                prev_volume,
                new_volume,
                ..
            } => *new_volume - *prev_volume,
        })
        .sum()
}

fn build_broker_summary(report: &BrokerOnsetReport) -> Option<String> {
    if report.events.is_empty() {
        return None;
    }

    let labels = report
        .events
        .iter()
        .take(2)
        .map(|event| {
            let who = event
                .institution_name
                .clone()
                .unwrap_or_else(|| event.broker_id.to_string());
            let side = match event.side {
                Side::Bid => "bid",
                Side::Ask => "ask",
            };
            format!("{who} on {side}{}", event.position)
        })
        .collect::<Vec<_>>();

    Some(format!(
        "{} new broker/institution onset{}: {}",
        report.events.len(),
        if report.events.len() == 1 { "" } else { "s" },
        labels.join(", ")
    ))
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use longport::quote::{
        Brokers, CapitalDistribution, CapitalDistributionResponse, Depth, MarketTemperature,
        PrePostQuote, TradeStatus,
    };
    use rust_decimal_macros::dec;
    use serde_json::json;

    use super::*;
    use crate::ontology::objects::{Broker, Institution, InstitutionClass, Market, Stock};
    use crate::ontology::store::ObjectStore;

    fn sym(value: &str) -> Symbol {
        Symbol(value.to_string())
    }

    fn make_quote(last_done: Decimal, minute: i64) -> SecurityQuote {
        SecurityQuote {
            symbol: "AAPL.US".into(),
            last_done,
            prev_close: dec!(99),
            open: dec!(99.5),
            high: dec!(101),
            low: dec!(98),
            timestamp: OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(minute),
            volume: 1_000 + minute,
            turnover: dec!(100000),
            trade_status: TradeStatus::Normal,
            pre_market_quote: None,
            post_market_quote: None,
            overnight_quote: None,
        }
    }

    fn make_extended_quote(last_done: Decimal, minute: i64) -> PrePostQuote {
        PrePostQuote {
            last_done,
            timestamp: OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(minute),
            volume: 100,
            turnover: dec!(1000),
            high: last_done,
            low: last_done,
            prev_close: last_done - dec!(1),
        }
    }

    fn make_trade(
        price: Decimal,
        volume: i64,
        seconds: i64,
        direction: longport::quote::TradeDirection,
    ) -> Trade {
        Trade {
            price,
            volume,
            timestamp: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(seconds),
            trade_type: "automatch".into(),
            direction,
            trade_session: longport::quote::TradeSession::Intraday,
        }
    }

    fn make_depth_level(position: i32, price: Decimal, volume: i64) -> Depth {
        Depth {
            position,
            price: Some(price),
            volume,
            order_num: 1,
        }
    }

    fn make_depth_snapshot(
        asks: Vec<Depth>,
        bids: Vec<Depth>,
        minute: i64,
    ) -> RawObservation<SecurityDepth> {
        RawObservation::new(
            SecurityDepth { asks, bids },
            OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(minute),
            OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(minute),
            RawEventSource::Push,
        )
    }

    fn make_broker_group(position: i32, broker_ids: &[i32]) -> Brokers {
        Brokers {
            position,
            broker_ids: broker_ids.to_vec(),
        }
    }

    fn make_broker_snapshot(
        ask_brokers: Vec<Brokers>,
        bid_brokers: Vec<Brokers>,
        minute: i64,
    ) -> RawObservation<SecurityBrokers> {
        RawObservation::new(
            SecurityBrokers {
                ask_brokers,
                bid_brokers,
            },
            OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(minute),
            OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(minute),
            RawEventSource::Push,
        )
    }

    fn make_intraday_line(minute: i64, price: Decimal, avg_price: Decimal) -> IntradayLine {
        serde_json::from_value(json!({
            "timestamp": (OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(minute)).format(&time::format_description::well_known::Rfc3339).unwrap(),
            "price": price,
            "volume": 1000,
            "turnover": "100000",
            "avg_price": avg_price
        }))
        .unwrap()
    }

    fn make_calc_index(
        symbol: &str,
        volume_ratio: Decimal,
        change_rate: Decimal,
    ) -> SecurityCalcIndex {
        serde_json::from_value(json!({
            "symbol": symbol,
            "turnover_rate": "1.2",
            "volume_ratio": volume_ratio.to_string(),
            "pe_ttm_ratio": null,
            "pb_ratio": null,
            "dividend_ratio_ttm": null,
            "amplitude": "2.5",
            "five_minutes_change_rate": "0.8",
            "change_rate": change_rate.to_string(),
            "ytd_change_rate": null,
            "five_day_change_rate": null,
            "ten_day_change_rate": null,
            "half_year_change_rate": null,
            "total_market_value": null,
            "capital_flow": "12345"
        }))
        .unwrap()
    }

    fn test_store_with_institution_mapping() -> ObjectStore {
        let institution = Institution {
            id: InstitutionId(100),
            name_en: "Barclays Asia".into(),
            name_cn: "Barclays".into(),
            name_hk: "Barclays".into(),
            broker_ids: HashSet::from([BrokerId(4497)]),
            class: InstitutionClass::Unknown,
        };
        let stock = Stock {
            symbol: sym("700.HK"),
            market: Market::Hk,
            name_en: "Tencent".into(),
            name_cn: "Tencent".into(),
            name_hk: "Tencent".into(),
            exchange: "HKEX".into(),
            lot_size: 100,
            sector_id: None,
            total_shares: 0,
            circulating_shares: 0,
            eps_ttm: Decimal::ZERO,
            bps: Decimal::ZERO,
            dividend_yield: Decimal::ZERO,
        };
        let mut store = ObjectStore::from_parts(vec![institution], vec![stock], vec![]);
        store.brokers.insert(
            BrokerId(4497),
            Broker {
                id: BrokerId(4497),
                institution_id: InstitutionId(100),
            },
        );
        store
    }

    #[test]
    fn quote_state_reports_latest_and_extended_quotes() {
        let mut store = RawEventStore::default();
        let symbol = sym("AAPL.US");
        let mut quote = make_quote(dec!(101), 1);
        quote.pre_market_quote = Some(make_extended_quote(dec!(99), 0));
        quote.post_market_quote = Some(make_extended_quote(dec!(102), 2));
        store.record_quote(symbol.clone(), quote, RawEventSource::Rest);

        let report = store.quote_state(&symbol, RawQueryWindow::Recent(1));
        assert_eq!(report.observation_count, 1);
        assert_eq!(report.last_done, Some(dec!(101)));
        assert_eq!(report.pre_market_last, Some(dec!(99)));
        assert_eq!(report.post_market_last, Some(dec!(102)));
    }

    #[test]
    fn intraday_profile_reports_latest_price_and_vwap_deviation() {
        let mut store = RawEventStore::default();
        let symbol = sym("AAPL.US");
        let mut lines = HashMap::new();
        lines.insert(
            symbol.clone(),
            vec![
                make_intraday_line(0, dec!(100), dec!(99)),
                make_intraday_line(1, dec!(101), dec!(100)),
            ],
        );
        store.record_intraday_snapshot(
            &lines,
            OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(1),
            RawEventSource::Rest,
        );

        let report = store.intraday_profile(&symbol, RawQueryWindow::Recent(1));
        assert_eq!(report.point_count, 2);
        assert_eq!(report.latest_price, Some(dec!(101)));
        assert_eq!(report.latest_avg_price, Some(dec!(100)));
        assert_eq!(report.vwap_deviation.unwrap().round_dp(2), dec!(0.01));
    }

    #[test]
    fn capital_distribution_shift_reports_deltas() {
        let mut store = RawEventStore::default();
        let symbol = sym("700.HK");
        let mut distributions = HashMap::new();
        distributions.insert(
            symbol.clone(),
            CapitalDistributionResponse {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                capital_in: CapitalDistribution {
                    large: dec!(100),
                    medium: dec!(20),
                    small: dec!(10),
                },
                capital_out: CapitalDistribution {
                    large: dec!(10),
                    medium: dec!(5),
                    small: dec!(8),
                },
            },
        );
        store.record_capital_distribution_snapshot(
            &distributions,
            OffsetDateTime::UNIX_EPOCH,
            RawEventSource::Rest,
        );
        distributions.insert(
            symbol.clone(),
            CapitalDistributionResponse {
                timestamp: OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(1),
                capital_in: CapitalDistribution {
                    large: dec!(140),
                    medium: dec!(20),
                    small: dec!(10),
                },
                capital_out: CapitalDistribution {
                    large: dec!(20),
                    medium: dec!(5),
                    small: dec!(12),
                },
            },
        );
        store.record_capital_distribution_snapshot(
            &distributions,
            OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(1),
            RawEventSource::Rest,
        );

        let report = store.capital_distribution_shift(&symbol, RawQueryWindow::Recent(2));
        assert_eq!(report.latest_large_net, dec!(120));
        assert_eq!(report.delta_large_net, dec!(30));
        assert_eq!(report.dominant_bucket.as_deref(), Some("large"));
    }

    #[test]
    fn capital_flow_shift_reports_velocity_and_acceleration() {
        let mut store = RawEventStore::default();
        let symbol = sym("700.HK");
        let mut flows = HashMap::new();
        flows.insert(
            symbol.clone(),
            vec![
                CapitalFlowLine {
                    inflow: dec!(100),
                    timestamp: OffsetDateTime::UNIX_EPOCH,
                },
                CapitalFlowLine {
                    inflow: dec!(160),
                    timestamp: OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(1),
                },
            ],
        );
        store.record_capital_flow_snapshot(
            &flows,
            OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(1),
            RawEventSource::Rest,
        );
        flows.insert(
            symbol.clone(),
            vec![
                CapitalFlowLine {
                    inflow: dec!(100),
                    timestamp: OffsetDateTime::UNIX_EPOCH,
                },
                CapitalFlowLine {
                    inflow: dec!(160),
                    timestamp: OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(1),
                },
                CapitalFlowLine {
                    inflow: dec!(250),
                    timestamp: OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(2),
                },
            ],
        );
        store.record_capital_flow_snapshot(
            &flows,
            OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(2),
            RawEventSource::Rest,
        );

        let report = store.capital_flow_shift(&symbol, RawQueryWindow::Recent(2));
        assert_eq!(report.latest_inflow, Some(dec!(250)));
        assert!(report.velocity.unwrap() > Decimal::ZERO);
        assert!(report.acceleration.unwrap() > Decimal::ZERO);
    }

    #[test]
    fn calc_index_state_reports_latest_and_deltas() {
        let mut store = RawEventStore::default();
        let symbol = sym("AAPL.US");
        let indexes = HashMap::from([(
            symbol.clone(),
            make_calc_index("AAPL.US", dec!(1.2), dec!(0.5)),
        )]);
        store.record_calc_index_snapshot(
            &indexes,
            OffsetDateTime::UNIX_EPOCH,
            RawEventSource::Rest,
        );
        let indexes2 = HashMap::from([(
            symbol.clone(),
            make_calc_index("AAPL.US", dec!(1.8), dec!(1.1)),
        )]);
        store.record_calc_index_snapshot(
            &indexes2,
            OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(1),
            RawEventSource::Rest,
        );

        let report = store.calc_index_state(&symbol, RawQueryWindow::Recent(2));
        assert_eq!(report.volume_ratio, Some(dec!(1.8)));
        assert_eq!(report.delta_volume_ratio, Some(dec!(0.6)));
        assert_eq!(report.delta_change_rate, Some(dec!(0.6)));
    }

    #[test]
    fn market_temperature_state_reports_latest_and_delta() {
        let mut store = RawEventStore::default();
        store.record_market_temperature(
            MarketTemperature {
                temperature: 40,
                valuation: 30,
                sentiment: 20,
                description: "warm".into(),
                timestamp: OffsetDateTime::UNIX_EPOCH,
            },
            RawEventSource::Rest,
        );
        store.record_market_temperature(
            MarketTemperature {
                temperature: 55,
                valuation: 35,
                sentiment: 18,
                description: "warming".into(),
                timestamp: OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(1),
            },
            RawEventSource::Rest,
        );

        let report = store.market_temperature_state(RawQueryWindow::Recent(2));
        assert_eq!(report.latest_temperature, Some(55));
        assert_eq!(report.delta_temperature, Some(15));
        assert_eq!(report.description.as_deref(), Some("warming"));
    }

    #[test]
    fn option_surface_state_reports_latest_surface() {
        let mut store = RawEventStore::default();
        let surface = OptionSurfaceObservation {
            underlying: sym("AAPL.US"),
            expiry_label: "2026-04-17".into(),
            atm_call_iv: Some(dec!(0.22)),
            atm_put_iv: Some(dec!(0.28)),
            put_call_skew: Some(dec!(0.27)),
            total_call_oi: 1000,
            total_put_oi: 1400,
            put_call_oi_ratio: Some(dec!(1.4)),
            atm_delta: Some(dec!(0.55)),
            atm_vega: Some(dec!(20)),
        };
        store.record_option_surface_snapshot(
            &[surface],
            OffsetDateTime::UNIX_EPOCH,
            RawEventSource::Rest,
        );

        let report = store.option_surface_state(&sym("AAPL.US"), RawQueryWindow::Recent(1));
        assert_eq!(report.expiry_label.as_deref(), Some("2026-04-17"));
        assert_eq!(report.put_call_oi_ratio, Some(dec!(1.4)));
    }

    #[test]
    fn export_longport_sources_includes_symbol_and_market_entries() {
        let mut store = RawEventStore::default();
        let symbol = sym("AAPL.US");
        store.record_quote(
            symbol.clone(),
            make_quote(dec!(101), 1),
            RawEventSource::Push,
        );
        store.record_trades(
            symbol.clone(),
            &[make_trade(
                dec!(101),
                100,
                0,
                longport::quote::TradeDirection::Up,
            )],
            OffsetDateTime::UNIX_EPOCH,
            RawEventSource::Push,
        );
        store.record_market_temperature(
            MarketTemperature {
                temperature: 50,
                valuation: 30,
                sentiment: 15,
                description: "steady".into(),
                timestamp: OffsetDateTime::UNIX_EPOCH,
            },
            RawEventSource::Rest,
        );

        let symbol_sources = store.export_longport_sources(
            &symbol,
            RawQueryWindow::Recent(5),
            &test_store_with_institution_mapping(),
        );
        assert!(symbol_sources.iter().any(|item| item.source == "trade"));
        assert!(symbol_sources.iter().any(|item| item.source == "quote"));

        let market_sources = store.export_market_sources(RawQueryWindow::Recent(5));
        assert_eq!(market_sources.len(), 1);
        assert_eq!(market_sources[0].source, "market_temperature");
        assert_eq!(market_sources[0].scope, "market");
    }

    #[test]
    fn quote_history_is_capped_per_symbol() {
        let mut store = RawEventStore::new(RawEventCaps {
            quotes_per_symbol: 2,
            ..RawEventCaps::default()
        });

        for minute in 0..3 {
            store.record_quote(
                sym("AAPL.US"),
                make_quote(dec!(100) + Decimal::from(minute), minute),
                RawEventSource::Push,
            );
        }

        let events = store.symbol_events(&sym("AAPL.US")).unwrap();
        assert_eq!(events.quotes().len(), 2);
        assert_eq!(events.quotes().front().unwrap().value.last_done, dec!(101));
        assert_eq!(events.quotes().back().unwrap().value.last_done, dec!(102));
    }

    #[test]
    fn trade_history_is_capped_across_batches() {
        let mut store = RawEventStore::new(RawEventCaps {
            trades_per_symbol: 3,
            ..RawEventCaps::default()
        });

        let trades = (0..4)
            .map(|i| Trade {
                price: dec!(10) + Decimal::from(i),
                volume: 100 + i as i64,
                timestamp: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(i.into()),
                trade_type: "automatch".into(),
                direction: longport::quote::TradeDirection::Neutral,
                trade_session: longport::quote::TradeSession::Intraday,
            })
            .collect::<Vec<_>>();

        store.record_trades(
            sym("700.HK"),
            &trades,
            OffsetDateTime::UNIX_EPOCH,
            RawEventSource::Push,
        );

        let events = store.symbol_events(&sym("700.HK")).unwrap();
        assert_eq!(events.trades().len(), 3);
        assert_eq!(events.trades().front().unwrap().value.price, dec!(11));
        assert_eq!(events.trades().back().unwrap().value.price, dec!(13));
    }

    #[test]
    fn trade_aggression_mixed_directions_reports_counts_and_ratios() {
        let mut store = RawEventStore::default();
        let symbol = sym("AAPL.US");
        let trades = vec![
            make_trade(dec!(10), 100, 0, longport::quote::TradeDirection::Up),
            make_trade(dec!(11), 50, 10, longport::quote::TradeDirection::Down),
            make_trade(dec!(12), 25, 20, longport::quote::TradeDirection::Neutral),
        ];
        store.record_trades(
            symbol.clone(),
            &trades,
            OffsetDateTime::UNIX_EPOCH,
            RawEventSource::Push,
        );

        let report = store.trade_aggression(&symbol, RawQueryWindow::Recent(10));
        assert_eq!(report.trade_count, 3);
        assert_eq!(report.buy_count, 1);
        assert_eq!(report.sell_count, 1);
        assert_eq!(report.neutral_count, 1);
        assert_eq!(report.buy_volume, 100);
        assert_eq!(report.sell_volume, 50);
        assert_eq!(report.neutral_volume, 25);
        assert_eq!(report.buy_notional, dec!(1000));
        assert_eq!(report.sell_notional, dec!(550));
        assert_eq!(report.neutral_notional, dec!(300));
        assert_eq!(report.net_volume_imbalance, 50);
        assert_eq!(report.net_notional_imbalance, dec!(450));
        assert_eq!(report.buy_volume_ratio.round_dp(4), dec!(0.5714));
        assert_eq!(report.sell_volume_ratio.round_dp(4), dec!(0.2857));
    }

    #[test]
    fn trade_aggression_filters_by_time_range() {
        let mut store = RawEventStore::default();
        let symbol = sym("AAPL.US");
        let trades = vec![
            make_trade(dec!(10), 100, 0, longport::quote::TradeDirection::Up),
            make_trade(dec!(11), 50, 60, longport::quote::TradeDirection::Down),
            make_trade(dec!(12), 25, 120, longport::quote::TradeDirection::Up),
        ];
        store.record_trades(
            symbol.clone(),
            &trades,
            OffsetDateTime::UNIX_EPOCH,
            RawEventSource::Push,
        );

        let report = store.trade_aggression(
            &symbol,
            RawQueryWindow::TimeRange {
                start: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(30),
                end: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(90),
            },
        );
        assert_eq!(report.trade_count, 1);
        assert_eq!(report.sell_count, 1);
        assert_eq!(
            report.window_start,
            Some(OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(60))
        );
        assert_eq!(
            report.window_end,
            Some(OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(60))
        );
    }

    #[test]
    fn trade_aggression_recent_selects_last_n() {
        let mut store = RawEventStore::default();
        let symbol = sym("AAPL.US");
        let trades = vec![
            make_trade(dec!(10), 100, 0, longport::quote::TradeDirection::Up),
            make_trade(dec!(11), 50, 10, longport::quote::TradeDirection::Down),
            make_trade(dec!(12), 25, 20, longport::quote::TradeDirection::Up),
        ];
        store.record_trades(
            symbol.clone(),
            &trades,
            OffsetDateTime::UNIX_EPOCH,
            RawEventSource::Push,
        );

        let report = store.trade_aggression(&symbol, RawQueryWindow::Recent(2));
        assert_eq!(report.trade_count, 2);
        assert_eq!(report.buy_count, 1);
        assert_eq!(report.sell_count, 1);
        assert_eq!(
            report.window_start,
            Some(OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(10))
        );
        assert_eq!(
            report.window_end,
            Some(OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(20))
        );
    }

    #[test]
    fn trade_aggression_empty_window_returns_zeroed_report() {
        let store = RawEventStore::default();
        let report = store.trade_aggression(&sym("MISSING.US"), RawQueryWindow::Recent(5));
        assert_eq!(report.trade_count, 0);
        assert_eq!(report.window_start, None);
        assert_eq!(report.window_end, None);
        assert_eq!(report.buy_volume_ratio, Decimal::ZERO);
    }

    #[test]
    fn depth_evolution_reports_net_and_step_deltas() {
        let mut store = RawEventStore::default();
        let symbol = sym("700.HK");
        let events = store.symbol_events_mut(symbol.clone());
        events.depths.push_back(make_depth_snapshot(
            vec![make_depth_level(1, dec!(10.2), 100)],
            vec![make_depth_level(1, dec!(10.0), 100)],
            0,
        ));
        events.depths.push_back(make_depth_snapshot(
            vec![make_depth_level(1, dec!(10.2), 90)],
            vec![make_depth_level(1, dec!(10.0), 120)],
            1,
        ));
        events.depths.push_back(make_depth_snapshot(
            vec![make_depth_level(1, dec!(10.2), 80)],
            vec![make_depth_level(1, dec!(10.0), 140)],
            2,
        ));

        let report = store.depth_evolution(&symbol, RawQueryWindow::Recent(3));
        assert_eq!(report.observation_count, 3);
        assert_eq!(report.step_deltas.len(), 2);
        let net = report.net_delta.as_ref().unwrap();
        assert_eq!(net.bid_changes.len(), 1);
        assert_eq!(net.ask_changes.len(), 1);
        assert_eq!(
            report.step_deltas[0].observed_at,
            OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(1)
        );
    }

    #[test]
    fn depth_evolution_with_fewer_than_two_snapshots_returns_no_deltas() {
        let mut store = RawEventStore::default();
        let symbol = sym("700.HK");
        let events = store.symbol_events_mut(symbol.clone());
        events.depths.push_back(make_depth_snapshot(
            vec![make_depth_level(1, dec!(10.2), 100)],
            vec![make_depth_level(1, dec!(10.0), 100)],
            0,
        ));

        let report = store.depth_evolution(&symbol, RawQueryWindow::Recent(5));
        assert_eq!(report.observation_count, 1);
        assert!(report.net_delta.is_none());
        assert!(report.step_deltas.is_empty());
    }

    #[test]
    fn depth_evolution_recent_window_uses_last_n_observations() {
        let mut store = RawEventStore::default();
        let symbol = sym("700.HK");
        let events = store.symbol_events_mut(symbol.clone());
        for minute in 0..4 {
            events.depths.push_back(make_depth_snapshot(
                vec![make_depth_level(1, dec!(10.2), 100 - minute * 5)],
                vec![make_depth_level(1, dec!(10.0), 100 + minute * 10)],
                minute,
            ));
        }

        let report = store.depth_evolution(&symbol, RawQueryWindow::Recent(2));
        assert_eq!(report.observation_count, 2);
        assert_eq!(
            report.window_start,
            Some(OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(2))
        );
        assert_eq!(
            report.window_end,
            Some(OffsetDateTime::UNIX_EPOCH + time::Duration::minutes(3))
        );
        assert_eq!(report.step_deltas.len(), 1);
    }

    #[test]
    fn broker_onset_detects_bid_and_ask_entries_once() {
        let mut store = RawEventStore::default();
        let symbol = sym("700.HK");
        let events = store.symbol_events_mut(symbol.clone());
        events
            .brokers
            .push_back(make_broker_snapshot(vec![], vec![], 0));
        events.brokers.push_back(make_broker_snapshot(
            vec![make_broker_group(1, &[5001])],
            vec![make_broker_group(2, &[4497])],
            1,
        ));
        events.brokers.push_back(make_broker_snapshot(
            vec![make_broker_group(1, &[5001])],
            vec![make_broker_group(1, &[4497])],
            2,
        ));

        let report = store.broker_onset(
            &symbol,
            RawQueryWindow::Recent(2),
            &test_store_with_institution_mapping(),
        );
        assert_eq!(report.snapshot_count, 2);
        assert_eq!(report.events.len(), 2);
        assert!(report
            .events
            .iter()
            .any(|event| event.side == Side::Bid && event.broker_id == BrokerId(4497)));
        assert!(report
            .events
            .iter()
            .any(|event| event.side == Side::Ask && event.broker_id == BrokerId(5001)));
    }

    #[test]
    fn broker_onset_does_not_refire_or_count_position_change() {
        let mut store = RawEventStore::default();
        let symbol = sym("700.HK");
        let events = store.symbol_events_mut(symbol.clone());
        events
            .brokers
            .push_back(make_broker_snapshot(vec![], vec![], 0));
        events.brokers.push_back(make_broker_snapshot(
            vec![],
            vec![make_broker_group(2, &[4497])],
            1,
        ));
        events.brokers.push_back(make_broker_snapshot(
            vec![],
            vec![make_broker_group(1, &[4497])],
            2,
        ));
        events.brokers.push_back(make_broker_snapshot(
            vec![],
            vec![make_broker_group(1, &[4497])],
            3,
        ));

        let report = store.broker_onset(
            &symbol,
            RawQueryWindow::Recent(4),
            &test_store_with_institution_mapping(),
        );
        assert_eq!(report.events.len(), 1);
        let event = &report.events[0];
        assert_eq!(event.position, 2);
        assert_eq!(event.institution_id, Some(InstitutionId(100)));
        assert_eq!(event.institution_name.as_deref(), Some("Barclays Asia"));
    }

    #[test]
    fn broker_onset_emits_unmapped_broker_without_institution() {
        let mut store = RawEventStore::default();
        let symbol = sym("700.HK");
        let events = store.symbol_events_mut(symbol.clone());
        events
            .brokers
            .push_back(make_broker_snapshot(vec![], vec![], 0));
        events.brokers.push_back(make_broker_snapshot(
            vec![],
            vec![make_broker_group(2, &[9999])],
            1,
        ));

        let report = store.broker_onset(
            &symbol,
            RawQueryWindow::Recent(2),
            &test_store_with_institution_mapping(),
        );
        assert_eq!(report.events.len(), 1);
        assert_eq!(report.events[0].institution_id, None);
        assert_eq!(report.events[0].institution_name, None);
    }

    #[test]
    fn broker_onset_empty_for_us_symbols_without_broker_history() {
        let store = RawEventStore::default();
        let report = store.broker_onset(
            &sym("AAPL.US"),
            RawQueryWindow::Recent(5),
            &test_store_with_institution_mapping(),
        );
        assert_eq!(report.snapshot_count, 0);
        assert!(report.events.is_empty());
    }

    #[test]
    fn explain_microstructure_builds_human_summary_from_reports() {
        let mut store = RawEventStore::default();
        let symbol = sym("700.HK");
        {
            let events = store.symbol_events_mut(symbol.clone());
            events.depths.push_back(make_depth_snapshot(
                vec![make_depth_level(1, dec!(10.2), 100)],
                vec![make_depth_level(1, dec!(10.0), 100)],
                0,
            ));
            events.depths.push_back(make_depth_snapshot(
                vec![make_depth_level(1, dec!(10.2), 80)],
                vec![make_depth_level(1, dec!(10.0), 140)],
                1,
            ));
            events
                .brokers
                .push_back(make_broker_snapshot(vec![], vec![], 0));
            events.brokers.push_back(make_broker_snapshot(
                vec![],
                vec![make_broker_group(2, &[4497])],
                1,
            ));
        }
        store.record_trades(
            symbol.clone(),
            &[
                make_trade(dec!(10), 100, 0, longport::quote::TradeDirection::Up),
                make_trade(dec!(10.1), 80, 10, longport::quote::TradeDirection::Up),
                make_trade(dec!(10.05), 20, 20, longport::quote::TradeDirection::Down),
            ],
            OffsetDateTime::UNIX_EPOCH,
            RawEventSource::Push,
        );

        let explanation = store.explain_microstructure(
            &symbol,
            RawQueryWindow::Recent(5),
            &test_store_with_institution_mapping(),
        );
        assert!(explanation.summary.contains("aggressive buying dominated"));
        assert!(explanation.summary.contains("bid-side liquidity built"));
        assert!(explanation.summary.contains("Barclays Asia"));
    }

    #[test]
    fn explain_microstructure_returns_empty_message_without_observations() {
        let store = RawEventStore::default();
        let explanation = store.explain_microstructure(
            &sym("AAPL.US"),
            RawQueryWindow::Recent(5),
            &test_store_with_institution_mapping(),
        );
        assert_eq!(explanation.summary, "no recent raw observations");
        assert!(explanation.trade_summary.is_none());
        assert!(explanation.depth_summary.is_none());
        assert!(explanation.broker_summary.is_none());
    }
}
