use std::collections::HashMap;

use futures::future::join_all;
use longport::quote::{
    Candlestick, CapitalDistributionResponse, CapitalFlowLine, MarketTemperature, QuoteContext,
    SecurityBrokers, SecurityCalcIndex, SecurityDepth, SecurityQuote, Trade,
};
use time::OffsetDateTime;

use super::objects::Symbol;

/// Raw API responses — combines WebSocket push state and REST data.
/// This struct isolates longport types — downstream code (links.rs) works with this,
/// not with longport directly.
pub struct RawSnapshot {
    pub timestamp: OffsetDateTime,
    pub brokers: HashMap<Symbol, SecurityBrokers>,
    pub calc_indexes: HashMap<Symbol, SecurityCalcIndex>,
    pub candlesticks: HashMap<Symbol, Vec<Candlestick>>,
    pub capital_flows: HashMap<Symbol, Vec<CapitalFlowLine>>,
    pub capital_distributions: HashMap<Symbol, CapitalDistributionResponse>,
    pub depths: HashMap<Symbol, SecurityDepth>,
    pub market_temperature: Option<MarketTemperature>,
    pub quotes: HashMap<Symbol, SecurityQuote>,
    pub trades: HashMap<Symbol, Vec<Trade>>,
}

impl RawSnapshot {
    #[cfg(test)]
    pub fn empty() -> Self {
        RawSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            brokers: HashMap::new(),
            calc_indexes: HashMap::new(),
            candlesticks: HashMap::new(),
            capital_flows: HashMap::new(),
            capital_distributions: HashMap::new(),
            depths: HashMap::new(),
            market_temperature: None,
            quotes: HashMap::new(),
            trades: HashMap::new(),
        }
    }
}

pub async fn fetch_quotes_only(
    ctx: &QuoteContext,
    watchlist: &[Symbol],
) -> HashMap<Symbol, SecurityQuote> {
    let symbols_vec: Vec<String> = watchlist.iter().map(|s| s.0.clone()).collect();
    match ctx.quote(symbols_vec).await {
        Ok(quotes) => quotes
            .into_iter()
            .map(|q| (Symbol(q.symbol.clone()), q))
            .collect(),
        Err(e) => {
            eprintln!("Warning: quote bootstrap failed: {}", e);
            HashMap::new()
        }
    }
}

/// Fetch broker queues, capital flows, and capital distributions for all watchlist symbols.
/// Individual API failures are logged and skipped — they don't affect other symbols.
pub async fn fetch(ctx: &QuoteContext, watchlist: &[Symbol]) -> RawSnapshot {
    let timestamp = OffsetDateTime::now_utc();

    // Fire all per-symbol requests concurrently via join_all.
    // Longport SDK handles internal queuing; a few transient failures are acceptable
    // since we refresh capital data every 60s anyway.
    let broker_futures: Vec<_> = watchlist
        .iter()
        .map(|sym| {
            let ctx = ctx.clone();
            let sym = sym.clone();
            async move {
                match ctx.brokers(sym.0.clone()).await {
                    Ok(b) => Some((sym, b)),
                    Err(e) => {
                        eprintln!("Warning: brokers({}) failed: {}", sym, e);
                        None
                    }
                }
            }
        })
        .collect();

    let flow_futures: Vec<_> = watchlist
        .iter()
        .map(|sym| {
            let ctx = ctx.clone();
            let sym = sym.clone();
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
        .collect();

    let dist_futures: Vec<_> = watchlist
        .iter()
        .map(|sym| {
            let ctx = ctx.clone();
            let sym = sym.clone();
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
        .collect();

    let depth_futures: Vec<_> = watchlist
        .iter()
        .map(|sym| {
            let ctx = ctx.clone();
            let sym = sym.clone();
            async move {
                match ctx.depth(sym.0.clone()).await {
                    Ok(d) => Some((sym, d)),
                    Err(e) => {
                        eprintln!("Warning: depth({}) failed: {}", sym, e);
                        None
                    }
                }
            }
        })
        .collect();

    let quote_future = {
        let ctx = ctx.clone();
        let symbols_vec: Vec<String> = watchlist.iter().map(|s| s.0.clone()).collect();
        async move {
            match ctx.quote(symbols_vec).await {
                Ok(quotes) => quotes,
                Err(e) => {
                    eprintln!("Warning: quote batch failed: {}", e);
                    vec![]
                }
            }
        }
    };

    let (broker_results, flow_results, dist_results, depth_results, quote_results) = tokio::join!(
        join_all(broker_futures),
        join_all(flow_futures),
        join_all(dist_futures),
        join_all(depth_futures),
        quote_future,
    );

    let brokers: HashMap<Symbol, SecurityBrokers> = broker_results.into_iter().flatten().collect();
    let capital_flows: HashMap<Symbol, Vec<CapitalFlowLine>> =
        flow_results.into_iter().flatten().collect();
    let capital_distributions: HashMap<Symbol, CapitalDistributionResponse> =
        dist_results.into_iter().flatten().collect();
    let depths: HashMap<Symbol, SecurityDepth> = depth_results.into_iter().flatten().collect();
    let quotes: HashMap<Symbol, SecurityQuote> = quote_results
        .into_iter()
        .map(|q| (Symbol(q.symbol.clone()), q))
        .collect();

    RawSnapshot {
        timestamp,
        brokers,
        calc_indexes: HashMap::new(),
        candlesticks: HashMap::new(),
        capital_flows,
        capital_distributions,
        depths,
        market_temperature: None,
        quotes,
        trades: HashMap::new(), // REST fetch doesn't include trades; populated from WebSocket push
    }
}
