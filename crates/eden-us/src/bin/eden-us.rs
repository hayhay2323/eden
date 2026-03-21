use std::collections::HashMap;
use std::sync::Arc;

use eden::ontology::links::{
    CalcIndexObservation, CandlestickObservation, CapitalFlow, MarketStatus, QuoteObservation,
};
use eden::ontology::objects::{SectorId, Stock, Symbol};
use eden::ontology::reasoning::TacticalSetup;
use eden::ontology::store::ObjectStore;
use eden_us::graph::decision::{
    UsDecisionSnapshot, UsSignalRecord, UsSignalScorecard,
};
use eden_us::graph::graph::UsGraph;
use eden_us::graph::insights::UsGraphInsights;
use eden_us::graph::propagation::{
    compute_cross_market_signals, minutes_since_hk_close, read_hk_snapshot, CrossMarketSignal,
};
use eden_us::action::tracker::{UsPositionTracker, UsStructuralFingerprint};
use eden_us::action::workflow::{UsActionWorkflow, UsActionStage};
use eden_us::pipeline::world::derive_backward_snapshot;
use eden_us::temporal::causality::compute_causal_timelines;
use eden_us::pipeline::dimensions::UsDimensionSnapshot;
use eden_us::pipeline::reasoning::UsReasoningSnapshot;
use eden_us::pipeline::signals::{
    HkCounterpartMoves, PreviousFlows, UsDerivedSignalSnapshot, UsEventSnapshot,
    UsObservationSnapshot,
};
use eden_us::temporal::buffer::UsTickHistory;
use eden_us::temporal::lineage::{compute_us_lineage_stats, UsLineageStats};
use eden_us::temporal::record::{UsSymbolSignals, UsTickRecord};
use eden_us::watchlist::{us_symbol_sector, US_SECTOR_NAMES, US_WATCHLIST};
use futures::stream::{self, StreamExt};
use longport::quote::{
    CalcIndex, Period, PushEvent, PushEventDetail, QuoteContext, SecurityCalcIndex, SecurityQuote,
    SubFlags, Trade, TradeSessions, TradeStatus,
};
use longport::Config;
use rust_decimal::Decimal;
use tokio::sync::mpsc;
use tokio::time::{Duration, Instant};

const DEBOUNCE_MS: u64 = 2000;

// ── US LiveState ──

struct UsLiveState {
    quotes: HashMap<Symbol, SecurityQuote>,
    trades: HashMap<Symbol, Vec<Trade>>,
    candlesticks: HashMap<Symbol, Vec<longport::quote::Candlestick>>,
    push_count: u64,
    dirty: bool,
}

impl UsLiveState {
    fn new() -> Self {
        Self {
            quotes: HashMap::new(),
            trades: HashMap::new(),
            candlesticks: HashMap::new(),
            push_count: 0,
            dirty: false,
        }
    }

    fn apply(&mut self, event: PushEvent) {
        let symbol = Symbol(event.symbol);
        self.push_count += 1;
        self.dirty = true;
        match event.detail {
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
                self.trades
                    .entry(symbol)
                    .or_default()
                    .extend(push_trades.trades);
            }
            PushEventDetail::Candlestick(candle) => {
                let entry = self.candlesticks.entry(symbol).or_default();
                entry.push(candle.candlestick);
                if entry.len() > 60 {
                    entry.drain(..entry.len() - 60);
                }
            }
            // US has no depth or broker push -- ignore
            _ => {}
        }
    }
}

// ── US RestSnapshot ──

struct UsRestSnapshot {
    calc_indexes: HashMap<Symbol, SecurityCalcIndex>,
    capital_flows: HashMap<Symbol, Vec<longport::quote::CapitalFlowLine>>,
}

impl UsRestSnapshot {
    fn empty() -> Self {
        Self {
            calc_indexes: HashMap::new(),
            capital_flows: HashMap::new(),
        }
    }
}

// ── Conversion helpers ──

fn market_status_from_trade_status(status: TradeStatus) -> MarketStatus {
    #[allow(unreachable_patterns)]
    match status {
        TradeStatus::Normal => MarketStatus::Normal,
        TradeStatus::Halted => MarketStatus::Halted,
        TradeStatus::SuspendTrade => MarketStatus::SuspendTrade,
        TradeStatus::ToBeOpened => MarketStatus::ToBeOpened,
        _ => MarketStatus::Other,
    }
}

fn build_quotes(raw: &HashMap<Symbol, SecurityQuote>) -> Vec<QuoteObservation> {
    raw.iter()
        .map(|(symbol, q)| QuoteObservation {
            symbol: symbol.clone(),
            last_done: q.last_done,
            prev_close: q.prev_close,
            open: q.open,
            high: q.high,
            low: q.low,
            volume: q.volume,
            turnover: q.turnover,
            timestamp: q.timestamp,
            market_status: market_status_from_trade_status(q.trade_status),
        })
        .collect()
}

fn build_capital_flows(
    raw: &HashMap<Symbol, Vec<longport::quote::CapitalFlowLine>>,
) -> Vec<CapitalFlow> {
    // Longport capital_flow inflow is in 萬元 (10k units),
    // but quote turnover is in 元. Multiply by 10000 to align units.
    let scale = Decimal::from(10000);
    raw.iter()
        .filter_map(|(symbol, lines)| {
            lines.last().map(|line| CapitalFlow {
                symbol: symbol.clone(),
                net_inflow: line.inflow * scale,
            })
        })
        .collect()
}

fn build_calc_indexes(raw: &HashMap<Symbol, SecurityCalcIndex>) -> Vec<CalcIndexObservation> {
    raw.iter()
        .map(|(symbol, idx)| CalcIndexObservation {
            symbol: symbol.clone(),
            turnover_rate: idx.turnover_rate,
            volume_ratio: idx.volume_ratio,
            pe_ttm_ratio: idx.pe_ttm_ratio,
            pb_ratio: idx.pb_ratio,
            dividend_ratio_ttm: idx.dividend_ratio_ttm,
            amplitude: idx.amplitude,
            five_minutes_change_rate: idx.five_minutes_change_rate,
        })
        .collect()
}

fn clamp_unit(value: Decimal) -> Decimal {
    value.clamp(-Decimal::ONE, Decimal::ONE)
}

fn build_candlesticks(
    raw: &HashMap<Symbol, Vec<longport::quote::Candlestick>>,
) -> Vec<CandlestickObservation> {
    raw.iter()
        .filter_map(|(symbol, candles)| {
            let latest = candles.last()?;
            let first = candles
                .iter()
                .rev()
                .take(5)
                .last()
                .copied()
                .unwrap_or(*latest);

            let window_return = if first.open > Decimal::ZERO {
                clamp_unit((latest.close - first.open) / first.open / Decimal::new(2, 2))
            } else {
                Decimal::ZERO
            };

            let latest_range = latest.high - latest.low;
            let body_bias = if latest_range > Decimal::ZERO {
                clamp_unit((latest.close - latest.open) / latest_range)
            } else {
                Decimal::ZERO
            };

            let recent = candles.iter().rev().take(5).collect::<Vec<_>>();
            let average_volume = if recent.is_empty() {
                Decimal::ZERO
            } else {
                Decimal::from(recent.iter().map(|c| c.volume).sum::<i64>())
                    / Decimal::from(recent.len() as i64)
            };
            let volume_ratio = if average_volume > Decimal::ZERO {
                Decimal::from(latest.volume) / average_volume
            } else {
                Decimal::ZERO
            };

            let window_high = candles
                .iter()
                .rev()
                .take(5)
                .map(|c| c.high)
                .max()
                .unwrap_or(latest.high);
            let window_low = candles
                .iter()
                .rev()
                .take(5)
                .map(|c| c.low)
                .min()
                .unwrap_or(latest.low);
            let range_ratio = if first.open > Decimal::ZERO {
                clamp_unit((window_high - window_low) / first.open / Decimal::new(8, 2))
            } else {
                Decimal::ZERO
            };

            Some(CandlestickObservation {
                symbol: symbol.clone(),
                candle_count: candles.len(),
                window_return,
                body_bias,
                volume_ratio,
                range_ratio,
            })
        })
        .collect()
}

// ── Main ──

#[tokio::main]
async fn main() {
    println!("=== Eden US — Real-time US Market Monitor ===\n");

    let config = Config::from_env().expect("failed to load Longport config from env");
    let (ctx, mut receiver) = QuoteContext::try_new(Arc::new(config))
        .await
        .expect("failed to connect to Longport");

    println!("Connected to Longport. Initializing US stocks...");

    // Fetch static_info for US watchlist stocks
    let watchlist_symbols: Vec<Symbol> =
        US_WATCHLIST.iter().map(|s| Symbol(s.to_string())).collect();
    let store = initialize_us_store(&ctx, &watchlist_symbols).await;
    println!("US Stocks: {}", store.stocks.len());

    // Subscribe to QUOTE + TRADE (no DEPTH, no BROKER for US)
    println!("\nSubscribing to WebSocket (QUOTE + TRADE)...");
    ctx.subscribe(US_WATCHLIST, SubFlags::QUOTE | SubFlags::TRADE)
        .await
        .expect("failed to subscribe");
    println!(
        "Subscribed to {} US symbols x 2 channels.",
        US_WATCHLIST.len()
    );

    // Subscribe to 1-minute candlesticks
    for symbol in US_WATCHLIST {
        if let Err(e) = ctx
            .subscribe_candlesticks(*symbol, Period::OneMinute, TradeSessions::Intraday)
            .await
        {
            eprintln!(
                "Warning: failed to subscribe candlestick for {}: {}",
                symbol, e
            );
        }
    }
    println!("Subscribed to 1-min candlesticks.");

    // Bootstrap with initial quotes
    println!("Fetching bootstrap quotes...");
    let initial_quotes =
        eden::ontology::snapshot::fetch_quotes_only(&ctx, &watchlist_symbols).await;

    let mut live = UsLiveState::new();
    live.quotes = initial_quotes;
    live.dirty = !live.quotes.is_empty();
    let mut rest = UsRestSnapshot::empty();

    // Reasoning state
    let mut tick_history = UsTickHistory::new(120);
    let mut signal_records: Vec<UsSignalRecord> = Vec::new();
    let mut previous_setups: Vec<TacticalSetup> = Vec::new();
    let mut previous_flows: PreviousFlows = HashMap::new();
    let hk_counterpart_moves: HkCounterpartMoves = HashMap::new();
    let mut lineage_stats = UsLineageStats::default();
    let mut prev_insights: Option<UsGraphInsights> = None;
    let mut position_tracker = UsPositionTracker::new();
    let mut workflows: Vec<UsActionWorkflow> = Vec::new();

    // Ensure data/ directory exists
    let _ = tokio::fs::create_dir_all("data").await;

    println!(
        "\nReal-time US monitoring active (debounce: {}ms)\n",
        DEBOUNCE_MS,
    );

    // Spawn push event forwarder
    let (push_tx, mut push_rx) = mpsc::channel::<PushEvent>(10000);
    tokio::spawn(async move {
        while let Some(event) = receiver.recv().await {
            if push_tx.try_send(event).is_err() {
                continue;
            }
        }
    });

    // Spawn REST data fetcher (capital flow + calc_indexes, every 60s)
    let (rest_tx, mut rest_rx) = mpsc::channel::<UsRestSnapshot>(1);
    let rest_ctx = ctx.clone();
    let rest_watchlist = watchlist_symbols.clone();
    tokio::spawn(async move {
        loop {
            let snapshot = fetch_us_rest_data(&rest_ctx, &rest_watchlist).await;
            if rest_tx.send(snapshot).await.is_err() {
                break;
            }
            tokio::time::sleep(Duration::from_secs(60)).await;
        }
    });

    let mut tick: u64 = 0;
    let debounce = Duration::from_millis(DEBOUNCE_MS);
    let mut bootstrap_pending = live.dirty;

    loop {
        let mut received_push = false;

        if bootstrap_pending {
            bootstrap_pending = false;
        } else {
            tokio::select! {
                maybe_event = push_rx.recv() => {
                    match maybe_event {
                        Some(event) => {
                            live.apply(event);
                            received_push = true;
                        }
                        None => {
                            eprintln!("Push channel closed. Exiting.");
                            break;
                        }
                    }
                }
                maybe_rest = rest_rx.recv() => {
                    if let Some(new_rest) = maybe_rest {
                        rest = new_rest;
                        live.dirty = true;
                    }
                }
            }
        }

        if received_push {
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
        }

        while let Ok(new_rest) = rest_rx.try_recv() {
            rest = new_rest;
            live.dirty = true;
        }

        if !live.dirty {
            continue;
        }

        tick += 1;
        let now = time::OffsetDateTime::now_utc();
        live.dirty = false;
        let trades_this_tick = std::mem::take(&mut live.trades);
        let _ = trades_this_tick; // trades available for future use

        // US market hours: 9:30-16:00 ET = 13:30-20:00 UTC (no DST adjustment needed — close enough)
        let utc_hour = now.hour();
        let utc_min = now.minute();
        let utc_total_min = utc_hour as u32 * 60 + utc_min as u32;
        let market_open = utc_total_min >= 13 * 60 + 30 && utc_total_min < 20 * 60;
        if !market_open {
            // Still write snapshot but mark as after-hours, skip reasoning
            if tick % 100 == 0 {
                println!("[US tick {}] after-hours (UTC {:02}:{:02}), skipping reasoning", tick, utc_hour, utc_min);
            }
            continue;
        }

        // Build link-level observations
        let quotes = build_quotes(&live.quotes);
        let capital_flows = build_capital_flows(&rest.capital_flows);
        let calc_indexes = build_calc_indexes(&rest.calc_indexes);
        let candlesticks = build_candlesticks(&live.candlesticks);

        // Build US dimensions
        let dim_snapshot = UsDimensionSnapshot::compute(
            &quotes,
            &capital_flows,
            &calc_indexes,
            &candlesticks,
            &store,
            now,
        );

        // Build US graph
        let sector_map: HashMap<Symbol, SectorId> = store
            .stocks
            .iter()
            .filter_map(|(sym, s)| s.sector_id.clone().map(|sid| (sym.clone(), sid)))
            .collect();
        let sector_names: HashMap<SectorId, String> = store
            .sectors
            .iter()
            .map(|(id, s)| (id.clone(), s.name.clone()))
            .collect();
        let graph = UsGraph::compute(&dim_snapshot, &sector_map, &sector_names);

        // Cross-market propagation: read HK snapshot if available
        let cross_market_signals = read_cross_market_signals(now);

        // ── Reasoning stack ──

        // 1. Observation snapshot
        let obs_snapshot = UsObservationSnapshot::from_raw(
            &quotes,
            &capital_flows,
            &calc_indexes,
            &candlesticks,
            now,
        );

        // 2. Event detection
        let event_snapshot = UsEventSnapshot::detect(
            &quotes,
            &calc_indexes,
            &capital_flows,
            &previous_flows,
            &hk_counterpart_moves,
            now,
        );

        // 3. Derived signals
        let hk_signal_map: HashMap<Symbol, Decimal> = cross_market_signals
            .iter()
            .map(|s| (s.us_symbol.clone(), s.propagation_confidence))
            .collect();
        let derived_snapshot =
            UsDerivedSignalSnapshot::compute(&dim_snapshot, &hk_signal_map, now);

        // 4. Reasoning: hypotheses + tactical setups
        let reasoning =
            UsReasoningSnapshot::derive(&event_snapshot, &derived_snapshot, &previous_setups);

        // 5. Decision: convergence + regime + suggestions
        let decision = UsDecisionSnapshot::compute(&graph, &cross_market_signals, tick);

        // 6. Build UsTickRecord
        let prev_record = tick_history.latest();
        let mut per_symbol_signals: HashMap<Symbol, UsSymbolSignals> = HashMap::new();
        for (sym, dims) in &dim_snapshot.dimensions {
            let composite = (dims.capital_flow_direction
                + dims.price_momentum
                + dims.volume_profile
                + dims.pre_post_market_anomaly
                + dims.valuation)
                / Decimal::from(5);
            let prev_pre_post = prev_record
                .and_then(|r| r.signals.get(sym))
                .map(|s| s.pre_post_market_anomaly)
                .unwrap_or(Decimal::ZERO);
            let mark_price = quotes.iter().find(|q| &q.symbol == sym).map(|q| q.last_done);
            per_symbol_signals.insert(
                sym.clone(),
                UsSymbolSignals {
                    mark_price,
                    composite,
                    capital_flow_direction: dims.capital_flow_direction,
                    price_momentum: dims.price_momentum,
                    volume_profile: dims.volume_profile,
                    pre_post_market_anomaly: dims.pre_post_market_anomaly,
                    valuation: dims.valuation,
                    pre_market_delta: dims.pre_post_market_anomaly - prev_pre_post,
                },
            );
        }

        let tick_record = UsTickRecord {
            tick_number: tick,
            timestamp: now,
            signals: per_symbol_signals,
            cross_market_signals: cross_market_signals.clone(),
            events: event_snapshot.events.clone(),
            derived_signals: derived_snapshot.signals.clone(),
            hypotheses: reasoning.hypotheses.clone(),
            tactical_setups: reasoning.tactical_setups.clone(),
            market_regime: decision.market_regime.bias,
        };
        tick_history.push(tick_record);

        // 7. Signal scorecard: record new suggestions, resolve old ones
        for suggestion in &decision.order_suggestions {
            signal_records.push(UsSignalRecord {
                symbol: suggestion.symbol.clone(),
                tick_emitted: tick,
                direction: suggestion.direction,
                composite_at_emission: suggestion.convergence.composite,
                price_at_emission: quotes
                    .iter()
                    .find(|q| q.symbol == suggestion.symbol)
                    .map(|q| q.last_done),
                resolved: false,
                price_at_resolution: None,
                hit: None,
                realized_return: None,
            });
        }
        for record in &mut signal_records {
            let current_price = quotes
                .iter()
                .find(|q| q.symbol == record.symbol)
                .map(|q| q.last_done);
            UsSignalScorecard::try_resolve(record, tick, current_price);
        }
        let scorecard = UsSignalScorecard::compute(&signal_records);

        // Update state for next tick
        previous_setups = reasoning.tactical_setups.clone();
        previous_flows = capital_flows
            .iter()
            .map(|cf| (cf.symbol.clone(), cf.net_inflow))
            .collect();

        // 8. Lineage stats every 30 ticks
        if tick % 30 == 0 && tick_history.len() > 1 {
            lineage_stats = compute_us_lineage_stats(&tick_history, 15);
        }

        // 9. Graph insights (pressure, rotation, clusters, stress, cross-market anomalies)
        let insights = UsGraphInsights::compute(
            &graph,
            &dim_snapshot,
            &cross_market_signals,
            prev_insights.as_ref(),
            tick,
        );

        // 10. Backward reasoning chains
        let sector_name_strings: HashMap<String, String> = sector_names
            .iter()
            .map(|(id, name)| (id.0.clone(), name.clone()))
            .collect();
        let backward = derive_backward_snapshot(
            &decision,
            &graph,
            &cross_market_signals,
            &sector_name_strings,
        );

        // 11. Causal timelines (every 10 ticks to avoid overhead)
        let causal_timelines = if tick % 10 == 0 && tick_history.len() > 2 {
            compute_causal_timelines(&tick_history)
        } else {
            HashMap::new()
        };

        // 12. Position tracker: auto-enter high-confidence setups, monitor exits
        for setup in &reasoning.tactical_setups {
            if setup.action == "enter" && setup.confidence >= Decimal::new(7, 1) {
                if let eden::ontology::reasoning::ReasoningScope::Symbol(sym) = &setup.scope {
                    if !position_tracker.is_active(sym) {
                        let price = quotes.iter().find(|q| &q.symbol == sym).map(|q| q.last_done);
                        if let Some(dims) = dim_snapshot.dimensions.get(sym) {
                            let fp = UsStructuralFingerprint::capture(sym.clone(), tick, price, dims);
                            position_tracker.enter(fp);
                            let mut wf = UsActionWorkflow::from_setup(setup, tick, price);
                            // Auto-system: immediately confirm + execute → Monitoring
                            wf.confirm();
                            if let Some(p) = price {
                                wf.execute(p);
                            }
                            workflows.push(wf);
                        }
                    }
                }
            }
        }
        let exit_candidates = position_tracker.auto_exit_candidates(&dim_snapshot);
        for deg in &exit_candidates {
            if deg.should_exit {
                position_tracker.exit(&deg.symbol);
                if let Some(wf) = workflows.iter_mut().find(|w| w.symbol == deg.symbol && matches!(w.stage, UsActionStage::Monitoring)) {
                    wf.review("auto-exit: structural degradation");
                }
            }
        }
        // Update monitoring for active workflows
        for wf in &mut workflows {
            if matches!(wf.stage, UsActionStage::Monitoring) {
                let price = quotes.iter().find(|q| q.symbol == wf.symbol).map(|q| q.last_done);
                if let Some(deg) = exit_candidates.iter().find(|d| d.symbol == wf.symbol) {
                    wf.update_monitoring(price, deg.clone());
                }
            }
        }
        // Prune stale workflows
        workflows.retain(|w| !w.is_stale(tick));

        prev_insights = Some(insights.clone());

        // ── Build live snapshot JSON ──
        let timestamp_str = now
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();

        let top_signals: Vec<serde_json::Value> = dim_snapshot
            .dimensions
            .iter()
            .map(|(sym, dims)| {
                let composite = (dims.capital_flow_direction
                    + dims.price_momentum
                    + dims.volume_profile
                    + dims.pre_post_market_anomaly
                    + dims.valuation)
                    / Decimal::from(5);
                serde_json::json!({
                    "symbol": sym.0,
                    "composite": composite,
                    "capital_flow_direction": dims.capital_flow_direction,
                    "price_momentum": dims.price_momentum,
                    "volume_profile": dims.volume_profile,
                    "pre_post_market_anomaly": dims.pre_post_market_anomaly,
                    "valuation": dims.valuation,
                })
            })
            .collect();

        let cross_market_json: Vec<serde_json::Value> = cross_market_signals
            .iter()
            .map(|sig| {
                serde_json::json!({
                    "us_symbol": sig.us_symbol.0,
                    "hk_symbol": sig.hk_symbol.0,
                    "hk_composite": sig.hk_composite,
                    "hk_inst_alignment": sig.hk_inst_alignment,
                    "propagation_confidence": sig.propagation_confidence,
                    "hk_timestamp": sig.hk_timestamp,
                    "time_since_hk_close_minutes": sig.time_since_hk_close_minutes,
                })
            })
            .collect();

        // Events: top 5 by magnitude
        let mut sorted_events = event_snapshot.events.clone();
        sorted_events.sort_by(|a, b| b.value.magnitude.cmp(&a.value.magnitude));
        let events_json: Vec<serde_json::Value> = sorted_events
            .iter()
            .take(5)
            .map(|e| {
                serde_json::json!({
                    "kind": format!("{:?}", e.value.kind),
                    "magnitude": e.value.magnitude,
                    "summary": e.value.summary,
                })
            })
            .collect();

        // Tactical cases: top 10 by confidence
        let tactical_json: Vec<serde_json::Value> = reasoning
            .tactical_setups
            .iter()
            .take(10)
            .map(|s| {
                serde_json::json!({
                    "setup_id": s.setup_id,
                    "title": s.title,
                    "action": s.action,
                    "confidence": s.confidence,
                    "confidence_gap": s.confidence_gap,
                    "heuristic_edge": s.heuristic_edge,
                    "entry_rationale": s.entry_rationale,
                })
            })
            .collect();

        // Convergence scores: sorted by abs(composite)
        let mut sorted_convergence: Vec<_> = decision.convergence_scores.iter().collect();
        sorted_convergence.sort_by(|a, b| b.1.composite.abs().cmp(&a.1.composite.abs()));
        let convergence_json: Vec<serde_json::Value> = sorted_convergence
            .iter()
            .take(10)
            .map(|(sym, score)| {
                serde_json::json!({
                    "symbol": sym.0,
                    "composite": score.composite,
                    "dimension_composite": score.dimension_composite,
                    "cross_stock_correlation": score.cross_stock_correlation,
                    "sector_coherence": score.sector_coherence,
                    "cross_market_propagation": score.cross_market_propagation,
                })
            })
            .collect();

        // Lineage JSON
        let lineage_json = if !lineage_stats.is_empty() {
            serde_json::json!({
                "by_template": lineage_stats.by_template.iter().map(|s| {
                    serde_json::json!({
                        "template": s.template,
                        "total": s.total,
                        "resolved": s.resolved,
                        "hits": s.hits,
                        "hit_rate": s.hit_rate,
                        "mean_return": s.mean_return,
                    })
                }).collect::<Vec<_>>(),
            })
        } else {
            serde_json::json!(null)
        };

        let live_snapshot = serde_json::json!({
            "tick": tick,
            "timestamp": timestamp_str,
            "market": "US",
            "stock_count": graph.stock_nodes.len(),
            "cross_market_pair_count": graph.cross_market_nodes.len(),
            "edge_count": graph.graph.edge_count(),
            "top_signals": top_signals,
            "convergence_scores": convergence_json,
            "cross_market_signals": cross_market_json,
            "events": events_json,
            "tactical_cases": tactical_json,
            "market_regime": decision.market_regime.bias.as_str(),
            "scorecard": {
                "total_signals": scorecard.total_signals,
                "resolved_signals": scorecard.resolved_signals,
                "hits": scorecard.hits,
                "misses": scorecard.misses,
                "hit_rate": scorecard.hit_rate,
                "mean_return": scorecard.mean_return,
            },
            "lineage": lineage_json,
            "observation_count": obs_snapshot.observations.len(),
            "hypothesis_count": reasoning.hypotheses.len(),
            "tick_history_len": tick_history.len(),
            // New modules
            "pressures": insights.pressures.iter().take(10).map(|p| serde_json::json!({
                "symbol": p.symbol.0,
                "capital_flow_pressure": p.capital_flow_pressure,
                "volume_intensity": p.volume_intensity,
                "momentum": p.momentum,
                "pressure_delta": p.pressure_delta,
                "pressure_duration": p.pressure_duration,
                "accelerating": p.accelerating,
            })).collect::<Vec<_>>(),
            "rotations": insights.rotations.iter().take(5).map(|r| serde_json::json!({
                "sector_a": r.sector_a.0,
                "sector_b": r.sector_b.0,
                "spread": r.spread,
                "spread_delta": r.spread_delta,
                "widening": r.widening,
            })).collect::<Vec<_>>(),
            "clusters": insights.clusters.iter().take(5).map(|c| serde_json::json!({
                "members": c.members.iter().map(|s| &s.0).collect::<Vec<_>>(),
                "directional_alignment": c.directional_alignment,
                "stability": c.stability,
                "age": c.age,
            })).collect::<Vec<_>>(),
            "stress": {
                "pressure_dispersion": insights.stress.pressure_dispersion,
                "momentum_consensus": insights.stress.momentum_consensus,
                "volume_anomaly": insights.stress.volume_anomaly,
                "composite_stress": insights.stress.composite_stress,
            },
            "cross_market_anomalies": insights.cross_market_anomalies.iter().map(|a| serde_json::json!({
                "us_symbol": a.us_symbol.0,
                "hk_symbol": a.hk_symbol.0,
                "expected_direction": a.expected_direction,
                "actual_direction": a.actual_direction,
                "divergence": a.divergence,
            })).collect::<Vec<_>>(),
            "backward_chains": backward.chains.iter().take(10).map(|c| serde_json::json!({
                "symbol": c.symbol.0,
                "conclusion": c.conclusion,
                "primary_driver": c.primary_driver,
                "confidence": c.confidence,
                "evidence": c.evidence.iter().take(5).map(|e| serde_json::json!({
                    "source": e.source,
                    "description": e.description,
                    "weight": e.weight,
                    "direction": e.direction,
                })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "active_positions": position_tracker.active_fingerprints().len(),
            "workflows": workflows.iter().take(10).map(|w| w.snapshot()).collect::<Vec<_>>(),
            "causal_leaders": causal_timelines.iter().take(10).map(|(sym, tl)| serde_json::json!({
                "symbol": sym.0,
                "current_leader": tl.current_leader,
                "leader_streak": tl.leader_streak,
                "flips": tl.flips.len(),
            })).collect::<Vec<_>>(),
        });

        // Print tick summary
        println!(
            "\n[US tick {}] {} | {} stocks | {} edges | regime={} | {} events | {} hyps | {} setups | scorecard {}/{} ({:.0}%) | {} push",
            tick,
            timestamp_str,
            graph.stock_nodes.len(),
            graph.graph.edge_count(),
            decision.market_regime.bias,
            event_snapshot.events.len(),
            reasoning.hypotheses.len(),
            reasoning.tactical_setups.len(),
            scorecard.hits,
            scorecard.resolved_signals,
            scorecard.hit_rate * Decimal::from(100),
            live.push_count,
        );

        // Top convergence scores
        if !sorted_convergence.is_empty() {
            println!("  Convergence:");
            for (sym, score) in sorted_convergence.iter().take(5) {
                let cm_tag = score
                    .cross_market_propagation
                    .map(|v| format!(" hk={}", v.round_dp(3)))
                    .unwrap_or_default();
                println!(
                    "    {} composite={} (dim={} corr={} sec={}){}",
                    sym,
                    score.composite.round_dp(4),
                    score.dimension_composite.round_dp(3),
                    score.cross_stock_correlation.round_dp(3),
                    score
                        .sector_coherence
                        .map(|v| format!("{}", v.round_dp(3)))
                        .unwrap_or_else(|| "-".into()),
                    cm_tag,
                );
            }
        }

        // Cross-market signals
        if !cross_market_signals.is_empty() {
            println!("  Cross-market:");
            for sig in &cross_market_signals {
                println!(
                    "    {} <- {} conf={} (hk_comp={} inst={} {}min ago)",
                    sig.us_symbol,
                    sig.hk_symbol,
                    sig.propagation_confidence,
                    sig.hk_composite,
                    sig.hk_inst_alignment,
                    sig.time_since_hk_close_minutes,
                );
            }
        }

        // Events
        if !sorted_events.is_empty() {
            println!("  Events:");
            for e in sorted_events.iter().take(5) {
                println!("    [{:?}] mag={} {}", e.value.kind, e.value.magnitude, e.value.summary);
            }
        }

        // Top tactical setups
        if !reasoning.tactical_setups.is_empty() {
            println!("  Tactical setups:");
            for setup in reasoning.tactical_setups.iter().take(5) {
                println!(
                    "    {} [{}] conf={} gap={} edge={}",
                    setup.title, setup.action, setup.confidence, setup.confidence_gap, setup.heuristic_edge,
                );
            }
        }

        // Lineage summary (when computed)
        if !lineage_stats.is_empty() {
            println!("  Lineage:");
            for ls in &lineage_stats.by_template {
                println!(
                    "    {} {}/{} resolved, hit_rate={} mean_ret={}",
                    ls.template, ls.resolved, ls.total, ls.hit_rate, ls.mean_return,
                );
            }
        }

        // Insights summary
        if !insights.pressures.is_empty() {
            println!("  Pressures:");
            for p in insights.pressures.iter().take(3) {
                println!(
                    "    {} flow={} vol={} mom={} {}{}",
                    p.symbol, p.capital_flow_pressure.round_dp(3), p.volume_intensity.round_dp(3),
                    p.momentum.round_dp(3),
                    if p.accelerating { "↑" } else { "" },
                    if p.pressure_duration > 1 { format!(" {}t", p.pressure_duration) } else { String::new() },
                );
            }
        }
        if !backward.chains.is_empty() {
            println!("  Backward:");
            for c in backward.chains.iter().take(3) {
                println!("    {} [{}]", c.conclusion, c.primary_driver);
            }
        }
        if position_tracker.active_fingerprints().len() > 0 {
            println!("  Positions: {} active, {} workflows", position_tracker.active_fingerprints().len(), workflows.len());
        }

        // Write snapshot to file (non-blocking)
        let snapshot_json = serde_json::to_string(&live_snapshot).unwrap_or_default();
        tokio::spawn(async move {
            let _ = tokio::fs::write("data/us_live_snapshot.json", snapshot_json).await;
        });
    }
}

// ── Initialization ──

async fn initialize_us_store(ctx: &QuoteContext, watchlist: &[Symbol]) -> Arc<ObjectStore> {
    let symbols_vec: Vec<String> = watchlist.iter().map(|s| s.0.clone()).collect();
    let static_infos = match ctx.static_info(symbols_vec).await {
        Ok(infos) => infos,
        Err(e) => {
            eprintln!("Warning: static_info failed: {}", e);
            vec![]
        }
    };

    let stocks: Vec<Stock> = static_infos
        .into_iter()
        .map(|info| Stock {
            symbol: Symbol(info.symbol.clone()),
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
        })
        .collect();

    let stock_map: HashMap<Symbol, Stock> =
        stocks.into_iter().map(|s| (s.symbol.clone(), s)).collect();

    // Build sector store from our static mapping
    let sectors: HashMap<SectorId, eden::ontology::objects::Sector> = US_SECTOR_NAMES
        .iter()
        .map(|(id, name)| {
            (
                SectorId(id.to_string()),
                eden::ontology::objects::Sector {
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
    })
}

// ── REST data fetcher ──

async fn fetch_us_rest_data(ctx: &QuoteContext, watchlist: &[Symbol]) -> UsRestSnapshot {
    const BATCH_CONCURRENCY: usize = 8;

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

    let calc_future = async {
        match ctx
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
        }
    };

    let (flow_results, calc_indexes) = tokio::join!(flow_future, calc_future);

    UsRestSnapshot {
        calc_indexes,
        capital_flows: flow_results.into_iter().flatten().collect(),
    }
}

// ── Cross-market signal reader ──

fn read_cross_market_signals(now: time::OffsetDateTime) -> Vec<CrossMarketSignal> {
    let hk_path = std::env::var("EDEN_LIVE_SNAPSHOT_PATH")
        .unwrap_or_else(|_| "data/live_snapshot.json".into());

    match read_hk_snapshot(&hk_path) {
        Ok(hk_snapshot) => {
            let minutes = minutes_since_hk_close(now);
            compute_cross_market_signals(&hk_snapshot, minutes)
        }
        Err(_) => Vec::new(), // HK not running — no cross-market signals
    }
}
