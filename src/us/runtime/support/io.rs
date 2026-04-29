use super::*;
use crate::ontology::links::OptionSurfaceObservation;
use longport::quote::{IntradayLine, SecurityCalcIndex, SecurityQuote};

/// Fetch intraday lines with a single retry on Longport rate-limit (301606).
/// Longport's 301606 errors surface when the terrain builder and tick-loop REST
/// fetch hit the ~6 req/s budget in parallel. A 500ms backoff + one retry is
/// enough to clear 95% of transient bursts without needing a global limiter.
async fn fetch_intraday_with_retry(
    ctx: &QuoteContext,
    sym: &Symbol,
) -> Result<Vec<IntradayLine>, longport::Error> {
    match ctx.intraday(sym.0.clone(), TradeSessions::Intraday).await {
        Ok(lines) => Ok(lines),
        Err(e) => {
            let msg = format!("{e}");
            if msg.contains("301606") || msg.contains("rate limit") {
                tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                ctx.intraday(sym.0.clone(), TradeSessions::Intraday).await
            } else {
                Err(e)
            }
        }
    }
}

pub(crate) async fn initialize_us_store(
    ctx: &QuoteContext,
    watchlist: &[Symbol],
) -> Arc<ObjectStore> {
    // Longport limits static_info to ~500 symbols per request, so chunk.
    let mut static_infos = Vec::new();
    for chunk in watchlist.chunks(400) {
        let symbols_vec: Vec<String> = chunk.iter().map(|s| s.0.clone()).collect();
        match ctx.static_info(symbols_vec).await {
            Ok(infos) => static_infos.extend(infos),
            Err(e) => {
                eprintln!("Warning: static_info failed for chunk: {}", e);
            }
        }
    }

    let stocks: Vec<Stock> = static_infos
        .into_iter()
        .map(|info| {
            let symbol = Symbol(info.symbol.clone());
            Stock {
                market: symbol.market(),
                symbol,
                name_en: info.name_en.clone(),
                name_cn: info.name_cn.clone(),
                name_hk: info.name_hk.clone(),
                exchange: info.exchange.clone(),
                lot_size: info.lot_size,
                sector_id: us_symbol_sector(&info.symbol).map(|s| SectorId(s.into())),
                total_shares: info.total_shares,
                circulating_shares: info.circulating_shares,
                eps_ttm: info.eps_ttm,
                bps: info.bps,
                dividend_yield: info.dividend_yield,
            }
        })
        .collect();

    let stock_map: HashMap<Symbol, Stock> =
        stocks.into_iter().map(|s| (s.symbol.clone(), s)).collect();

    let sectors: HashMap<SectorId, crate::ontology::objects::Sector> = us_sector_names()
        .iter()
        .map(|(id, name)| {
            (
                SectorId(id.to_string()),
                crate::ontology::objects::Sector {
                    id: SectorId(id.to_string()),
                    name: name.to_string(),
                },
            )
        })
        .collect();

    Arc::new(ObjectStore {
        institutions: HashMap::new(),
        brokers: HashMap::new(),
        stocks: stock_map,
        sectors,
        broker_to_institution: HashMap::new(),
        knowledge: std::sync::RwLock::new(crate::ontology::store::AccumulatedKnowledge::empty()),
    })
}

pub(crate) async fn fetch_us_rest_data(ctx: &QuoteContext, watchlist: &[Symbol]) -> UsRestSnapshot {
    if !is_us_regular_market_hours(time::OffsetDateTime::now_utc()) {
        return UsRestSnapshot::empty();
    }

    let (calc_indexes, quotes) = fetch_us_batch_quotes_and_indexes(ctx, watchlist).await;

    // Longport SDK auto-throttles at ~6 req/s. Serialize intraday fetches so the
    // tick-loop REST path never contends with terrain-builder parallel calls.
    // Combined with fetch_intraday_with_retry, this eliminates the 301606 noise.
    const API_CONCURRENCY: usize = 1;
    // calc_indexes batch already provides CapitalFlow indicator for ALL symbols (0 extra reqs).
    // Per-symbol capital_flow() is redundant — removed to save 80 reqs/cycle.
    // Only fetch intraday for top N (needed for VWAP divergence detection).
    const INTRADAY_TOP_N: usize = 20;

    // Step 2: Determine top symbols by turnover for per-symbol endpoints
    let mut ranked_symbols: Vec<_> = quotes
        .iter()
        .filter(|(_, q)| q.turnover > rust_decimal::Decimal::ZERO)
        .collect::<Vec<_>>();
    ranked_symbols.sort_by(|a, b| b.1.turnover.cmp(&a.1.turnover));

    let intraday_watchlist: Vec<Symbol> = ranked_symbols
        .iter()
        .take(INTRADAY_TOP_N)
        .map(|(sym, _)| (*sym).clone())
        .collect();

    // Step 3: Per-symbol endpoints — only intraday (capital_flow covered by calc_indexes batch)
    let intraday_results: Vec<_> = stream::iter(intraday_watchlist.into_iter())
        .map(|sym| {
            let ctx = ctx.clone();
            async move {
                match fetch_intraday_with_retry(&ctx, &sym).await {
                    Ok(lines) => Some((sym, lines)),
                    Err(e) => {
                        eprintln!("Warning: intraday({}) failed after retry: {}", sym, e);
                        None
                    }
                }
            }
        })
        .buffer_unordered(API_CONCURRENCY)
        .collect()
        .await;

    // Step 4: Option surfaces for top 20 by turnover
    let option_watchlist: Vec<Symbol> = ranked_symbols
        .iter()
        .take(20)
        .map(|(sym, _)| (*sym).clone())
        .collect();
    let option_surfaces = if !option_watchlist.is_empty() {
        fetch_us_option_surfaces(ctx, &option_watchlist, &quotes).await
    } else {
        Vec::new()
    };

    UsRestSnapshot {
        quotes,
        calc_indexes,
        capital_flows: HashMap::new(), // capital flow covered by calc_indexes batch
        intraday_lines: intraday_results.into_iter().flatten().collect(),
        option_surfaces,
    }
}

pub(crate) async fn fetch_us_bootstrap_rest_data(
    ctx: &QuoteContext,
    watchlist: &[Symbol],
) -> UsRestSnapshot {
    if !is_us_regular_market_hours(time::OffsetDateTime::now_utc()) {
        return UsRestSnapshot::empty();
    }

    let (calc_indexes, quotes) = fetch_us_batch_quotes_and_indexes(ctx, watchlist).await;
    UsRestSnapshot {
        quotes,
        calc_indexes,
        capital_flows: HashMap::new(),
        intraday_lines: HashMap::new(),
        option_surfaces: Vec::new(),
    }
}

async fn fetch_us_batch_quotes_and_indexes(
    ctx: &QuoteContext,
    watchlist: &[Symbol],
) -> (
    HashMap<Symbol, SecurityCalcIndex>,
    HashMap<Symbol, SecurityQuote>,
) {
    const BATCH_CHUNK: usize = 500;

    let calc_future = async {
        let mut all_indexes = HashMap::new();
        for chunk in watchlist.chunks(BATCH_CHUNK) {
            match ctx
                .calc_indexes(
                    chunk.iter().map(|s| s.0.clone()).collect::<Vec<_>>(),
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
                Ok(indexes) => {
                    for idx in indexes {
                        all_indexes.insert(Symbol(idx.symbol.clone()), idx);
                    }
                }
                Err(e) => {
                    eprintln!("Warning: calc_indexes failed for chunk: {}", e);
                }
            }
        }
        all_indexes
    };

    let quote_future = async {
        let mut all_quotes = HashMap::new();
        for chunk in watchlist.chunks(BATCH_CHUNK) {
            match ctx
                .quote(chunk.iter().map(|s| s.0.clone()).collect::<Vec<_>>())
                .await
            {
                Ok(quotes) => {
                    for quote in quotes {
                        all_quotes.insert(Symbol(quote.symbol.clone()), quote);
                    }
                }
                Err(e) => {
                    eprintln!("Warning: US quote batch failed for chunk: {}", e);
                }
            }
        }
        all_quotes
    };

    tokio::join!(calc_future, quote_future)
}

pub(crate) async fn fetch_us_option_surfaces(
    ctx: &QuoteContext,
    watchlist: &[Symbol],
    quotes: &HashMap<Symbol, SecurityQuote>,
) -> Vec<OptionSurfaceObservation> {
    let mut surfaces = Vec::new();

    for sym in watchlist {
        let last_price = match quotes.get(sym) {
            Some(q) if q.last_done > Decimal::ZERO => q.last_done,
            _ => continue,
        };

        let expiry_dates = match ctx.option_chain_expiry_date_list(sym.0.clone()).await {
            Ok(dates) if !dates.is_empty() => dates,
            _ => continue,
        };

        let nearest_expiry = expiry_dates[0];

        let chain = match ctx
            .option_chain_info_by_date(sym.0.clone(), nearest_expiry)
            .await
        {
            Ok(c) if !c.is_empty() => c,
            _ => continue,
        };

        let mut best_call: Option<String> = None;
        let mut best_put: Option<String> = None;
        let mut best_call_dist = Decimal::MAX;
        let mut best_put_dist = Decimal::MAX;

        for info in &chain {
            let strike = info.price;
            let dist = (strike - last_price).abs();
            if !info.call_symbol.is_empty() && dist < best_call_dist {
                best_call_dist = dist;
                best_call = Some(info.call_symbol.clone());
            }
            if !info.put_symbol.is_empty() && dist < best_put_dist {
                best_put_dist = dist;
                best_put = Some(info.put_symbol.clone());
            }
        }

        let mut option_symbols = Vec::new();
        if let Some(ref c) = best_call {
            option_symbols.push(c.clone());
        }
        if let Some(ref p) = best_put {
            option_symbols.push(p.clone());
        }
        if option_symbols.is_empty() {
            continue;
        }

        let greeks = match ctx
            .calc_indexes(
                option_symbols,
                [
                    CalcIndex::ImpliedVolatility,
                    CalcIndex::Delta,
                    CalcIndex::Vega,
                    CalcIndex::OpenInterest,
                ],
            )
            .await
        {
            Ok(g) => g,
            Err(_) => continue,
        };

        let call_greeks = best_call
            .as_ref()
            .and_then(|cs| greeks.iter().find(|g| &g.symbol == cs));
        let put_greeks = best_put
            .as_ref()
            .and_then(|ps| greeks.iter().find(|g| &g.symbol == ps));

        let call_iv = call_greeks.and_then(|g| g.implied_volatility);
        let put_iv = put_greeks.and_then(|g| g.implied_volatility);
        let skew = match (put_iv, call_iv) {
            (Some(p), Some(c)) if c > Decimal::ZERO => Some(p / c - Decimal::ONE),
            _ => None,
        };
        let call_oi = call_greeks.and_then(|g| g.open_interest).unwrap_or(0);
        let put_oi = put_greeks.and_then(|g| g.open_interest).unwrap_or(0);
        let pc_ratio = if call_oi > 0 {
            Some(Decimal::from(put_oi) / Decimal::from(call_oi))
        } else {
            None
        };

        let expiry_label = format!(
            "{:04}-{:02}-{:02}",
            nearest_expiry.year(),
            nearest_expiry.month() as u8,
            nearest_expiry.day(),
        );

        surfaces.push(OptionSurfaceObservation {
            underlying: sym.clone(),
            expiry_label,
            atm_call_iv: call_iv,
            atm_put_iv: put_iv,
            put_call_skew: skew,
            total_call_oi: call_oi,
            total_put_oi: put_oi,
            put_call_oi_ratio: pc_ratio,
            atm_delta: call_greeks.and_then(|g| g.delta),
            atm_vega: call_greeks.and_then(|g| g.vega),
        });
    }

    surfaces
}
