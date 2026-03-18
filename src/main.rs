use longport::quote::QuoteContext;
use longport::Config;

use eden::ontology::links::LinkSnapshot;
use eden::ontology::objects::{BrokerId, Symbol};
use eden::ontology::snapshot;
use eden::ontology::store;
use eden::action::narrative::NarrativeSnapshot;
use eden::logic::tension::TensionSnapshot;
use eden::pipeline::dimensions::DimensionSnapshot;

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

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    let config = Config::from_env().expect("failed to load Longport config from env");
    let (ctx, _) = QuoteContext::try_new(config.into())
        .await
        .expect("failed to connect to Longport");

    println!("Connected to Longport. Initializing ObjectStore...");

    let store = store::initialize(&ctx, WATCHLIST).await;

    // Stats
    println!("\n=== ObjectStore Stats ===");
    println!("Institutions: {}", store.institutions.len());
    println!("Brokers:      {}", store.brokers.len());
    println!("Stocks:       {}", store.stocks.len());
    println!("Sectors:      {}", store.sectors.len());

    // Verify: look up broker 4497 → should be Barclays Asia
    let test_broker = BrokerId(4497);
    match store.institution_for_broker(&test_broker) {
        Some(inst) => {
            println!("\n=== Broker {} Lookup ===", test_broker);
            println!("Institution: {} ({})", inst.name_en, inst.id);
            println!("Class:       {:?}", inst.class);
            println!("All broker seats: {:?}", inst.broker_ids);
        }
        None => {
            println!("\nBroker {} not found in any institution", test_broker);
        }
    }

    // ── Layer 2: Links ──
    println!("\n=== Fetching Links snapshot... ===");
    let watchlist_symbols: Vec<Symbol> = WATCHLIST.iter().map(|s| Symbol(s.to_string())).collect();
    let raw = snapshot::fetch(&ctx, &watchlist_symbols).await;
    let links = LinkSnapshot::compute(&raw, &store);

    println!("\n=== LinkSnapshot Stats ===");
    println!("Broker queue entries:    {}", links.broker_queues.len());
    println!("Institution activities:  {}", links.institution_activities.len());
    println!("Cross-stock presences:   {}", links.cross_stock_presences.len());
    println!("Capital flows:           {}", links.capital_flows.len());
    println!("Capital breakdowns:      {}", links.capital_breakdowns.len());
    println!("Order books:             {}", links.order_books.len());
    println!("Quotes:                  {}", links.quotes.len());

    // ── Order book imbalance ──
    println!("\n=== Order Book Imbalance (Bid vs Ask) ===");
    let mut obs: Vec<_> = links.order_books.iter().collect();
    obs.sort_by_key(|o| std::cmp::Reverse(o.total_bid_volume.saturating_add(o.total_ask_volume)));
    for ob in &obs {
        let total = ob.total_bid_volume + ob.total_ask_volume;
        let bid_pct = if total > 0 { ob.total_bid_volume as f64 / total as f64 * 100.0 } else { 0.0 };
        println!("  {:>8}  bid {:<12} ask {:<12} bid%={:.1}%  spread={:?}",
            ob.symbol, ob.total_bid_volume, ob.total_ask_volume, bid_pct, ob.spread);
    }

    // ── Capital flow ranking ──
    println!("\n=== Capital Flow Ranking (Net Inflow) ===");
    let mut flows: Vec<_> = links.capital_flows.iter().collect();
    flows.sort_by(|a, b| b.net_inflow.cmp(&a.net_inflow));
    for f in &flows {
        println!("  {:>8}  net_inflow={}", f.symbol, f.net_inflow);
    }

    // ── Capital breakdown ──
    println!("\n=== Capital Breakdown (Large / Medium / Small net) ===");
    let mut bds: Vec<_> = links.capital_breakdowns.iter().collect();
    bds.sort_by(|a, b| b.large_net.cmp(&a.large_net));
    for b in &bds {
        println!("  {:>8}  large={:<14} medium={:<14} small={}", b.symbol, b.large_net, b.medium_net, b.small_net);
    }

    // ── Cross-stock presences (top institutions) ──
    println!("\n=== Top Cross-Stock Institutions ===");
    let mut cross: Vec<_> = links.cross_stock_presences.iter().collect();
    cross.sort_by_key(|c| std::cmp::Reverse(c.symbols.len()));
    for c in cross.iter().take(10) {
        let inst_name = store.institutions.get(&c.institution_id)
            .map(|i| i.name_en.as_str()).unwrap_or("?");
        println!("  {} ({}) — {} stocks, ask={}, bid={}",
            inst_name, c.institution_id, c.symbols.len(), c.ask_symbols.len(), c.bid_symbols.len());
    }

    // ── Quote summary ──
    println!("\n=== Quote Summary ===");
    let mut qs: Vec<_> = links.quotes.iter().collect();
    qs.sort_by(|a, b| b.volume.cmp(&a.volume));
    for q in &qs {
        let chg = q.last_done - q.prev_close;
        let chg_pct = if q.prev_close != rust_decimal::Decimal::ZERO {
            (chg / q.prev_close * rust_decimal::Decimal::new(100, 0)).round_dp(2)
        } else { rust_decimal::Decimal::ZERO };
        println!("  {:>8}  last={:<10} chg={:<+10} ({:>+6}%)  vol={:<12} status={:?}",
            q.symbol, q.last_done, chg, chg_pct, q.volume, q.market_status);
    }

    // Find Barclays cross-stock presence
    let barclays_id = store
        .institution_for_broker(&BrokerId(4497))
        .map(|inst| inst.id);
    // 700.HK order book detail
    if let Some(ob) = links.order_books.iter().find(|o| o.symbol == Symbol("700.HK".into())) {
        println!("\n=== 700.HK Order Book ===");
        println!("Ask levels: {} (total vol: {})", ob.ask_level_count, ob.total_ask_volume);
        println!("Bid levels: {} (total vol: {})", ob.bid_level_count, ob.total_bid_volume);
        println!("Spread: {:?}", ob.spread);
    }

    // 700.HK quote detail
    if let Some(q) = links.quotes.iter().find(|q| q.symbol == Symbol("700.HK".into())) {
        println!("\n=== 700.HK Quote ===");
        println!("Last: {} | Prev Close: {}", q.last_done, q.prev_close);
        println!("Open: {} | High: {} | Low: {}", q.open, q.high, q.low);
        println!("Volume: {} | Turnover: {}", q.volume, q.turnover);
        println!("Status: {:?}", q.market_status);
    }

    // ── Layer 3: Pipeline — Dimension Vectors ──
    let dim_snapshot = DimensionSnapshot::compute(&links);
    println!("\n=== Dimension Vectors ===");
    let pct = rust_decimal::Decimal::new(100, 0);
    let mut dim_syms: Vec<_> = dim_snapshot.dimensions.iter().collect();
    dim_syms.sort_by(|a, b| a.0 .0.cmp(&b.0 .0));
    for (sym, d) in &dim_syms {
        println!(
            "  {:>8}  book={:>+7}%  capital={:>+7}%  size={:>+7}%  inst={:>+7}%",
            sym,
            (d.order_book_pressure * pct).round_dp(1),
            (d.capital_flow_direction * pct).round_dp(2),
            (d.capital_size_divergence * pct).round_dp(1),
            (d.institutional_direction * pct).round_dp(1),
        );
    }

    // ── Layer 4: Logic — Tension Analysis ──
    let tension_snapshot = TensionSnapshot::compute(&dim_snapshot);
    let mut tension_syms: Vec<_> = tension_snapshot.tensions.iter().collect();
    tension_syms.sort_by(|a, b| a.1.coherence.cmp(&b.1.coherence)); // most tense first

    println!("\n=== Tension Analysis (most conflicted first) ===");
    for (sym, t) in &tension_syms {
        let tense_pairs: Vec<_> = t.pairs.iter().filter(|p| p.product < rust_decimal::Decimal::ZERO).collect();
        let dir_arrow = if t.mean_direction > rust_decimal::Decimal::ZERO { "▲" } else if t.mean_direction < rust_decimal::Decimal::ZERO { "▼" } else { "—" };
        print!(
            "  {:>8}  coherence={:>+6}  direction={} {:>+6}",
            sym,
            (t.coherence * pct).round_dp(1),
            dir_arrow,
            (t.mean_direction * pct).round_dp(1),
        );
        if tense_pairs.is_empty() {
            println!("  (aligned)");
        } else {
            let labels: Vec<String> = tense_pairs
                .iter()
                .map(|p| format!("{}↔{}", p.dim_a, p.dim_b))
                .collect();
            println!("  tensions: {}", labels.join(", "));
        }
    }

    // ── Layer 5: Action — Market Narratives ──
    let narrative_snapshot = NarrativeSnapshot::compute(&tension_snapshot, &dim_snapshot);
    let mut narr_syms: Vec<_> = narrative_snapshot.narratives.iter().collect();
    narr_syms.sort_by(|a, b| a.0 .0.cmp(&b.0 .0));

    println!("\n=== Market Narratives ===");
    for (sym, n) in &narr_syms {
        let dir_arrow = if n.mean_direction > rust_decimal::Decimal::ZERO { "▲" } else if n.mean_direction < rust_decimal::Decimal::ZERO { "▼" } else { "—" };
        let strongest = &n.readings[0];
        print!(
            "  {:>8}  {:<18} coherence={:>+6}%  dir={}{:>+6}%  strongest: {}({:>+4}%)",
            sym,
            n.regime.to_string(),
            (n.coherence * pct).round_dp(1),
            dir_arrow,
            (n.mean_direction * pct).round_dp(1),
            strongest.dimension.short_name(),
            (strongest.value * pct).round_dp(0),
        );
        if !n.contradictions.is_empty() {
            let labels: Vec<String> = n.contradictions.iter()
                .map(|p| format!("{}↔{}", p.dim_a, p.dim_b))
                .collect();
            println!("  tensions: {}", labels.join(", "));
        } else {
            println!();
        }
    }

    if let Some(bid) = barclays_id {
        if let Some(cross) = links
            .cross_stock_presences
            .iter()
            .find(|c| c.institution_id == bid)
        {
            println!("\n=== Barclays Cross-Stock Presence ===");
            println!("Stocks: {:?}", cross.symbols);
            println!("Ask:    {:?}", cross.ask_symbols);
            println!("Bid:    {:?}", cross.bid_symbols);
        } else {
            println!("\nBarclays not present in multiple stocks this snapshot");
        }
    }
}
