use super::*;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SignalScope {
    Market,
    Symbol(Symbol),
    Institution(InstitutionId),
    Sector(SectorId),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ObservationRecord {
    Quote {
        symbol: Symbol,
        last_done: Decimal,
        turnover: Decimal,
        market_status: String,
        pre_market_last: Option<Decimal>,
        post_market_last: Option<Decimal>,
    },
    OrderBook {
        symbol: Symbol,
        total_bid_volume: i64,
        total_ask_volume: i64,
        spread: Option<Decimal>,
    },
    CapitalFlow {
        symbol: Symbol,
        net_inflow: Decimal,
    },
    CapitalFlowSeries {
        symbol: Symbol,
        point_count: usize,
        latest_inflow: Decimal,
        velocity: Decimal,
    },
    CapitalBreakdown {
        symbol: Symbol,
        large_net: Decimal,
        medium_net: Decimal,
        small_net: Decimal,
    },
    CalcIndex {
        symbol: Symbol,
        turnover_rate: Option<Decimal>,
        volume_ratio: Option<Decimal>,
        pe_ttm_ratio: Option<Decimal>,
        pb_ratio: Option<Decimal>,
        dividend_ratio_ttm: Option<Decimal>,
        amplitude: Option<Decimal>,
        five_minutes_change_rate: Option<Decimal>,
    },
    Candlestick {
        symbol: Symbol,
        candle_count: usize,
        window_return: Decimal,
        body_bias: Decimal,
        volume_ratio: Decimal,
        range_ratio: Decimal,
    },
    InstitutionActivity {
        symbol: Symbol,
        institution_id: String,
        seat_count: usize,
    },
    TradeActivity {
        symbol: Symbol,
        trade_count: usize,
        total_volume: i64,
        buy_volume: i64,
        sell_volume: i64,
        vwap: Decimal,
        pre_market_volume: i64,
        post_market_volume: i64,
    },
    MarketTemperature {
        temperature: Decimal,
        valuation: Decimal,
        sentiment: Decimal,
        description: String,
    },
    BrokerActivity {
        symbol: Symbol,
        broker_id: i32,
        institution_id: Option<i32>,
        side: String,
        position: i32,
        duration_ticks: u64,
        replenish_count: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MarketEventKind {
    OrderBookDislocation,
    VolumeDislocation,
    SmartMoneyPressure,
    CandlestickBreakout,
    CompositeAcceleration,
    InstitutionalFlip,
    MarketStressElevated,
    StressRegimeShift,
    ManualReviewRequired,
    SharedHolderAnomaly,
    IcebergDetected,
    BrokerClusterFormation,
    BrokerSideFlip,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketEventRecord {
    pub scope: SignalScope,
    pub kind: MarketEventKind,
    pub magnitude: Decimal,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DerivedSignalKind {
    StructuralComposite,
    Convergence,
    ValuationSupport,
    ActivityMomentum,
    CandlestickConviction,
    SmartMoneyPressure,
    MarketStress,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivedSignalRecord {
    pub scope: SignalScope,
    pub kind: DerivedSignalKind,
    pub strength: Decimal,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationSnapshot {
    pub timestamp: OffsetDateTime,
    pub observations: Vec<Observation<ObservationRecord>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSnapshot {
    pub timestamp: OffsetDateTime,
    pub events: Vec<Event<MarketEventRecord>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivedSignalSnapshot {
    pub timestamp: OffsetDateTime,
    pub signals: Vec<DerivedSignal<DerivedSignalRecord>>,
}
