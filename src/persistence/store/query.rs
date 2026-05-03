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
    fetch_optional_record_by_field, fetch_ordered_records, fetch_ordered_records_custom,
    fetch_ranked_records, fetch_recent_tick_window, fetch_records_by_field_order,
    fetch_records_in_time_range, fetch_tick_archives_in_range, StoreError,
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
        let matched = loop {
            let mut result = self
                .db
                .query("SELECT * FROM tick_record ORDER BY tick_number DESC LIMIT $limit")
                .bind(("limit", scan_limit))
                .await?;
            let records: Vec<TickRecord> = result.take(0)?;
            let exhausted = records.len() < scan_limit;
            let matched = records
                .into_iter()
                .filter(|record| record.signals.contains_key(symbol))
                .collect::<Vec<_>>();
            if matched.len() >= limit || exhausted || scan_limit > 32_768 {
                break matched;
            }
            scan_limit = scan_limit.saturating_mul(2);
        };

        if matched.len() > limit {
            let mut matched = matched;
            matched.truncate(limit);
            return Ok(matched);
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
        let market = if archive.market.trim().is_empty() {
            "unknown"
        } else {
            archive.market.as_str()
        };
        let id = format!("tick_archive_{}_{}", market, archive.tick_number);
        crate::persistence::store_helpers::upsert_record_checked(
            &self.db,
            "tick_archive",
            &id,
            archive,
        )
        .await
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
        self.replay_market_tick_archives_after(None, after_tick, limit)
            .await
    }

    /// Load a replay batch for one market strictly after the provided tick_number.
    pub async fn replay_market_tick_archives_after(
        &self,
        market: Option<&str>,
        after_tick: Option<u64>,
        limit: usize,
    ) -> Result<Vec<crate::ontology::microstructure::TickArchive>, StoreError> {
        self.replay_market_tick_archives_after_cursor(
            market,
            after_tick.map(|tick| ("~", tick)),
            limit,
        )
        .await
    }

    /// Load a replay batch after a stable `(market, tick_number)` cursor.
    ///
    /// When no market filter is supplied, ordering by `(tick_number, market)`
    /// prevents mixed HK/US archives with the same tick number from being
    /// skipped between pages. Market-scoped replay also includes legacy
    /// archives whose market field is missing/empty/unknown so old DBs remain
    /// replayable after migration 043.
    pub async fn replay_market_tick_archives_after_cursor(
        &self,
        market: Option<&str>,
        after_cursor: Option<(&str, u64)>,
        limit: usize,
    ) -> Result<Vec<crate::ontology::microstructure::TickArchive>, StoreError> {
        if limit == 0 {
            return Ok(Vec::new());
        }

        let has_cursor = after_cursor.is_some();
        let mut query = match (market.is_some(), has_cursor) {
            (true, true) => self.db.query(
                "SELECT * FROM tick_archive WHERE (market = $market OR market = 'unknown' OR market = '' OR market = NONE) AND tick_number > $after_tick ORDER BY tick_number ASC, market ASC LIMIT $limit",
            ),
            (true, false) => self.db.query(
                "SELECT * FROM tick_archive WHERE market = $market OR market = 'unknown' OR market = '' OR market = NONE ORDER BY tick_number ASC, market ASC LIMIT $limit",
            ),
            (false, true) => self.db.query(
                "SELECT * FROM tick_archive WHERE tick_number > $after_tick OR (tick_number = $after_tick AND market > $after_market) ORDER BY tick_number ASC, market ASC LIMIT $limit",
            ),
            (false, false) => self
                .db
                .query("SELECT * FROM tick_archive ORDER BY tick_number ASC, market ASC LIMIT $limit"),
        };
        if let Some(market) = market {
            query = query.bind(("market", market.to_string()));
        }
        if let Some((after_market, after_tick)) = after_cursor {
            query = query
                .bind(("after_market", after_market.to_string()))
                .bind(("after_tick", after_tick));
        }
        let mut result = query.bind(("limit", limit)).await?;
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

    /// Load all candidate mechanisms for a given market.
    pub async fn load_candidate_mechanisms(
        &self,
        market: &str,
    ) -> Result<Vec<crate::persistence::candidate_mechanism::CandidateMechanismRecord>, StoreError>
    {
        fetch_records_by_field_order(
            &self.db,
            "candidate_mechanism",
            "market",
            market,
            "last_seen_tick",
            false,
            500,
        )
        .await
    }

    /// Load all causal schemas for a given market.
    pub async fn load_causal_schemas(
        &self,
        market: &str,
    ) -> Result<Vec<crate::persistence::causal_schema::CausalSchemaRecord>, StoreError> {
        fetch_records_by_field_order(
            &self.db,
            "causal_schema",
            "market",
            market,
            "last_applied_tick",
            false,
            200,
        )
        .await
    }

    pub async fn load_edge_learning_ledger(
        &self,
        market: &str,
    ) -> Result<
        Option<crate::persistence::edge_learning_ledger::EdgeLearningLedgerRecord>,
        StoreError,
    > {
        fetch_optional_record_by_field(&self.db, "edge_learning_ledger", "market", market).await
    }

    pub async fn load_discovered_archetypes(
        &self,
        market: &str,
    ) -> Result<Vec<crate::persistence::discovered_archetype::DiscoveredArchetypeRecord>, StoreError>
    {
        fetch_records_by_field_order(
            &self.db,
            "discovered_archetype",
            "market",
            market,
            "samples",
            false,
            128,
        )
        .await
    }

    pub async fn load_horizon_evaluations_for_setup(
        &self,
        setup_id: &str,
    ) -> Result<Vec<crate::persistence::horizon_evaluation::HorizonEvaluationRecord>, StoreError>
    {
        fetch_records_by_field_order(
            &self.db,
            "horizon_evaluation",
            "setup_id",
            setup_id,
            "due_at",
            true,
            64,
        )
        .await
    }

    /// Step 1 of the runtime live-settle path (Finding 1 audit, 2026-04-19):
    /// load every HorizonEvaluationRecord whose status is still `Pending`
    /// and whose `due_at <= now`. Caller is expected to flip these to
    /// `Due` (step 1 of Pending→Due→Resolved) and write them back via
    /// `write_horizon_evaluations`.
    ///
    /// The upgrade from `Due` to `Resolved` requires a computed
    /// `HorizonResult` (net_return + follow_through) which depends on
    /// price data not available in the generic store layer — that
    /// upgrade remains a separate follow-up.
    pub async fn pending_horizons_past_due(
        &self,
        now: time::OffsetDateTime,
    ) -> Result<Vec<crate::persistence::horizon_evaluation::HorizonEvaluationRecord>, StoreError>
    {
        let now_str = now
            .format(&time::format_description::well_known::Rfc3339)
            .map_err(|e| -> StoreError { Box::new(e) })?;
        // Note: EvaluationStatus uses #[serde(rename_all = "snake_case")],
        // so "Pending" serialises as "pending". SurrealDB's WHERE clause
        // compares against the stored string.
        let mut result = self
            .db
            .query(
                "SELECT * FROM horizon_evaluation \
                 WHERE status = 'pending' AND due_at <= $now \
                 ORDER BY due_at ASC \
                 LIMIT 256",
            )
            .bind(("now", now_str))
            .await?;
        Ok(result.take(0)?)
    }

    pub async fn unresolved_horizons_for_market(
        &self,
        market: &str,
        limit: usize,
    ) -> Result<Vec<crate::persistence::horizon_evaluation::HorizonEvaluationRecord>, StoreError>
    {
        let mut result = self
            .db
            .query(
                "SELECT * FROM horizon_evaluation \
                 WHERE market = $market AND (status = 'pending' OR status = 'due') \
                 ORDER BY setup_id ASC, due_at ASC LIMIT $limit",
            )
            .bind(("market", market.to_string()))
            .bind(("limit", limit))
            .await?;
        Ok(result.take(0)?)
    }

    /// Return the set of `record_id`s already present in `horizon_evaluation`
    /// for any of the given `setup_id`s. Used by `persist_horizon_evaluations`
    /// to implement insert-if-not-exists semantics.
    pub async fn horizon_evaluation_record_ids_for_setups(
        &self,
        setup_ids: &[String],
    ) -> Result<std::collections::HashSet<String>, StoreError> {
        if setup_ids.is_empty() {
            return Ok(std::collections::HashSet::new());
        }
        #[derive(serde::Deserialize)]
        struct IdRow {
            record_id: String,
        }
        let mut result = self
            .db
            .query("SELECT record_id FROM horizon_evaluation WHERE setup_id INSIDE $ids")
            .bind(("ids", setup_ids.to_vec()))
            .await?;
        let rows: Vec<IdRow> = result.take(0)?;
        Ok(rows.into_iter().map(|r| r.record_id).collect())
    }

    pub async fn load_case_resolution_for_setup(
        &self,
        setup_id: &str,
    ) -> Result<Option<crate::persistence::case_resolution::CaseResolutionRecord>, StoreError> {
        let mut records: Vec<crate::persistence::case_resolution::CaseResolutionRecord> =
            fetch_records_by_field_order(
                &self.db,
                "case_resolution",
                "setup_id",
                setup_id,
                "updated_at",
                false, // descending — newest first
                1,
            )
            .await?;
        Ok(records.pop())
    }

    /// Load all case_resolution records. Used by shard recompute to aggregate
    /// outcome counts per (intent_kind, bucket, signature). Capped at 10,000
    /// records; callers filter in application code.
    pub async fn load_all_case_resolutions(
        &self,
    ) -> Result<Vec<crate::persistence::case_resolution::CaseResolutionRecord>, StoreError> {
        fetch_ordered_records(&self.db, "case_resolution", "updated_at", false, 10_000).await
    }

    /// Load all `case_resolution` records for a single archetype shard identified
    /// by `(intent_kind, primary_horizon, signature)`. No row-count cap — this is
    /// the correct query to use when recomputing shard outcome distribution counts.
    pub async fn load_case_resolutions_for_shard(
        &self,
        intent_kind: &str,
        primary_horizon: crate::ontology::horizon::HorizonBucket,
        signature: &str,
    ) -> Result<Vec<crate::persistence::case_resolution::CaseResolutionRecord>, StoreError> {
        // HorizonBucket serializes as snake_case (e.g. "fast5m") via serde.
        let ph_str = serde_json::to_value(primary_horizon)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| format!("{primary_horizon:?}").to_lowercase());
        let mut result = self
            .db
            .query("SELECT * FROM case_resolution WHERE intent_kind = $ik AND primary_horizon = $ph AND signature = $sig")
            .bind(("ik", intent_kind.to_string()))
            .bind(("ph", ph_str))
            .bind(("sig", signature.to_string()))
            .await?;
        Ok(result.take(0)?)
    }

    /// Load a single discovered_archetype record by its canonical key.
    pub async fn load_archetype_by_key(
        &self,
        archetype_key: &str,
    ) -> Result<
        Option<crate::persistence::discovered_archetype::DiscoveredArchetypeRecord>,
        StoreError,
    > {
        fetch_optional_record_by_field(
            &self.db,
            "discovered_archetype",
            "archetype_key",
            archetype_key,
        )
        .await
    }
}

fn causal_scope_key(scope: &ReasoningScope) -> String {
    scope_node_id(scope)
}
