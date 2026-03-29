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
    pub(super) push_rx: tokio::sync::mpsc::Receiver<PushEvent>,
    pub(super) rest_rx: tokio::sync::mpsc::Receiver<RestSnapshot>,
    pub(super) tick: u64,
    pub(super) debounce: std::time::Duration,
    pub(super) bootstrap_pending: bool,
}

pub(super) async fn initialize_hk_runtime() -> HkRuntimeBootstrap {
    let config = match Config::from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!(
                "Live runtime failed to load Longport config from env: {}",
                error
            );
            std::process::exit(1);
        }
    };
    let (ctx, receiver) = match QuoteContext::try_new(Arc::new(config)).await {
        Ok(value) => value,
        Err(error) => {
            eprintln!("Live runtime failed to connect to Longport: {}", error);
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
    let polymarket_configs = match load_polymarket_configs() {
        Ok(configs) => configs,
        Err(error) => {
            eprintln!("Warning: {}", error);
            vec![]
        }
    };
    if !polymarket_configs.is_empty() {
        println!(
            "Loaded {} Polymarket market priors from POLYMARKET_MARKETS.",
            polymarket_configs.len()
        );
    }

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
    let history = TickHistory::new(300);
    let prev_insights = None;
    let conflict_history = ConflictHistory::new();
    let edge_registry = TemporalEdgeRegistry::new();
    let node_registry = TemporalNodeRegistry::new();
    let broker_registry = TemporalBrokerRegistry::new();
    let scorecard = SignalScorecard::new(500, 15);
    let bridge_service = FileSystemBridgeService::default();
    let analyst_service = DefaultAnalystService;
    let bridge_snapshot_path =
        resolve_artifact_path(MarketId::Hk, ArtifactKind::BridgeSnapshot);
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
    prepare_runtime_artifact_path(&bridge_snapshot_path).await;

    #[cfg(feature = "persistence")]
    if let Some(ref db) = runtime.store {
        let restored =
            eden::ontology::store::AccumulatedKnowledge::restore_from(db, "hk").await;
        *store.knowledge.write().unwrap() = restored;
    }

    runtime.log_monitoring_active("Real-time event-driven monitoring active");

    let push_rx = runtime.spawn_push_forwarder(receiver, 10_000);
    let tick = 0;
    let debounce = runtime.debounce_duration();

    let rest_ctx = ctx.clone();
    let rest_watchlist = watchlist_symbols.clone();
    let rest_polymarket = polymarket_configs.clone();
    let rest_rx = runtime.spawn_rest_refresh(1, move || {
        let rest_ctx = rest_ctx.clone();
        let rest_watchlist = rest_watchlist.clone();
        let rest_polymarket = rest_polymarket.clone();
        async move { fetch_rest_data(&rest_ctx, &rest_watchlist, &rest_polymarket).await }
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
        tick,
        debounce,
        bootstrap_pending,
    }
}
