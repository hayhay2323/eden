use super::*;

pub(super) const TRADE_BUFFER_CAP_PER_SYMBOL: usize = 2_000;
pub(super) const POLYMARKET_WARNING_INTERVAL: std::time::Duration = std::time::Duration::from_secs(300);

/// Live market state accumulated from WebSocket push events.
pub(super) struct LiveState {
    pub(super) depths: HashMap<Symbol, SecurityDepth>,
    pub(super) brokers: HashMap<Symbol, SecurityBrokers>,
    pub(super) quotes: HashMap<Symbol, SecurityQuote>,
    pub(super) trades: HashMap<Symbol, Vec<Trade>>,
    pub(super) candlesticks: HashMap<Symbol, Vec<longport::quote::Candlestick>>,
    pub(super) push_count: u64,
    pub(super) dirty: bool, // true if new pushes since last pipeline run
}

impl LiveState {
    pub(super) fn new() -> Self {
        Self {
            depths: HashMap::new(),
            brokers: HashMap::new(),
            quotes: HashMap::new(),
            trades: HashMap::new(),
            candlesticks: HashMap::new(),
            push_count: 0,
            dirty: false,
        }
    }

    pub(super) fn apply(&mut self, event: PushEvent) {
        let symbol = Symbol(event.symbol);
        self.push_count += 1;
        self.dirty = true;
        match event.detail {
            PushEventDetail::Depth(depth) => {
                self.depths.insert(
                    symbol,
                    SecurityDepth {
                        asks: depth.asks,
                        bids: depth.bids,
                    },
                );
            }
            PushEventDetail::Brokers(brokers) => {
                self.brokers.insert(
                    symbol,
                    SecurityBrokers {
                        ask_brokers: brokers.ask_brokers,
                        bid_brokers: brokers.bid_brokers,
                    },
                );
            }
            PushEventDetail::Quote(quote) => {
                let existing = self.quotes.get(&symbol);
                self.quotes.insert(
                    symbol.clone(),
                    SecurityQuote {
                        symbol: symbol.0,
                        last_done: quote.last_done,
                        prev_close: existing.map(|q| q.prev_close).unwrap_or(Decimal::ZERO),
                        open: quote.open,
                        high: quote.high,
                        low: quote.low,
                        timestamp: quote.timestamp,
                        volume: quote.volume,
                        turnover: quote.turnover,
                        trade_status: quote.trade_status,
                        pre_market_quote: None,
                        post_market_quote: None,
                        overnight_quote: None,
                    },
                );
            }
            PushEventDetail::Trade(push_trades) => {
                let entry = self.trades.entry(symbol).or_default();
                append_trades_with_cap(entry, push_trades.trades);
            }
            PushEventDetail::Candlestick(candle) => {
                let entry = self.candlesticks.entry(symbol).or_default();
                entry.push(candle.candlestick);
                // Keep last 60 candles (1 hour of 1-min data)
                if entry.len() > 60 {
                    entry.drain(..entry.len() - 60);
                }
            }
        }
    }

    /// Merge live push state with REST-fetched capital data into a RawSnapshot.
    /// Consumes accumulated trades (they're per-tick, not cumulative).
    pub(super) fn to_raw_snapshot(&mut self, rest: &RestSnapshot) -> RawSnapshot {
        let trades = std::mem::take(&mut self.trades);
        self.dirty = false;
        RawSnapshot {
            timestamp: time::OffsetDateTime::now_utc(),
            brokers: self.brokers.clone(),
            calc_indexes: rest.calc_indexes.clone(),
            candlesticks: self.candlesticks.clone(),
            depths: self.depths.clone(),
            market_temperature: rest.market_temperature.clone(),
            quotes: self.quotes.clone(),
            trades,
            capital_flows: rest.capital_flows.clone(),
            capital_distributions: rest.capital_distributions.clone(),
        }
    }
}

pub(super) fn append_trades_with_cap(buffer: &mut Vec<Trade>, mut trades: Vec<Trade>) {
    buffer.append(&mut trades);
    if buffer.len() > TRADE_BUFFER_CAP_PER_SYMBOL {
        buffer.drain(..buffer.len() - TRADE_BUFFER_CAP_PER_SYMBOL);
    }
}

/// REST-only data that doesn't come via push.
pub(super) struct RestSnapshot {
    pub(super) calc_indexes: HashMap<Symbol, SecurityCalcIndex>,
    pub(super) capital_flows: HashMap<Symbol, Vec<longport::quote::CapitalFlowLine>>,
    pub(super) capital_distributions: HashMap<Symbol, longport::quote::CapitalDistributionResponse>,
    pub(super) market_temperature: Option<MarketTemperature>,
    pub(super) polymarket: PolymarketSnapshot,
}

impl RestSnapshot {
    pub(super) fn empty() -> Self {
        Self {
            calc_indexes: HashMap::new(),
            capital_flows: HashMap::new(),
            capital_distributions: HashMap::new(),
            market_temperature: None,
            polymarket: PolymarketSnapshot::default(),
        }
    }
}

pub(super) struct HkTickState<'a> {
    pub(super) live: &'a mut LiveState,
    pub(super) rest: &'a mut RestSnapshot,
    pub(super) rest_updated: &'a mut bool,
}

impl TickState<PushEvent, RestSnapshot> for HkTickState<'_> {
    fn apply_push(&mut self, event: PushEvent) {
        self.live.apply(event);
    }

    fn apply_update(&mut self, update: RestSnapshot) {
        *self.rest = update;
        *self.rest_updated = true;
        self.live.dirty = true;
    }

    fn is_dirty(&self) -> bool {
        self.live.dirty
    }

    fn clear_dirty(&mut self) {
        self.live.dirty = false;
    }
}

pub(super) async fn fetch_market_context(
    ctx: &QuoteContext,
    watchlist: &[Symbol],
) -> (
    HashMap<Symbol, SecurityCalcIndex>,
    Option<MarketTemperature>,
) {
    let calc_indexes = match ctx
        .calc_indexes(
            watchlist.iter().map(|s| s.0.clone()).collect::<Vec<_>>(),
            [
                CalcIndex::TurnoverRate,
                CalcIndex::VolumeRatio,
                CalcIndex::PeTtmRatio,
                CalcIndex::PbRatio,
                CalcIndex::Amplitude,
                CalcIndex::FiveMinutesChangeRate,
                CalcIndex::DividendRatioTtm,
            ],
        )
        .await
    {
        Ok(indexes) => indexes
            .into_iter()
            .map(|idx| (Symbol(idx.symbol.clone()), idx))
            .collect(),
        Err(e) => {
            eprintln!("Warning: calc_indexes failed: {}", e);
            HashMap::new()
        }
    };

    let market_temperature = match ctx.market_temperature(Market::HK).await {
        Ok(temp) => Some(temp),
        Err(e) => {
            eprintln!("Warning: market_temperature failed: {}", e);
            None
        }
    };

    (calc_indexes, market_temperature)
}

/// Fetch REST-only data that doesn't come via push.
/// Batches requests to stay under Longport's 10 req/s rate limit.
pub(super) async fn fetch_rest_data(
    ctx: &QuoteContext,
    watchlist: &[Symbol],
    polymarket_configs: &[PolymarketMarketConfig],
) -> RestSnapshot {
    use futures::stream::{self, StreamExt};

    const BATCH_CONCURRENCY: usize = 8; // max concurrent requests per stream

    let flow_future = stream::iter(watchlist.iter().cloned())
        .map(|sym| {
            let ctx = ctx.clone();
            async move {
                match ctx.capital_flow(sym.0.clone()).await {
                    Ok(f) => Some((sym, f)),
                    Err(e) => {
                        eprintln!("Warning: capital_flow({}) failed: {}", sym, e);
                        None
                    }
                }
            }
        })
        .buffer_unordered(BATCH_CONCURRENCY)
        .collect::<Vec<_>>();

    let dist_future = stream::iter(watchlist.iter().cloned())
        .map(|sym| {
            let ctx = ctx.clone();
            async move {
                match ctx.capital_distribution(sym.0.clone()).await {
                    Ok(d) => Some((sym, d)),
                    Err(e) => {
                        eprintln!("Warning: capital_distribution({}) failed: {}", sym, e);
                        None
                    }
                }
            }
        })
        .buffer_unordered(BATCH_CONCURRENCY)
        .collect::<Vec<_>>();

    let market_context_future = fetch_market_context(ctx, watchlist);
    let polymarket_future = fetch_polymarket_snapshot(polymarket_configs);

    let (flow_results, dist_results, (calc_indexes, market_temperature), polymarket_snapshot) = tokio::join!(
        flow_future,
        dist_future,
        market_context_future,
        polymarket_future
    );

    RestSnapshot {
        calc_indexes,
        capital_flows: flow_results.into_iter().flatten().collect(),
        capital_distributions: dist_results.into_iter().flatten().collect(),
        market_temperature,
        polymarket: polymarket_snapshot.unwrap_or_else(|error| {
            rate_limited_polymarket_warning(&format!(
                "Warning: Polymarket refresh failed: {}",
                error
            ));
            PolymarketSnapshot::default()
        }),
    }
}

pub(super) fn rate_limited_polymarket_warning(message: &str) {
    static LAST_WARNING_AT: OnceLock<Mutex<Option<Instant>>> = OnceLock::new();
    let mutex = LAST_WARNING_AT.get_or_init(|| Mutex::new(None));
    let Ok(mut guard) = mutex.lock() else {
        eprintln!("{}", message);
        return;
    };
    let should_log = guard
        .map(|instant| instant.elapsed() >= POLYMARKET_WARNING_INTERVAL)
        .unwrap_or(true);
    if should_log {
        eprintln!("{}", message);
        *guard = Some(Instant::now());
    }
}
