use super::*;

// A 5-bar trading range around 8% of the opening price is already an outsized
// intraday expansion for the HK names we ingest, so we cap range conviction there.
pub(super) fn candle_range_normalizer() -> Decimal {
    Decimal::new(8, 2)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
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
    pub net_inflow: YuanAmount,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct YuanAmount(Decimal);

impl YuanAmount {
    pub fn from_yuan(value: Decimal) -> Self {
        Self(value)
    }

    pub fn from_ten_thousands(value: Decimal) -> Self {
        Self(value * Decimal::from(10_000))
    }

    pub fn as_yuan(self) -> Decimal {
        self.0
    }
}

#[derive(Debug, Clone)]
pub struct CapitalFlowPoint {
    pub timestamp: OffsetDateTime,
    pub inflow: Decimal,
}

#[derive(Debug, Clone)]
pub struct CapitalFlowTimeSeries {
    pub symbol: Symbol,
    pub points: Vec<CapitalFlowPoint>,
    pub latest_inflow: YuanAmount,
    pub velocity: Decimal,
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
    pub top3_volume_ratio: Decimal,
    pub volume_weighted_distance: Decimal,
    pub best_level_ratio: Decimal,
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
pub struct ExtendedSessionQuote {
    pub last_done: Decimal,
    pub timestamp: OffsetDateTime,
    pub volume: i64,
    pub turnover: Decimal,
    pub high: Decimal,
    pub low: Decimal,
    pub prev_close: Decimal,
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
    pub pre_market: Option<ExtendedSessionQuote>,
    pub post_market: Option<ExtendedSessionQuote>,
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
    pub change_rate: Option<Decimal>,
    pub ytd_change_rate: Option<Decimal>,
    pub five_day_change_rate: Option<Decimal>,
    pub ten_day_change_rate: Option<Decimal>,
    pub half_year_change_rate: Option<Decimal>,
    pub total_market_value: Option<Decimal>,
    pub capital_flow: Option<Decimal>,
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
    pub session: TradeSession,
}

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
    pub pre_market_volume: i64,
    pub post_market_volume: i64,
}

#[derive(Debug)]
pub struct LinkSnapshot {
    pub timestamp: OffsetDateTime,
    pub broker_queues: Vec<BrokerQueueEntry>,
    pub calc_indexes: Vec<CalcIndexObservation>,
    pub candlesticks: Vec<CandlestickObservation>,
    pub institution_activities: Vec<InstitutionActivity>,
    pub cross_stock_presences: Vec<CrossStockPresence>,
    pub capital_flows: Vec<CapitalFlow>,
    pub capital_flow_series: Vec<CapitalFlowTimeSeries>,
    pub capital_breakdowns: Vec<CapitalBreakdown>,
    pub market_temperature: Option<MarketTemperatureObservation>,
    pub order_books: Vec<OrderBookObservation>,
    pub quotes: Vec<QuoteObservation>,
    pub trade_activities: Vec<TradeActivity>,
    pub intraday: Vec<IntradayObservation>,
}

#[derive(Debug, Clone)]
pub struct IntradayObservation {
    pub symbol: Symbol,
    pub avg_price: Decimal,
    pub last_price: Decimal,
    pub vwap_deviation: Decimal,
    pub point_count: usize,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct OptionSurfaceObservation {
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

#[derive(Debug, Clone, serde::Serialize)]
pub struct WarrantSentimentObservation {
    pub underlying: Symbol,
    pub total_warrants: usize,
    pub call_warrant_count: usize,
    pub put_warrant_count: usize,
    pub top_call_outstanding_ratio: Option<Decimal>,
    pub top_put_outstanding_ratio: Option<Decimal>,
    pub weighted_call_iv: Option<Decimal>,
    pub weighted_put_iv: Option<Decimal>,
}
