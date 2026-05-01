#[cfg(feature = "persistence")]
use super::persistence::{
    persist_market_knowledge_projection, persist_market_lineage_projection,
    run_latest_store_operation, schedule_latest_store_operation, schedule_store_batch_operations,
    schedule_store_operation,
};
use super::*;
use crate::core::runtime_tasks::RuntimeTaskHandle;
#[cfg(feature = "persistence")]
use crate::ontology::links::CrossStockPresence;
#[cfg(feature = "persistence")]
use crate::ontology::microstructure::TickArchive;
#[cfg(feature = "persistence")]
use crate::persistence::case_reasoning_assessment::CaseReasoningAssessmentRecord;
#[cfg(feature = "persistence")]
use crate::persistence::lineage_metric_row::rows_from_lineage_stats;
#[cfg(feature = "persistence")]
use crate::persistence::us_lineage_metric_row::rows_from_us_lineage_stats;
#[cfg(feature = "persistence")]
use crate::temporal::lineage::LineageStats;
#[cfg(feature = "persistence")]
use crate::temporal::record::TickRecord;
#[cfg(feature = "persistence")]
use crate::us::temporal::lineage::UsLineageStats;
#[cfg(feature = "persistence")]
use crate::us::temporal::record::UsTickRecord;

#[derive(Debug, Clone)]
pub struct RuntimeInfraConfig {
    pub market: MarketId,
    pub debounce_ms: u64,
    pub rest_refresh_secs: u64,
    pub metrics_every_ticks: u64,
    pub db_path: String,
    pub runtime_log_path: Option<String>,
}

#[cfg(test)]
mod runtime_config_tests {
    use super::*;
    use std::sync::{Mutex, OnceLock};

    fn env_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }

    fn restore_env(saved: &[(&str, Option<String>)]) {
        for (key, value) in saved {
            match value {
                Some(value) => std::env::set_var(key, value),
                None => std::env::remove_var(key),
            }
        }
    }

    #[test]
    fn runtime_infra_config_prefers_market_specific_overrides() {
        let _guard = env_lock().lock().unwrap();
        let saved = [
            (
                "EDEN_US_DEBOUNCE_MS",
                std::env::var("EDEN_US_DEBOUNCE_MS").ok(),
            ),
            ("EDEN_DEBOUNCE_MS", std::env::var("EDEN_DEBOUNCE_MS").ok()),
            (
                "EDEN_US_REST_REFRESH_SECS",
                std::env::var("EDEN_US_REST_REFRESH_SECS").ok(),
            ),
            (
                "EDEN_REST_REFRESH_SECS",
                std::env::var("EDEN_REST_REFRESH_SECS").ok(),
            ),
            (
                "EDEN_US_METRICS_EVERY_TICKS",
                std::env::var("EDEN_US_METRICS_EVERY_TICKS").ok(),
            ),
            (
                "EDEN_METRICS_EVERY_TICKS",
                std::env::var("EDEN_METRICS_EVERY_TICKS").ok(),
            ),
            ("EDEN_US_DB_PATH", std::env::var("EDEN_US_DB_PATH").ok()),
            ("EDEN_DB_PATH", std::env::var("EDEN_DB_PATH").ok()),
            (
                "EDEN_US_RUNTIME_LOG_PATH",
                std::env::var("EDEN_US_RUNTIME_LOG_PATH").ok(),
            ),
            (
                "EDEN_RUNTIME_LOG_PATH",
                std::env::var("EDEN_RUNTIME_LOG_PATH").ok(),
            ),
        ];

        std::env::set_var("EDEN_US_DEBOUNCE_MS", "111");
        std::env::set_var("EDEN_DEBOUNCE_MS", "999");
        std::env::set_var("EDEN_US_REST_REFRESH_SECS", "7");
        std::env::set_var("EDEN_REST_REFRESH_SECS", "99");
        std::env::set_var("EDEN_US_METRICS_EVERY_TICKS", "13");
        std::env::set_var("EDEN_METRICS_EVERY_TICKS", "77");
        std::env::set_var("EDEN_US_DB_PATH", "/tmp/us.db");
        std::env::set_var("EDEN_DB_PATH", "/tmp/fallback.db");
        std::env::set_var("EDEN_US_RUNTIME_LOG_PATH", "/tmp/us-runtime.log");
        std::env::set_var("EDEN_RUNTIME_LOG_PATH", "/tmp/fallback-runtime.log");

        let config = RuntimeInfraConfig::load(MarketId::Us).unwrap();
        assert_eq!(config.debounce_ms, 111);
        assert_eq!(config.rest_refresh_secs, 7);
        assert_eq!(config.metrics_every_ticks, 13);
        assert_eq!(config.db_path, "/tmp/us.db");
        assert_eq!(
            config.runtime_log_path.as_deref(),
            Some("/tmp/us-runtime.log")
        );

        restore_env(&saved);
    }

    #[test]
    fn runtime_infra_config_rejects_zero_intervals() {
        let _guard = env_lock().lock().unwrap();
        let saved = [
            (
                "EDEN_HK_DEBOUNCE_MS",
                std::env::var("EDEN_HK_DEBOUNCE_MS").ok(),
            ),
            (
                "EDEN_HK_REST_REFRESH_SECS",
                std::env::var("EDEN_HK_REST_REFRESH_SECS").ok(),
            ),
            (
                "EDEN_HK_METRICS_EVERY_TICKS",
                std::env::var("EDEN_HK_METRICS_EVERY_TICKS").ok(),
            ),
        ];

        std::env::set_var("EDEN_HK_DEBOUNCE_MS", "0");
        let error = RuntimeInfraConfig::load(MarketId::Hk).unwrap_err();
        assert!(error.contains("runtime debounce must be greater than 0ms"));

        std::env::remove_var("EDEN_HK_DEBOUNCE_MS");
        std::env::set_var("EDEN_HK_REST_REFRESH_SECS", "0");
        let error = RuntimeInfraConfig::load(MarketId::Hk).unwrap_err();
        assert!(error.contains("runtime rest refresh interval must be greater than 0s"));

        std::env::remove_var("EDEN_HK_REST_REFRESH_SECS");
        std::env::set_var("EDEN_HK_METRICS_EVERY_TICKS", "0");
        let error = RuntimeInfraConfig::load(MarketId::Hk).unwrap_err();
        assert!(error.contains("runtime metrics interval must be greater than 0 ticks"));

        restore_env(&saved);
    }
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
    pub agent_perception_path: String,
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
    /// Latest regime fingerprint bucket key per market, set by the
    /// runtime's per-tick fingerprint emit, read by
    /// `persist_case_reasoning_assessments_for_cases` to stamp on each
    /// newly-built `CaseReasoningAssessmentRecord`. Wraps in
    /// Arc<RwLock> so the runtime can clone-share this context across
    /// async tasks while keeping the bucket value mutable.
    pub current_regime_buckets:
        Arc<std::sync::RwLock<std::collections::HashMap<crate::cases::CaseMarket, String>>>,
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
            agent_perception_path: resolve_artifact_path(
                market,
                ArtifactKind::Perception,
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
        ensure_snapshot_parent(&paths.agent_perception_path).await;
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
        tap: Option<crate::core::runtime::telemetry::PushTap>,
        health: Option<
            std::sync::Arc<crate::core::runtime::push_health::PushReceiverHealth>,
        >,
    ) -> mpsc::Receiver<Vec<PushEvent>> {
        spawn_batched_push_forwarder(
            receiver,
            channel_capacity,
            batch_size,
            self.counters.clone(),
            self.config.clone(),
            tap,
            health,
        )
    }

    pub fn config_clone(&self) -> RuntimeInfraConfig {
        self.config.clone()
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
    pub async fn persist_tactical_setups(
        &self,
        market: CaseMarket,
        records: Vec<crate::persistence::tactical_setup::TacticalSetupRecord>,
    ) {
        let latest_recorded_at = records
            .iter()
            .map(|record| record.recorded_at.unix_timestamp_nanos())
            .max();
        let Some(latest_recorded_at) = latest_recorded_at else {
            return;
        };
        let market_slug = match market {
            CaseMarket::Hk => "hk",
            CaseMarket::Us => "us",
        };
        let (label, issue_code, error_prefix) = match market {
            CaseMarket::Hk => (
                "write tactical setups",
                "write_hk_tactical_setups_failed",
                "failed to write tactical setups",
            ),
            CaseMarket::Us => (
                "write US tactical setups",
                "write_us_tactical_setups_failed",
                "failed to write US tactical setups",
            ),
        };
        schedule_latest_store_operation(
            &self.store,
            &self.persistence_limit,
            &self.config,
            label,
            issue_code,
            error_prefix,
            format!("tactical_setup:{market_slug}"),
            latest_recorded_at,
            move |store_ref| async move { store_ref.write_tactical_setups(&records).await },
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_action_workflows(
        &self,
        market: CaseMarket,
        mut records: Vec<crate::persistence::action_workflow::ActionWorkflowRecord>,
    ) {
        let latest_recorded_at = records
            .iter()
            .map(|record| record.recorded_at.unix_timestamp_nanos())
            .max();
        let Some(latest_recorded_at) = latest_recorded_at else {
            return;
        };
        let market_slug = match market {
            CaseMarket::Hk => "hk",
            CaseMarket::Us => "us",
        };
        let (label, issue_code, error_prefix) = match market {
            CaseMarket::Hk => (
                "write action workflows",
                "write_hk_action_workflows_failed",
                "failed to write action workflows",
            ),
            CaseMarket::Us => (
                "write US action workflows",
                "write_us_action_workflows_failed",
                "failed to write US action workflows",
            ),
        };
        schedule_latest_store_operation(
            &self.store,
            &self.persistence_limit,
            &self.config,
            label,
            issue_code,
            error_prefix,
            format!("action_workflow:{market_slug}"),
            latest_recorded_at,
            move |store_ref| async move {
                let workflow_ids = records
                    .iter()
                    .map(|record| record.workflow_id.clone())
                    .collect::<Vec<_>>();
                let existing_by_id = store_ref
                    .action_workflows_by_ids(&workflow_ids)
                    .await?
                    .into_iter()
                    .map(|record| (record.workflow_id.clone(), record))
                    .collect::<std::collections::HashMap<_, _>>();
                let is_system_actor =
                    |actor: Option<&str>| matches!(actor, Some("eden" | "eden-auto" | "tracker"));
                for record in &mut records {
                    if let Some(existing) = existing_by_id.get(record.workflow_id.as_str()) {
                        let existing_actor_is_manual = !is_system_actor(existing.actor.as_deref());
                        if record.owner.is_none() {
                            record.owner = existing.owner.clone();
                        }
                        if record.reviewer.is_none() {
                            record.reviewer = existing.reviewer.clone();
                        }
                        if record.queue_pin.is_none() {
                            record.queue_pin = existing.queue_pin.clone();
                        }
                        if existing_actor_is_manual
                            && (record.actor.is_none() || is_system_actor(record.actor.as_deref()))
                        {
                            record.actor = existing.actor.clone();
                            if existing.note.is_some() {
                                record.note = existing.note.clone();
                            }
                        }
                        if existing_actor_is_manual {
                            record.current_stage = existing.current_stage;
                            record.execution_policy = existing.execution_policy;
                            record.governance_reason_code =
                                crate::action::workflow::governance_reason_code(
                                    Some(record.current_stage),
                                    record.execution_policy,
                                );
                        }
                    }
                }
                store_ref.write_action_workflows(&records).await
            },
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_action_workflow_events(
        &self,
        market: CaseMarket,
        events: Vec<crate::persistence::action_workflow::ActionWorkflowEventRecord>,
    ) {
        let (label, issue_code, error_prefix) = match market {
            CaseMarket::Hk => (
                "write action workflow events",
                "write_hk_action_workflow_events_failed",
                "failed to write action workflow events",
            ),
            CaseMarket::Us => (
                "write US action workflow events",
                "write_us_action_workflow_events_failed",
                "failed to write US action workflow events",
            ),
        };
        self.schedule_store_batch_operations(
            label,
            issue_code,
            error_prefix,
            events,
            |store_ref, event| async move { store_ref.write_action_workflow_event(&event).await },
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_hypothesis_tracks(
        &self,
        market: CaseMarket,
        records: Vec<crate::persistence::hypothesis_track::HypothesisTrackRecord>,
    ) {
        let latest_updated_at = records
            .iter()
            .map(|record| record.last_updated_at.unix_timestamp_nanos())
            .max();
        let Some(latest_updated_at) = latest_updated_at else {
            return;
        };
        let market_slug = match market {
            CaseMarket::Hk => "hk",
            CaseMarket::Us => "us",
        };
        let (label, issue_code, error_prefix) = match market {
            CaseMarket::Hk => (
                "write hypothesis tracks",
                "write_hk_hypothesis_tracks_failed",
                "failed to write hypothesis tracks",
            ),
            CaseMarket::Us => (
                "write US hypothesis tracks",
                "write_us_hypothesis_tracks_failed",
                "failed to write hypothesis tracks",
            ),
        };
        schedule_latest_store_operation(
            &self.store,
            &self.persistence_limit,
            &self.config,
            label,
            issue_code,
            error_prefix,
            format!("hypothesis_track:{market_slug}"),
            latest_updated_at,
            move |store_ref| async move { store_ref.write_hypothesis_tracks(&records).await },
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_edge_learning_ledger(
        &self,
        market_slug: &'static str,
        record: crate::persistence::edge_learning_ledger::EdgeLearningLedgerRecord,
        version: i128,
    ) {
        run_latest_store_operation(
            &self.store,
            &self.persistence_limit,
            &self.config,
            "write edge learning ledger",
            "write_edge_learning_ledger_failed",
            "failed to write edge learning ledger",
            format!("edge_learning_ledger:{market_slug}"),
            version,
            move |store_ref| async move { store_ref.write_edge_learning_ledger(&record).await },
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_candidate_mechanisms(
        &self,
        market_slug: &'static str,
        records: Vec<crate::persistence::candidate_mechanism::CandidateMechanismRecord>,
        version: i128,
    ) {
        run_latest_store_operation(
            &self.store,
            &self.persistence_limit,
            &self.config,
            "write candidate mechanisms",
            "write_candidate_mechanisms_failed",
            "failed to write candidate mechanisms",
            format!("candidate_mechanism:{market_slug}"),
            version,
            move |store_ref| async move { store_ref.write_candidate_mechanisms(&records).await },
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_discovered_archetypes(
        &self,
        market_slug: &'static str,
        records: Vec<crate::persistence::discovered_archetype::DiscoveredArchetypeRecord>,
        version: i128,
    ) {
        run_latest_store_operation(
            &self.store,
            &self.persistence_limit,
            &self.config,
            "write discovered archetypes",
            "write_discovered_archetypes_failed",
            "failed to write discovered archetypes",
            format!("discovered_archetype:{market_slug}"),
            version,
            move |store_ref| async move { store_ref.write_discovered_archetypes(&records).await },
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_case_realized_outcomes_rows(
        &self,
        market_slug: &'static str,
        records: Vec<crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord>,
        version: i128,
    ) {
        run_latest_store_operation(
            &self.store,
            &self.persistence_limit,
            &self.config,
            "write case realized outcomes",
            "write_case_realized_outcomes_failed",
            "failed to write case realized outcomes",
            format!("case_realized_outcome:{market_slug}"),
            version,
            move |store_ref| async move { store_ref.write_case_realized_outcomes(&records).await },
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_symbol_perception_states(
        &self,
        market: CaseMarket,
        records: Vec<crate::persistence::symbol_perception_state::SymbolPerceptionStateRecord>,
    ) {
        let latest_updated_at = records
            .iter()
            .map(|record| record.updated_at.unix_timestamp_nanos())
            .max();
        let Some(latest_updated_at) = latest_updated_at else {
            return;
        };
        let (market_slug, label, issue_code, error_prefix) = match market {
            CaseMarket::Hk => (
                "hk",
                "sync HK symbol perception states",
                "sync_hk_symbol_perception_states_failed",
                "failed to sync HK symbol perception states",
            ),
            CaseMarket::Us => (
                "us",
                "sync US symbol perception states",
                "sync_us_symbol_perception_states_failed",
                "failed to sync US symbol perception states",
            ),
        };
        schedule_latest_store_operation(
            &self.store,
            &self.persistence_limit,
            &self.config,
            label,
            issue_code,
            error_prefix,
            format!("symbol_perception_state:{market_slug}"),
            latest_updated_at,
            move |store_ref| async move {
                store_ref
                    .sync_symbol_perception_states(market_slug, &records)
                    .await
            },
        )
        .await;
    }

    /// Writes pending horizon evaluation records for each NEW setup.
    ///
    /// INVARIANT: `setup_id` is treated as immutable case identity. Once
    /// a horizon record exists for (setup_id, horizon), this function will
    /// NOT overwrite it — the original `due_at` must be preserved so that
    /// the horizon can mature at its originally scheduled reference
    /// window. If a future producer violates this (e.g. mutates case
    /// scope/horizon under a stable setup_id), stale horizon records will
    /// be frozen and must be explicitly pruned.
    ///
    /// Current producers honoring the invariant:
    ///   - src/pipeline/pressure/bridge.rs:161 (`pf:{symbol}:{direction}:{bucket}` —
    ///     stable across ticks, but bucket-scoped so horizon identity stays stable)
    ///   - src/us/runtime/support/attention.rs:188 (carries forward via
    ///     `.clone()`, no mutation)
    ///   - src/us/action/workflow.rs:136 (inherits id from input setup)
    #[cfg(feature = "persistence")]
    pub async fn persist_horizon_evaluations(
        &self,
        market: CaseMarket,
        setups: &[TacticalSetup],
        now: time::OffsetDateTime,
    ) {
        let market_label = match market {
            CaseMarket::Hk => "hk",
            CaseMarket::Us => "us",
        };
        let candidates: Vec<crate::persistence::horizon_evaluation::HorizonEvaluationRecord> =
            setups
                .iter()
                .flat_map(|s| horizon_records_for_setup(s, market_label, now))
                .collect();
        if candidates.is_empty() {
            return;
        }
        // Collect the unique setup_ids present in this batch so we can query for
        // already-existing horizon records without a full-table scan.
        let setup_ids: Vec<String> = {
            let mut ids: Vec<String> = candidates.iter().map(|r| r.setup_id.clone()).collect();
            ids.sort_unstable();
            ids.dedup();
            ids
        };
        let (label, issue_code, error_prefix) = match market {
            CaseMarket::Hk => (
                "write horizon evaluations",
                "write_hk_horizon_evaluations_failed",
                "failed to write horizon evaluations",
            ),
            CaseMarket::Us => (
                "write US horizon evaluations",
                "write_us_horizon_evaluations_failed",
                "failed to write US horizon evaluations",
            ),
        };
        self.schedule_store_operation(
            label,
            issue_code,
            error_prefix,
            move |store_ref| async move {
                let existing_ids = store_ref
                    .horizon_evaluation_record_ids_for_setups(&setup_ids)
                    .await?;
                let new_records: Vec<_> = candidates
                    .into_iter()
                    .filter(|r| !existing_ids.contains(&r.record_id))
                    .collect();
                if new_records.is_empty() {
                    return Ok(());
                }
                store_ref.write_horizon_evaluations(&new_records).await
            },
        )
        .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_case_reasoning_assessments_for_cases(
        &self,
        market: CaseMarket,
        cases: &[CaseSummary],
        hypotheses: &[Hypothesis],
        setups: &[TacticalSetup],
        recorded_at: time::OffsetDateTime,
        source: &'static str,
    ) {
        let hypothesis_by_id = hypotheses
            .iter()
            .map(|hypothesis| (hypothesis.hypothesis_id.as_str(), hypothesis))
            .collect::<std::collections::HashMap<_, _>>();
        let setup_by_id = setups
            .iter()
            .map(|setup| (setup.setup_id.as_str(), setup))
            .collect::<std::collections::HashMap<_, _>>();

        // Read the current regime fingerprint bucket for this market
        // (set by the runtime's per-tick fingerprint emit). Stamp it
        // on every record so `derive_learning_feedback` can build
        // `(mechanism, "regime_bucket", bucket_key)` entries in
        // `conditioned_totals`. Without this stamp the regime-
        // conditional learning scope never accumulates data even
        // though the wiring exists.
        let regime_bucket = self
            .current_regime_buckets
            .read()
            .ok()
            .and_then(|map| map.get(&market).cloned());

        let records = cases
            .iter()
            .map(|case| {
                let mut record =
                    CaseReasoningAssessmentRecord::from_case_summary(case, recorded_at, source);
                if let Some(setup) = setup_by_id.get(case.setup_id.as_str()).copied() {
                    let hypothesis = hypothesis_by_id.get(setup.hypothesis_id.as_str()).copied();
                    record.apply_setup_projection(setup, hypothesis);
                }
                record.regime_bucket = regime_bucket.clone();
                record
            })
            .collect::<Vec<_>>();
        if records.is_empty() {
            return;
        }
        self.persist_case_reasoning_assessments(market, records)
            .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_hk_case_realized_outcomes(&self, records: Vec<CaseRealizedOutcomeRecord>) {
        // Resolution hook: settle horizon evaluations and write case_resolution records.
        // This runs BEFORE the auto-assessment logic so that resolution data is available
        // for downstream doctrine pressure aggregation.
        // Non-blocking: any resolution failure is logged and does not abort the rest.
        if let Some(ref store) = self.store {
            for outcome in &records {
                match settle_horizon_records_from_realized_outcome(store, outcome).await {
                    Ok(Some(settled)) => {
                        // Extract intent_kind and signature from the persisted TacticalSetupRecord.
                        // These are optional enrichments; empty strings are acceptable.
                        let (intent_kind, signature) = store
                            .tactical_setups_by_ids(&[settled.setup_id.clone()])
                            .await
                            .ok()
                            .and_then(|mut recs| recs.pop())
                            .map(|rec| {
                                let ik = rec
                                    .inferred_intent
                                    .as_ref()
                                    .map(|i| format!("{:?}", i.kind).to_ascii_lowercase())
                                    .unwrap_or_default();
                                let sig = rec
                                    .case_signature
                                    .as_ref()
                                    .map(|s| {
                                        format!(
                                            "{}:{}:{}",
                                            format!("{:?}", s.topology).to_ascii_lowercase(),
                                            format!("{:?}", s.temporal_shape).to_ascii_lowercase(),
                                            format!("{:?}", s.conflict_shape).to_ascii_lowercase(),
                                        )
                                    })
                                    .unwrap_or_default();
                                (ik, sig)
                            })
                            .unwrap_or_default();

                        // Progressive bootstrap settle (P1 step 1 of the full
                        // runtime settle path).
                        //
                        // The original single-call pattern hard-coded
                        // all_settled=true and skipped the Provisional→Final
                        // upgrade gate entirely: `apply_case_resolution_update`
                        // is only called when an existing resolution record
                        // needs refinement, and the bootstrap only ever wrote
                        // Final in one shot. Learning loop therefore never
                        // saw Provisional finality — `delta_from_case_resolution`
                        // always returned the 1.0 Final-Confirmed branch
                        // instead of the progressive 0.5→1.0 it was designed
                        // for. The resolution_history was also single-entry
                        // per case, which made it impossible to tell "this
                        // case was decided in one atomic write" from "this
                        // case matured through a Provisional phase first".
                        //
                        // The fix writes two calls instead of one when
                        // supplementals exist:
                        //   step 1: primary only, all_settled=false → Provisional
                        //   step 2: full supplementals, all_settled=true → Final
                        // This guarantees `apply_case_resolution_update` runs
                        // at least once per case close, exercising the
                        // upgrade gate and refinement-to-Final rule. When
                        // there are no supplementals the progressive pattern
                        // would be redundant (primary alone with all_settled
                        // already produces a well-defined single-write path),
                        // so we keep the single call in that branch.
                        //
                        // Stepping stone: the true runtime settle path
                        // (tick-loop hook that flips Pending→Due and writes
                        // Provisional as individual horizons mature) still
                        // needs the new horizon_evaluation_due_before query
                        // + per-horizon return computation. This change
                        // alone removes the silent bug flagged in memory
                        // without introducing fake per-horizon outcomes.
                        let has_supplementals = !settled.supplementals.is_empty();
                        if has_supplementals {
                            if let Err(e) = upsert_case_resolution_for_setup(
                                store,
                                &settled.setup_id,
                                &settled.market,
                                settled.symbol.as_deref(),
                                settled.primary_horizon,
                                &settled.primary_resolution,
                                &settled.primary_result,
                                &[],
                                false, // Provisional — primary only, gate fires on step 2
                                settled.primary_horizon,
                                settled.at,
                                "primary_horizon_settled",
                                &intent_kind,
                                &signature,
                            )
                            .await
                            {
                                eprintln!(
                                    "[resolution] provisional upsert failed for {}: {e}",
                                    settled.setup_id
                                );
                            }
                        }
                        if let Err(e) = upsert_case_resolution_for_setup(
                            store,
                            &settled.setup_id,
                            &settled.market,
                            settled.symbol.as_deref(),
                            settled.primary_horizon,
                            &settled.primary_resolution,
                            &settled.primary_result,
                            &settled.supplementals,
                            true, // Final — all horizons settled atomically
                            settled.primary_horizon,
                            settled.at,
                            "realized_outcome_settled",
                            &intent_kind,
                            &signature,
                        )
                        .await
                        {
                            eprintln!(
                                "[resolution] upsert_case_resolution_for_setup failed for {}: {e}",
                                settled.setup_id
                            );
                        }
                    }
                    Ok(None) => {
                        // No horizon records for this setup (predates Horizon Wave 3) — skip.
                    }
                    Err(e) => {
                        eprintln!(
                            "[resolution] settle_horizon_records_from_realized_outcome failed for {}: {e}",
                            outcome.setup_id
                        );
                    }
                }
            }
        }

        // Auto-generate assessments from realized outcomes to feed doctrine pressure
        let mut auto_assessments =
            crate::persistence::case_reasoning_assessment::auto_assessments_from_outcomes(&records);
        let setup_ids = records
            .iter()
            .map(|record| record.setup_id.clone())
            .collect::<Vec<_>>();
        if let Some(ref store) = self.store {
            if let Ok(setup_records) = store.tactical_setups_by_ids(&setup_ids).await {
                crate::persistence::case_reasoning_assessment::apply_setup_records_to_assessments(
                    &mut auto_assessments,
                    &setup_records,
                );
            }
        }
        let market_key: &'static str = match records.first().map(|record| record.market.as_str()) {
            Some("us") => "us",
            _ => "hk",
        };
        let realized_version = records
            .iter()
            .map(|record| record.resolved_at.unix_timestamp_nanos())
            .max()
            .unwrap_or(0);
        if let Some(ref store) = self.store {
            let mut assessments_for_archetypes = store
                .recent_case_reasoning_assessments_by_market(market_key, 500)
                .await
                .unwrap_or_default();
            assessments_for_archetypes.extend(auto_assessments.iter().cloned());
            let archetypes = crate::persistence::discovered_archetype::build_discovered_archetypes(
                market_key,
                &assessments_for_archetypes,
                &records,
                time::OffsetDateTime::now_utc(),
            );
            if !archetypes.is_empty() {
                self.persist_discovered_archetypes(market_key, archetypes, realized_version)
                    .await;
            }
        }
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
        self.persist_case_realized_outcomes_rows(market_key, records, realized_version)
            .await;
    }

    #[cfg(feature = "persistence")]
    pub async fn persist_projection_followups(
        &self,
        market: CaseMarket,
        knowledge_bundle: KnowledgePersistenceBundle,
        cases: &[CaseSummary],
        setups: &[TacticalSetup],
        hypotheses: &[Hypothesis],
        recorded_at: time::OffsetDateTime,
        source: &'static str,
        realized_outcomes: Option<Vec<CaseRealizedOutcomeRecord>>,
    ) {
        use std::sync::atomic::{AtomicU64, Ordering};
        static BREAKDOWN_COUNTER: AtomicU64 = AtomicU64::new(0);
        let breakdown_count = BREAKDOWN_COUNTER.fetch_add(1, Ordering::Relaxed);
        let log_breakdown = breakdown_count.is_multiple_of(10);

        let t0 = Instant::now();
        self.persist_knowledge_projection(knowledge_bundle).await;
        let t_kb = t0.elapsed();

        let t1 = Instant::now();
        if !setups.is_empty() {
            let hypothesis_by_id = hypotheses
                .iter()
                .map(|hypothesis| (hypothesis.hypothesis_id.as_str(), hypothesis))
                .collect::<std::collections::HashMap<_, _>>();
            let setup_records = setups
                .iter()
                .map(|setup| {
                    let hypothesis = hypothesis_by_id.get(setup.hypothesis_id.as_str()).copied();
                    crate::persistence::tactical_setup::TacticalSetupRecord::from_setup_with_hypothesis(
                        setup,
                        hypothesis,
                        recorded_at,
                    )
                })
                .collect::<Vec<_>>();
            self.persist_tactical_setups(market, setup_records).await;
            // Wave 3 Task 15: write pending horizon evaluation records at case open
            self.persist_horizon_evaluations(market, setups, recorded_at)
                .await;
        }
        let t_setups_horizons = t1.elapsed();

        let t2 = Instant::now();
        self.persist_case_reasoning_assessments_for_cases(
            market,
            cases,
            hypotheses,
            setups,
            recorded_at,
            source,
        )
        .await;
        let t_assessments = t2.elapsed();

        let t3 = Instant::now();
        let outcome_count = realized_outcomes.as_ref().map(|v| v.len()).unwrap_or(0);
        if let Some(records) = realized_outcomes {
            self.persist_hk_case_realized_outcomes(records).await;
        }
        let t_outcomes = t3.elapsed();

        if log_breakdown {
            eprintln!(
                "[persist_followups_breakdown] kb={}ms setups+horizons={}ms assessments={}ms outcomes={}ms (n_setups={} n_cases={} n_outcomes={})",
                t_kb.as_millis(),
                t_setups_horizons.as_millis(),
                t_assessments.as_millis(),
                t_outcomes.as_millis(),
                setups.len(),
                cases.len(),
                outcome_count,
            );
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
        use std::sync::atomic::{AtomicU64, Ordering};
        static OUTER_BREAKDOWN_COUNTER: AtomicU64 = AtomicU64::new(0);
        let outer_count = OUTER_BREAKDOWN_COUNTER.fetch_add(1, Ordering::Relaxed);
        let log_outer = outer_count.is_multiple_of(10);

        let t_build = Instant::now();
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
        let build_ms = t_build.elapsed().as_millis();

        let t_persist = Instant::now();
        self.persist_projection_followups(
            market,
            knowledge_bundle,
            cases,
            setups,
            hypotheses,
            recorded_at,
            source,
            realized_outcomes,
        )
        .await;
        let persist_ms = t_persist.elapsed().as_millis();

        if log_outer {
            eprintln!(
                "[persist_followups_outer] build={build_ms}ms persist={persist_ms}ms \
                 (n_macro_events={} n_decisions={} n_hyp={} n_setups={} n_cases={} \
                  n_snap_links={} n_rec_links={} n_active_pos={})",
                macro_events.len(),
                decisions.len(),
                hypotheses.len(),
                setups.len(),
                cases.len(),
                snapshot_knowledge_links.len(),
                recommendation_knowledge_links.len(),
                active_positions.len(),
            );
        }
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
        mut stage_timer: Option<&mut crate::core::runtime::TickStageTimer>,
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
        if let Some(ref mut timer) = stage_timer {
            timer.mark("S21b3a_publish_projection");
        }
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
        if let Some(timer) = stage_timer {
            timer.mark("S21b3b_persist_followups");
        }
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

/// Wave 3 Task 15: free helper — build pending horizon evaluation records for a single setup.
/// Used by `persist_horizon_evaluations` and available for unit testing without the full runtime.
#[cfg(feature = "persistence")]
pub fn horizon_records_for_setup(
    setup: &TacticalSetup,
    market: &str,
    now: time::OffsetDateTime,
) -> Vec<crate::persistence::horizon_evaluation::HorizonEvaluationRecord> {
    crate::persistence::horizon_evaluation::HorizonEvaluationRecord::pending_for_case(
        &setup.setup_id,
        market,
        &setup.horizon,
        now,
    )
}

/// Outcome of `settle_horizon_records_from_realized_outcome`: the settled primary
/// and supplemental horizons ready for `upsert_case_resolution_for_setup`.
#[cfg(feature = "persistence")]
pub struct SettledHorizons {
    pub setup_id: String,
    pub market: String,
    pub symbol: Option<String>,
    pub primary_horizon: crate::ontology::horizon::HorizonBucket,
    pub primary_resolution: crate::ontology::resolution::HorizonResolution,
    pub primary_result: crate::persistence::horizon_evaluation::HorizonResult,
    pub supplementals: Vec<(
        crate::ontology::horizon::HorizonBucket,
        crate::ontology::resolution::HorizonResolution,
        crate::persistence::horizon_evaluation::HorizonResult,
    )>,
    pub at: time::OffsetDateTime,
}

/// Settle all pending horizon evaluation records for the setup referenced by a
/// `CaseRealizedOutcomeRecord`, then return the settled data ready for
/// `upsert_case_resolution_for_setup`.
///
/// # Simplification
/// `all_settled = true` is used on the first pass because a realized-outcome
/// event resolves the case atomically. Future iterations can add per-horizon
/// progressive settlement driven by `due_at` tracking.
///
/// Runtime live-settle step 1 (audit Finding 1, 2026-04-19): sweep all
/// Pending horizons whose `due_at` has passed and flip them to `Due`.
///
/// Before this existed, `horizon_evaluation` records stayed Pending
/// indefinitely in live sessions — `settle_horizon_records_from_realized_outcome`
/// only ran on realized-outcome persistence, so operator-visible
/// horizon breakdown was stale between open and realization.
///
/// This sweep is intentionally conservative: Pending → Due only (no
/// result computation). The Due → Resolved upgrade needs per-horizon
/// net_return from price data, which requires deeper integration with
/// tick-loop mark prices. That remains a follow-up spec. EarlyExited
/// on exit-signal is also a follow-up — this sweep doesn't touch
/// non-Pending records.
///
/// Returns the number of records flipped. Graceful: any error is
/// logged and produces Ok(0) so the caller (tick loop) never aborts.
#[cfg(feature = "persistence")]
pub async fn sweep_pending_horizons_to_due(
    store: &crate::persistence::store::EdenStore,
    now: time::OffsetDateTime,
) -> usize {
    use crate::persistence::horizon_evaluation::EvaluationStatus;

    let mut records = match store.pending_horizons_past_due(now).await {
        Ok(r) => r,
        Err(e) => {
            eprintln!("[horizon] sweep query failed: {e}");
            return 0;
        }
    };
    if records.is_empty() {
        return 0;
    }
    for record in &mut records {
        record.status = EvaluationStatus::Due;
    }
    let n = records.len();
    if let Err(e) = store.write_horizon_evaluations(&records).await {
        eprintln!("[horizon] sweep write failed: {e}");
        return 0;
    }
    eprintln!("[horizon] swept {} pending → due (now={})", n, now);
    n
}

#[cfg(feature = "persistence")]
fn case_signature_key(signature: Option<&crate::ontology::CaseSignature>) -> String {
    signature
        .map(|signature| {
            format!(
                "{}:{}:{}",
                format!("{:?}", signature.topology).to_ascii_lowercase(),
                format!("{:?}", signature.temporal_shape).to_ascii_lowercase(),
                format!("{:?}", signature.conflict_shape).to_ascii_lowercase(),
            )
        })
        .unwrap_or_default()
}

#[cfg(feature = "persistence")]
fn setup_scope_symbol(setup: &crate::ontology::TacticalSetup) -> Option<String> {
    match &setup.scope {
        crate::ontology::ReasoningScope::Symbol(symbol) => Some(symbol.0.clone()),
        _ => None,
    }
}

#[cfg(feature = "persistence")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SettleContextSource {
    InMemory,
    Persisted,
}

#[cfg(feature = "persistence")]
fn select_settle_entry_context<T: Clone>(
    setup_id: &str,
    in_memory: &std::collections::HashMap<String, T>,
    persisted: &std::collections::HashMap<String, T>,
) -> Option<(T, SettleContextSource)> {
    in_memory
        .get(setup_id)
        .cloned()
        .map(|value| (value, SettleContextSource::InMemory))
        .or_else(|| {
            persisted
                .get(setup_id)
                .cloned()
                .map(|value| (value, SettleContextSource::Persisted))
        })
}

#[cfg(feature = "persistence")]
fn hk_entry_contexts_from_history(
    history: &crate::temporal::buffer::TickHistory,
) -> std::collections::HashMap<
    String,
    (
        crate::ontology::TacticalSetup,
        Option<crate::ontology::Hypothesis>,
    ),
> {
    let mut entry_context_by_id = std::collections::HashMap::<
        String,
        (
            crate::ontology::TacticalSetup,
            Option<crate::ontology::Hypothesis>,
        ),
    >::new();
    for record in history.latest_n(history.len()) {
        for setup in &record.tactical_setups {
            entry_context_by_id
                .entry(setup.setup_id.clone())
                .or_insert_with(|| {
                    let hypothesis = record
                        .hypotheses
                        .iter()
                        .find(|item| item.hypothesis_id == setup.hypothesis_id)
                        .cloned();
                    (setup.clone(), hypothesis)
                });
        }
    }
    entry_context_by_id
}

#[cfg(feature = "persistence")]
fn us_entry_contexts_from_history(
    history: &crate::us::temporal::buffer::UsTickHistory,
) -> std::collections::HashMap<
    String,
    (
        crate::ontology::TacticalSetup,
        Option<crate::ontology::Hypothesis>,
    ),
> {
    let mut entry_context_by_id = std::collections::HashMap::<
        String,
        (
            crate::ontology::TacticalSetup,
            Option<crate::ontology::Hypothesis>,
        ),
    >::new();
    for record in history.latest_n(history.len()) {
        for setup in &record.tactical_setups {
            entry_context_by_id
                .entry(setup.setup_id.clone())
                .or_insert_with(|| {
                    let hypothesis = record
                        .hypotheses
                        .iter()
                        .find(|item| item.hypothesis_id == setup.hypothesis_id)
                        .cloned();
                    (setup.clone(), hypothesis)
                });
        }
    }
    entry_context_by_id
}

#[cfg(feature = "persistence")]
async fn load_recent_hk_history_from_store(
    store: &crate::persistence::store::EdenStore,
    limit: usize,
) -> Option<crate::temporal::buffer::TickHistory> {
    let records = match store.recent_tick_window(limit).await {
        Ok(records) => records,
        Err(error) => {
            eprintln!("[horizon][hk] fallback tick-window query failed: {error}");
            return None;
        }
    };
    if records.is_empty() {
        return None;
    }
    let mut history = crate::temporal::buffer::TickHistory::new(records.len().max(1));
    for record in records {
        history.push(record);
    }
    Some(history)
}

#[cfg(feature = "persistence")]
async fn load_recent_us_history_from_store(
    store: &crate::persistence::store::EdenStore,
    limit: usize,
) -> Option<crate::us::temporal::buffer::UsTickHistory> {
    let records = match store.recent_us_tick_window(limit).await {
        Ok(records) => records,
        Err(error) => {
            eprintln!("[horizon][us] fallback tick-window query failed: {error}");
            return None;
        }
    };
    if records.is_empty() {
        return None;
    }
    let mut history = crate::us::temporal::buffer::UsTickHistory::new(records.len().max(1));
    for record in records {
        history.push(record);
    }
    Some(history)
}

#[cfg(feature = "persistence")]
pub async fn settle_live_horizons_hk(
    store: &crate::persistence::store::EdenStore,
    history: &crate::temporal::buffer::TickHistory,
    setups: &[crate::ontology::TacticalSetup],
    hypotheses: &[crate::ontology::Hypothesis],
    now: time::OffsetDateTime,
) -> usize {
    use crate::persistence::horizon_evaluation::{
        settle_horizon_evaluation, EvaluationStatus, HorizonResult,
    };

    let unresolved = match store.unresolved_horizons_for_market("hk", 512).await {
        Ok(records) => records,
        Err(error) => {
            eprintln!("[horizon][hk] unresolved query failed: {error}");
            return 0;
        }
    };
    if unresolved.is_empty() {
        return 0;
    }

    let setup_by_id = setups
        .iter()
        .map(|setup| (setup.setup_id.as_str(), setup))
        .collect::<std::collections::HashMap<_, _>>();
    let hypothesis_by_id = hypotheses
        .iter()
        .map(|hypothesis| (hypothesis.hypothesis_id.as_str(), hypothesis))
        .collect::<std::collections::HashMap<_, _>>();
    let entry_context_by_id = hk_entry_contexts_from_history(history);

    let mut grouped = std::collections::BTreeMap::<
        String,
        Vec<crate::persistence::horizon_evaluation::HorizonEvaluationRecord>,
    >::new();
    for record in unresolved {
        grouped
            .entry(record.setup_id.clone())
            .or_default()
            .push(record);
    }

    let needs_persisted_fallback = grouped
        .keys()
        .any(|setup_id| !entry_context_by_id.contains_key(setup_id));
    let (persisted_history, persisted_entry_context_by_id) = if needs_persisted_fallback {
        if let Some(history) = load_recent_hk_history_from_store(store, 8_192).await {
            let contexts = hk_entry_contexts_from_history(&history);
            (Some(history), contexts)
        } else {
            (None, std::collections::HashMap::new())
        }
    } else {
        (None, std::collections::HashMap::new())
    };

    let mut settled_count = 0usize;
    for (setup_id, records) in &mut grouped {
        let Some(((entry_setup, entry_hypothesis), context_source)) = select_settle_entry_context(
            setup_id,
            &entry_context_by_id,
            &persisted_entry_context_by_id,
        ) else {
            // Entry context missing (see US twin for rationale —
            // applies symmetrically to HK orphaned setup_ids).
            let newly_expired =
                crate::persistence::horizon_evaluation::decrement_attempts_or_expire(records);
            if newly_expired > 0 {
                eprintln!(
                    "[horizon][hk] expired {} horizons for {} after {} settle attempts \
                     (entry context never recovered)",
                    newly_expired,
                    setup_id,
                    crate::persistence::horizon_evaluation::DEFAULT_SETTLE_ATTEMPTS,
                );
            }
            if let Err(error) = store.write_horizon_evaluations(records).await {
                eprintln!(
                    "[horizon][hk] failed to persist attempts decrement for {setup_id}: {error}"
                );
            }
            continue;
        };
        let outcome_history = match context_source {
            SettleContextSource::InMemory => history,
            SettleContextSource::Persisted => persisted_history
                .as_ref()
                .expect("persisted history loaded when persisted context selected"),
        };
        let current_setup = setup_by_id.get(setup_id.as_str()).copied();
        let current_hypothesis = current_setup
            .and_then(|setup| hypothesis_by_id.get(setup.hypothesis_id.as_str()).copied());
        let metadata_setup = current_setup.cloned().unwrap_or(entry_setup);
        let metadata_hypothesis = current_hypothesis.cloned().or(entry_hypothesis);
        let intent = metadata_setup.intent_hypothesis(metadata_hypothesis.as_ref());
        let exit_signal = intent.exit_signals.first().cloned();
        let violations = intent.expectation_violations.clone();
        let mut changed = false;
        let mut triggered_by = None;
        let mut reason = None;

        if let Some(exit_signal) = exit_signal {
            if records.iter().any(|record| {
                matches!(
                    record.status,
                    EvaluationStatus::Pending | EvaluationStatus::Due
                )
            }) {
                if let Some(outcome) = crate::temporal::lineage::evaluate_case_outcome_until(
                    outcome_history,
                    setup_id,
                    now,
                ) {
                    let result = HorizonResult {
                        net_return: outcome.net_return,
                        resolved_at: outcome.resolved_at,
                        follow_through: if outcome.max_favorable_excursion
                            > rust_decimal_macros::dec!(0.003)
                        {
                            rust_decimal::Decimal::ONE
                        } else {
                            rust_decimal::Decimal::ZERO
                        },
                    };
                    for record in records.iter_mut().filter(|record| {
                        matches!(
                            record.status,
                            EvaluationStatus::Pending | EvaluationStatus::Due
                        )
                    }) {
                        settle_horizon_evaluation(
                            record,
                            result.clone(),
                            Some(exit_signal.kind),
                            &violations,
                            EvaluationStatus::EarlyExited,
                        );
                        settled_count += 1;
                        changed = true;
                        triggered_by = Some(record.horizon);
                    }
                    reason =
                        Some(format!("exit_signal_{:?}", exit_signal.kind).to_ascii_lowercase());
                }
            }
        } else {
            for record in records.iter_mut().filter(|record| {
                matches!(
                    record.status,
                    EvaluationStatus::Pending | EvaluationStatus::Due
                ) && record.due_at <= now
            }) {
                if let Some(outcome) = crate::temporal::lineage::evaluate_case_outcome_until(
                    outcome_history,
                    setup_id,
                    record.due_at,
                ) {
                    let result = HorizonResult {
                        net_return: outcome.net_return,
                        resolved_at: outcome.resolved_at,
                        follow_through: if outcome.max_favorable_excursion
                            > rust_decimal_macros::dec!(0.003)
                        {
                            rust_decimal::Decimal::ONE
                        } else {
                            rust_decimal::Decimal::ZERO
                        },
                    };
                    settle_horizon_evaluation(
                        record,
                        result,
                        None,
                        &violations,
                        EvaluationStatus::Resolved,
                    );
                    settled_count += 1;
                    changed = true;
                    triggered_by = Some(record.horizon);
                    reason = Some("due_at_reached".to_string());
                }
            }
        }

        if !changed {
            continue;
        }
        if let Err(error) = store.write_horizon_evaluations(records).await {
            eprintln!("[horizon][hk] write failed for {}: {}", setup_id, error);
            continue;
        }

        let Some(primary_record) = records.iter().find(|record| {
            record.primary
                && matches!(
                    record.status,
                    EvaluationStatus::Resolved | EvaluationStatus::EarlyExited
                )
        }) else {
            continue;
        };
        let Some(primary_resolution) = primary_record.resolution.clone() else {
            continue;
        };
        let Some(primary_result) = primary_record.result.clone() else {
            continue;
        };
        let supplementals = records
            .iter()
            .filter(|record| !record.primary)
            .filter_map(|record| {
                let resolution = record.resolution.clone()?;
                let result = record.result.clone()?;
                Some((record.horizon, resolution, result))
            })
            .collect::<Vec<_>>();
        let all_settled = records.iter().all(|record| {
            matches!(
                record.status,
                EvaluationStatus::Resolved | EvaluationStatus::EarlyExited
            )
        });
        let symbol = setup_scope_symbol(&metadata_setup);
        let intent_kind = format!("{:?}", intent.kind).to_ascii_lowercase();
        let derived_signature = metadata_setup.case_signature(metadata_hypothesis.as_ref());
        let signature = case_signature_key(Some(
            intent
                .supporting_case_signature
                .as_ref()
                .unwrap_or(&derived_signature),
        ));
        if let Err(error) = upsert_case_resolution_for_setup(
            store,
            setup_id,
            "hk",
            symbol.as_deref(),
            primary_record.horizon,
            &primary_resolution,
            &primary_result,
            &supplementals,
            all_settled,
            triggered_by.unwrap_or(primary_record.horizon),
            primary_result.resolved_at,
            reason.as_deref().unwrap_or("live_horizon_settled"),
            &intent_kind,
            &signature,
        )
        .await
        {
            eprintln!(
                "[horizon][hk] case resolution upsert failed for {}: {}",
                setup_id, error
            );
        }
    }

    settled_count
}

#[cfg(feature = "persistence")]
pub async fn settle_live_horizons_us(
    store: &crate::persistence::store::EdenStore,
    history: &crate::us::temporal::buffer::UsTickHistory,
    setups: &[crate::ontology::TacticalSetup],
    hypotheses: &[crate::ontology::Hypothesis],
    now: time::OffsetDateTime,
) -> usize {
    use crate::persistence::horizon_evaluation::{
        settle_horizon_evaluation, EvaluationStatus, HorizonResult,
    };

    let unresolved = match store.unresolved_horizons_for_market("us", 512).await {
        Ok(records) => records,
        Err(error) => {
            eprintln!("[horizon][us] unresolved query failed: {error}");
            return 0;
        }
    };
    if unresolved.is_empty() {
        return 0;
    }

    let setup_by_id = setups
        .iter()
        .map(|setup| (setup.setup_id.as_str(), setup))
        .collect::<std::collections::HashMap<_, _>>();
    let hypothesis_by_id = hypotheses
        .iter()
        .map(|hypothesis| (hypothesis.hypothesis_id.as_str(), hypothesis))
        .collect::<std::collections::HashMap<_, _>>();
    let entry_context_by_id = us_entry_contexts_from_history(history);

    let mut grouped = std::collections::BTreeMap::<
        String,
        Vec<crate::persistence::horizon_evaluation::HorizonEvaluationRecord>,
    >::new();
    for record in unresolved {
        grouped
            .entry(record.setup_id.clone())
            .or_default()
            .push(record);
    }

    let needs_persisted_fallback = grouped
        .keys()
        .any(|setup_id| !entry_context_by_id.contains_key(setup_id));
    let (persisted_history, persisted_entry_context_by_id) = if needs_persisted_fallback {
        if let Some(history) = load_recent_us_history_from_store(store, 8_192).await {
            let contexts = us_entry_contexts_from_history(&history);
            (Some(history), contexts)
        } else {
            (None, std::collections::HashMap::new())
        }
    } else {
        (None, std::collections::HashMap::new())
    };

    let mut settled_count = 0usize;
    for (setup_id, records) in &mut grouped {
        let Some(((entry_setup, entry_hypothesis), context_source)) = select_settle_entry_context(
            setup_id,
            &entry_context_by_id,
            &persisted_entry_context_by_id,
        ) else {
            // Entry context missing — typical for `pf:*` setups whose
            // hour-direction flipped, leaving the original setup_id
            // orphaned. Decrement the per-record attempts counter and
            // expire any that have run out. Logging only on transitions
            // — see the 4 154 / session log spam observed pre-fix.
            let newly_expired =
                crate::persistence::horizon_evaluation::decrement_attempts_or_expire(records);
            if newly_expired > 0 {
                eprintln!(
                    "[horizon][us] expired {} horizons for {} after {} settle attempts \
                     (entry context never recovered)",
                    newly_expired,
                    setup_id,
                    crate::persistence::horizon_evaluation::DEFAULT_SETTLE_ATTEMPTS,
                );
            }
            if let Err(error) = store.write_horizon_evaluations(records).await {
                eprintln!(
                    "[horizon][us] failed to persist attempts decrement for {setup_id}: {error}"
                );
            }
            continue;
        };
        let outcome_history = match context_source {
            SettleContextSource::InMemory => history,
            SettleContextSource::Persisted => persisted_history
                .as_ref()
                .expect("persisted history loaded when persisted context selected"),
        };
        let current_setup = setup_by_id.get(setup_id.as_str()).copied();
        let current_hypothesis = current_setup
            .and_then(|setup| hypothesis_by_id.get(setup.hypothesis_id.as_str()).copied());
        let metadata_setup = current_setup.cloned().unwrap_or(entry_setup);
        let metadata_hypothesis = current_hypothesis.cloned().or(entry_hypothesis);
        let intent = metadata_setup.intent_hypothesis(metadata_hypothesis.as_ref());
        let exit_signal = intent.exit_signals.first().cloned();
        let violations = intent.expectation_violations.clone();
        let mut changed = false;
        let mut triggered_by = None;
        let mut reason = None;

        if let Some(exit_signal) = exit_signal {
            if records.iter().any(|record| {
                matches!(
                    record.status,
                    EvaluationStatus::Pending | EvaluationStatus::Due
                )
            }) {
                if let Some(outcome) = crate::us::temporal::outcomes::evaluate_us_case_outcome_until(
                    outcome_history,
                    setup_id,
                    now,
                ) {
                    let result = HorizonResult {
                        net_return: outcome.net_return,
                        resolved_at: outcome.resolved_at,
                        follow_through: if outcome.max_favorable_excursion
                            > rust_decimal_macros::dec!(0.003)
                        {
                            rust_decimal::Decimal::ONE
                        } else {
                            rust_decimal::Decimal::ZERO
                        },
                    };
                    for record in records.iter_mut().filter(|record| {
                        matches!(
                            record.status,
                            EvaluationStatus::Pending | EvaluationStatus::Due
                        )
                    }) {
                        settle_horizon_evaluation(
                            record,
                            result.clone(),
                            Some(exit_signal.kind),
                            &violations,
                            EvaluationStatus::EarlyExited,
                        );
                        settled_count += 1;
                        changed = true;
                        triggered_by = Some(record.horizon);
                    }
                    reason =
                        Some(format!("exit_signal_{:?}", exit_signal.kind).to_ascii_lowercase());
                }
            }
        } else {
            for record in records.iter_mut().filter(|record| {
                matches!(
                    record.status,
                    EvaluationStatus::Pending | EvaluationStatus::Due
                ) && record.due_at <= now
            }) {
                if let Some(outcome) = crate::us::temporal::outcomes::evaluate_us_case_outcome_until(
                    outcome_history,
                    setup_id,
                    record.due_at,
                ) {
                    let result = HorizonResult {
                        net_return: outcome.net_return,
                        resolved_at: outcome.resolved_at,
                        follow_through: if outcome.max_favorable_excursion
                            > rust_decimal_macros::dec!(0.003)
                        {
                            rust_decimal::Decimal::ONE
                        } else {
                            rust_decimal::Decimal::ZERO
                        },
                    };
                    settle_horizon_evaluation(
                        record,
                        result,
                        None,
                        &violations,
                        EvaluationStatus::Resolved,
                    );
                    settled_count += 1;
                    changed = true;
                    triggered_by = Some(record.horizon);
                    reason = Some("due_at_reached".to_string());
                }
            }
        }

        if !changed {
            continue;
        }
        if let Err(error) = store.write_horizon_evaluations(records).await {
            eprintln!("[horizon][us] write failed for {}: {}", setup_id, error);
            continue;
        }

        let Some(primary_record) = records.iter().find(|record| {
            record.primary
                && matches!(
                    record.status,
                    EvaluationStatus::Resolved | EvaluationStatus::EarlyExited
                )
        }) else {
            continue;
        };
        let Some(primary_resolution) = primary_record.resolution.clone() else {
            continue;
        };
        let Some(primary_result) = primary_record.result.clone() else {
            continue;
        };
        let supplementals = records
            .iter()
            .filter(|record| !record.primary)
            .filter_map(|record| {
                let resolution = record.resolution.clone()?;
                let result = record.result.clone()?;
                Some((record.horizon, resolution, result))
            })
            .collect::<Vec<_>>();
        let all_settled = records.iter().all(|record| {
            matches!(
                record.status,
                EvaluationStatus::Resolved | EvaluationStatus::EarlyExited
            )
        });
        let symbol = setup_scope_symbol(&metadata_setup);
        let intent_kind = format!("{:?}", intent.kind).to_ascii_lowercase();
        let derived_signature = metadata_setup.case_signature(metadata_hypothesis.as_ref());
        let signature = case_signature_key(Some(
            intent
                .supporting_case_signature
                .as_ref()
                .unwrap_or(&derived_signature),
        ));
        if let Err(error) = upsert_case_resolution_for_setup(
            store,
            setup_id,
            "us",
            symbol.as_deref(),
            primary_record.horizon,
            &primary_resolution,
            &primary_result,
            &supplementals,
            all_settled,
            triggered_by.unwrap_or(primary_record.horizon),
            primary_result.resolved_at,
            reason.as_deref().unwrap_or("live_horizon_settled"),
            &intent_kind,
            &signature,
        )
        .await
        {
            eprintln!(
                "[horizon][us] case resolution upsert failed for {}: {}",
                setup_id, error
            );
        }
    }

    settled_count
}

/// Returns `Ok(None)` when no horizon records exist for the setup (e.g. records
/// that predate Horizon Wave 3). In that case the caller should skip resolution.
#[cfg(feature = "persistence")]
pub async fn settle_horizon_records_from_realized_outcome(
    store: &crate::persistence::store::EdenStore,
    outcome: &crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord,
) -> Result<Option<SettledHorizons>, Box<dyn std::error::Error + Send + Sync>> {
    use crate::persistence::horizon_evaluation::{
        settle_horizon_evaluation, EvaluationStatus, HorizonResult,
    };

    let mut records = store
        .load_horizon_evaluations_for_setup(&outcome.setup_id)
        .await?;

    if records.is_empty() {
        return Ok(None);
    }

    // Build a HorizonResult from the realized outcome. `follow_through` is
    // mapped as a continuous proxy: 1.0 for follow-through, 0.0 otherwise.
    // `max_favorable_excursion / max_adverse_excursion` are not yet available
    // as a ratio in this bridging period, so the bool mapping is used.
    let follow_through = if outcome.followed_through {
        rust_decimal_macros::dec!(1.0)
    } else {
        rust_decimal_macros::dec!(0.0)
    };
    let result_template = HorizonResult {
        net_return: outcome.net_return,
        resolved_at: outcome.resolved_at,
        follow_through,
    };

    // Settle every Pending or Due record. Records already Resolved/EarlyExited
    // are left untouched to preserve any earlier settlement (defensive).
    for record in &mut records {
        if record.status == EvaluationStatus::Pending || record.status == EvaluationStatus::Due {
            settle_horizon_evaluation(
                record,
                result_template.clone(),
                None,
                &[],
                EvaluationStatus::Resolved,
            );
        }
    }

    // Persist the settled records back.
    store.write_horizon_evaluations(&records).await?;

    eprintln!(
        "[resolution][bootstrap] settled {} horizons for setup {} (all_settled=true)",
        records.len(),
        outcome.setup_id
    );

    // Split into primary and supplementals.
    let primary_idx = records.iter().position(|r| r.primary).unwrap_or(0);
    let primary = &records[primary_idx];
    let primary_resolution = primary
        .resolution
        .clone()
        .expect("settle_horizon_evaluation always sets resolution");
    let primary_result = primary
        .result
        .clone()
        .expect("settle_horizon_evaluation always sets result");

    let supplementals = records
        .iter()
        .enumerate()
        .filter(|(i, _)| *i != primary_idx)
        .filter_map(|(_, r)| {
            let res = r.resolution.clone()?;
            let result = r.result.clone()?;
            Some((r.horizon, res, result))
        })
        .collect();

    Ok(Some(SettledHorizons {
        setup_id: outcome.setup_id.clone(),
        market: outcome.market.clone(),
        symbol: outcome.symbol.clone(),
        primary_horizon: primary.horizon,
        primary_resolution,
        primary_result,
        supplementals,
        at: outcome.resolved_at,
    }))
}

/// Wave 3 Task 14: load-aggregate-upgrade-write helper for case resolution.
///
/// Called after each horizon settles. On the first settle it creates a fresh
/// `CaseResolutionRecord`; on subsequent settles it runs `apply_case_resolution_update`
/// through the upgrade gate and writes only when something changed.
///
/// Feature-gated: production callers must live behind `#[cfg(feature = "persistence")]`.
///
/// Currently wired via the **bootstrap path** in `persist_hk_case_realized_outcomes` →
/// `settle_horizon_records_from_realized_outcome`. That path always sets
/// `all_settled=true` and never exercises the Provisional → Final upgrade gate.
///
/// TODO(runtime settle path): wire due-at / early-exit settle into the
/// tick loop. The realized-outcome bootstrap path is a fallback that writes
/// resolution records after the fact (always all_settled=true, never
/// exercises the Provisional -> Final upgrade gate). The true runtime
/// settle path must:
///   - flip Pending -> Due when reference window reached
///   - run settle_horizon_evaluation(...) for each maturing horizon individually
///   - call upsert_case_resolution_for_setup progressively (primary first,
///     supplementals later) so apply_case_resolution_update actually
///     exercises the upgrade gate and refinement-to-Final rule
///   - handle early-exit triggers (intent exit signals) without waiting
///     for due_at
#[cfg(feature = "persistence")]
pub async fn upsert_case_resolution_for_setup(
    store: &crate::persistence::store::EdenStore,
    setup_id: &str,
    market: &str,
    symbol: Option<&str>,
    primary_horizon: crate::ontology::horizon::HorizonBucket,
    primary: &crate::ontology::resolution::HorizonResolution,
    primary_result: &crate::persistence::horizon_evaluation::HorizonResult,
    supplementals: &[(
        crate::ontology::horizon::HorizonBucket,
        crate::ontology::resolution::HorizonResolution,
        crate::persistence::horizon_evaluation::HorizonResult,
    )],
    all_settled: bool,
    triggered_by: crate::ontology::horizon::HorizonBucket,
    at: time::OffsetDateTime,
    reason: &str,
    // intent_kind: from TacticalSetup.inferred_intent.kind, empty string when unavailable.
    intent_kind: &str,
    // signature: case signature string, empty string when unavailable.
    signature: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use crate::ontology::resolution::{
        aggregate_case_resolution, apply_case_resolution_update,
        initial_case_resolution_transition, ResolutionSource, ResolutionUpdate, UpdateOutcome,
    };
    use crate::persistence::case_resolution::CaseResolutionRecord;

    let new_resolution =
        aggregate_case_resolution(primary, supplementals, primary_result, all_settled);

    let existing = store.load_case_resolution_for_setup(setup_id).await?;

    let record = match existing {
        None => {
            let initial_transition = initial_case_resolution_transition(
                &new_resolution,
                triggered_by,
                at,
                reason.to_string(),
            );
            let mut snapshot = Vec::with_capacity(1 + supplementals.len());
            snapshot.push(primary.clone());
            for (_, r, _) in supplementals {
                snapshot.push(r.clone());
            }
            CaseResolutionRecord {
                record_id: CaseResolutionRecord::build_id(setup_id),
                setup_id: setup_id.to_string(),
                market: market.to_string(),
                symbol: symbol.map(|s| s.to_string()),
                primary_horizon,
                resolution: new_resolution,
                resolution_source: ResolutionSource::Auto,
                horizon_resolution_snapshot: snapshot,
                resolution_history: vec![initial_transition],
                created_at: at,
                updated_at: at,
                intent_kind: intent_kind.to_string(),
                signature: signature.to_string(),
            }
        }
        Some(mut existing) => {
            let update = ResolutionUpdate {
                new_resolution: new_resolution.clone(),
                triggered_by_horizon: triggered_by,
                at,
                reason: reason.to_string(),
            };
            match apply_case_resolution_update(
                &mut existing.resolution,
                &mut existing.resolution_history,
                update,
            ) {
                UpdateOutcome::Applied => {
                    existing.horizon_resolution_snapshot.clear();
                    existing.horizon_resolution_snapshot.push(primary.clone());
                    for (_, r, _) in supplementals {
                        existing.horizon_resolution_snapshot.push(r.clone());
                    }
                    existing.updated_at = at;
                    // Update intent_kind / signature if new values are non-empty
                    // (they may not be present on old records).
                    if !intent_kind.is_empty() {
                        existing.intent_kind = intent_kind.to_string();
                    }
                    if !signature.is_empty() {
                        existing.signature = signature.to_string();
                    }
                }
                UpdateOutcome::NoChange => return Ok(()),
                UpdateOutcome::RejectedFinal => {
                    eprintln!("[resolution] upgrade rejected for {setup_id}: resolution is Final");
                    return Ok(());
                }
                UpdateOutcome::RejectedDowngrade => {
                    eprintln!(
                        "[resolution] downgrade rejected for {setup_id}: {:?} -> {:?}",
                        existing.resolution.kind, new_resolution.kind,
                    );
                    return Ok(());
                }
            }
            existing
        }
    };

    store.write_case_resolutions(&[record]).await?;

    // Trigger archetype shard recompute for this (intent_kind, bucket, signature)
    // so distribution counts stay current.
    if !intent_kind.is_empty() {
        if let Err(e) =
            crate::persistence::discovered_archetype::recompute_archetype_shard_distribution(
                store,
                intent_kind,
                primary_horizon,
                signature,
            )
            .await
        {
            eprintln!("[resolution] archetype shard recompute failed: {e}");
        }
    }

    Ok(())
}

#[cfg(all(test, feature = "persistence"))]
mod horizon_helper_tests {
    use super::*;
    use crate::action::workflow::{ActionExecutionPolicy, ActionGovernanceReasonCode, ActionStage};
    use crate::core::runtime::telemetry::RuntimeCounters;
    use crate::ontology::horizon::{
        CaseHorizon, HorizonBucket, HorizonExpiry, SecondaryHorizon, SessionPhase, Urgency,
    };
    use crate::ontology::Symbol;
    use crate::ontology::{DecisionLineage, ProvenanceMetadata, ProvenanceSource, ReasoningScope};
    use crate::persistence::action_workflow::ActionWorkflowRecord;
    use crate::persistence::store::EdenStore;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use time::macros::datetime;
    use time::OffsetDateTime;
    use tokio::sync::Semaphore;

    fn sample_setup(setup_id: &str, primary: HorizonBucket) -> TacticalSetup {
        TacticalSetup {
            setup_id: setup_id.into(),
            hypothesis_id: String::new(),
            runner_up_hypothesis_id: None,
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            ),
            lineage: DecisionLineage {
                based_on: vec![],
                blocked_by: vec![],
                promoted_by: vec![],
                falsified_by: vec![],
            },
            scope: ReasoningScope::Symbol(Symbol("TEST".into())),
            title: String::new(),
            action: String::new().into(),
            direction: None,
            horizon: CaseHorizon::new(
                primary,
                Urgency::Immediate,
                SessionPhase::Opening,
                HorizonExpiry::UntilNextBucket,
                vec![SecondaryHorizon {
                    bucket: HorizonBucket::Mid30m,
                    confidence: dec!(0.7),
                }],
            ),
            confidence: Decimal::ZERO,
            confidence_gap: Decimal::ZERO,
            heuristic_edge: Decimal::ZERO,
            convergence_score: None,
            convergence_detail: None,
            workflow_id: None,
            entry_rationale: String::new(),
            causal_narrative: None,
            risk_notes: vec![],
            review_reason_code: None,
            policy_verdict: None,
        }
    }

    #[test]
    fn helper_builds_one_record_per_horizon_bucket() {
        let setup = sample_setup("setup-1", HorizonBucket::Fast5m);
        let now = datetime!(2026-04-13 14:00 UTC);
        let records = horizon_records_for_setup(&setup, "us", now);
        assert_eq!(records.len(), 2); // primary Fast5m + secondary Mid30m
        assert!(records[0].primary);
        assert!(!records[1].primary);
        assert_eq!(records[0].horizon, HorizonBucket::Fast5m);
        assert_eq!(records[1].horizon, HorizonBucket::Mid30m);
    }

    #[cfg(feature = "persistence")]
    #[test]
    fn settle_entry_context_prefers_in_memory_then_persisted() {
        let mut in_memory = std::collections::HashMap::new();
        let mut persisted = std::collections::HashMap::new();
        in_memory.insert("setup-a".to_string(), 1_u32);
        persisted.insert("setup-a".to_string(), 2_u32);
        persisted.insert("setup-b".to_string(), 3_u32);

        assert_eq!(
            select_settle_entry_context("setup-a", &in_memory, &persisted),
            Some((1, SettleContextSource::InMemory))
        );
        assert_eq!(
            select_settle_entry_context("setup-b", &in_memory, &persisted),
            Some((3, SettleContextSource::Persisted))
        );
        assert_eq!(
            select_settle_entry_context::<u32>("missing", &in_memory, &persisted),
            None
        );
    }

    fn temp_db_path(label: &str) -> std::path::PathBuf {
        let unique = format!(
            "eden-runtime-context-test-{}-{}-{}",
            label,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_nanos()
        );
        std::env::temp_dir().join(unique)
    }

    #[tokio::test]
    async fn persist_action_workflows_preserves_manual_stage_and_metadata() {
        let path = temp_db_path("workflow-merge");
        let store = EdenStore::open(path.to_str().unwrap()).await.unwrap();
        let existing = ActionWorkflowRecord {
            workflow_id: "order:700.HK:buy".into(),
            title: "BUY 700.HK".into(),
            payload: serde_json::json!({ "market": "hk", "symbol": "700.HK" }),
            current_stage: ActionStage::Review,
            execution_policy: ActionExecutionPolicy::ReviewRequired,
            governance_reason_code: ActionGovernanceReasonCode::TerminalReviewStage,
            recorded_at: OffsetDateTime::UNIX_EPOCH,
            actor: Some("operator".into()),
            owner: Some("alice".into()),
            reviewer: Some("bob".into()),
            queue_pin: Some("frontend-review-list".into()),
            note: Some("manual review lock".into()),
        };
        store.write_action_workflow(&existing).await.unwrap();

        let runtime = PreparedRuntimeContext {
            config: RuntimeInfraConfig {
                market: MarketId::Hk,
                debounce_ms: 1,
                rest_refresh_secs: 1,
                metrics_every_ticks: 1,
                db_path: path.to_string_lossy().to_string(),
                runtime_log_path: None,
            },
            counters: RuntimeCounters::default(),
            projection_state: ProjectionStateCache::new(),
            artifacts: AgentArtifactPaths {
                live_snapshot_path: String::new(),
                agent_snapshot_path: String::new(),
                operational_snapshot_path: String::new(),
                agent_briefing_path: String::new(),
                agent_session_path: String::new(),
                agent_watchlist_path: String::new(),
                agent_recommendations_path: String::new(),
                agent_perception_path: String::new(),
                agent_recommendation_journal_path: String::new(),
                agent_scoreboard_path: String::new(),
                agent_eod_review_path: String::new(),
                agent_narration_path: String::new(),
                agent_runtime_narration_path: String::new(),
                agent_analysis_path: String::new(),
            },
            runtime_task: None,
            store: Some(store.clone()),
            persistence_limit: std::sync::Arc::new(Semaphore::new(1)),
            current_regime_buckets: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
        };

        let generated = ActionWorkflowRecord {
            workflow_id: existing.workflow_id.clone(),
            title: existing.title.clone(),
            payload: existing.payload.clone(),
            current_stage: ActionStage::Suggest,
            execution_policy: ActionExecutionPolicy::ReviewRequired,
            governance_reason_code: ActionGovernanceReasonCode::WorkflowTransitionWindow,
            recorded_at: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(5),
            actor: Some("eden".into()),
            owner: None,
            reviewer: None,
            queue_pin: None,
            note: Some("generated".into()),
        };

        runtime
            .persist_action_workflows(CaseMarket::Hk, vec![generated])
            .await;

        let mut loaded = None;
        for _ in 0..10 {
            loaded = store
                .action_workflow_by_id(&existing.workflow_id)
                .await
                .unwrap();
            if loaded.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let loaded = loaded.expect("workflow should exist");
        assert_eq!(loaded.current_stage, ActionStage::Review);
        assert_eq!(loaded.actor.as_deref(), Some("operator"));
        assert_eq!(loaded.owner.as_deref(), Some("alice"));
        assert_eq!(loaded.reviewer.as_deref(), Some("bob"));
        assert_eq!(loaded.queue_pin.as_deref(), Some("frontend-review-list"));
        assert_eq!(loaded.note.as_deref(), Some("manual review lock"));

        drop(store);
        let _ = std::fs::remove_dir_all(path);
    }

    #[tokio::test]
    async fn persist_action_workflows_does_not_auto_advance_manual_stage() {
        let path = temp_db_path("workflow-manual-stage");
        let store = EdenStore::open(path.to_str().unwrap()).await.unwrap();
        let existing = ActionWorkflowRecord {
            workflow_id: "order:700.HK:buy".into(),
            title: "BUY 700.HK".into(),
            payload: serde_json::json!({ "market": "hk", "symbol": "700.HK" }),
            current_stage: ActionStage::Confirm,
            execution_policy: ActionExecutionPolicy::ReviewRequired,
            governance_reason_code: ActionGovernanceReasonCode::WorkflowTransitionWindow,
            recorded_at: OffsetDateTime::UNIX_EPOCH,
            actor: Some("operator".into()),
            owner: Some("alice".into()),
            reviewer: None,
            queue_pin: None,
            note: Some("hold at confirm".into()),
        };
        store.write_action_workflow(&existing).await.unwrap();

        let runtime = PreparedRuntimeContext {
            config: RuntimeInfraConfig {
                market: MarketId::Hk,
                debounce_ms: 1,
                rest_refresh_secs: 1,
                metrics_every_ticks: 1,
                db_path: path.to_string_lossy().to_string(),
                runtime_log_path: None,
            },
            counters: RuntimeCounters::default(),
            projection_state: ProjectionStateCache::new(),
            artifacts: AgentArtifactPaths {
                live_snapshot_path: String::new(),
                agent_snapshot_path: String::new(),
                operational_snapshot_path: String::new(),
                agent_briefing_path: String::new(),
                agent_session_path: String::new(),
                agent_watchlist_path: String::new(),
                agent_recommendations_path: String::new(),
                agent_perception_path: String::new(),
                agent_recommendation_journal_path: String::new(),
                agent_scoreboard_path: String::new(),
                agent_eod_review_path: String::new(),
                agent_narration_path: String::new(),
                agent_runtime_narration_path: String::new(),
                agent_analysis_path: String::new(),
            },
            runtime_task: None,
            store: Some(store.clone()),
            persistence_limit: std::sync::Arc::new(Semaphore::new(1)),
            current_regime_buckets: std::sync::Arc::new(std::sync::RwLock::new(
                std::collections::HashMap::new(),
            )),
        };

        let generated = ActionWorkflowRecord {
            workflow_id: existing.workflow_id.clone(),
            title: existing.title.clone(),
            payload: existing.payload.clone(),
            current_stage: ActionStage::Monitor,
            execution_policy: ActionExecutionPolicy::ReviewRequired,
            governance_reason_code: ActionGovernanceReasonCode::WorkflowTransitionWindow,
            recorded_at: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(5),
            actor: Some("eden".into()),
            owner: None,
            reviewer: None,
            queue_pin: None,
            note: Some("auto advance".into()),
        };

        runtime
            .persist_action_workflows(CaseMarket::Hk, vec![generated])
            .await;

        let mut loaded = None;
        for _ in 0..10 {
            loaded = store
                .action_workflow_by_id(&existing.workflow_id)
                .await
                .unwrap();
            if loaded.is_some() {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let loaded = loaded.expect("workflow should exist");
        assert_eq!(loaded.current_stage, ActionStage::Confirm);
        assert_eq!(loaded.actor.as_deref(), Some("operator"));
        assert_eq!(loaded.note.as_deref(), Some("hold at confirm"));

        drop(store);
        let _ = std::fs::remove_dir_all(path);
    }
}
