use super::*;
use crate::temporal::session::is_hk_regular_market_hours;

pub(super) const TRADE_BUFFER_CAP_PER_SYMBOL: usize = 2_000;
pub(super) const POLYMARKET_WARNING_INTERVAL: std::time::Duration =
    std::time::Duration::from_secs(300);

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

    pub(super) fn apply_batch(&mut self, events: Vec<PushEvent>) {
        for event in events {
            self.apply(event);
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
#[allow(dead_code)]
pub(super) struct RestSnapshot {
    pub(super) calc_indexes: HashMap<Symbol, SecurityCalcIndex>,
    pub(super) capital_flows: HashMap<Symbol, Vec<longport::quote::CapitalFlowLine>>,
    pub(super) capital_distributions: HashMap<Symbol, longport::quote::CapitalDistributionResponse>,
    pub(super) market_temperature: Option<MarketTemperature>,
    pub(super) polymarket: PolymarketSnapshot,
    pub(super) intraday_lines: HashMap<Symbol, Vec<longport::quote::IntradayLine>>,
}

impl RestSnapshot {
    pub(super) fn empty() -> Self {
        Self {
            calc_indexes: HashMap::new(),
            capital_flows: HashMap::new(),
            capital_distributions: HashMap::new(),
            market_temperature: None,
            polymarket: PolymarketSnapshot::default(),
            intraday_lines: HashMap::new(),
        }
    }
}

pub(super) struct HkTickState<'a> {
    pub(super) live: &'a mut LiveState,
    pub(super) rest: &'a mut RestSnapshot,
    pub(super) rest_updated: &'a mut bool,
}

impl TickState<Vec<PushEvent>, RestSnapshot> for HkTickState<'_> {
    fn apply_push(&mut self, events: Vec<PushEvent>) {
        self.live.apply_batch(events);
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
                CalcIndex::YtdChangeRate,
                CalcIndex::FiveDayChangeRate,
                CalcIndex::TenDayChangeRate,
                CalcIndex::HalfYearChangeRate,
                CalcIndex::TotalMarketValue,
                CalcIndex::CapitalFlow,
                CalcIndex::ChangeRate,
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
///
/// Longport SDK throttles to ~6 req/s internally. Per-symbol endpoints
/// (capital_flow, capital_distribution, intraday) are the bottleneck:
///   494 symbols × 3 endpoints ÷ 6 req/s = ~247s (far exceeds 60s cycle).
///
/// Solution: fetch batch endpoints for ALL symbols, then fetch per-symbol
/// data only for the top N by turnover (matching US runtime strategy).
pub(super) async fn fetch_rest_data(
    ctx: &QuoteContext,
    watchlist: &[Symbol],
    polymarket_configs: &[PolymarketMarketConfig],
) -> RestSnapshot {
    if !is_hk_regular_market_hours(time::OffsetDateTime::now_utc()) {
        return RestSnapshot {
            calc_indexes: HashMap::new(),
            capital_flows: HashMap::new(),
            capital_distributions: HashMap::new(),
            market_temperature: None,
            polymarket: PolymarketSnapshot::default(),
            intraday_lines: HashMap::new(),
        };
    }

    use futures::stream::{self, StreamExt};

    // Longport SDK auto-throttles at ~6 req/s. Keep concurrency low.
    const API_CONCURRENCY: usize = 2;
    // calc_indexes batch provides CapitalFlow indicator for ALL symbols (0 extra reqs).
    // Per-symbol capital_flow() is redundant — removed.
    // capital_distribution provides institutional breakdown (large/medium/small) — keep.
    const CAPITAL_DIST_TOP_N: usize = 40;
    const INTRADAY_TOP_N: usize = 20;

    // Step 1: Batch endpoints first (calc_indexes + market_temperature + polymarket).
    // These are 1-2 requests total regardless of symbol count.
    let market_context_future = fetch_market_context(ctx, watchlist);
    let polymarket_future = fetch_polymarket_snapshot(polymarket_configs);

    let quote_future = async {
        match ctx
            .quote(watchlist.iter().map(|s| s.0.clone()).collect::<Vec<_>>())
            .await
        {
            Ok(quotes) => quotes
                .into_iter()
                .map(|q| (Symbol(q.symbol.clone()), q))
                .collect::<HashMap<_, _>>(),
            Err(e) => {
                eprintln!("Warning: HK quote batch failed: {}", e);
                HashMap::new()
            }
        }
    };

    let ((calc_indexes, market_temperature), polymarket_snapshot, quotes) =
        tokio::join!(market_context_future, polymarket_future, quote_future);

    // Step 2: Rank symbols by turnover to select top N for per-symbol endpoints.
    let mut ranked_symbols: Vec<_> = quotes
        .iter()
        .map(|(sym, q)| (sym.clone(), q.turnover))
        .collect();
    ranked_symbols.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    let top_dist_symbols: Vec<Symbol> = ranked_symbols
        .iter()
        .take(CAPITAL_DIST_TOP_N)
        .map(|(sym, _)| sym.clone())
        .collect();
    let top_intraday_symbols: Vec<Symbol> = ranked_symbols
        .iter()
        .take(INTRADAY_TOP_N)
        .map(|(sym, _)| sym.clone())
        .collect();

    // Step 3: Per-symbol endpoints for top N only.
    // 40 dist + 20 intraday = 60 requests ÷ 6 req/s = ~10s (well within 60s cycle).
    let dist_future = stream::iter(top_dist_symbols.into_iter())
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
        .buffer_unordered(API_CONCURRENCY)
        .collect::<Vec<_>>();

    let intraday_future = stream::iter(top_intraday_symbols.into_iter())
        .map(|sym| {
            let ctx = ctx.clone();
            async move {
                match ctx
                    .intraday(sym.0.clone(), longport::quote::TradeSessions::Intraday)
                    .await
                {
                    Ok(lines) => Some((sym, lines)),
                    Err(e) => {
                        eprintln!("Warning: intraday({}) failed: {}", sym, e);
                        None
                    }
                }
            }
        })
        .buffer_unordered(API_CONCURRENCY)
        .collect::<Vec<_>>();

    let (dist_results, intraday_results) =
        tokio::join!(dist_future, intraday_future);

    RestSnapshot {
        calc_indexes,
        capital_flows: HashMap::new(), // capital flow covered by calc_indexes batch
        capital_distributions: dist_results.into_iter().flatten().collect(),
        market_temperature,
        polymarket: polymarket_snapshot.unwrap_or_else(|error| {
            rate_limited_polymarket_warning(&format!(
                "Warning: Polymarket refresh failed: {}",
                error
            ));
            PolymarketSnapshot::default()
        }),
        intraday_lines: intraday_results.into_iter().flatten().collect(),
    }
}

#[allow(dead_code)]
pub(super) async fn fetch_warrant_sentiment(
    ctx: &QuoteContext,
    watchlist: &[Symbol],
) -> Vec<crate::ontology::links::WarrantSentimentObservation> {
    use longport::quote::{SortOrderType, WarrantSortBy};

    let mut results = Vec::new();
    for sym in watchlist {
        let warrants = match ctx
            .warrant_list(
                sym.0.clone(),
                WarrantSortBy::LastDone,
                SortOrderType::Descending,
                None,
                None,
                None,
                None,
                None,
            )
            .await
        {
            Ok(w) => w,
            Err(_) => continue,
        };
        if warrants.is_empty() {
            continue;
        }

        let mut call_count = 0usize;
        let mut put_count = 0usize;
        let mut top_call_oustanding: Option<Decimal> = None;
        let mut top_put_outstanding: Option<Decimal> = None;
        let mut call_iv_sum = Decimal::ZERO;
        let mut call_iv_n = 0usize;
        let mut put_iv_sum = Decimal::ZERO;
        let mut put_iv_n = 0usize;

        for w in &warrants {
            let is_call = matches!(w.warrant_type, longport::quote::WarrantType::Call);
            let is_put = matches!(w.warrant_type, longport::quote::WarrantType::Put);

            if is_call {
                call_count += 1;
                if let Some(iv) = w.implied_volatility {
                    if iv > Decimal::ZERO {
                        call_iv_sum += iv;
                        call_iv_n += 1;
                    }
                }
                if top_call_oustanding.is_none()
                    || w.outstanding_ratio > top_call_oustanding.unwrap_or(Decimal::ZERO)
                {
                    top_call_oustanding = Some(w.outstanding_ratio);
                }
            } else if is_put {
                put_count += 1;
                if let Some(iv) = w.implied_volatility {
                    if iv > Decimal::ZERO {
                        put_iv_sum += iv;
                        put_iv_n += 1;
                    }
                }
                if top_put_outstanding.is_none()
                    || w.outstanding_ratio > top_put_outstanding.unwrap_or(Decimal::ZERO)
                {
                    top_put_outstanding = Some(w.outstanding_ratio);
                }
            }
        }

        results.push(crate::ontology::links::WarrantSentimentObservation {
            underlying: sym.clone(),
            total_warrants: warrants.len(),
            call_warrant_count: call_count,
            put_warrant_count: put_count,
            top_call_outstanding_ratio: top_call_oustanding,
            top_put_outstanding_ratio: top_put_outstanding,
            weighted_call_iv: if call_iv_n > 0 {
                Some(call_iv_sum / Decimal::from(call_iv_n as i64))
            } else {
                None
            },
            weighted_put_iv: if put_iv_n > 0 {
                Some(put_iv_sum / Decimal::from(put_iv_n as i64))
            } else {
                None
            },
        });
    }

    results
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
