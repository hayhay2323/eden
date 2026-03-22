use std::collections::HashMap;
use std::sync::Arc;

use crate::live_snapshot::{
    ensure_snapshot_parent, snapshot_path, spawn_write_snapshot, LiveBackwardChain,
    LiveCausalLeader, LiveCrossMarketAnomaly, LiveCrossMarketSignal, LiveEvent,
    LiveHypothesisTrack, LiveLineageMetric, LiveMarket, LiveMarketRegime, LivePressure,
    LiveScorecard, LiveSignal, LiveSnapshot, LiveStressSnapshot, LiveTacticalCase,
};
use crate::ontology::links::{
    CalcIndexObservation, CandlestickObservation, CapitalFlow, MarketStatus, QuoteObservation,
};
use crate::ontology::objects::{SectorId, Stock, Symbol};
use crate::ontology::reasoning::TacticalSetup;
use crate::ontology::store::ObjectStore;
#[cfg(feature = "persistence")]
use crate::persistence::store::EdenStore;
#[cfg(feature = "persistence")]
use crate::persistence::us_lineage_metric_row::rows_from_us_lineage_stats;
#[cfg(feature = "persistence")]
use crate::persistence::us_lineage_snapshot::UsLineageSnapshotRecord;
use crate::runtime_loop::{next_tick, spawn_periodic_fetch, TickState};
use crate::us::action::tracker::{UsPositionTracker, UsStructuralFingerprint};
use crate::us::action::workflow::{UsActionStage, UsActionWorkflow};
use crate::us::graph::decision::{UsDecisionSnapshot, UsSignalRecord, UsSignalScorecard};
use crate::us::graph::graph::UsGraph;
use crate::us::graph::insights::UsGraphInsights;
use crate::us::graph::propagation::{
    compute_cross_market_signals, minutes_since_hk_close, read_hk_snapshot, CrossMarketSignal,
};
use crate::us::pipeline::dimensions::UsDimensionSnapshot;
use crate::us::pipeline::reasoning::UsReasoningSnapshot;
use crate::us::pipeline::signals::{
    HkCounterpartMoves, PreviousFlows, UsDerivedSignalSnapshot, UsEventSnapshot,
    UsObservationSnapshot,
};
use crate::us::pipeline::world::derive_backward_snapshot;
use crate::us::temporal::buffer::UsTickHistory;
use crate::us::temporal::causality::compute_causal_timelines;
use crate::us::temporal::lineage::{compute_us_lineage_stats, UsLineageStats};
use crate::us::temporal::record::{UsSymbolSignals, UsTickRecord};
use crate::us::watchlist::{us_symbol_sector, US_SECTOR_NAMES, US_WATCHLIST};
use futures::stream::{self, StreamExt};
use longport::quote::{
    CalcIndex, Period, PushEvent, PushEventDetail, QuoteContext, SecurityCalcIndex, SecurityQuote,
    SubFlags, Trade, TradeSessions, TradeStatus,
};
use longport::Config;
use rust_decimal::Decimal;
use tokio::sync::mpsc;
use tokio::time::Duration;

const DEBOUNCE_MS: u64 = 2000;
const US_LINEAGE_RESOLUTION_LAG: u64 = 15;

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

struct UsTickState<'a> {
    live: &'a mut UsLiveState,
    rest: &'a mut UsRestSnapshot,
}

impl TickState<PushEvent, UsRestSnapshot> for UsTickState<'_> {
    fn apply_push(&mut self, event: PushEvent) {
        self.live.apply(event);
    }

    fn apply_update(&mut self, update: UsRestSnapshot) {
        *self.rest = update;
        self.live.dirty = true;
    }

    fn is_dirty(&self) -> bool {
        self.live.dirty
    }

    fn clear_dirty(&mut self) {
        self.live.dirty = false;
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

fn us_sector_name(store: &Arc<ObjectStore>, symbol: &Symbol) -> Option<String> {
    let sector_id = store.stocks.get(symbol)?.sector_id.as_ref()?;
    store
        .sectors
        .get(sector_id)
        .map(|sector| sector.name.clone())
}

// ── Runtime entry ──

pub async fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Eden US — Real-time US Market Monitor ===\n");

    let config = Config::from_env()?;
    let (ctx, mut receiver) = QuoteContext::try_new(Arc::new(config)).await?;

    println!("Connected to Longport. Initializing US stocks...");

    // Fetch static_info for US watchlist stocks
    let watchlist_symbols: Vec<Symbol> =
        US_WATCHLIST.iter().map(|s| Symbol(s.to_string())).collect();
    let store = initialize_us_store(&ctx, &watchlist_symbols).await;
    println!("US Stocks: {}", store.stocks.len());

    // Subscribe to QUOTE + TRADE (no DEPTH, no BROKER for US)
    println!("\nSubscribing to WebSocket (QUOTE + TRADE)...");
    ctx.subscribe(US_WATCHLIST, SubFlags::QUOTE | SubFlags::TRADE)
        .await?;
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
        crate::ontology::snapshot::fetch_quotes_only(&ctx, &watchlist_symbols).await;

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

    let snapshot_path = snapshot_path("EDEN_US_LIVE_SNAPSHOT_PATH", "data/us_live_snapshot.json");
    ensure_snapshot_parent(&snapshot_path).await;

    #[cfg(feature = "persistence")]
    let eden_store = {
        let eden_db_path = std::env::var("EDEN_DB_PATH").unwrap_or_else(|_| "data/eden.db".into());
        match EdenStore::open(&eden_db_path).await {
            Ok(store) => {
                println!("SurrealDB opened at {}", eden_db_path);
                Some(store)
            }
            Err(error) => {
                eprintln!(
                    "Warning: SurrealDB failed to open for US runtime: {}. Running without persistence.",
                    error
                );
                None
            }
        }
    };

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

    let rest_ctx = ctx.clone();
    let rest_watchlist = watchlist_symbols.clone();
    let mut rest_rx = spawn_periodic_fetch(1, Duration::from_secs(60), move || {
        let rest_ctx = rest_ctx.clone();
        let rest_watchlist = rest_watchlist.clone();
        async move { fetch_us_rest_data(&rest_ctx, &rest_watchlist).await }
    });

    let mut tick: u64 = 0;
    let debounce = Duration::from_millis(DEBOUNCE_MS);
    let mut bootstrap_pending = live.dirty;

    loop {
        let Some(tick_advance) = ({
            let mut tick_state = UsTickState {
                live: &mut live,
                rest: &mut rest,
            };
            match next_tick(
                &mut bootstrap_pending,
                &mut push_rx,
                &mut rest_rx,
                debounce,
                &mut tick_state,
                &mut tick,
            )
            .await
            {
                Ok(result) => result,
                Err(()) => {
                    eprintln!("Push channel closed. Exiting.");
                    break;
                }
            }
        }) else {
            continue;
        };

        let now = tick_advance.now;
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
                println!(
                    "[US tick {}] after-hours (UTC {:02}:{:02}), skipping reasoning",
                    tick, utc_hour, utc_min
                );
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
        let derived_snapshot = UsDerivedSignalSnapshot::compute(&dim_snapshot, &hk_signal_map, now);

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
            let mark_price = quotes
                .iter()
                .find(|q| &q.symbol == sym)
                .map(|q| q.last_done);
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
        #[cfg(feature = "persistence")]
        if let Some(ref store) = eden_store {
            let store_ref = store.clone();
            let record = tick_record.clone();
            tokio::spawn(async move {
                if let Err(error) = store_ref.write_us_tick(&record).await {
                    eprintln!("Warning: failed to write US tick: {}", error);
                }
            });
        }
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
            lineage_stats = compute_us_lineage_stats(&tick_history, US_LINEAGE_RESOLUTION_LAG);
            #[cfg(feature = "persistence")]
            if let Some(ref store) = eden_store {
                let snapshot = UsLineageSnapshotRecord::new(
                    tick,
                    now,
                    tick_history.len(),
                    US_LINEAGE_RESOLUTION_LAG,
                    &lineage_stats,
                );
                let rows = rows_from_us_lineage_stats(
                    snapshot.record_id(),
                    tick,
                    now,
                    tick_history.len(),
                    US_LINEAGE_RESOLUTION_LAG,
                    &lineage_stats,
                );
                let store_ref = store.clone();
                tokio::spawn(async move {
                    if let Err(error) = store_ref.write_us_lineage_snapshot(&snapshot).await {
                        eprintln!("Warning: failed to write US lineage snapshot: {}", error);
                    }
                    if let Err(error) = store_ref.write_us_lineage_metric_rows(&rows).await {
                        eprintln!("Warning: failed to write US lineage metric rows: {}", error);
                    }
                });
            }
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
                if let crate::ontology::reasoning::ReasoningScope::Symbol(sym) = &setup.scope {
                    if !position_tracker.is_active(sym) {
                        let price = quotes
                            .iter()
                            .find(|q| &q.symbol == sym)
                            .map(|q| q.last_done);
                        if let Some(dims) = dim_snapshot.dimensions.get(sym) {
                            let fp =
                                UsStructuralFingerprint::capture(sym.clone(), tick, price, dims);
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
                if let Some(wf) = workflows.iter_mut().find(|w| {
                    w.symbol == deg.symbol && matches!(w.stage, UsActionStage::Monitoring)
                }) {
                    wf.review("auto-exit: structural degradation");
                }
            }
        }
        // Update monitoring for active workflows
        for wf in &mut workflows {
            if matches!(wf.stage, UsActionStage::Monitoring) {
                let price = quotes
                    .iter()
                    .find(|q| q.symbol == wf.symbol)
                    .map(|q| q.last_done);
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

        let mut top_signals = dim_snapshot
            .dimensions
            .iter()
            .map(|(symbol, dims)| LiveSignal {
                symbol: symbol.0.clone(),
                sector: us_sector_name(&store, symbol),
                composite: (dims.capital_flow_direction
                    + dims.price_momentum
                    + dims.volume_profile
                    + dims.pre_post_market_anomaly
                    + dims.valuation)
                    / Decimal::from(5),
                mark_price: None,
                dimension_composite: None,
                capital_flow_direction: dims.capital_flow_direction,
                price_momentum: dims.price_momentum,
                volume_profile: dims.volume_profile,
                pre_post_market_anomaly: dims.pre_post_market_anomaly,
                valuation: dims.valuation,
                cross_stock_correlation: None,
                sector_coherence: None,
                cross_market_propagation: None,
            })
            .collect::<Vec<_>>();
        top_signals.sort_by(|a, b| b.composite.abs().cmp(&a.composite.abs()));
        top_signals.truncate(20);

        // Events: top 5 by magnitude
        let mut sorted_events = event_snapshot.events.clone();
        sorted_events.sort_by(|a, b| b.value.magnitude.cmp(&a.value.magnitude));
        let mut sorted_convergence: Vec<_> = decision.convergence_scores.iter().collect();
        sorted_convergence.sort_by(|a, b| b.1.composite.abs().cmp(&a.1.composite.abs()));

        let live_snapshot = LiveSnapshot {
            tick,
            timestamp: timestamp_str.clone(),
            market: LiveMarket::Us,
            stock_count: graph.stock_nodes.len(),
            edge_count: graph.graph.edge_count(),
            hypothesis_count: reasoning.hypotheses.len(),
            observation_count: obs_snapshot.observations.len(),
            active_positions: position_tracker.active_fingerprints().len(),
            market_regime: LiveMarketRegime {
                bias: decision.market_regime.bias.as_str().to_string(),
                confidence: decision.market_regime.confidence,
                breadth_up: decision.market_regime.breadth_up,
                breadth_down: decision.market_regime.breadth_down,
                average_return: decision.market_regime.macro_return,
                directional_consensus: None,
                pre_market_sentiment: Some(decision.market_regime.pre_market_sentiment),
            },
            stress: LiveStressSnapshot {
                composite_stress: insights.stress.composite_stress,
                sector_synchrony: None,
                pressure_consensus: None,
                momentum_consensus: Some(insights.stress.momentum_consensus),
                pressure_dispersion: Some(insights.stress.pressure_dispersion),
                volume_anomaly: Some(insights.stress.volume_anomaly),
            },
            scorecard: LiveScorecard {
                total_signals: scorecard.total_signals,
                resolved_signals: scorecard.resolved_signals,
                hits: scorecard.hits,
                misses: scorecard.misses,
                hit_rate: scorecard.hit_rate,
                mean_return: scorecard.mean_return,
            },
            tactical_cases: reasoning
                .tactical_setups
                .iter()
                .take(10)
                .map(|item| {
                    let family_label = reasoning
                        .hypotheses
                        .iter()
                        .find(|hypothesis| hypothesis.hypothesis_id == item.hypothesis_id)
                        .map(|hypothesis| hypothesis.family_label.clone());
                    let counter_label = item
                        .runner_up_hypothesis_id
                        .as_ref()
                        .and_then(|id| {
                            reasoning
                                .hypotheses
                                .iter()
                                .find(|hypothesis| hypothesis.hypothesis_id == *id)
                        })
                        .map(|hypothesis| hypothesis.family_label.clone());

                    LiveTacticalCase {
                        setup_id: item.setup_id.clone(),
                        symbol: match &item.scope {
                            crate::ontology::reasoning::ReasoningScope::Symbol(symbol) => {
                                symbol.0.clone()
                            }
                            _ => String::new(),
                        },
                        title: item.title.clone(),
                        action: item.action.clone(),
                        confidence: item.confidence,
                        confidence_gap: item.confidence_gap,
                        heuristic_edge: item.heuristic_edge,
                        entry_rationale: item.entry_rationale.clone(),
                        family_label,
                        counter_label,
                    }
                })
                .collect(),
            hypothesis_tracks: Vec::<LiveHypothesisTrack>::new(),
            top_signals,
            convergence_scores: decision
                .convergence_scores
                .iter()
                .map(|(symbol, score)| LiveSignal {
                    symbol: symbol.0.clone(),
                    sector: us_sector_name(&store, symbol),
                    composite: score.composite,
                    mark_price: None,
                    dimension_composite: Some(score.dimension_composite),
                    capital_flow_direction: score.capital_flow_direction,
                    price_momentum: score.price_momentum,
                    volume_profile: score.volume_profile,
                    pre_post_market_anomaly: score.pre_post_market_anomaly,
                    valuation: score.valuation,
                    cross_stock_correlation: Some(score.cross_stock_correlation),
                    sector_coherence: score.sector_coherence,
                    cross_market_propagation: score.cross_market_propagation,
                })
                .collect(),
            pressures: insights
                .pressures
                .iter()
                .take(10)
                .map(|item| LivePressure {
                    symbol: item.symbol.0.clone(),
                    sector: us_sector_name(&store, &item.symbol),
                    capital_flow_pressure: item.capital_flow_pressure,
                    momentum: item.momentum,
                    pressure_delta: item.pressure_delta,
                    pressure_duration: item.pressure_duration,
                    accelerating: item.accelerating,
                })
                .collect(),
            backward_chains: backward
                .chains
                .iter()
                .take(10)
                .map(|item| LiveBackwardChain {
                    symbol: item.symbol.0.clone(),
                    conclusion: item.conclusion.clone(),
                    primary_driver: item.primary_driver.clone(),
                    confidence: item.confidence,
                    evidence: item
                        .evidence
                        .iter()
                        .take(5)
                        .map(|e| crate::live_snapshot::LiveEvidence {
                            source: e.source.clone(),
                            description: e.description.clone(),
                            weight: e.weight,
                            direction: e.direction,
                        })
                        .collect(),
                })
                .collect(),
            causal_leaders: causal_timelines
                .iter()
                .take(10)
                .map(|(symbol, item)| LiveCausalLeader {
                    symbol: symbol.0.clone(),
                    current_leader: item.current_leader.clone(),
                    leader_streak: item.leader_streak,
                    flips: item.flips.len(),
                })
                .collect(),
            events: sorted_events
                .iter()
                .take(8)
                .map(|item| LiveEvent {
                    kind: format!("{:?}", item.value.kind),
                    magnitude: item.value.magnitude,
                    summary: item.value.summary.clone(),
                })
                .collect(),
            cross_market_signals: cross_market_signals
                .iter()
                .map(|item| LiveCrossMarketSignal {
                    us_symbol: item.us_symbol.0.clone(),
                    hk_symbol: item.hk_symbol.0.clone(),
                    propagation_confidence: item.propagation_confidence,
                    time_since_hk_close_minutes: Some(item.time_since_hk_close_minutes),
                })
                .collect(),
            cross_market_anomalies: insights
                .cross_market_anomalies
                .iter()
                .map(|item| LiveCrossMarketAnomaly {
                    us_symbol: item.us_symbol.0.clone(),
                    hk_symbol: item.hk_symbol.0.clone(),
                    expected_direction: item.expected_direction,
                    actual_direction: item.actual_direction,
                    divergence: item.divergence,
                })
                .collect(),
            lineage: if !lineage_stats.is_empty() {
                lineage_stats
                    .by_template
                    .iter()
                    .map(|item| LiveLineageMetric {
                        template: item.template.clone(),
                        total: item.total,
                        resolved: item.resolved,
                        hits: item.hits,
                        hit_rate: item.hit_rate,
                        mean_return: item.mean_return,
                    })
                    .collect()
            } else {
                Vec::new()
            },
        };

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
                println!(
                    "    [{:?}] mag={} {}",
                    e.value.kind, e.value.magnitude, e.value.summary
                );
            }
        }

        // Top tactical setups
        if !reasoning.tactical_setups.is_empty() {
            println!("  Tactical setups:");
            for setup in reasoning.tactical_setups.iter().take(5) {
                println!(
                    "    {} [{}] conf={} gap={} edge={}",
                    setup.title,
                    setup.action,
                    setup.confidence,
                    setup.confidence_gap,
                    setup.heuristic_edge,
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
                    p.symbol,
                    p.capital_flow_pressure.round_dp(3),
                    p.volume_intensity.round_dp(3),
                    p.momentum.round_dp(3),
                    if p.accelerating { "↑" } else { "" },
                    if p.pressure_duration > 1 {
                        format!(" {}t", p.pressure_duration)
                    } else {
                        String::new()
                    },
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
            println!(
                "  Positions: {} active, {} workflows",
                position_tracker.active_fingerprints().len(),
                workflows.len()
            );
        }

        // Write snapshot to file (non-blocking)
        spawn_write_snapshot(snapshot_path.clone(), live_snapshot);
    }

    Ok(())
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
    let sectors: HashMap<SectorId, crate::ontology::objects::Sector> = US_SECTOR_NAMES
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
