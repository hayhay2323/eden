use super::support::{fetch_us_rest_data, initialize_us_store, UsLiveState, UsRestSnapshot};
use super::*;
use crate::core::runtime::PreparedRuntimeContext;

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
    pub(super) tick: u64,
    pub(super) debounce: std::time::Duration,
    pub(super) bootstrap_pending: bool,
    pub(super) absence_memory: crate::pipeline::reasoning::AbsenceMemory,
    pub(super) energy_momentum: crate::graph::energy::EnergyMomentum,
    #[cfg(feature = "persistence")]
    pub(super) cached_us_learning_feedback: Option<ReasoningLearningFeedback>,
    #[cfg(feature = "persistence")]
    pub(super) cached_us_reviewer_doctrine:
        Option<crate::pipeline::reasoning::ReviewerDoctrinePressure>,
}

pub(super) async fn initialize_us_runtime() -> Result<UsRuntimeBootstrap, Box<dyn std::error::Error>>
{
    println!("=== Eden US — Real-time US Market Monitor ===\n");

    let config = Config::from_env()?;
    let (ctx, receiver) = QuoteContext::try_new(Arc::new(config)).await?;

    println!("Connected to Longport. Initializing US stocks...");

    let watchlist_symbols: Vec<Symbol> =
        US_WATCHLIST.iter().map(|s| Symbol(s.to_string())).collect();
    let store = initialize_us_store(&ctx, &watchlist_symbols).await;
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
    ctx.subscribe(
        &ws_symbols,
        SubFlags::QUOTE | SubFlags::TRADE,
    )
    .await?;
    println!(
        "Subscribed to {} US symbols x 2 channels. ({} symbols via REST only.)",
        ws_symbols.len(),
        US_WATCHLIST.len().saturating_sub(WS_SUBSCRIPTION_LIMIT),
    );

    for symbol in &ws_symbols {
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

    // 500 ticks gives stable medium-term lineage stats.
    // Previously 120 caused wild hit_rate swings (90% → 25% in one hour).
    let tick_history = UsTickHistory::new(500);
    let signal_records: Vec<UsSignalRecord> = Vec::new();
    let scorecard_accumulator = UsSignalScorecardAccumulator::default();
    let signal_momentum = crate::us::temporal::lineage::SignalMomentumTracker::default();
    let previous_setups: Vec<TacticalSetup> = Vec::new();
    let previous_tracks: Vec<crate::ontology::reasoning::HypothesisTrack> = Vec::new();
    let previous_flows: PreviousFlows = HashMap::new();
    let lineage_stats = UsLineageStats::default();
    let lineage_accumulator =
        crate::us::temporal::lineage::UsLineageFamilyAccumulator::default();
    let lineage_prev_resolved: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();
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

    let restored_tick_count = 0usize;
    #[cfg(feature = "persistence")]
    if let Some(ref db) = runtime.store {
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
                previous_tracks = if restored_records.len() >= 2 {
                    let previous_record = restored_records[restored_records.len() - 2];
                    let baseline_previous_tracks =
                        crate::pipeline::reasoning::derive_hypothesis_tracks(
                            previous_record.timestamp,
                            &previous_record.tactical_setups,
                            &[],
                            &[],
                        );
                    crate::pipeline::reasoning::derive_hypothesis_tracks(
                        latest.timestamp,
                        &latest.tactical_setups,
                        &previous_record.tactical_setups,
                        &baseline_previous_tracks,
                    )
                } else {
                    crate::pipeline::reasoning::derive_hypothesis_tracks(
                        latest.timestamp,
                        &latest.tactical_setups,
                        &[],
                        &[],
                    )
                };
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
        }),
    );

    let push_rx = runtime.spawn_batched_push_forwarder(
        receiver,
        US_PUSH_BATCH_CHANNEL_CAP,
        US_PUSH_BATCH_SIZE,
    );

    let rest_ctx = ctx.clone();
    let rest_watchlist = watchlist_symbols.clone();
    let rest_rx = runtime.spawn_rest_refresh(1, move || {
        let rest_ctx = rest_ctx.clone();
        let rest_watchlist = rest_watchlist.clone();
        async move { fetch_us_rest_data(&rest_ctx, &rest_watchlist).await }
    });
    let restored_tick = tick_history
        .latest()
        .map(|record| record.tick_number)
        .unwrap_or(0);

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
        tick: restored_tick,
        debounce,
        bootstrap_pending,
        absence_memory: crate::pipeline::reasoning::AbsenceMemory::default(),
        energy_momentum: crate::graph::energy::EnergyMomentum::default(),
        #[cfg(feature = "persistence")]
        cached_us_learning_feedback: None,
        #[cfg(feature = "persistence")]
        cached_us_reviewer_doctrine: None,
    })
}
