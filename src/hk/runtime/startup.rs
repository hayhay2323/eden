use super::*;
use crate::core::runtime::PreparedRuntimeContext;

pub(super) struct HkRuntimeBootstrap {
    pub(super) store: std::sync::Arc<eden::ontology::store::ObjectStore>,
    pub(super) live: LiveState,
    pub(super) rest: RestSnapshot,
    pub(super) tracker: PositionTracker,
    pub(super) history: TickHistory,
    pub(super) prev_insights: Option<GraphInsights>,
    pub(super) conflict_history: ConflictHistory,
    pub(super) edge_registry: TemporalEdgeRegistry,
    pub(super) node_registry: TemporalNodeRegistry,
    pub(super) broker_registry: TemporalBrokerRegistry,
    pub(super) scorecard: SignalScorecard,
    pub(super) bridge_service: FileSystemBridgeService,
    pub(super) analyst_service: DefaultAnalystService,
    pub(super) bridge_snapshot_path: String,
    pub(super) runtime: PreparedRuntimeContext,
    pub(super) push_rx: tokio::sync::mpsc::Receiver<Vec<PushEvent>>,
    pub(super) rest_rx: tokio::sync::mpsc::Receiver<RestSnapshot>,
    /// Pressure-event bus instantiated before the push forwarder so the
    /// upstream tap can publish PressureEvents directly from the
    /// longport receiver — bypassing the bounded batch channel that
    /// drops events when the tick loop falls behind (C4 fix).
    pub(super) pressure_event_bus:
        std::sync::Arc<eden::pipeline::pressure_events::EventBusHandle>,
    pub(super) tick: u64,
    pub(super) debounce: std::time::Duration,
    pub(super) bootstrap_pending: bool,
    pub(super) previous_symbol_states: Vec<crate::pipeline::state_engine::PersistentSymbolState>,
    pub(super) lineage_accumulator: crate::temporal::lineage::LineageFamilyAccumulator,
    pub(super) lineage_prev_resolved: std::collections::HashMap<String, usize>,
    pub(super) eden_ledger: crate::persistence::case_realized_outcome::EdenLedgerAccumulator,
}

// Bumped 8_192 → 32_768 (4x), symmetric with US side. See us/runtime.rs for rationale.
const HK_PUSH_BATCH_CHANNEL_CAP: usize = 32_768;
const HK_PUSH_BATCH_SIZE: usize = 2_048;

pub(super) async fn initialize_hk_runtime() -> HkRuntimeBootstrap {
    let config = match Config::from_env() {
        Ok(config) => Arc::new(config),
        Err(error) => {
            eprintln!(
                "Live runtime failed to load Longport config from env: {}",
                error
            );
            std::process::exit(1);
        }
    };
    let (ctx, receiver) = match eden::core::runtime::connect_with_retry(
        || {
            let config = config.clone();
            async move { QuoteContext::try_new(config).await }
        },
        eden::core::runtime::RetryPolicy::longport_startup(),
        "hk QuoteContext::try_new",
    )
    .await
    {
        Ok(value) => value,
        Err(error) => {
            eprintln!(
                "Live runtime failed to connect to Longport after retries: {}",
                error
            );
            std::process::exit(1);
        }
    };

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

    println!("\nSubscribing to WebSocket (DEPTH + BROKER + QUOTE + TRADE)...");
    if let Err(error) = ctx
        .subscribe(
            WATCHLIST,
            SubFlags::DEPTH | SubFlags::BROKER | SubFlags::QUOTE | SubFlags::TRADE,
        )
        .await
    {
        eprintln!(
            "Live runtime failed to subscribe to Longport streams: {}",
            error
        );
        std::process::exit(1);
    }
    println!("Subscribed to {} symbols × 4 channels.", WATCHLIST.len());

    for symbol in WATCHLIST {
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

    println!("Fetching bootstrap quotes...");
    let initial_quotes = snapshot::fetch_quotes_only(&ctx, &watchlist_symbols).await;

    let mut live = LiveState::new();
    live.quotes = initial_quotes;
    live.dirty = !live.quotes.is_empty();
    let rest = RestSnapshot::empty();
    let bootstrap_pending = live.dirty;

    let tracker = PositionTracker::new();
    #[allow(unused_mut)]
    let mut history = TickHistory::new(500);
    let prev_insights = None;
    let conflict_history = ConflictHistory::new();
    let edge_registry = TemporalEdgeRegistry::new();
    let node_registry = TemporalNodeRegistry::new();
    let broker_registry = TemporalBrokerRegistry::new();
    let scorecard = SignalScorecard::new(500, 50);
    let bridge_service = FileSystemBridgeService::default();
    let analyst_service = DefaultAnalystService;
    let bridge_snapshot_path = resolve_artifact_path(MarketId::Hk, ArtifactKind::BridgeSnapshot);
    #[cfg(feature = "persistence")]
    let persistence_slots = PERSISTENCE_MAX_IN_FLIGHT;
    #[cfg(not(feature = "persistence"))]
    let persistence_slots = 1usize;
    let runtime = prepare_runtime_context_or_exit(
        MarketId::Hk,
        persistence_slots,
        "SurrealDB failed to open",
    )
    .await;
    #[cfg(feature = "persistence")]
    if let Err(message) = crate::core::runtime::ensure_persistence_store_available(
        MarketId::Hk,
        runtime.store.is_some(),
    ) {
        eprintln!("{message}");
        std::process::exit(2);
    }
    prepare_runtime_artifact_path(&bridge_snapshot_path).await;

    #[allow(unused_mut)]
    let mut restored_tick_count = 0usize;
    #[allow(unused_mut)]
    let mut restored_previous_tracks = 0usize;
    #[allow(unused_mut)]
    let mut restored_previous_symbol_states =
        Vec::<crate::pipeline::state_engine::PersistentSymbolState>::new();
    #[allow(unused_mut)]
    let mut lineage_accumulator = crate::temporal::lineage::LineageFamilyAccumulator::default();
    #[allow(unused_mut)]
    let mut lineage_prev_resolved: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
    #[allow(unused_mut)]
    let mut eden_ledger =
        crate::persistence::case_realized_outcome::EdenLedgerAccumulator::default();
    #[cfg(feature = "persistence")]
    if let Some(ref db) = runtime.store {
        let restored = eden::ontology::store::AccumulatedKnowledge::restore_from(db, "hk").await;
        *store.knowledge_write() = restored;
        if let Ok(records) = db.recent_tick_window(500).await {
            restored_tick_count = records.len();
            for record in records {
                history.push(record);
            }
            restored_previous_tracks = history
                .latest()
                .map(|tick| tick.hypothesis_tracks.len())
                .unwrap_or(0);
            if let Some(latest) = history.latest() {
                eprintln!(
                    "[hk] restored {} persisted ticks (latest tick={}, previous_tracks={})",
                    restored_tick_count, latest.tick_number, restored_previous_tracks
                );
            }
            let seed_stats =
                crate::temporal::lineage::compute_lineage_stats(&history, super::LINEAGE_WINDOW);
            lineage_accumulator.ingest(&seed_stats, &lineage_prev_resolved);
            lineage_prev_resolved = seed_stats
                .family_contexts
                .iter()
                .map(|entry| (entry.family.clone(), entry.resolved))
                .collect();
        }
        if let Ok(records) = db
            .recent_symbol_perception_states_by_market("hk", WATCHLIST.len())
            .await
        {
            restored_previous_symbol_states = records
                .into_iter()
                .map(|record| record.to_state())
                .collect();
        }
        if let Ok(records) = db.recent_case_realized_outcomes_by_market("hk", 500).await {
            eden_ledger.record_batch(&records);
            if !records.is_empty() {
                eprintln!(
                    "[hk] hydrated eden_ledger with {} realized outcomes",
                    records.len()
                );
            }
        }
    }

    runtime.log_monitoring_active("Real-time event-driven monitoring active");
    runtime.runtime_task_heartbeat(
        "hk runtime monitoring active",
        serde_json::json!({
            "phase": "startup_complete",
            "market": "hk",
            "quotes": live.quotes.len(),
            "watchlist_symbols": WATCHLIST.len(),
            "bootstrap_pending": bootstrap_pending,
            "restored_tick_history_len": restored_tick_count,
            "restored_previous_tracks": restored_previous_tracks,
            "restored_previous_symbol_states": restored_previous_symbol_states.len(),
        }),
    );

    // C4 fix: build the pressure-event bus BEFORE the push forwarder so
    // the forwarder's tap can demux every longport event into the bus —
    // even events whose batches the bounded push channel later drops.
    let pressure_event_bus = std::sync::Arc::new(
        eden::pipeline::pressure_events::spawn_bus(),
    );
    let bus_for_tap = std::sync::Arc::clone(&pressure_event_bus);
    let raw_event_journal = eden::core::raw_event_journal::RawEventJournal::spawn("hk");
    let journal_for_tap = raw_event_journal.clone();
    let tap: crate::core::runtime::PushTap = Box::new(move |evt: &PushEvent| {
        journal_for_tap.record_push(&evt.symbol, &evt.detail);
        for pe in eden::pipeline::pressure_events::demux_push_event(evt) {
            bus_for_tap.publish(pe);
        }
    });
    let push_health = std::sync::Arc::new(
        eden::core::runtime::PushReceiverHealth::new(std::time::Duration::from_secs(60)),
    );
    let push_rx = runtime.spawn_batched_push_forwarder(
        receiver,
        HK_PUSH_BATCH_CHANNEL_CAP,
        HK_PUSH_BATCH_SIZE,
        Some(tap),
        Some(push_health.clone()),
    );
    eden::core::runtime::spawn_push_health_monitor(
        push_health,
        std::time::Duration::from_secs(5),
        runtime.config_clone(),
    );
    let tick = history
        .latest()
        .map(|record| record.tick_number)
        .unwrap_or(0);
    let debounce = runtime.debounce_duration();

    let rest_ctx = ctx.clone();
    let rest_watchlist = watchlist_symbols.clone();
    let rest_rx = runtime.spawn_rest_refresh(1, move || {
        let rest_ctx = rest_ctx.clone();
        let rest_watchlist = rest_watchlist.clone();
        async move { fetch_rest_data(&rest_ctx, &rest_watchlist).await }
    });

    HkRuntimeBootstrap {
        store,
        live,
        rest,
        tracker,
        history,
        prev_insights,
        conflict_history,
        edge_registry,
        node_registry,
        broker_registry,
        scorecard,
        bridge_service,
        analyst_service,
        bridge_snapshot_path,
        runtime,
        push_rx,
        rest_rx,
        pressure_event_bus,
        tick,
        debounce,
        bootstrap_pending,
        previous_symbol_states: restored_previous_symbol_states,
        lineage_accumulator,
        lineage_prev_resolved,
        eden_ledger,
    }
}
