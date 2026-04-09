#[cfg(feature = "persistence")]
use super::persistence::{
    persist_market_knowledge_projection, persist_market_lineage_projection,
    schedule_store_batch_operations, schedule_store_operation,
};
use super::*;
use crate::core::runtime_tasks::RuntimeTaskHandle;

#[derive(Debug, Clone)]
pub struct RuntimeInfraConfig {
    pub market: MarketId,
    pub debounce_ms: u64,
    pub rest_refresh_secs: u64,
    pub metrics_every_ticks: u64,
    pub db_path: String,
    pub runtime_log_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct AgentArtifactPaths {
    pub live_snapshot_path: String,
    pub agent_snapshot_path: String,
    pub operational_snapshot_path: String,
    pub agent_briefing_path: String,
    pub agent_session_path: String,
    pub agent_watchlist_path: String,
    pub agent_recommendations_path: String,
    pub agent_recommendation_journal_path: String,
    pub agent_scoreboard_path: String,
    pub agent_eod_review_path: String,
    pub agent_narration_path: String,
    pub agent_runtime_narration_path: String,
    pub agent_analysis_path: String,
}

#[derive(Debug, Clone)]
pub struct ProjectionStateCache {
    pub previous_agent_snapshot: Option<AgentSnapshot>,
    pub previous_agent_session: Option<AgentSession>,
    pub previous_agent_scoreboard: Option<AgentAlertScoreboard>,
    pub analyst_limit: Arc<Semaphore>,
}

#[derive(Debug, Clone)]
pub struct PreparedRuntimeContext {
    pub config: RuntimeInfraConfig,
    pub counters: RuntimeCounters,
    pub projection_state: ProjectionStateCache,
    pub artifacts: AgentArtifactPaths,
    pub runtime_task: Option<RuntimeTaskHandle>,
    #[cfg(feature = "persistence")]
    pub store: Option<EdenStore>,
    #[cfg(feature = "persistence")]
    pub persistence_limit: Arc<Semaphore>,
}

impl RuntimeInfraConfig {
    pub fn load(market: MarketId) -> Result<Self, String> {
        let slug = market.slug().to_ascii_uppercase();
        let default_debounce_ms = match market {
            MarketId::Hk | MarketId::Us => 250,
        };
        let debounce_ms = load_u64_override(
            &format!("EDEN_{}_DEBOUNCE_MS", slug),
            "EDEN_DEBOUNCE_MS",
            default_debounce_ms,
        )?;
        let rest_refresh_secs = load_u64_override(
            &format!("EDEN_{}_REST_REFRESH_SECS", slug),
            "EDEN_REST_REFRESH_SECS",
            60,
        )?;
        let metrics_every_ticks = load_u64_override(
            &format!("EDEN_{}_METRICS_EVERY_TICKS", slug),
            "EDEN_METRICS_EVERY_TICKS",
            25,
        )?;
        let db_path = load_string_override(
            &format!("EDEN_{}_DB_PATH", slug),
            "EDEN_DB_PATH",
            "data/eden.db",
        );
        let runtime_log_path = load_optional_string_override(
            &format!("EDEN_{}_RUNTIME_LOG_PATH", slug),
            "EDEN_RUNTIME_LOG_PATH",
        );

        if debounce_ms == 0 {
            return Err("runtime debounce must be greater than 0ms".into());
        }
        if rest_refresh_secs == 0 {
            return Err("runtime rest refresh interval must be greater than 0s".into());
        }
        if metrics_every_ticks == 0 {
            return Err("runtime metrics interval must be greater than 0 ticks".into());
        }

        Ok(Self {
            market,
            debounce_ms,
            rest_refresh_secs,
            metrics_every_ticks,
            db_path,
            runtime_log_path,
        })
    }

    pub fn debounce_duration(&self) -> Duration {
        Duration::from_millis(self.debounce_ms)
    }

    pub fn rest_refresh_duration(&self) -> Duration {
        Duration::from_secs(self.rest_refresh_secs)
    }

    pub fn log_startup(&self, persistence_enabled: bool) {
        println!(
            "[runtime:{}] debounce={}ms rest_refresh={}s metrics_every={}ticks persistence={} db={}",
            self.market.slug(),
            self.debounce_ms,
            self.rest_refresh_secs,
            self.metrics_every_ticks,
            if persistence_enabled { "on" } else { "off" },
            self.db_path
        );
        emit_runtime_log(
            self,
            "startup",
            json!({
                "debounce_ms": self.debounce_ms,
                "rest_refresh_secs": self.rest_refresh_secs,
                "metrics_every_ticks": self.metrics_every_ticks,
                "persistence_enabled": persistence_enabled,
                "db_path": self.db_path,
            }),
        );
    }
}

impl AgentArtifactPaths {
    pub async fn prepare(market: MarketId) -> Self {
        let paths = Self {
            live_snapshot_path: resolve_artifact_path(market, ArtifactKind::LiveSnapshot),
            agent_snapshot_path: resolve_artifact_path(market, ArtifactKind::AgentSnapshot),
            operational_snapshot_path: resolve_artifact_path(
                market,
                ArtifactKind::OperationalSnapshot,
            ),
            agent_briefing_path: resolve_artifact_path(market, ArtifactKind::Briefing),
            agent_session_path: resolve_artifact_path(market, ArtifactKind::Session),
            agent_watchlist_path: resolve_artifact_path(market, ArtifactKind::Watchlist),
            agent_recommendations_path: resolve_artifact_path(
                market,
                ArtifactKind::Recommendations,
            ),
            agent_recommendation_journal_path: resolve_artifact_path(
                market,
                ArtifactKind::RecommendationJournal,
            ),
            agent_scoreboard_path: resolve_artifact_path(market, ArtifactKind::Scoreboard),
            agent_eod_review_path: resolve_artifact_path(market, ArtifactKind::EodReview),
            agent_narration_path: resolve_artifact_path(market, ArtifactKind::Narration),
            agent_runtime_narration_path: resolve_artifact_path(
                market,
                ArtifactKind::RuntimeNarration,
            ),
            agent_analysis_path: resolve_artifact_path(market, ArtifactKind::Analysis),
        };

        ensure_snapshot_parent(&paths.live_snapshot_path).await;
        ensure_snapshot_parent(&paths.agent_snapshot_path).await;
        ensure_snapshot_parent(&paths.operational_snapshot_path).await;
        ensure_snapshot_parent(&paths.agent_briefing_path).await;
        ensure_snapshot_parent(&paths.agent_session_path).await;
        ensure_snapshot_parent(&paths.agent_watchlist_path).await;
        ensure_snapshot_parent(&paths.agent_recommendations_path).await;
        ensure_snapshot_parent(&paths.agent_recommendation_journal_path).await;
        ensure_snapshot_parent(&paths.agent_scoreboard_path).await;
        ensure_snapshot_parent(&paths.agent_eod_review_path).await;
        ensure_snapshot_parent(&paths.agent_narration_path).await;
        ensure_snapshot_parent(&paths.agent_runtime_narration_path).await;
        ensure_snapshot_parent(&paths.agent_analysis_path).await;

        paths
    }
}

pub async fn prepare_runtime_artifact_path(path: &str) {
    ensure_snapshot_parent(path).await;
}

impl ProjectionStateCache {
    pub fn new() -> Self {
        Self {
            previous_agent_snapshot: None,
            previous_agent_session: None,
            previous_agent_scoreboard: None,
            analyst_limit: Arc::new(Semaphore::new(1)),
        }
    }
}

impl PreparedRuntimeContext {
    pub fn log_monitoring_active(&self, label: &str) {
        log_runtime_monitoring_active(&self.config, label);
    }

    pub fn runtime_task_heartbeat(&self, detail: impl Into<String>, metadata: serde_json::Value) {
        if let Some(handle) = &self.runtime_task {
            let _ = handle.heartbeat(detail, metadata);
        }
    }

    pub fn complete_runtime_task(&self, detail: impl Into<String>, metadata: serde_json::Value) {
        if let Some(handle) = &self.runtime_task {
            let _ = handle.complete(detail, metadata);
        }
    }

    pub fn spawn_rest_refresh<U, F, Fut>(&self, capacity: usize, fetcher: F) -> mpsc::Receiver<U>
    where
        U: Send + 'static,
        F: FnMut() -> Fut + Send + 'static,
        Fut: std::future::Future<Output = U> + Send + 'static,
    {
        spawn_runtime_rest_refresh(&self.config, capacity, fetcher)
    }

    pub fn persist_projection_bundle(&self, market: CaseMarket, projection: &ProjectionBundle) {
        crate::core::persistence_sink::persist_projection_bundle(market, projection);
    }

    pub fn publish_projection<S: AnalystService>(
        &mut self,
        market_id: MarketId,
        case_market: CaseMarket,
        projection: &ProjectionBundle,
        extra_artifacts: Vec<(String, String)>,
        analyst_service: &S,
        tick: u64,
        push_count: u64,
        tick_started_at: Instant,
        received_push: bool,
        received_update: bool,
    ) {
        self.persist_projection_bundle(case_market, projection);
        self.write_projection_artifacts(market_id, projection, extra_artifacts);
        self.finalize_projection(
            analyst_service,
            case_market,
            projection,
            tick,
            push_count,
            tick_started_at,
            received_push,
            received_update,
        );
    }

    pub async fn publish_projection_stage<S, F, Fut>(
        &mut self,
        market_id: MarketId,
        case_market: CaseMarket,
        projection: &ProjectionBundle,
        extra_artifacts: Vec<(String, String)>,
        analyst_service: &S,
        tick: u64,
        push_count: u64,
        tick_started_at: Instant,
        received_push: bool,
        received_update: bool,
        followups: F,
    ) where
        S: AnalystService,
        F: FnOnce(&PreparedRuntimeContext) -> Fut,
        Fut: std::future::Future<Output = ()>,
    {
        self.publish_projection(
            market_id,
            case_market,
            projection,
            extra_artifacts,
            analyst_service,
            tick,
            push_count,
            tick_started_at,
            received_push,
            received_update,
        );
        followups(self).await;
    }

    pub fn debounce_duration(&self) -> Duration {
        self.config.debounce_duration()
    }

    pub fn spawn_push_forwarder(
        &self,
        receiver: mpsc::UnboundedReceiver<PushEvent>,
        capacity: usize,
    ) -> mpsc::Receiver<PushEvent> {
        spawn_push_forwarder(
            receiver,
            capacity,
            self.counters.clone(),
            self.config.clone(),
        )
    }

    pub fn spawn_batched_push_forwarder(
        &self,
        receiver: mpsc::UnboundedReceiver<PushEvent>,
        channel_capacity: usize,
        batch_size: usize,
    ) -> mpsc::Receiver<Vec<PushEvent>> {
        spawn_batched_push_forwarder(
            receiver,
            channel_capacity,
            batch_size,
            self.counters.clone(),
            self.config.clone(),
        )
    }

    pub async fn begin_tick<P, U, S>(
        &self,
        bootstrap_pending: &mut bool,
        push_rx: &mut mpsc::Receiver<P>,
        update_rx: &mut mpsc::Receiver<U>,
        debounce: Duration,
        state: &mut S,
        tick: &mut u64,
    ) -> Option<RuntimeTickBoundary>
    where
        S: TickState<P, U>,
    {
        begin_runtime_tick(
            bootstrap_pending,
            push_rx,
            update_rx,
            &self.config,
            &self.counters,
            debounce,
            state,
            tick,
        )
        .await
    }

    pub fn write_projection_artifacts(
        &self,
        market: MarketId,
        projection: &ProjectionBundle,
        extra_artifacts: Vec<(String, String)>,
    ) {
        write_projection_artifacts(market, &self.artifacts, projection, extra_artifacts);
    }

    pub fn finalize_projection<S: AnalystService>(
        &mut self,
        analyst_service: &S,
        market: CaseMarket,
        projection: &ProjectionBundle,
        tick: u64,
        push_count: u64,
        tick_started_at: Instant,
        received_push: bool,
        received_update: bool,
    ) {
        finalize_runtime_projection(
            analyst_service,
            market,
            projection,
            &mut self.projection_state,
            &self.config,
            &self.counters,
            tick,
            push_count,
            tick_started_at,
            received_push,
            received_update,
        );
    }

    #[cfg(feature = "persistence")]
    pub async fn schedule_store_operation<F, Fut, E>(
        &self,
        label: &'static str,
        issue_code: &'static str,
        error_prefix: &'static str,
        action: F,
    ) where
        F: FnOnce(EdenStore) -> Fut + Send + 'static,
        Fut: Future<Output = Result<(), E>> + Send + 'static,
        E: Display + Send + 'static,
    {
        schedule_store_operation(
            &self.store,
            &self.persistence_limit,
            &self.config,
            label,
            issue_code,
            error_prefix,
            action,
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn schedule_store_batch_operations<T, F, Fut, E>(
        &self,
        label: &'static str,
        issue_code: &'static str,
        error_prefix: &'static str,
        items: Vec<T>,
        action: F,
    ) where
        T: Send + 'static,
        F: Fn(EdenStore, T) -> Fut + Send + Sync + Copy + 'static,
        Fut: Future<Output = Result<(), E>> + Send + 'static,
        E: Display + Send + 'static,
    {
        schedule_store_batch_operations(
            &self.store,
            &self.persistence_limit,
            &self.config,
            label,
            issue_code,
            error_prefix,
            items,
            action,
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_knowledge_projection(&self, bundle: KnowledgePersistenceBundle) {
        persist_market_knowledge_projection(
            &self.store,
            &self.persistence_limit,
            &self.config,
            bundle,
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_knowledge_followups(
        &self,
        market: LiveMarket,
        tick_number: u64,
        recorded_at: time::OffsetDateTime,
        snapshot_knowledge_links: &[AgentKnowledgeLink],
        recommendation_knowledge_links: &[AgentKnowledgeLink],
        macro_events: &[AgentMacroEvent],
        decisions: &[AgentDecision],
        hypotheses: &[Hypothesis],
        setups: &[TacticalSetup],
        cases: &[CaseSummary],
        world_state: Option<&WorldStateSnapshot>,
        backward_reasoning: Option<&BackwardReasoningSnapshot>,
        active_positions: &[ActionNode],
    ) {
        let bundle = self.build_knowledge_followup_bundle(
            market,
            tick_number,
            recorded_at,
            snapshot_knowledge_links,
            recommendation_knowledge_links,
            macro_events,
            decisions,
            hypotheses,
            setups,
            cases,
            world_state,
            backward_reasoning,
            active_positions,
        );
        self.persist_knowledge_projection(bundle).await;
    }

    #[cfg(feature = "persistence")]
    pub fn build_knowledge_persistence_bundle(
        &self,
        market: LiveMarket,
        tick_number: u64,
        recorded_at: time::OffsetDateTime,
        macro_events: &[AgentMacroEvent],
        decisions: &[crate::agent::AgentDecision],
        hypotheses: &[Hypothesis],
        setups: &[TacticalSetup],
        cases: &[CaseSummary],
        world_state: Option<&WorldStateSnapshot>,
        backward_reasoning: Option<&BackwardReasoningSnapshot>,
        active_positions: &[ActionNode],
        links: &[AgentKnowledgeLink],
        events: &[AgentKnowledgeEvent],
    ) -> KnowledgePersistenceBundle {
        build_market_knowledge_persistence_bundle(
            market,
            tick_number,
            recorded_at,
            macro_events,
            decisions,
            hypotheses,
            setups,
            cases,
            world_state,
            backward_reasoning,
            active_positions,
            links,
            events,
        )
    }

    #[cfg(feature = "persistence")]
    pub fn build_knowledge_followup_bundle(
        &self,
        market: LiveMarket,
        tick_number: u64,
        recorded_at: time::OffsetDateTime,
        snapshot_knowledge_links: &[AgentKnowledgeLink],
        recommendation_knowledge_links: &[AgentKnowledgeLink],
        macro_events: &[AgentMacroEvent],
        decisions: &[AgentDecision],
        hypotheses: &[Hypothesis],
        setups: &[TacticalSetup],
        cases: &[CaseSummary],
        world_state: Option<&WorldStateSnapshot>,
        backward_reasoning: Option<&BackwardReasoningSnapshot>,
        active_positions: &[ActionNode],
    ) -> KnowledgePersistenceBundle {
        build_market_knowledge_followup_bundle(
            market,
            tick_number,
            recorded_at,
            snapshot_knowledge_links,
            recommendation_knowledge_links,
            macro_events,
            decisions,
            hypotheses,
            setups,
            cases,
            world_state,
            backward_reasoning,
            active_positions,
        )
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_lineage_projection(&self, bundle: LineagePersistenceBundle) {
        persist_market_lineage_projection(
            &self.store,
            &self.persistence_limit,
            &self.config,
            bundle,
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_case_reasoning_assessments(
        &self,
        market: CaseMarket,
        records: Vec<CaseReasoningAssessmentRecord>,
    ) {
        let (label, issue_code, error_prefix) = match market {
            CaseMarket::Hk => (
                "write case reasoning assessments",
                "write_hk_case_reasoning_assessments_failed",
                "failed to write case reasoning assessments",
            ),
            CaseMarket::Us => (
                "write US case reasoning assessments",
                "write_us_case_reasoning_assessments_failed",
                "failed to write US case reasoning assessments",
            ),
        };
        self.schedule_store_operation(label, issue_code, error_prefix, move |store_ref| async move {
            store_ref.write_case_reasoning_assessments(&records).await
        })
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_case_reasoning_assessments_for_cases(
        &self,
        market: CaseMarket,
        cases: &[CaseSummary],
        recorded_at: time::OffsetDateTime,
        source: &'static str,
    ) {
        let records = cases
            .iter()
            .map(|case| CaseReasoningAssessmentRecord::from_case_summary(case, recorded_at, source))
            .collect::<Vec<_>>();
        if records.is_empty() {
            return;
        }
        self.persist_case_reasoning_assessments(market, records)
            .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_hk_case_realized_outcomes(&self, records: Vec<CaseRealizedOutcomeRecord>) {
        // Auto-generate assessments from realized outcomes to feed doctrine pressure
        let auto_assessments =
            crate::persistence::case_reasoning_assessment::auto_assessments_from_outcomes(&records);
        if !auto_assessments.is_empty() {
            self.schedule_store_operation(
                "write auto-assessments from outcomes",
                "write_auto_assessments_from_outcomes_failed",
                "failed to write auto-assessments from realized outcomes",
                move |store_ref| async move {
                    store_ref
                        .write_case_reasoning_assessments(&auto_assessments)
                        .await
                },
            )
            .await;
        }
        self.schedule_store_operation(
            "write case realized outcomes",
            "write_hk_case_realized_outcomes_failed",
            "failed to write case realized outcomes",
            move |store_ref| async move { store_ref.write_case_realized_outcomes(&records).await },
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_projection_followups(
        &self,
        market: CaseMarket,
        knowledge_bundle: KnowledgePersistenceBundle,
        cases: &[CaseSummary],
        recorded_at: time::OffsetDateTime,
        source: &'static str,
        realized_outcomes: Option<Vec<CaseRealizedOutcomeRecord>>,
    ) {
        self.persist_knowledge_projection(knowledge_bundle).await;
        self.persist_case_reasoning_assessments_for_cases(market, cases, recorded_at, source)
            .await;
        if let Some(records) = realized_outcomes {
            self.persist_hk_case_realized_outcomes(records).await;
        }
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_projection_followups_from_inputs(
        &self,
        market: CaseMarket,
        live_market: LiveMarket,
        tick_number: u64,
        recorded_at: time::OffsetDateTime,
        snapshot_knowledge_links: &[AgentKnowledgeLink],
        recommendation_knowledge_links: &[AgentKnowledgeLink],
        macro_events: &[AgentMacroEvent],
        decisions: &[AgentDecision],
        hypotheses: &[Hypothesis],
        setups: &[TacticalSetup],
        cases: &[CaseSummary],
        world_state: Option<&WorldStateSnapshot>,
        backward_reasoning: Option<&BackwardReasoningSnapshot>,
        active_positions: &[ActionNode],
        source: &'static str,
        realized_outcomes: Option<Vec<CaseRealizedOutcomeRecord>>,
    ) {
        let knowledge_bundle = self.build_knowledge_followup_bundle(
            live_market,
            tick_number,
            recorded_at,
            snapshot_knowledge_links,
            recommendation_knowledge_links,
            macro_events,
            decisions,
            hypotheses,
            setups,
            cases,
            world_state,
            backward_reasoning,
            active_positions,
        );
        self.persist_projection_followups(
            market,
            knowledge_bundle,
            cases,
            recorded_at,
            source,
            realized_outcomes,
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    #[allow(clippy::too_many_arguments)]
    pub async fn publish_projection_with_followups_from_inputs<S: AnalystService>(
        &mut self,
        market_id: MarketId,
        case_market: CaseMarket,
        projection: &ProjectionBundle,
        extra_artifacts: Vec<(String, String)>,
        analyst_service: &S,
        tick: u64,
        push_count: u64,
        tick_started_at: Instant,
        received_push: bool,
        received_update: bool,
        cases: &[CaseSummary],
        recorded_at: time::OffsetDateTime,
        source: &'static str,
        snapshot_knowledge_links: &[AgentKnowledgeLink],
        recommendation_knowledge_links: &[AgentKnowledgeLink],
        macro_events: &[AgentMacroEvent],
        decisions: &[AgentDecision],
        hypotheses: &[Hypothesis],
        setups: &[TacticalSetup],
        world_state: Option<&WorldStateSnapshot>,
        backward_reasoning: Option<&BackwardReasoningSnapshot>,
        active_positions: &[ActionNode],
        realized_outcomes: Option<Vec<CaseRealizedOutcomeRecord>>,
    ) {
        self.publish_projection(
            market_id,
            case_market,
            projection,
            extra_artifacts,
            analyst_service,
            tick,
            push_count,
            tick_started_at,
            received_push,
            received_update,
        );
        self.persist_projection_followups_from_inputs(
            case_market,
            LiveMarket::from(market_id),
            tick,
            recorded_at,
            snapshot_knowledge_links,
            recommendation_knowledge_links,
            macro_events,
            decisions,
            hypotheses,
            setups,
            cases,
            world_state,
            backward_reasoning,
            active_positions,
            source,
            realized_outcomes,
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_us_tick(&self, record: UsTickRecord) {
        self.schedule_store_operation(
            "write US tick",
            "write_us_tick_failed",
            "failed to write US tick",
            move |store_ref| async move { store_ref.write_us_tick(&record).await },
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_hk_tick(&self, record: TickRecord) {
        self.schedule_store_operation(
            "write tick",
            "write_hk_tick_failed",
            "failed to write tick",
            move |store_ref| async move { store_ref.write_tick(&record).await },
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_market_tick_archive(&self, archive: TickArchive) {
        let (label, issue_code, error_prefix) = match self.config.market {
            MarketId::Hk => (
                "write tick archive",
                "write_hk_tick_archive_failed",
                "write_tick_archive failed",
            ),
            MarketId::Us => (
                "write US tick archive",
                "write_us_tick_archive_failed",
                "write_tick_archive failed",
            ),
        };
        self.schedule_store_operation(
            label,
            issue_code,
            error_prefix,
            move |store_ref| async move { store_ref.write_tick_archive(&archive).await },
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_hk_institution_states(
        &self,
        presences: Vec<CrossStockPresence>,
        recorded_at: time::OffsetDateTime,
    ) {
        self.schedule_store_operation(
            "write institution states",
            "write_hk_institution_states_failed",
            "failed to write institution states",
            move |store_ref| async move {
                store_ref
                    .write_institution_states(&presences, recorded_at)
                    .await
            },
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_hk_lineage_projection(
        &self,
        snapshot: LineageSnapshotRecord,
        rows: Vec<LineageMetricRowRecord>,
    ) {
        self.persist_lineage_projection(LineagePersistenceBundle::Hk { snapshot, rows })
            .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_us_lineage_projection(
        &self,
        snapshot: UsLineageSnapshotRecord,
        rows: Vec<UsLineageMetricRowRecord>,
    ) {
        self.persist_lineage_projection(LineagePersistenceBundle::Us { snapshot, rows })
            .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_hk_lineage_stats(
        &self,
        tick_number: u64,
        recorded_at: time::OffsetDateTime,
        window_size: usize,
        stats: &LineageStats,
    ) {
        let snapshot = LineageSnapshotRecord::new(tick_number, recorded_at, window_size, stats);
        let rows = rows_from_lineage_stats(
            snapshot.record_id(),
            tick_number,
            recorded_at,
            window_size,
            stats,
        );
        self.persist_hk_lineage_projection(snapshot, rows).await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_us_lineage_stats(
        &self,
        tick_number: u64,
        recorded_at: time::OffsetDateTime,
        window_size: usize,
        resolution_lag: u64,
        stats: &UsLineageStats,
    ) {
        let snapshot = UsLineageSnapshotRecord::new(
            tick_number,
            recorded_at,
            window_size,
            resolution_lag,
            stats,
        );
        let rows = rows_from_us_lineage_stats(
            snapshot.record_id(),
            tick_number,
            recorded_at,
            window_size,
            resolution_lag,
            stats,
        );
        self.persist_us_lineage_projection(snapshot, rows).await;
    }
}
