use super::*;
use crate::core::runtime::PreparedRuntimeContext;
use super::support::{fetch_us_rest_data, initialize_us_store, UsLiveState, UsRestSnapshot};

pub(super) struct UsRuntimeBootstrap {
    pub(super) store: Arc<ObjectStore>,
    pub(super) live: UsLiveState,
    pub(super) rest: UsRestSnapshot,
    pub(super) tick_history: UsTickHistory,
    pub(super) signal_records: Vec<UsSignalRecord>,
    pub(super) previous_setups: Vec<TacticalSetup>,
    pub(super) previous_tracks: Vec<crate::ontology::reasoning::HypothesisTrack>,
    pub(super) previous_flows: PreviousFlows,
    pub(super) lineage_stats: UsLineageStats,
    pub(super) prev_insights: Option<UsGraphInsights>,
    pub(super) position_tracker: UsPositionTracker,
    pub(super) workflows: Vec<UsActionWorkflow>,
    pub(super) bridge_service: FileSystemBridgeService,
    pub(super) analyst_service: DefaultAnalystService,
    pub(super) runtime: PreparedRuntimeContext,
    pub(super) push_rx: tokio::sync::mpsc::Receiver<Vec<PushEvent>>,
    pub(super) rest_rx: tokio::sync::mpsc::Receiver<UsRestSnapshot>,
    pub(super) tick: u64,
    pub(super) debounce: std::time::Duration,
    pub(super) bootstrap_pending: bool,
    #[cfg(feature = "persistence")]
    pub(super) cached_us_learning_feedback: Option<ReasoningLearningFeedback>,
}

pub(super) async fn initialize_us_runtime() -> Result<UsRuntimeBootstrap, Box<dyn std::error::Error>> {
    println!("=== Eden US — Real-time US Market Monitor ===\n");

    let config = Config::from_env()?;
    let (ctx, receiver) = QuoteContext::try_new(Arc::new(config)).await?;

    println!("Connected to Longport. Initializing US stocks...");

    let watchlist_symbols: Vec<Symbol> =
        US_WATCHLIST.iter().map(|s| Symbol(s.to_string())).collect();
    let store = initialize_us_store(&ctx, &watchlist_symbols).await;
    println!("US Stocks: {}", store.stocks.len());

    println!("\nSubscribing to WebSocket (QUOTE + TRADE)...");
    ctx.subscribe(US_WATCHLIST, SubFlags::QUOTE | SubFlags::TRADE)
        .await?;
    println!(
        "Subscribed to {} US symbols x 2 channels.",
        US_WATCHLIST.len()
    );

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

    println!("Fetching bootstrap quotes...");
    let initial_quotes =
        crate::ontology::snapshot::fetch_quotes_only(&ctx, &watchlist_symbols).await;

    let mut live = UsLiveState::new();
    live.quotes = initial_quotes;
    live.dirty = !live.quotes.is_empty();
    let rest = UsRestSnapshot::empty();
    let bootstrap_pending = live.dirty;

    let tick_history = UsTickHistory::new(120);
    let signal_records: Vec<UsSignalRecord> = Vec::new();
    let previous_setups: Vec<TacticalSetup> = Vec::new();
    let previous_tracks: Vec<crate::ontology::reasoning::HypothesisTrack> = Vec::new();
    let previous_flows: PreviousFlows = HashMap::new();
    let lineage_stats = UsLineageStats::default();
    let prev_insights: Option<UsGraphInsights> = None;
    let position_tracker = UsPositionTracker::new();
    let workflows: Vec<UsActionWorkflow> = Vec::new();
    let bridge_service = FileSystemBridgeService::default();
    let analyst_service = DefaultAnalystService;

    #[cfg(feature = "persistence")]
    let persistence_slots = US_PERSISTENCE_MAX_IN_FLIGHT;
    #[cfg(not(feature = "persistence"))]
    let persistence_slots = 1usize;
    let runtime = prepare_runtime_context_or_exit(
        MarketId::Us,
        persistence_slots,
        "SurrealDB failed to open for US runtime",
    )
    .await;
    let debounce = runtime.debounce_duration();

    #[cfg(feature = "persistence")]
    if let Some(ref db) = runtime.store {
        let restored =
            crate::ontology::store::AccumulatedKnowledge::restore_from(db, "us").await;
        *store.knowledge.write().unwrap() = restored;
    }

    runtime.log_monitoring_active("Real-time US monitoring active");

    let push_rx =
        runtime.spawn_batched_push_forwarder(receiver, US_PUSH_BATCH_CHANNEL_CAP, US_PUSH_BATCH_SIZE);

    let rest_ctx = ctx.clone();
    let rest_watchlist = watchlist_symbols.clone();
    let rest_rx = runtime.spawn_rest_refresh(1, move || {
        let rest_ctx = rest_ctx.clone();
        let rest_watchlist = rest_watchlist.clone();
        async move { fetch_us_rest_data(&rest_ctx, &rest_watchlist).await }
    });

    Ok(UsRuntimeBootstrap {
        store,
        live,
        rest,
        tick_history,
        signal_records,
        previous_setups,
        previous_tracks,
        previous_flows,
        lineage_stats,
        prev_insights,
        position_tracker,
        workflows,
        bridge_service,
        analyst_service,
        runtime,
        push_rx,
        rest_rx,
        tick: 0,
        debounce,
        bootstrap_pending,
        #[cfg(feature = "persistence")]
        cached_us_learning_feedback: None,
    })
}
