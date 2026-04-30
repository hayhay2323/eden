#[cfg(feature = "persistence")]
use std::fmt::Display;
#[cfg(feature = "persistence")]
use std::future::Future;
use std::sync::Arc;
use std::time::{Duration, Instant};

use longport::quote::PushEvent;
use tokio::sync::mpsc;
use tokio::sync::Semaphore;

use crate::agent::{AgentAlertScoreboard, AgentSession, AgentSnapshot};
use crate::cases::CaseMarket;
use crate::core::analyst_service::AnalystService;
use crate::core::artifact_repository::resolve_artifact_path;
use crate::core::market::ArtifactKind;
use crate::core::market::MarketId;
use crate::core::market::MarketRegistry;
use crate::core::projection::ProjectionBundle;
use crate::core::runtime_loop::TickState;
use crate::core::runtime_tasks::{
    default_runtime_tasks_path, RuntimeTaskCreateRequest, RuntimeTaskKind, RuntimeTaskStore,
};
use crate::live_snapshot::ensure_snapshot_parent;
#[path = "runtime/context.rs"]
mod context;
#[path = "runtime/persistence.rs"]
mod persistence;
#[path = "runtime/projection.rs"]
mod projection_runtime;
#[path = "runtime/telemetry.rs"]
mod telemetry;
#[path = "runtime/tick.rs"]
mod tick_runtime;
#[cfg(feature = "persistence")]
use crate::agent::AgentDecision;
#[cfg(feature = "persistence")]
use crate::cases::CaseSummary;
#[cfg(feature = "persistence")]
use crate::live_snapshot::LiveMarket;
#[cfg(feature = "persistence")]
use crate::ontology::{
    ActionNode, AgentKnowledgeEvent, AgentKnowledgeLink, AgentMacroEvent,
    BackwardReasoningSnapshot, Hypothesis, TacticalSetup, WorldStateSnapshot,
};
#[cfg(feature = "persistence")]
use crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord;
#[cfg(feature = "persistence")]
use crate::persistence::lineage_metric_row::LineageMetricRowRecord;
#[cfg(feature = "persistence")]
use crate::persistence::lineage_snapshot::LineageSnapshotRecord;
#[cfg(feature = "persistence")]
use crate::persistence::store::EdenStore;
#[cfg(feature = "persistence")]
use crate::persistence::us_lineage_metric_row::UsLineageMetricRowRecord;
#[cfg(feature = "persistence")]
use crate::persistence::us_lineage_snapshot::UsLineageSnapshotRecord;
pub use context::prepare_runtime_artifact_path;
#[cfg(feature = "persistence")]
pub use context::sweep_pending_horizons_to_due;
#[cfg(feature = "persistence")]
pub use context::{settle_live_horizons_hk, settle_live_horizons_us};
pub use context::{
    AgentArtifactPaths, PreparedRuntimeContext, ProjectionStateCache, RuntimeInfraConfig,
};
#[cfg(feature = "persistence")]
use persistence::{
    build_market_knowledge_followup_bundle, build_market_knowledge_persistence_bundle,
    KnowledgePersistenceBundle, LineagePersistenceBundle,
};
use projection_runtime::{finalize_runtime_projection, write_projection_artifacts};
use serde_json::json;
pub use telemetry::PushTap;
use telemetry::{
    emit_runtime_log, load_optional_string_override, load_string_override, load_u64_override,
    spawn_batched_push_forwarder, spawn_push_forwarder, RuntimeCounters,
};
use tick_runtime::{
    begin_runtime_tick, log_runtime_monitoring_active, spawn_runtime_rest_refresh,
    RuntimeTickBoundary,
};

pub async fn prepare_runtime_context(
    market: MarketId,
    #[allow(unused_variables)] persistence_max_in_flight: usize,
    _db_failure_context: &str,
) -> Result<PreparedRuntimeContext, String> {
    let config = RuntimeInfraConfig::load(market)?;
    config.log_startup(cfg!(feature = "persistence"));
    let counters = RuntimeCounters::default();
    let projection_state = ProjectionStateCache::new();
    let artifacts = AgentArtifactPaths::prepare(market).await;
    let runtime_task = match RuntimeTaskStore::load(default_runtime_tasks_path()) {
        Ok(store) => match store.create_handle(RuntimeTaskCreateRequest {
            label: format!(
                "{} runtime loop",
                MarketRegistry::definition(market).display_name
            ),
            kind: RuntimeTaskKind::RuntimeLoop,
            market: Some(market.slug().to_string()),
            owner: Some("runtime".into()),
            detail: Some("starting runtime loop".into()),
            metadata: Some(json!({
                "market": market.slug(),
                "pid": std::process::id(),
                "db_path": config.db_path,
                "debounce_ms": config.debounce_ms,
                "rest_refresh_secs": config.rest_refresh_secs,
                "metrics_every_ticks": config.metrics_every_ticks,
                "persistence_enabled": cfg!(feature = "persistence"),
            })),
        }) {
            Ok((_, handle)) => Some(handle),
            Err(error) => {
                eprintln!(
                    "Warning: failed to register runtime task for {}: {}",
                    MarketRegistry::definition(market).display_name,
                    error
                );
                None
            }
        },
        Err(error) => {
            eprintln!(
                "Warning: failed to open runtime task registry for {}: {}",
                MarketRegistry::definition(market).display_name,
                error
            );
            None
        }
    };

    #[cfg(feature = "persistence")]
    let store = match EdenStore::open(&config.db_path).await {
        Ok(store) => Some(store),
        Err(error) => {
            eprintln!("Warning: {} ({})", _db_failure_context, error);
            None
        }
    };
    #[cfg(feature = "persistence")]
    let persistence_limit = Arc::new(Semaphore::new(persistence_max_in_flight));

    Ok(PreparedRuntimeContext {
        config,
        counters,
        projection_state,
        artifacts,
        runtime_task,
        #[cfg(feature = "persistence")]
        store,
        #[cfg(feature = "persistence")]
        persistence_limit,
        current_regime_buckets: Arc::new(std::sync::RwLock::new(std::collections::HashMap::new())),
    })
}

pub async fn prepare_runtime_context_or_exit(
    market: MarketId,
    persistence_max_in_flight: usize,
    db_failure_context: &str,
) -> PreparedRuntimeContext {
    prepare_runtime_context(market, persistence_max_in_flight, db_failure_context)
        .await
        .unwrap_or_else(|error| {
            eprintln!(
                "Invalid {} runtime config: {}",
                MarketRegistry::definition(market).display_name,
                error
            );
            std::process::exit(2);
        })
}

#[cfg(feature = "persistence")]
pub fn ensure_persistence_store_available(
    market: MarketId,
    store_available: bool,
) -> Result<(), String> {
    if store_available {
        Ok(())
    } else {
        Err(format!(
            "{} runtime requires persistence store availability; refusing to start without SurrealDB.",
            MarketRegistry::definition(market).display_name
        ))
    }
}

#[cfg(all(test, feature = "persistence"))]
mod runtime_preflight_tests {
    use super::*;

    #[test]
    fn persistence_store_preflight_messages_match_market() {
        let hk = ensure_persistence_store_available(MarketId::Hk, false).unwrap_err();
        assert_eq!(
            hk,
            "Hong Kong runtime requires persistence store availability; refusing to start without SurrealDB."
        );

        let us = ensure_persistence_store_available(MarketId::Us, false).unwrap_err();
        assert_eq!(
            us,
            "United States runtime requires persistence store availability; refusing to start without SurrealDB."
        );
    }

    #[test]
    fn persistence_store_preflight_accepts_available_store() {
        assert!(ensure_persistence_store_available(MarketId::Hk, true).is_ok());
        assert!(ensure_persistence_store_available(MarketId::Us, true).is_ok());
    }
}
