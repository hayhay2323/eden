use super::support::{
    fetch_us_bootstrap_rest_data, fetch_us_rest_data, initialize_us_store, UsLiveState,
    UsRestSnapshot,
};
use super::*;
use crate::core::runtime::PreparedRuntimeContext;
use crate::live_snapshot::spawn_write_snapshot;

pub(super) struct UsRuntimeBootstrap {
    pub(super) store: Arc<ObjectStore>,
    pub(super) live: UsLiveState,
    pub(super) rest: UsRestSnapshot,
    pub(super) tick_history: UsTickHistory,
    pub(super) signal_records: Vec<UsSignalRecord>,
    pub(super) scorecard_accumulator: UsSignalScorecardAccumulator,
    pub(super) signal_momentum: crate::us::temporal::lineage::SignalMomentumTracker,
    pub(super) previous_setups: Vec<TacticalSetup>,
    pub(super) previous_tracks: Vec<crate::ontology::reasoning::HypothesisTrack>,
    pub(super) previous_flows: PreviousFlows,
    pub(super) lineage_stats: UsLineageStats,
    pub(super) lineage_accumulator: crate::us::temporal::lineage::UsLineageFamilyAccumulator,
    pub(super) lineage_prev_resolved: std::collections::HashMap<String, usize>,
    pub(super) prev_insights: Option<UsGraphInsights>,
    pub(super) position_tracker: UsPositionTracker,
    pub(super) workflows: Vec<UsActionWorkflow>,
    pub(super) bridge_service: FileSystemBridgeService,
    pub(super) analyst_service: DefaultAnalystService,
    pub(super) runtime: PreparedRuntimeContext,
    pub(super) push_rx: tokio::sync::mpsc::Receiver<Vec<PushEvent>>,
    pub(super) rest_rx: tokio::sync::mpsc::Receiver<UsRestSnapshot>,
    /// Pressure-event bus instantiated before the push forwarder so the
    /// upstream tap can publish PressureEvents directly from the
    /// longport receiver — bypassing the bounded batch channel that
    /// drops events when the tick loop falls behind (C4 fix).
    pub(super) pressure_event_bus:
        std::sync::Arc<crate::pipeline::pressure_events::EventBusHandle>,
    pub(super) tick: u64,
    pub(super) debounce: std::time::Duration,
    pub(super) bootstrap_pending: bool,
    pub(super) energy_momentum: crate::graph::energy::EnergyMomentum,
    pub(super) previous_symbol_states: Vec<crate::pipeline::state_engine::PersistentSymbolState>,
    pub(super) eden_ledger: crate::persistence::case_realized_outcome::EdenLedgerAccumulator,
    #[cfg(feature = "persistence")]
    pub(super) cached_us_learning_feedback: Option<ReasoningLearningFeedback>,
}

pub(super) async fn initialize_us_runtime() -> Result<UsRuntimeBootstrap, Box<dyn std::error::Error>>
{
    eprintln!("[us][startup] begin initialize_us_runtime");
    println!("=== Eden US — Real-time US Market Monitor ===\n");

    eprintln!("[us][startup] loading config from env");
    let config = Config::from_env()?;
    eprintln!("[us][startup] creating Longport quote context");
    let (ctx, receiver) = QuoteContext::try_new(Arc::new(config)).await?;
    eprintln!("[us][startup] Longport quote context ready");

    println!("Connected to Longport. Initializing US stocks...");

    let watchlist_symbols: Vec<Symbol> =
        US_WATCHLIST.iter().map(|s| Symbol(s.to_string())).collect();
    eprintln!(
        "[us][startup] initializing store for {} watchlist symbols",
        watchlist_symbols.len()
    );
    let store = initialize_us_store(&ctx, &watchlist_symbols).await;
    eprintln!("[us][startup] store initialized");
    println!("US Stocks: {}", store.stocks.len());

    // Longport subscription limit: ~500 symbols. Subscribe top 500 for real-time push,
    // remaining symbols get data via REST polling (every 60s).
    const WS_SUBSCRIPTION_LIMIT: usize = 500;
    let ws_symbols: Vec<&str> = US_WATCHLIST
        .iter()
        .take(WS_SUBSCRIPTION_LIMIT)
        .copied()
        .collect();
    println!(
        "\nSubscribing to WebSocket (QUOTE + TRADE) for top {} symbols...",
        ws_symbols.len()
    );
    eprintln!(
        "[us][startup] subscribing websocket streams for {} symbols",
        ws_symbols.len()
    );
    ctx.subscribe(&ws_symbols, SubFlags::QUOTE | SubFlags::TRADE)
        .await?;
    eprintln!("[us][startup] websocket subscription complete");
    println!(
        "Subscribed to {} US symbols x 2 channels. ({} symbols via REST only.)",
        ws_symbols.len(),
        US_WATCHLIST.len().saturating_sub(WS_SUBSCRIPTION_LIMIT),
    );

    let candlestick_ctx = ctx.clone();
    let candlestick_symbols = ws_symbols
        .iter()
        .map(|symbol| (*symbol).to_string())
        .collect::<Vec<_>>();
    tokio::spawn(async move {
        eprintln!(
            "[us] subscribing to 1-min candlesticks in background for {} symbols...",
            candlestick_symbols.len()
        );
        let mut subscribed = 0usize;
        for symbol in candlestick_symbols {
            match candlestick_ctx
                .subscribe_candlesticks(symbol.clone(), Period::OneMinute, TradeSessions::Intraday)
                .await
            {
                Ok(_) => subscribed += 1,
                Err(e) => {
                    eprintln!(
                        "Warning: failed to subscribe candlestick for {}: {}",
                        symbol, e
                    );
                }
            }
        }
        eprintln!(
            "[us] subscribed to 1-min candlesticks for {} symbols.",
            subscribed
        );
    });

    println!("Fetching bootstrap quotes...");
    eprintln!("[us][startup] fetching bootstrap quotes");
    let initial_quotes =
        crate::ontology::snapshot::fetch_quotes_only(&ctx, &watchlist_symbols).await;
    eprintln!(
        "[us][startup] bootstrap quotes fetched ({})",
        initial_quotes.len()
    );

    let mut live = UsLiveState::new();
    live.quotes = initial_quotes;
    eprintln!("[us][startup] fetching REST bootstrap snapshot");
    let mut rest = fetch_us_bootstrap_rest_data(&ctx, &watchlist_symbols).await;
    eprintln!(
        "[us][startup] REST bootstrap snapshot fetched (quotes={}, calc_indexes={}, capital_flows={}, intraday={}, option_surfaces={})",
        rest.quotes.len(),
        rest.calc_indexes.len(),
        rest.capital_flows.len(),
        rest.intraday_lines.len(),
        rest.option_surfaces.len()
    );
    let ingested_at = time::OffsetDateTime::now_utc();
    live.record_rest_snapshot(&rest, ingested_at);
    for (symbol, quote) in std::mem::take(&mut rest.quotes) {
        let merged = merge_rest_quote(live.quotes.get(&symbol), quote);
        live.quotes.insert(symbol, merged);
    }
    live.dirty = !live.quotes.is_empty()
        || !rest.calc_indexes.is_empty()
        || !rest.capital_flows.is_empty()
        || !rest.intraday_lines.is_empty()
        || !rest.option_surfaces.is_empty();
    let bootstrap_pending = live.dirty;

    // 500 ticks gives stable medium-term lineage stats.
    // Previously 120 caused wild hit_rate swings (90% → 25% in one hour).
    #[allow(unused_mut)]
    let mut tick_history = UsTickHistory::new(500);
    let signal_records: Vec<UsSignalRecord> = Vec::new();
    let scorecard_accumulator = UsSignalScorecardAccumulator::default();
    #[allow(unused_mut)]
    let mut signal_momentum = crate::us::temporal::lineage::SignalMomentumTracker::default();
    #[allow(unused_mut)]
    let mut previous_setups: Vec<TacticalSetup> = Vec::new();
    #[allow(unused_mut)]
    let mut previous_tracks: Vec<crate::ontology::reasoning::HypothesisTrack> = Vec::new();
    let previous_flows: PreviousFlows = HashMap::new();
    #[allow(unused_mut)]
    let mut lineage_stats = UsLineageStats::default();
    #[allow(unused_mut)]
    let mut lineage_accumulator =
        crate::us::temporal::lineage::UsLineageFamilyAccumulator::default();
    #[allow(unused_mut)]
    let mut lineage_prev_resolved: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    let prev_insights: Option<UsGraphInsights> = None;
    #[allow(unused_mut)]
    let mut position_tracker = UsPositionTracker::new();
    #[allow(unused_mut)]
    let mut workflows: Vec<UsActionWorkflow> = Vec::new();
    let bridge_service = FileSystemBridgeService::default();
    let analyst_service = DefaultAnalystService;

    #[cfg(feature = "persistence")]
    let persistence_slots = US_PERSISTENCE_MAX_IN_FLIGHT;
    #[cfg(not(feature = "persistence"))]
    let persistence_slots = 1usize;
    eprintln!(
        "[us][startup] preparing runtime context (persistence_slots={})",
        persistence_slots
    );
    let runtime = prepare_runtime_context_or_exit(
        MarketId::Us,
        persistence_slots,
        "SurrealDB failed to open for US runtime",
    )
    .await;
    #[cfg(feature = "persistence")]
    {
        eprintln!(
            "[us][startup] runtime context prepared (store_available={})",
            runtime.store.is_some()
        );
        if let Err(message) = crate::core::runtime::ensure_persistence_store_available(
            MarketId::Us,
            runtime.store.is_some(),
        ) {
            eprintln!("{message}");
            std::process::exit(2);
        }
    }
    #[cfg(not(feature = "persistence"))]
    eprintln!("[us][startup] runtime context prepared (persistence feature disabled)");
    let debounce = runtime.debounce_duration();

    #[allow(unused_mut)]
    let mut restored_tick_count = 0usize;
    #[allow(unused_mut)]
    let mut restored_previous_symbol_states =
        Vec::<crate::pipeline::state_engine::PersistentSymbolState>::new();
    #[allow(unused_mut)]
    let mut eden_ledger =
        crate::persistence::case_realized_outcome::EdenLedgerAccumulator::default();
    #[cfg(feature = "persistence")]
    if let Some(ref db) = runtime.store {
        eprintln!("[us][startup] restoring accumulated knowledge and recent ticks");
        let restored = crate::ontology::store::AccumulatedKnowledge::restore_from(db, "us").await;
        *store.knowledge_write() = restored;
        if let Ok(records) = db.recent_us_tick_window(500).await {
            restored_tick_count = records.len();
            for record in records {
                tick_history.push(record);
            }
            let restored_records = tick_history.latest_n(2);
            if let Some(latest) = restored_records.last().copied() {
                previous_setups = latest.tactical_setups.clone();
                previous_tracks = Vec::new();
                lineage_stats = compute_us_lineage_stats(&tick_history, SIGNAL_RESOLUTION_LAG);
                lineage_accumulator.ingest(&lineage_stats, &lineage_prev_resolved);
                lineage_prev_resolved = lineage_stats
                    .by_template
                    .iter()
                    .map(|entry| (entry.template.clone(), entry.resolved))
                    .collect();
                signal_momentum.restore_from_us_history(&tick_history);
                eprintln!(
                    "[us] restored {} persisted ticks (latest tick={}, momentum_symbols={})",
                    restored_tick_count,
                    latest.tick_number,
                    signal_momentum.convergence.len()
                );
            }
        }
        if let Ok(records) = db
            .recent_symbol_perception_states_by_market("us", US_WATCHLIST.len())
            .await
        {
            restored_previous_symbol_states = records
                .into_iter()
                .map(|record| record.to_state())
                .collect();
        }
        if let Ok(records) = db.recent_case_realized_outcomes_by_market("us", 500).await {
            eden_ledger.record_batch(&records);
            if !records.is_empty() {
                eprintln!(
                    "[us] hydrated eden_ledger with {} realized outcomes",
                    records.len()
                );
            }
        }
        // Active workflow recovery happens on the first live tick via
        // `restore_persisted_us_workflows(...)` in `runtime.rs`, where
        // current canonical dimensions are available for
        // position-tracker rehydration. `initialize_us_runtime()`
        // intentionally stops at restoring tick history + latest
        // symbol state.
        eprintln!(
            "[us][startup] restore complete (ticks={}, previous_symbol_states={})",
            restored_tick_count,
            restored_previous_symbol_states.len()
        );
    }

    runtime.log_monitoring_active("Real-time US monitoring active");
    runtime.runtime_task_heartbeat(
        "us runtime monitoring active",
        serde_json::json!({
            "phase": "startup_complete",
            "market": "us",
            "quotes": live.quotes.len(),
            "watchlist_symbols": US_WATCHLIST.len(),
            "bootstrap_pending": bootstrap_pending,
            "restored_tick_history_len": restored_tick_count,
            "restored_previous_setups": previous_setups.len(),
            "restored_previous_tracks": previous_tracks.len(),
            "restored_momentum_symbols": signal_momentum.convergence.len(),
            "restored_previous_symbol_states": restored_previous_symbol_states.len(),
        }),
    );

    // C4 fix: build the pressure-event bus BEFORE the push forwarder so
    // the forwarder's tap can demux every longport event into the bus —
    // even events whose batches the bounded push channel later drops.
    let pressure_event_bus = std::sync::Arc::new(
        crate::pipeline::pressure_events::spawn_bus(),
    );
    let bus_for_tap = std::sync::Arc::clone(&pressure_event_bus);
    let tap: crate::core::runtime::PushTap = Box::new(move |evt: &PushEvent| {
        for pe in crate::pipeline::pressure_events::demux_push_event(evt) {
            bus_for_tap.publish(pe);
        }
    });
    let push_rx = runtime.spawn_batched_push_forwarder(
        receiver,
        US_PUSH_BATCH_CHANNEL_CAP,
        US_PUSH_BATCH_SIZE,
        Some(tap),
    );
    eprintln!("[us][startup] spawned batched push forwarder");

    let rest_ctx = ctx.clone();
    let rest_watchlist = watchlist_symbols.clone();
    let rest_rx = runtime.spawn_rest_refresh(1, move || {
        let rest_ctx = rest_ctx.clone();
        let rest_watchlist = rest_watchlist.clone();
        async move { fetch_us_rest_data(&rest_ctx, &rest_watchlist).await }
    });
    eprintln!("[us][startup] spawned REST refresh loop");
    let restored_tick = tick_history
        .latest()
        .map(|record| record.tick_number)
        .unwrap_or(0);
    let now = time::OffsetDateTime::now_utc();
    let should_write_bootstrap_snapshot = bootstrap_pending
        || !live.quotes.is_empty()
        || !rest.calc_indexes.is_empty()
        || !rest.capital_flows.is_empty()
        || !rest.intraday_lines.is_empty()
        || !rest.option_surfaces.is_empty();
    if should_write_bootstrap_snapshot {
        eprintln!("[us][startup] writing bootstrap live snapshot");
        let timestamp_str = now
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();
        let bootstrap_snapshot = build_us_bootstrap_snapshot(
            restored_tick,
            timestamp_str,
            &store,
            &live,
            &rest,
            &restored_previous_symbol_states,
            &[],
            None,
        );
        spawn_write_snapshot(
            runtime.artifacts.live_snapshot_path.clone(),
            bootstrap_snapshot,
        );
    }
    eprintln!("[us][startup] initialize_us_runtime complete");

    Ok(UsRuntimeBootstrap {
        store,
        live,
        rest,
        tick_history,
        signal_records,
        scorecard_accumulator,
        signal_momentum,
        previous_setups,
        previous_tracks,
        previous_flows,
        lineage_stats,
        lineage_accumulator,
        lineage_prev_resolved,
        prev_insights,
        position_tracker,
        workflows,
        bridge_service,
        analyst_service,
        runtime,
        push_rx,
        rest_rx,
        pressure_event_bus,
        tick: restored_tick,
        debounce,
        bootstrap_pending,
        energy_momentum: crate::graph::energy::EnergyMomentum::default(),
        previous_symbol_states: restored_previous_symbol_states,
        eden_ledger,
        #[cfg(feature = "persistence")]
        cached_us_learning_feedback: None,
    })
}
