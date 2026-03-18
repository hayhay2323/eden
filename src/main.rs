use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use longport::quote::{
    PushEvent, PushEventDetail, QuoteContext, SecurityBrokers, SecurityDepth, SecurityQuote,
    SubFlags, Trade,
};
use longport::Config;
use rust_decimal::Decimal;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};

use eden::action::narrative::NarrativeSnapshot;
use eden::graph::decision::{DecisionSnapshot, OrderDirection};
use eden::graph::graph::BrainGraph;
use eden::graph::tracker::PositionTracker;
use eden::logic::tension::TensionSnapshot;
use eden::ontology::links::LinkSnapshot;
use eden::ontology::objects::{BrokerId, Symbol};
use eden::ontology::snapshot::{self, RawSnapshot};
use eden::ontology::store;
use eden::pipeline::dimensions::DimensionSnapshot;
use eden::temporal::buffer::TickHistory;
use eden::temporal::record::TickRecord;
use eden::temporal::analysis::compute_dynamics;

const WATCHLIST: &[&str] = &[
    "700.HK",   // Tencent
    "9988.HK",  // Alibaba
    "3690.HK",  // Meituan
    "9618.HK",  // JD.com
    "1810.HK",  // Xiaomi
    "9888.HK",  // Baidu
    "268.HK",   // Kingdee
    "5.HK",     // HSBC
    "388.HK",   // HKEX
    "1398.HK",  // ICBC
    "3988.HK",  // Bank of China
    "939.HK",   // CCB
    "883.HK",   // CNOOC
    "857.HK",   // PetroChina
    "386.HK",   // Sinopec
    "941.HK",   // China Mobile
    "16.HK",    // SHK Properties
    "1109.HK",  // China Resources Land
    "2318.HK",  // Ping An
    "1299.HK",  // AIA
    "9868.HK",  // XPeng
    "2015.HK",  // Li Auto
    "2269.HK",  // WuXi Bio
];

/// Debounce window: after receiving a push event, wait this long for more
/// before running the pipeline. Batches rapid-fire events without adding latency.
const DEBOUNCE_MS: u64 = 2000;

/// Live market state accumulated from WebSocket push events.
struct LiveState {
    depths: HashMap<Symbol, SecurityDepth>,
    brokers: HashMap<Symbol, SecurityBrokers>,
    quotes: HashMap<Symbol, SecurityQuote>,
    trades: HashMap<Symbol, Vec<Trade>>,
    push_count: u64,
    dirty: bool, // true if new pushes since last pipeline run
}

impl LiveState {
    fn new() -> Self {
        Self {
            depths: HashMap::new(),
            brokers: HashMap::new(),
            quotes: HashMap::new(),
            trades: HashMap::new(),
            push_count: 0,
            dirty: false,
        }
    }

    fn apply(&mut self, event: PushEvent) {
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
                        prev_close: existing
                            .map(|q| q.prev_close)
                            .unwrap_or(Decimal::ZERO),
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
                entry.extend(push_trades.trades);
            }
            _ => {} // Candlestick — not used
        }
    }

    /// Merge live push state with REST-fetched capital data into a RawSnapshot.
    /// Consumes accumulated trades (they're per-tick, not cumulative).
    fn to_raw_snapshot(&mut self, rest: &RestSnapshot) -> RawSnapshot {
        let trades = std::mem::take(&mut self.trades);
        self.dirty = false;
        RawSnapshot {
            timestamp: time::OffsetDateTime::now_utc(),
            brokers: self.brokers.clone(),
            depths: self.depths.clone(),
            quotes: self.quotes.clone(),
            trades,
            capital_flows: rest.capital_flows.clone(),
            capital_distributions: rest.capital_distributions.clone(),
        }
    }
}

/// REST-only data that doesn't come via push.
struct RestSnapshot {
    capital_flows: HashMap<Symbol, Vec<longport::quote::CapitalFlowLine>>,
    capital_distributions: HashMap<Symbol, longport::quote::CapitalDistributionResponse>,
}

/// Fetch only capital flow + distribution via REST (not push-able).
async fn fetch_capital_data(ctx: &QuoteContext, watchlist: &[Symbol]) -> RestSnapshot {
    use futures::future::join_all;

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

    let (flow_results, dist_results) = tokio::join!(join_all(flow_futures), join_all(dist_futures));

    RestSnapshot {
        capital_flows: flow_results.into_iter().flatten().collect(),
        capital_distributions: dist_results.into_iter().flatten().collect(),
    }
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let config = Config::from_env().expect("failed to load Longport config from env");
    let (ctx, mut receiver) = QuoteContext::try_new(Arc::new(config))
        .await
        .expect("failed to connect to Longport");

    println!("Connected to Longport. Initializing ObjectStore...");

    let store = store::initialize(&ctx, WATCHLIST).await;

    println!("\n=== ObjectStore Stats ===");
    println!("Institutions: {}", store.institutions.len());
    println!("Brokers:      {}", store.brokers.len());
    println!("Stocks:       {}", store.stocks.len());
    println!("Sectors:      {}", store.sectors.len());

    let test_broker = BrokerId(4497);
    if let Some(inst) = store.institution_for_broker(&test_broker) {
        println!("\nBroker {} → {} ({})", test_broker, inst.name_en, inst.id);
    }

    let watchlist_symbols: Vec<Symbol> = WATCHLIST.iter().map(|s| Symbol(s.to_string())).collect();

    // ── Subscribe to ALL real-time push types ──
    println!("\nSubscribing to WebSocket (DEPTH + BROKER + QUOTE + TRADE)...");
    ctx.subscribe(
        WATCHLIST,
        SubFlags::DEPTH | SubFlags::BROKER | SubFlags::QUOTE | SubFlags::TRADE,
    )
    .await
    .expect("failed to subscribe");
    println!("Subscribed to {} symbols × 4 channels.", WATCHLIST.len());

    // ── Seed with initial REST snapshot ──
    println!("Fetching initial snapshot...");
    let initial_raw = snapshot::fetch(&ctx, &watchlist_symbols).await;

    let mut live = LiveState::new();
    live.depths = initial_raw.depths;
    live.brokers = initial_raw.brokers;
    live.quotes = initial_raw.quotes;

    let mut rest = RestSnapshot {
        capital_flows: initial_raw.capital_flows,
        capital_distributions: initial_raw.capital_distributions,
    };

    let mut tracker = PositionTracker::new();
    let mut history = TickHistory::new(300); // ~10 min at 2s debounce
    let pct = Decimal::new(100, 0);

    println!(
        "\nReal-time event-driven monitoring active (debounce: {}ms)\n",
        DEBOUNCE_MS,
    );

    // ── Spawn push event forwarder ──
    let (push_tx, mut push_rx) = mpsc::unbounded_channel::<PushEvent>();
    tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            if push_tx.send(event).is_err() {
                break;
            }
        }
    });

    let mut tick: u64 = 0;
    let debounce = Duration::from_millis(DEBOUNCE_MS);

    // Refresh capital data every 60s (REST-only data)
    let capital_refresh_interval = Duration::from_secs(60);
    let mut last_capital_refresh = Instant::now();

    loop {
        // Wait for at least one push event
        match push_rx.recv().await {
            Some(event) => live.apply(event),
            None => {
                eprintln!("Push channel closed. Exiting.");
                break;
            }
        }

        // Debounce: drain all events that arrive within the window
        let deadline = Instant::now() + debounce;
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                break;
            }
            match tokio::time::timeout(remaining, push_rx.recv()).await {
                Ok(Some(event)) => live.apply(event),
                _ => break,
            }
        }

        if !live.dirty {
            continue;
        }

        tick += 1;
        let now = time::OffsetDateTime::now_utc();

        // Refresh capital data periodically via REST
        if last_capital_refresh.elapsed() >= capital_refresh_interval {
            rest = fetch_capital_data(&ctx, &watchlist_symbols).await;
            last_capital_refresh = Instant::now();
        }

        println!("══════════════════════════════════════════════════════════");
        println!(
            "  #{:<4}  {}  │  {} total pushes",
            tick,
            now.format(&time::format_description::well_known::Rfc3339)
                .unwrap_or_else(|_| now.to_string()),
            live.push_count,
        );
        println!("══════════════════════════════════════════════════════════");

        // ── Build snapshot and run full pipeline ──
        let raw = live.to_raw_snapshot(&rest);

        // Show trade activity if any
        let trade_symbols: Vec<_> = raw
            .trades
            .iter()
            .filter(|(_, t)| !t.is_empty())
            .map(|(s, t)| (s.clone(), t.len(), t.iter().map(|t| t.volume).sum::<i64>()))
            .collect();

        let links = LinkSnapshot::compute(&raw, &store);
        let dim_snapshot = DimensionSnapshot::compute(&links);
        let tension_snapshot = TensionSnapshot::compute(&dim_snapshot);
        let narrative_snapshot = NarrativeSnapshot::compute(&tension_snapshot, &dim_snapshot);
        let brain = BrainGraph::compute(&narrative_snapshot, &dim_snapshot, &links, &store);

        let active_fps = tracker.active_fingerprints();
        let decision = DecisionSnapshot::compute(&brain, &links, &active_fps, &store);

        let newly_entered = tracker.auto_enter(&decision.convergence_scores, &brain);
        let new_set: HashSet<&Symbol> = newly_entered.iter().collect();

        // ── Capture tick record into history ──
        let tick_record = TickRecord::capture(
            tick,
            now,
            &decision.convergence_scores,
            &dim_snapshot.dimensions,
            &links.order_books,
            &links.trade_activities,
            &decision.degradations,
        );
        history.push(tick_record);

        // ── Compute temporal dynamics ──
        let dynamics = compute_dynamics(&history);

        // ── Display: Convergence Scores ──
        println!("\n── Convergence Scores ──");
        let mut conv_syms: Vec<_> = decision.convergence_scores.iter().collect();
        conv_syms.sort_by(|a, b| b.1.composite.abs().cmp(&a.1.composite.abs()));
        for (sym, c) in &conv_syms {
            let dir = if c.composite > Decimal::ZERO {
                "▲"
            } else if c.composite < Decimal::ZERO {
                "▼"
            } else {
                "—"
            };
            println!(
                "  {:>8}  composite={}{:>+7}%  inst={:>+7}%  sector={:>+7}%  corr={:>+7}%",
                sym,
                dir,
                (c.composite * pct).round_dp(1),
                (c.institutional_alignment * pct).round_dp(1),
                c.sector_coherence
                    .map(|s| format!("{:>+7}", (s * pct).round_dp(1)))
                    .unwrap_or_else(|| "    n/a".into()),
                (c.cross_stock_correlation * pct).round_dp(1),
            );
        }

        // ── Display: Temporal Dynamics ──
        if history.len() >= 2 {
            let mut dyn_syms: Vec<_> = dynamics.iter().collect();
            dyn_syms.sort_by(|a, b| b.1.composite_delta.abs().cmp(&a.1.composite_delta.abs()));
            println!("\n── Signal Dynamics (biggest movers) ──");
            for (sym, d) in dyn_syms.iter().take(10) {
                let accel = if d.composite_acceleration > Decimal::ZERO { "accelerating" }
                    else if d.composite_acceleration < Decimal::ZERO { "decelerating" }
                    else { "steady" };
                println!(
                    "  {:>8}  delta={:>+7}%  {}  duration={} ticks  inst_delta={:>+7}%  bid_wall={:>+6}%  ask_wall={:>+6}%  buy_ratio={:>5}%",
                    sym,
                    (d.composite_delta * pct).round_dp(1),
                    accel,
                    d.composite_duration,
                    (d.inst_alignment_delta * pct).round_dp(1),
                    (d.bid_wall_delta * pct).round_dp(1),
                    (d.ask_wall_delta * pct).round_dp(1),
                    (d.buy_ratio_trend * pct).round_dp(0),
                );
            }
        }

        // ── Display: Order Suggestions ──
        if !decision.order_suggestions.is_empty() {
            println!("\n── Order Suggestions ──");
            for s in &decision.order_suggestions {
                let dir = match s.direction {
                    OrderDirection::Buy => "BUY ",
                    OrderDirection::Sell => "SELL",
                };
                let tag = if new_set.contains(&s.symbol) {
                    " [NEW]"
                } else {
                    ""
                };
                println!(
                    "  {:>8}  {}  qty={}  price=[{} - {}]  composite={:>+7}%{}",
                    s.symbol,
                    dir,
                    s.suggested_quantity,
                    s.price_low
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "?".into()),
                    s.price_high
                        .map(|p| p.to_string())
                        .unwrap_or_else(|| "?".into()),
                    (s.convergence.composite * pct).round_dp(1),
                    tag,
                );
            }
        }

        // ── Display: Structural Degradation ──
        if !decision.degradations.is_empty() {
            println!("\n── Structural Degradation (active positions) ──");
            let mut deg_syms: Vec<_> = decision.degradations.iter().collect();
            deg_syms.sort_by(|a, b| b.1.composite_degradation.cmp(&a.1.composite_degradation));
            for (sym, d) in &deg_syms {
                println!(
                    "  {:>8}  degradation={:>+7}%  inst_retain={:>+7}%  sector_chg={:>+7}%  corr_retain={:>+7}%  dim_drift={:>+7}%",
                    sym,
                    (d.composite_degradation * pct).round_dp(1),
                    (d.institution_retention * pct).round_dp(1),
                    (d.sector_coherence_change * pct).round_dp(1),
                    (d.correlation_retention * pct).round_dp(1),
                    (d.dimension_drift * pct).round_dp(1),
                );
            }
        }

        // ── Display: Trade Activity ──
        if !trade_symbols.is_empty() {
            println!("\n── Trade Ticks ──");
            let mut sorted = trade_symbols;
            sorted.sort_by(|a, b| b.2.cmp(&a.2));
            for (sym, count, vol) in sorted.iter().take(10) {
                // Find buy/sell breakdown from links
                if let Some(ta) = links.trade_activities.iter().find(|t| &t.symbol == sym) {
                    let buy_pct = if ta.total_volume > 0 {
                        ta.buy_volume as f64 / ta.total_volume as f64 * 100.0
                    } else {
                        0.0
                    };
                    println!(
                        "  {:>8}  {} ticks  vol={}  buy={:.0}%  vwap={}",
                        sym, count, vol, buy_pct, ta.vwap.round_dp(3),
                    );
                }
            }
        }

        // ── Display: Depth Profile ──
        let mut profiles: Vec<_> = links
            .order_books
            .iter()
            .filter(|ob| ob.bid_profile.active_levels > 0 || ob.ask_profile.active_levels > 0)
            .collect();
        profiles.sort_by(|a, b| {
            let a_imbal = (a.bid_profile.top3_volume_ratio - a.ask_profile.top3_volume_ratio).abs();
            let b_imbal = (b.bid_profile.top3_volume_ratio - b.ask_profile.top3_volume_ratio).abs();
            b_imbal.cmp(&a_imbal)
        });
        if !profiles.is_empty() {
            println!("\n── Depth Profile (top asymmetry) ──");
            for ob in profiles.iter().take(10) {
                println!(
                    "  {:>8}  bid[top3={:>5}% best={:>5}% lvls={}]  ask[top3={:>5}% best={:>5}% lvls={}]  spread={:?}",
                    ob.symbol,
                    (ob.bid_profile.top3_volume_ratio * pct).round_dp(1),
                    (ob.bid_profile.best_level_ratio * pct).round_dp(1),
                    ob.bid_profile.active_levels,
                    (ob.ask_profile.top3_volume_ratio * pct).round_dp(1),
                    (ob.ask_profile.best_level_ratio * pct).round_dp(1),
                    ob.ask_profile.active_levels,
                    ob.spread,
                );
            }
        }

        // ── Summary ──
        println!(
            "\n  Tracked: {} | New: {} | History: {}/{} ticks | Data: {} depths, {} brokers, {} quotes",
            tracker.active_count(),
            newly_entered.len(),
            history.len(),
            300,
            live.depths.len(),
            live.brokers.len(),
            live.quotes.len(),
        );
        println!();
    }
}
