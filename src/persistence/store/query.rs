use crate::ontology::{scope_node_id, ReasoningScope, Symbol};
use crate::persistence::lineage_metric_row::LineageMetricRowRecord;
use crate::persistence::lineage_snapshot::LineageSnapshotRecord;
use crate::persistence::us_lineage_metric_row::UsLineageMetricRowRecord;
use crate::temporal::buffer::TickHistory;
use crate::temporal::causality::{compute_causal_timelines, CausalTimeline};
use crate::temporal::lineage::{compute_lineage_stats, LineageStats};
use crate::temporal::record::{SymbolSignals, TickRecord};
use crate::us::temporal::buffer::UsTickHistory;
use crate::us::temporal::causality::{
    compute_causal_timelines as compute_us_causal_timelines, UsCausalTimeline,
};
use crate::us::temporal::lineage::{compute_us_lineage_stats, UsLineageStats};
use crate::us::temporal::record::UsTickRecord;

use super::super::store_helpers::{
    fetch_ordered_records, fetch_ordered_records_custom, fetch_ranked_records,
    fetch_recent_tick_window, fetch_records_in_time_range, fetch_tick_archives_in_range,
    StoreError,
};
use super::EdenStore;

impl EdenStore {
    /// Query recent tick records for a symbol.
    pub async fn recent_ticks(
        &self,
        symbol: &Symbol,
        limit: usize,
    ) -> Result<Vec<TickRecord>, StoreError> {
        let mut scan_limit = limit.max(1).saturating_mul(4);
        let mut matched = Vec::new();

        loop {
            let mut result = self
                .db
                .query("SELECT * FROM tick_record ORDER BY tick_number DESC LIMIT $limit")
                .bind(("limit", scan_limit))
                .await?;
            let records: Vec<TickRecord> = result.take(0)?;
            let exhausted = records.len() < scan_limit;
            matched = records
                .into_iter()
                .filter(|record| record.signals.contains_key(symbol))
                .collect::<Vec<_>>();
            if matched.len() >= limit || exhausted || scan_limit > 32_768 {
                break;
            }
            scan_limit = scan_limit.saturating_mul(2);
        }

        if matched.len() > limit {
            matched.truncate(limit);
        }
        Ok(matched)
    }

    /// Query a recent tick window in chronological order.
    pub async fn recent_tick_window(&self, limit: usize) -> Result<Vec<TickRecord>, StoreError> {
        fetch_recent_tick_window(&self.db, "tick_record", limit).await
    }

    /// Query a recent US tick window in chronological order.
    pub async fn recent_us_tick_window(
        &self,
        limit: usize,
    ) -> Result<Vec<UsTickRecord>, StoreError> {
        fetch_recent_tick_window(&self.db, "us_tick_record", limit).await
    }

    /// Query recent tick records whose backward reasoning includes the requested leaf scope.
    pub async fn recent_causal_history(
        &self,
        leaf_scope_key: &str,
        limit: usize,
    ) -> Result<Vec<TickRecord>, StoreError> {
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
    ) -> Result<Option<CausalTimeline>, StoreError> {
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
    pub async fn recent_lineage_stats(&self, limit: usize) -> Result<LineageStats, StoreError> {
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
    ) -> Result<UsLineageStats, StoreError> {
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
    ) -> Result<Vec<LineageSnapshotRecord>, StoreError> {
        fetch_ordered_records(&self.db, "lineage_snapshot", "tick_number", false, limit).await
    }

    /// Query recent flattened lineage metric rows in reverse chronological order.
    pub async fn recent_lineage_metric_rows(
        &self,
        limit: usize,
    ) -> Result<Vec<LineageMetricRowRecord>, StoreError> {
        fetch_ordered_records_custom(
            &self.db,
            "SELECT * FROM lineage_metric_row ORDER BY tick_number DESC, bucket ASC, rank ASC LIMIT $limit",
            limit,
        )
        .await
    }

    /// Query recent flattened lineage rows with a bounded rank range and headroom for filtering.
    pub async fn recent_ranked_lineage_metric_rows(
        &self,
        snapshots: usize,
        max_rank: usize,
    ) -> Result<Vec<LineageMetricRowRecord>, StoreError> {
        let rank_limit = max_rank.max(1);
        let max_rows = snapshots
            .saturating_mul(rank_limit)
            .saturating_mul(6)
            .saturating_mul(8)
            .max(64);
        fetch_ranked_records(
            &self.db,
            "lineage_metric_row",
            rank_limit,
            max_rows,
            "tick_number DESC, bucket ASC, rank ASC",
        )
        .await
    }

    /// Query recent flattened US lineage metric rows in reverse chronological order.
    pub async fn recent_us_lineage_metric_rows(
        &self,
        limit: usize,
    ) -> Result<Vec<UsLineageMetricRowRecord>, StoreError> {
        fetch_ordered_records_custom(
            &self.db,
            "SELECT * FROM us_lineage_metric_row ORDER BY tick_number DESC, bucket ASC, rank ASC LIMIT $limit",
            limit,
        )
        .await
    }

    /// Query recent flattened US lineage rows with bounded rank range and headroom for filtering.
    pub async fn recent_ranked_us_lineage_metric_rows(
        &self,
        snapshots: usize,
        max_rank: usize,
    ) -> Result<Vec<UsLineageMetricRowRecord>, StoreError> {
        let rank_limit = max_rank.max(1);
        let max_rows = snapshots
            .saturating_mul(rank_limit)
            .saturating_mul(4)
            .max(64);
        fetch_ranked_records(
            &self.db,
            "us_lineage_metric_row",
            rank_limit,
            max_rows,
            "tick_number DESC, bucket ASC, rank ASC",
        )
        .await
    }

    /// Reconstruct the recent US causal timeline for one symbol.
    pub async fn recent_us_causal_timeline(
        &self,
        symbol: &str,
        limit: usize,
    ) -> Result<Option<UsCausalTimeline>, StoreError> {
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

    /// Persist a full-fidelity tick archive snapshot.
    pub async fn write_tick_archive(
        &self,
        archive: &crate::ontology::microstructure::TickArchive,
    ) -> Result<(), StoreError> {
        let id = format!("tick_archive_{}", archive.tick_number);
        crate::persistence::store_helpers::upsert_record_checked(&self.db, "tick_archive", &id, archive).await
    }

    pub async fn query_order_books(
        &self,
        symbol: &crate::ontology::objects::Symbol,
        from: time::OffsetDateTime,
        to: time::OffsetDateTime,
    ) -> Result<Vec<crate::ontology::microstructure::ArchivedOrderBook>, StoreError> {
        let archives = fetch_tick_archives_in_range(&self.db, from, to).await?;
        let sym = symbol;
        Ok(archives
            .into_iter()
            .flat_map(|a| {
                a.order_books
                    .into_iter()
                    .filter(|ob| &ob.symbol == sym)
                    .collect::<Vec<_>>()
            })
            .collect())
    }

    pub async fn query_trades(
        &self,
        symbol: &crate::ontology::objects::Symbol,
        from: time::OffsetDateTime,
        to: time::OffsetDateTime,
    ) -> Result<Vec<crate::ontology::microstructure::ArchivedTrade>, StoreError> {
        let archives = fetch_tick_archives_in_range(&self.db, from, to).await?;
        let sym = symbol;
        Ok(archives
            .into_iter()
            .flat_map(|a| {
                a.trades
                    .into_iter()
                    .filter(|t| &t.symbol == sym)
                    .collect::<Vec<_>>()
            })
            .collect())
    }

    pub async fn query_capital_flows(
        &self,
        symbol: &crate::ontology::objects::Symbol,
        from: time::OffsetDateTime,
        to: time::OffsetDateTime,
    ) -> Result<Vec<crate::ontology::microstructure::ArchivedCapitalFlowSeries>, StoreError> {
        let archives = fetch_tick_archives_in_range(&self.db, from, to).await?;
        let sym = symbol;
        Ok(archives
            .into_iter()
            .flat_map(|a| {
                a.capital_flows
                    .into_iter()
                    .filter(|cf| &cf.symbol == sym)
                    .collect::<Vec<_>>()
            })
            .collect())
    }

    /// Load tick archives ordered by tick_number for replay.
    pub async fn replay_tick_archives(
        &self,
        limit: usize,
    ) -> Result<Vec<crate::ontology::microstructure::TickArchive>, StoreError> {
        self.replay_tick_archives_after(None, limit).await
    }

    /// Load a replay batch strictly after the provided tick_number.
    pub async fn replay_tick_archives_after(
        &self,
        after_tick: Option<u64>,
        limit: usize,
    ) -> Result<Vec<crate::ontology::microstructure::TickArchive>, StoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let mut result = self
            .db
            .query(
                "SELECT * FROM tick_archive WHERE tick_number > $after ORDER BY tick_number ASC LIMIT $limit",
            )
            .bind(("after", after_tick.unwrap_or(0)))
            .bind(("limit", limit))
            .await?;
        let records: Vec<crate::ontology::microstructure::TickArchive> = result.take(0)?;
        Ok(records)
    }

    pub async fn compact_tick_archives(
        &self,
        before: time::OffsetDateTime,
    ) -> Result<(), StoreError> {
        let ts = before
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap();
        self.db
            .query("DELETE FROM tick_archive WHERE timestamp < <datetime>$before_ts")
            .bind(("before_ts", ts))
            .await?;
        Ok(())
    }

    pub async fn query_tick_records(
        &self,
        from: time::OffsetDateTime,
        to: time::OffsetDateTime,
        limit: usize,
    ) -> Result<Vec<crate::temporal::record::TickRecord>, StoreError> {
        fetch_records_in_time_range(&self.db, "tick_record", "tick_number", from, to, limit).await
    }

    pub async fn query_symbol_history(
        &self,
        symbol: &crate::ontology::objects::Symbol,
        last_n: usize,
    ) -> Result<Vec<SymbolSignals>, StoreError> {
        let records: Vec<crate::temporal::record::TickRecord> =
            fetch_recent_tick_window(&self.db, "tick_record", last_n).await?;
        let sym = symbol;
        Ok(records
            .into_iter()
            .filter_map(|r| r.signals.get(sym).cloned())
            .collect())
    }
}

fn causal_scope_key(scope: &ReasoningScope) -> String {
    scope_node_id(scope)
}
