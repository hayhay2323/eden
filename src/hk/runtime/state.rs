use super::*;
use crate::pipeline::raw_events::{RawEventSource, RawEventStore};
use crate::temporal::session::is_hk_regular_market_hours;

pub(super) const TRADE_BUFFER_CAP_PER_SYMBOL: usize = 2_000;

/// Live market state accumulated from WebSocket push events.
pub(super) struct LiveState {
    pub(super) depths: HashMap<Symbol, SecurityDepth>,
    pub(super) brokers: HashMap<Symbol, SecurityBrokers>,
    pub(super) quotes: HashMap<Symbol, SecurityQuote>,
    pub(super) trades: HashMap<Symbol, Vec<Trade>>,
    pub(super) candlesticks: HashMap<Symbol, Vec<longport::quote::Candlestick>>,
    pub(super) raw_events: RawEventStore,
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
            raw_events: RawEventStore::default(),
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
                let snapshot = SecurityDepth {
                    asks: depth.asks,
                    bids: depth.bids,
                };
                self.raw_events.record_depth(
                    symbol.clone(),
                    snapshot.clone(),
                    time::OffsetDateTime::now_utc(),
                    RawEventSource::Push,
                );
                self.depths.insert(symbol, snapshot);
            }
            PushEventDetail::Brokers(brokers) => {
                let snapshot = SecurityBrokers {
                    ask_brokers: brokers.ask_brokers,
                    bid_brokers: brokers.bid_brokers,
                };
                self.raw_events.record_brokers(
                    symbol.clone(),
                    snapshot.clone(),
                    time::OffsetDateTime::now_utc(),
                    RawEventSource::Push,
                );
                self.brokers.insert(symbol, snapshot);
            }
            PushEventDetail::Quote(quote) => {
                let existing = self.quotes.get(&symbol);
                let merged = SecurityQuote {
                    symbol: symbol.0.clone(),
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
                };
                self.raw_events
                    .record_quote(symbol.clone(), merged.clone(), RawEventSource::Push);
                self.quotes.insert(symbol, merged);
            }
            PushEventDetail::Trade(push_trades) => {
                self.raw_events.record_trades(
                    symbol.clone(),
                    &push_trades.trades,
                    time::OffsetDateTime::now_utc(),
                    RawEventSource::Push,
                );
                let entry = self.trades.entry(symbol).or_default();
                append_trades_with_cap(entry, push_trades.trades);
            }
            PushEventDetail::Candlestick(candle) => {
                self.raw_events.record_candlestick(
                    symbol.clone(),
                    candle.candlestick.clone(),
                    RawEventSource::Push,
                );
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
            intraday_lines: rest.intraday_lines.clone(),
            market_temperature: rest.market_temperature.clone(),
            option_surfaces: Vec::new(),
            quotes: self.quotes.clone(),
            trades,
            capital_flows: rest.capital_flows.clone(),
            capital_distributions: rest.capital_distributions.clone(),
        }
    }

    // 2026-04-29: removed `to_canonical_snapshot` here — it duplicated the
    // 8 HashMap clones already done by `to_raw_snapshot`. Callers should
    // build raw once via `to_raw_snapshot` and then call
    // `RawSnapshot::to_canonical_snapshot` (which takes `&self`) on the
    // result. See HK runtime call site for the pattern.
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
    pub(super) intraday_lines: HashMap<Symbol, Vec<longport::quote::IntradayLine>>,
    /// Warrant sentiment per underlying HK symbol (call/put counts, IV,
    /// top outstanding ratio). Populated from Longport warrant_list, which
    /// returns derivative warrants issued on each underlying. HK warrant
    /// market is ~25-40% of daily turnover so this is a first-class edge
    /// source, not decoration — see feedback_hk_microstructure_first.
    ///
    /// Field was missing until 2026-04-17; fetch_warrant_sentiment had been
    /// written but carried `#[allow(dead_code)]` because it was never
    /// called from the REST cycle.
    pub(super) warrants: HashMap<Symbol, crate::ontology::links::WarrantSentimentObservation>,
}

impl RestSnapshot {
    pub(super) fn empty() -> Self {
        Self {
            calc_indexes: HashMap::new(),
            capital_flows: HashMap::new(),
            capital_distributions: HashMap::new(),
            market_temperature: None,
            intraday_lines: HashMap::new(),
            warrants: HashMap::new(),
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
        // C4 fix: pressure-event bus publish moved upstream into the
        // longport push tap (see startup.rs). apply_push now only
        // ingests into live state — the bus already saw every event
        // before this batch was assembled.
        self.live.apply_batch(events);
    }

    fn apply_update(&mut self, update: RestSnapshot) {
        let ingested_at = time::OffsetDateTime::now_utc();
        self.live.raw_events.record_calc_index_snapshot(
            &update.calc_indexes,
            ingested_at,
            RawEventSource::Rest,
        );
        self.live.raw_events.record_capital_flow_snapshot(
            &update.capital_flows,
            ingested_at,
            RawEventSource::Rest,
        );
        self.live.raw_events.record_capital_distribution_snapshot(
            &update.capital_distributions,
            ingested_at,
            RawEventSource::Rest,
        );
        self.live.raw_events.record_intraday_snapshot(
            &update.intraday_lines,
            ingested_at,
            RawEventSource::Rest,
        );
        if let Some(temperature) = update.market_temperature.clone() {
            self.live
                .raw_events
                .record_market_temperature(temperature, RawEventSource::Rest);
        }
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
pub(super) async fn fetch_rest_data(ctx: &QuoteContext, watchlist: &[Symbol]) -> RestSnapshot {
    if !is_hk_regular_market_hours(time::OffsetDateTime::now_utc()) {
        return RestSnapshot {
            calc_indexes: HashMap::new(),
            capital_flows: HashMap::new(),
            capital_distributions: HashMap::new(),
            market_temperature: None,
            intraday_lines: HashMap::new(),
            warrants: HashMap::new(),
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
    /// Warrant fetch scope — only the top N underlyings by turnover. Every
    /// warrant_list call is 1 req; capping at 20 keeps the total REST
    /// budget under control (20 + 40 + 20 = 80 reqs ÷ 6 req/s ≈ 13s,
    /// comfortably within the 60s cycle).
    const WARRANT_TOP_N: usize = 20;

    // Step 1: Batch endpoints first (calc_indexes + market_temperature).
    // These are 1-2 requests total regardless of symbol count.
    let market_context_future = fetch_market_context(ctx, watchlist);

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

    let ((calc_indexes, market_temperature), quotes) =
        tokio::join!(market_context_future, quote_future);

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
    let top_warrant_underlyings: Vec<Symbol> = ranked_symbols
        .iter()
        .take(WARRANT_TOP_N)
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

    // Warrant sentiment per underlying — Longport returns the list of
    // call/put warrants issued on each underlying, from which we derive
    // call/put counts, weighted IV, and top outstanding ratios. HK warrant
    // flow is a substantial driver of underlying price (call warrant
    // outstanding ratio rising while underlying flat = retail positioning
    // ahead of a move).
    let warrant_future = fetch_warrant_sentiment(ctx, &top_warrant_underlyings);

    let (dist_results, intraday_results, warrant_results) =
        tokio::join!(dist_future, intraday_future, warrant_future);

    RestSnapshot {
        calc_indexes,
        capital_flows: HashMap::new(), // capital flow covered by calc_indexes batch
        capital_distributions: dist_results.into_iter().flatten().collect(),
        market_temperature,
        intraday_lines: intraday_results.into_iter().flatten().collect(),
        warrants: warrant_results
            .into_iter()
            .map(|obs| (obs.underlying.clone(), obs))
            .collect(),
    }
}

/// Fetch per-underlying warrant sentiment. Live since 2026-04-17; had been
/// scaffolded but never wired into the REST cycle before then, carrying
/// `#[allow(dead_code)]`. Now used from `fetch_rest_data` for the top 20
/// underlyings by turnover. See `RestSnapshot.warrants` for the consumer
/// side.
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
            Err(e) => {
                eprintln!("Warning: warrant_list({}) failed: {}", sym, e);
                continue;
            }
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
