use std::collections::HashMap;

use futures::future::join_all;
use longport::quote::{
    CapitalDistributionResponse, CapitalFlowLine, QuoteContext, SecurityBrokers, SecurityDepth,
    SecurityQuote,
};
use time::OffsetDateTime;

use super::objects::Symbol;

/// Raw API responses from a single polling cycle.
/// This struct isolates longport types — downstream code (links.rs) works with this,
/// not with longport directly.
pub struct RawSnapshot {
    pub timestamp: OffsetDateTime,
    pub brokers: HashMap<Symbol, SecurityBrokers>,
    pub capital_flows: HashMap<Symbol, Vec<CapitalFlowLine>>,
    pub capital_distributions: HashMap<Symbol, CapitalDistributionResponse>,
    pub depths: HashMap<Symbol, SecurityDepth>,
    pub quotes: HashMap<Symbol, SecurityQuote>,
}

impl RawSnapshot {
    #[cfg(test)]
    pub fn empty() -> Self {
        RawSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            brokers: HashMap::new(),
            capital_flows: HashMap::new(),
            capital_distributions: HashMap::new(),
            depths: HashMap::new(),
            quotes: HashMap::new(),
        }
    }
}

/// Fetch broker queues, capital flows, and capital distributions for all watchlist symbols.
/// Individual API failures are logged and skipped — they don't affect other symbols.
pub async fn fetch(ctx: &QuoteContext, watchlist: &[Symbol]) -> RawSnapshot {
    let timestamp = OffsetDateTime::now_utc();

    // Fetch all three APIs for each symbol concurrently
    let broker_futures: Vec<_> = watchlist
        .iter()
        .map(|sym| {
            let ctx = ctx.clone();
            let symbol_str = sym.0.clone();
            let sym = sym.clone();
            async move {
                match ctx.brokers(symbol_str).await {
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
            let symbol_str = sym.0.clone();
            let sym = sym.clone();
            async move {
                match ctx.capital_flow(symbol_str).await {
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
            let symbol_str = sym.0.clone();
            let sym = sym.clone();
            async move {
                match ctx.capital_distribution(symbol_str).await {
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
            let symbol_str = sym.0.clone();
            let sym = sym.clone();
            async move {
                match ctx.depth(symbol_str).await {
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

    let brokers: HashMap<Symbol, SecurityBrokers> =
        broker_results.into_iter().flatten().collect();
    let capital_flows: HashMap<Symbol, Vec<CapitalFlowLine>> =
        flow_results.into_iter().flatten().collect();
    let capital_distributions: HashMap<Symbol, CapitalDistributionResponse> =
        dist_results.into_iter().flatten().collect();
    let depths: HashMap<Symbol, SecurityDepth> =
        depth_results.into_iter().flatten().collect();
    let quotes: HashMap<Symbol, SecurityQuote> = quote_results
        .into_iter()
        .map(|q| (Symbol(q.symbol.clone()), q))
        .collect();

    RawSnapshot {
        timestamp,
        brokers,
        capital_flows,
        capital_distributions,
        depths,
        quotes,
    }
}
