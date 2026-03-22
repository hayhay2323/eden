use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::Surreal;

use crate::ontology::links::CrossStockPresence;
use crate::ontology::{ReasoningScope, Symbol};
use crate::persistence::action_workflow::{ActionWorkflowEventRecord, ActionWorkflowRecord};
use crate::persistence::case_realized_outcome::CaseRealizedOutcomeRecord;
use crate::persistence::case_reasoning_assessment::CaseReasoningAssessmentRecord;
use crate::persistence::hypothesis_track::HypothesisTrackRecord;
use crate::persistence::lineage_metric_row::LineageMetricRowRecord;
use crate::persistence::lineage_snapshot::LineageSnapshotRecord;
use crate::persistence::tactical_setup::TacticalSetupRecord;
use crate::temporal::buffer::TickHistory;
use crate::temporal::causality::{compute_causal_timelines, CausalTimeline};
use crate::temporal::lineage::{compute_lineage_stats, LineageStats};
use crate::temporal::record::TickRecord;
use crate::us::temporal::buffer::UsTickHistory;
use crate::us::temporal::causality::{
    compute_causal_timelines as compute_us_causal_timelines, UsCausalTimeline,
};
use crate::us::temporal::lineage::{compute_us_lineage_stats, UsLineageStats};
use crate::us::temporal::record::UsTickRecord;
use crate::{
    persistence::us_lineage_metric_row::UsLineageMetricRowRecord,
    persistence::us_lineage_snapshot::UsLineageSnapshotRecord,
};

use super::schema;

#[derive(Clone)]
pub struct EdenStore {
    db: Surreal<Db>,
}

impl EdenStore {
    /// Open or create the SurrealDB database at the given path.
    pub async fn open(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let db = Surreal::new::<RocksDb>(path).await?;
        db.use_ns("eden").use_db("market").await?;
        db.query(schema::SCHEMA).await?;
        Ok(Self { db })
    }

    /// Persist a tick record.
    pub async fn write_tick(&self, record: &TickRecord) -> Result<(), Box<dyn std::error::Error>> {
        let id = format!(
            "tick_{}_{}",
            record.timestamp.unix_timestamp(),
            record.tick_number
        );
        let _: Option<serde_json::Value> = self
            .db
            .upsert(("tick_record", &id))
            .content(record.clone())
            .await?;
        Ok(())
    }

    /// Persist a US tick record.
    pub async fn write_us_tick(
        &self,
        record: &UsTickRecord,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let id = format!(
            "us_tick_{}_{}",
            record.timestamp.unix_timestamp(),
            record.tick_number
        );
        let _: Option<serde_json::Value> = self
            .db
            .upsert(("us_tick_record", &id))
            .content(record.clone())
            .await?;
        Ok(())
    }

    /// Persist the latest known action workflow state.
    pub async fn write_action_workflow(
        &self,
        record: &ActionWorkflowRecord,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _: Option<serde_json::Value> = self
            .db
            .upsert(("action_workflow", record.record_id()))
            .content(record.clone())
            .await?;
        Ok(())
    }

    /// Persist an append-only action workflow event.
    pub async fn write_action_workflow_event(
        &self,
        record: &ActionWorkflowEventRecord,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _: Option<serde_json::Value> = self
            .db
            .upsert(("action_workflow_event", record.record_id()))
            .content(record.clone())
            .await?;
        Ok(())
    }

    /// Persist the latest tactical setup projection.
    pub async fn write_tactical_setup(
        &self,
        record: &TacticalSetupRecord,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _: Option<serde_json::Value> = self
            .db
            .upsert(("tactical_setup", record.record_id()))
            .content(record.clone())
            .await?;
        Ok(())
    }

    /// Persist one reasoning assessment snapshot for a case.
    pub async fn write_case_reasoning_assessment(
        &self,
        record: &CaseReasoningAssessmentRecord,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _: Option<serde_json::Value> = self
            .db
            .upsert(("case_reasoning_assessment", record.record_id()))
            .content(record.clone())
            .await?;
        Ok(())
    }

    /// Persist many reasoning assessment snapshots.
    pub async fn write_case_reasoning_assessments(
        &self,
        records: &[CaseReasoningAssessmentRecord],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if records.is_empty() {
            return Ok(());
        }

        let mut request = self.db.query(build_batch_upsert_query(
            "case_reasoning_assessment",
            records.len(),
        ));
        for (index, record) in records.iter().enumerate() {
            let id_key = format!("id_{index}");
            let record_key = format!("record_{index}");
            request = request
                .bind((id_key, record.record_id().to_string()))
                .bind((record_key, record.clone()));
        }
        request.await?;
        Ok(())
    }

    /// Persist latest resolved per-case outcomes.
    pub async fn write_case_realized_outcomes(
        &self,
        records: &[CaseRealizedOutcomeRecord],
    ) -> Result<(), Box<dyn std::error::Error>> {
        if records.is_empty() {
            return Ok(());
        }

        let mut request = self.db.query(build_batch_upsert_query(
            "case_realized_outcome",
            records.len(),
        ));
        for (index, record) in records.iter().enumerate() {
            let id_key = format!("id_{index}");
            let record_key = format!("record_{index}");
            request = request
                .bind((id_key, record.record_id().to_string()))
                .bind((record_key, record.clone()));
        }
        request.await?;
        Ok(())
    }

    /// Query the latest tactical setup projection by setup id.
    pub async fn tactical_setup_by_id(
        &self,
        setup_id: &str,
    ) -> Result<Option<TacticalSetupRecord>, Box<dyn std::error::Error>> {
        let query = format!(
            "SELECT * FROM tactical_setup WHERE setup_id = {} LIMIT 1",
            serde_json::to_string(setup_id)?
        );
        let mut result = self.db.query(&query).await?;
        let mut records: Vec<TacticalSetupRecord> = result.take(0)?;
        Ok(records.pop())
    }

    /// Query latest tactical setups by setup ids in one round-trip.
    pub async fn tactical_setups_by_ids(
        &self,
        setup_ids: &[String],
    ) -> Result<Vec<TacticalSetupRecord>, Box<dyn std::error::Error>> {
        if setup_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut result = self
            .db
            .query("SELECT * FROM tactical_setup WHERE setup_id INSIDE $setup_ids")
            .bind(("setup_ids", setup_ids.to_vec()))
            .await?;
        let records: Vec<TacticalSetupRecord> = result.take(0)?;
        Ok(records)
    }

    /// Query recent reasoning assessments for one setup, newest first.
    pub async fn recent_case_reasoning_assessments(
        &self,
        setup_id: &str,
        limit: usize,
    ) -> Result<Vec<CaseReasoningAssessmentRecord>, Box<dyn std::error::Error>> {
        let query = format!(
            "SELECT * FROM case_reasoning_assessment WHERE setup_id = {} ORDER BY recorded_at DESC LIMIT {}",
            serde_json::to_string(setup_id)?,
            limit,
        );
        let mut result = self.db.query(&query).await?;
        let records: Vec<CaseReasoningAssessmentRecord> = result.take(0)?;
        Ok(records)
    }

    /// Query recent reasoning assessments for one market, newest first.
    pub async fn recent_case_reasoning_assessments_by_market(
        &self,
        market: &str,
        limit: usize,
    ) -> Result<Vec<CaseReasoningAssessmentRecord>, Box<dyn std::error::Error>> {
        let query = format!(
            "SELECT * FROM case_reasoning_assessment WHERE market = {} ORDER BY recorded_at DESC LIMIT {}",
            serde_json::to_string(market)?,
            limit,
        );
        let mut result = self.db.query(&query).await?;
        let records: Vec<CaseReasoningAssessmentRecord> = result.take(0)?;
        Ok(records)
    }

    /// Query recent realized case outcomes for one market, newest first.
    pub async fn recent_case_realized_outcomes_by_market(
        &self,
        market: &str,
        limit: usize,
    ) -> Result<Vec<CaseRealizedOutcomeRecord>, Box<dyn std::error::Error>> {
        let query = format!(
            "SELECT * FROM case_realized_outcome WHERE market = {} ORDER BY resolved_at DESC LIMIT {}",
            serde_json::to_string(market)?,
            limit,
        );
        let mut result = self.db.query(&query).await?;
        let records: Vec<CaseRealizedOutcomeRecord> = result.take(0)?;
        Ok(records)
    }

    /// Persist the latest hypothesis track projection.
    pub async fn write_hypothesis_track(
        &self,
        record: &HypothesisTrackRecord,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _: Option<serde_json::Value> = self
            .db
            .upsert(("hypothesis_track", record.record_id()))
            .content(record.clone())
            .await?;
        Ok(())
    }

    /// Query the latest action workflow state by workflow id.
    pub async fn action_workflow_by_id(
        &self,
        workflow_id: &str,
    ) -> Result<Option<ActionWorkflowRecord>, Box<dyn std::error::Error>> {
        let query = format!(
            "SELECT * FROM action_workflow WHERE workflow_id = {} LIMIT 1",
            serde_json::to_string(workflow_id)?
        );
        let mut result = self.db.query(&query).await?;
        let mut records: Vec<ActionWorkflowRecord> = result.take(0)?;
        Ok(records.pop())
    }

    /// Query latest action workflows by workflow ids in one round-trip.
    pub async fn action_workflows_by_ids(
        &self,
        workflow_ids: &[String],
    ) -> Result<Vec<ActionWorkflowRecord>, Box<dyn std::error::Error>> {
        if workflow_ids.is_empty() {
            return Ok(Vec::new());
        }

        let mut result = self
            .db
            .query("SELECT * FROM action_workflow WHERE workflow_id INSIDE $workflow_ids")
            .bind(("workflow_ids", workflow_ids.to_vec()))
            .await?;
        let records: Vec<ActionWorkflowRecord> = result.take(0)?;
        Ok(records)
    }

    /// Query workflow events in chronological order.
    pub async fn action_workflow_events(
        &self,
        workflow_id: &str,
    ) -> Result<Vec<ActionWorkflowEventRecord>, Box<dyn std::error::Error>> {
        let query = format!(
            "SELECT * FROM action_workflow_event WHERE workflow_id = {} ORDER BY recorded_at ASC",
            serde_json::to_string(workflow_id)?
        );
        let mut result = self.db.query(&query).await?;
        let records: Vec<ActionWorkflowEventRecord> = result.take(0)?;
        Ok(records)
    }

    /// Query the latest workflow mutation timestamp, if any.
    pub async fn latest_action_workflow_recorded_at(
        &self,
    ) -> Result<Option<time::OffsetDateTime>, Box<dyn std::error::Error>> {
        let mut result = self
            .db
            .query("SELECT * FROM action_workflow ORDER BY recorded_at DESC LIMIT 1")
            .await?;
        let mut records: Vec<ActionWorkflowRecord> = result.take(0)?;
        Ok(records.pop().map(|record| record.recorded_at))
    }

    /// Persist one lineage evaluation snapshot for historical leaderboard review.
    pub async fn write_lineage_snapshot(
        &self,
        record: &LineageSnapshotRecord,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _: Option<serde_json::Value> = self
            .db
            .upsert(("lineage_snapshot", record.record_id()))
            .content(record.clone())
            .await?;
        Ok(())
    }

    /// Persist one US lineage evaluation snapshot.
    pub async fn write_us_lineage_snapshot(
        &self,
        record: &UsLineageSnapshotRecord,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let _: Option<serde_json::Value> = self
            .db
            .upsert(("us_lineage_snapshot", record.record_id()))
            .content(record.clone())
            .await?;
        Ok(())
    }

    /// Persist flattened lineage metric rows for fast filtered lookups.
    pub async fn write_lineage_metric_rows(
        &self,
        records: &[LineageMetricRowRecord],
    ) -> Result<(), Box<dyn std::error::Error>> {
        for record in records {
            let _: Option<serde_json::Value> = self
                .db
                .upsert(("lineage_metric_row", &record.row_id))
                .content(record.clone())
                .await?;
        }
        Ok(())
    }

    /// Persist flattened US lineage metric rows.
    pub async fn write_us_lineage_metric_rows(
        &self,
        records: &[UsLineageMetricRowRecord],
    ) -> Result<(), Box<dyn std::error::Error>> {
        for record in records {
            let _: Option<serde_json::Value> = self
                .db
                .upsert(("us_lineage_metric_row", &record.row_id))
                .content(record.clone())
                .await?;
        }
        Ok(())
    }

    /// Persist institution cross-stock presences in a single batch query.
    pub async fn write_institution_states(
        &self,
        presences: &[CrossStockPresence],
        timestamp: time::OffsetDateTime,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if presences.is_empty() {
            return Ok(());
        }

        let ts_str = timestamp
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();

        // Build all records as a single batch
        let records: Vec<serde_json::Value> = presences
            .iter()
            .map(|p| {
                serde_json::json!({
                    "institution_id": p.institution_id.0,
                    "timestamp": ts_str,
                    "symbols": p.symbols.iter().map(|s| &s.0).collect::<Vec<_>>(),
                    "ask_symbols": p.ask_symbols.iter().map(|s| &s.0).collect::<Vec<_>>(),
                    "bid_symbols": p.bid_symbols.iter().map(|s| &s.0).collect::<Vec<_>>(),
                    "seat_count": p.symbols.len(),
                })
            })
            .collect();

        // Single INSERT with all records — one DB round-trip instead of O(n)
        self.db
            .query("INSERT INTO institution_state $records")
            .bind(("records", records))
            .await?;

        Ok(())
    }

    /// Query recent tick records for a symbol.
    pub async fn recent_ticks(
        &self,
        symbol: &Symbol,
        limit: usize,
    ) -> Result<Vec<TickRecord>, Box<dyn std::error::Error>> {
        let sym = validated_surreal_field_key(&symbol.0)?;
        let query = format!(
            "SELECT * FROM tick_record WHERE signals.`{sym}`.composite != NONE ORDER BY tick_number DESC LIMIT {limit}",
            sym = sym,
            limit = limit,
        );
        let mut result = self.db.query(&query).await?;
        let records: Vec<TickRecord> = result.take(0)?;
        Ok(records)
    }

    /// Query a recent tick window in chronological order.
    pub async fn recent_tick_window(
        &self,
        limit: usize,
    ) -> Result<Vec<TickRecord>, Box<dyn std::error::Error>> {
        let query = format!(
            "SELECT * FROM tick_record ORDER BY tick_number DESC LIMIT {limit}",
            limit = limit,
        );
        let mut result = self.db.query(&query).await?;
        let mut records: Vec<TickRecord> = result.take(0)?;
        records.sort_by_key(|record| record.tick_number);
        Ok(records)
    }

    /// Query a recent US tick window in chronological order.
    pub async fn recent_us_tick_window(
        &self,
        limit: usize,
    ) -> Result<Vec<UsTickRecord>, Box<dyn std::error::Error>> {
        let query = format!(
            "SELECT * FROM us_tick_record ORDER BY tick_number DESC LIMIT {limit}",
            limit = limit,
        );
        let mut result = self.db.query(&query).await?;
        let mut records: Vec<UsTickRecord> = result.take(0)?;
        records.sort_by_key(|record| record.tick_number);
        Ok(records)
    }

    /// Query recent tick records whose backward reasoning includes the requested leaf scope.
    pub async fn recent_causal_history(
        &self,
        leaf_scope_key: &str,
        limit: usize,
    ) -> Result<Vec<TickRecord>, Box<dyn std::error::Error>> {
        let mut records = self.recent_tick_window(limit).await?;
        records.retain(|record| {
            record
                .backward_reasoning
                .investigations
                .iter()
                .any(|investigation| causal_scope_key(&investigation.leaf_scope) == leaf_scope_key)
        });
        Ok(records)
    }

    /// Reconstruct the recent causal timeline for one leaf scope from persisted tick records.
    pub async fn recent_causal_timeline(
        &self,
        leaf_scope_key: &str,
        limit: usize,
    ) -> Result<Option<CausalTimeline>, Box<dyn std::error::Error>> {
        let records = self.recent_causal_history(leaf_scope_key, limit).await?;
        if records.is_empty() {
            return Ok(None);
        }

        let mut history = TickHistory::new(records.len().max(1));
        for record in records {
            history.push(record);
        }

        Ok(compute_causal_timelines(&history).remove(leaf_scope_key))
    }

    /// Reconstruct recent lineage evaluation statistics from persisted tick history.
    pub async fn recent_lineage_stats(
        &self,
        limit: usize,
    ) -> Result<LineageStats, Box<dyn std::error::Error>> {
        let records = self.recent_tick_window(limit).await?;
        if records.is_empty() {
            return Ok(LineageStats::default());
        }

        let mut history = TickHistory::new(records.len().max(1));
        for record in records {
            history.push(record);
        }

        Ok(compute_lineage_stats(&history, limit))
    }

    /// Reconstruct recent US lineage evaluation statistics from persisted tick history.
    pub async fn recent_us_lineage_stats(
        &self,
        limit: usize,
        resolution_lag: u64,
    ) -> Result<UsLineageStats, Box<dyn std::error::Error>> {
        let records = self.recent_us_tick_window(limit).await?;
        if records.is_empty() {
            return Ok(UsLineageStats::default());
        }

        let mut history = UsTickHistory::new(records.len().max(1));
        for record in records {
            history.push(record);
        }

        Ok(compute_us_lineage_stats(&history, resolution_lag))
    }

    /// Query recent persisted lineage snapshots in reverse chronological order.
    pub async fn recent_lineage_snapshots(
        &self,
        limit: usize,
    ) -> Result<Vec<LineageSnapshotRecord>, Box<dyn std::error::Error>> {
        let query = format!(
            "SELECT * FROM lineage_snapshot ORDER BY tick_number DESC LIMIT {limit}",
            limit = limit,
        );
        let mut result = self.db.query(&query).await?;
        let records: Vec<LineageSnapshotRecord> = result.take(0)?;
        Ok(records)
    }

    /// Query recent flattened lineage metric rows in reverse chronological order.
    pub async fn recent_lineage_metric_rows(
        &self,
        limit: usize,
    ) -> Result<Vec<LineageMetricRowRecord>, Box<dyn std::error::Error>> {
        let query = format!(
            "SELECT * FROM lineage_metric_row ORDER BY tick_number DESC, bucket ASC, rank ASC LIMIT {limit}",
            limit = limit,
        );
        let mut result = self.db.query(&query).await?;
        let records: Vec<LineageMetricRowRecord> = result.take(0)?;
        Ok(records)
    }

    /// Query recent flattened lineage rows with a bounded rank range and headroom for filtering.
    pub async fn recent_ranked_lineage_metric_rows(
        &self,
        snapshots: usize,
        max_rank: usize,
    ) -> Result<Vec<LineageMetricRowRecord>, Box<dyn std::error::Error>> {
        let rank_limit = max_rank.max(1);
        let max_rows = snapshots
            .saturating_mul(rank_limit)
            .saturating_mul(6)
            .saturating_mul(8)
            .max(64);
        let query = format!(
            "SELECT * FROM lineage_metric_row WHERE rank < {rank_limit} ORDER BY tick_number DESC, bucket ASC, rank ASC LIMIT {max_rows}",
            rank_limit = rank_limit,
            max_rows = max_rows,
        );
        let mut result = self.db.query(&query).await?;
        let records: Vec<LineageMetricRowRecord> = result.take(0)?;
        Ok(records)
    }

    /// Query recent flattened US lineage metric rows in reverse chronological order.
    pub async fn recent_us_lineage_metric_rows(
        &self,
        limit: usize,
    ) -> Result<Vec<UsLineageMetricRowRecord>, Box<dyn std::error::Error>> {
        let query = format!(
            "SELECT * FROM us_lineage_metric_row ORDER BY tick_number DESC, bucket ASC, rank ASC LIMIT {limit}",
            limit = limit,
        );
        let mut result = self.db.query(&query).await?;
        let records: Vec<UsLineageMetricRowRecord> = result.take(0)?;
        Ok(records)
    }

    /// Query recent flattened US lineage rows with bounded rank range and headroom for filtering.
    pub async fn recent_ranked_us_lineage_metric_rows(
        &self,
        snapshots: usize,
        max_rank: usize,
    ) -> Result<Vec<UsLineageMetricRowRecord>, Box<dyn std::error::Error>> {
        let rank_limit = max_rank.max(1);
        let max_rows = snapshots
            .saturating_mul(rank_limit)
            .saturating_mul(4)
            .max(64);
        let query = format!(
            "SELECT * FROM us_lineage_metric_row WHERE rank < {rank_limit} ORDER BY tick_number DESC, bucket ASC, rank ASC LIMIT {max_rows}",
            rank_limit = rank_limit,
            max_rows = max_rows,
        );
        let mut result = self.db.query(&query).await?;
        let records: Vec<UsLineageMetricRowRecord> = result.take(0)?;
        Ok(records)
    }

    /// Reconstruct the recent US causal timeline for one symbol.
    pub async fn recent_us_causal_timeline(
        &self,
        symbol: &str,
        limit: usize,
    ) -> Result<Option<UsCausalTimeline>, Box<dyn std::error::Error>> {
        let records = self.recent_us_tick_window(limit).await?;
        if records.is_empty() {
            return Ok(None);
        }

        let mut history = UsTickHistory::new(records.len().max(1));
        for record in records {
            history.push(record);
        }

        Ok(compute_us_causal_timelines(&history)
            .remove(&crate::ontology::objects::Symbol(symbol.into())))
    }
}

fn validated_surreal_field_key(value: &str) -> Result<&str, Box<dyn std::error::Error>> {
    let valid = !value.is_empty()
        && value
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '_' | '-' | ':'));
    if valid {
        Ok(value)
    } else {
        Err(format!("unsupported symbol key `{value}` for dynamic query").into())
    }
}

fn build_batch_upsert_query(table: &str, records: usize) -> String {
    let mut query = String::new();
    for index in 0..records {
        query.push_str(&format!(
            "UPSERT type::thing('{table}', $id_{index}) CONTENT $record_{index};"
        ));
    }
    query
}

fn causal_scope_key(scope: &ReasoningScope) -> String {
    match scope {
        ReasoningScope::Market => "market".into(),
        ReasoningScope::Symbol(symbol) => symbol.to_string(),
        ReasoningScope::Sector(sector) => format!("sector:{}", sector),
        ReasoningScope::Institution(institution) => format!("institution:{}", institution),
        ReasoningScope::Theme(theme) => format!("theme:{}", theme),
        ReasoningScope::Region(region) => format!("region:{}", region),
        ReasoningScope::Custom(value) => format!("custom:{}", value),
    }
}
