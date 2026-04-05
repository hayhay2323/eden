use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::persistence::lineage_snapshot::LineageSnapshotRecord;
use crate::temporal::lineage::{
    matches_bucket, ContextualLineageOutcome, FamilyContextLineageOutcome, LineageFilters,
    LineageOutcome, LineageStats,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LineageMetricRowRecord {
    pub row_id: String,
    pub snapshot_id: String,
    pub tick_number: u64,
    #[serde(with = "rfc3339")]
    pub recorded_at: OffsetDateTime,
    pub window_size: usize,
    pub bucket: String,
    pub rank: usize,
    pub label: String,
    pub family: Option<String>,
    pub session: Option<String>,
    pub market_regime: Option<String>,
    pub total: usize,
    pub resolved: usize,
    pub hits: usize,
    pub hit_rate: Decimal,
    pub mean_return: Decimal,
    pub mean_net_return: Decimal,
    #[serde(default)]
    pub follow_expectancy: Decimal,
    #[serde(default)]
    pub fade_expectancy: Decimal,
    #[serde(default)]
    pub wait_expectancy: Decimal,
    pub mean_mfe: Decimal,
    pub mean_mae: Decimal,
    pub follow_through_rate: Decimal,
    pub invalidation_rate: Decimal,
    pub structure_retention_rate: Decimal,
    pub mean_convergence_score: Decimal,
    pub mean_external_delta: Decimal,
    pub external_follow_through_rate: Decimal,
}

impl LineageMetricRowRecord {
    fn outcome_row(
        snapshot_id: &str,
        tick_number: u64,
        recorded_at: OffsetDateTime,
        window_size: usize,
        bucket: &str,
        rank: usize,
        item: &LineageOutcome,
    ) -> Self {
        Self {
            row_id: format!("{}:{}:{}", snapshot_id, bucket, rank),
            snapshot_id: snapshot_id.into(),
            tick_number,
            recorded_at,
            window_size,
            bucket: bucket.into(),
            rank,
            label: item.label.clone(),
            family: None,
            session: None,
            market_regime: None,
            total: item.total,
            resolved: item.resolved,
            hits: item.hits,
            hit_rate: item.hit_rate,
            mean_return: item.mean_return,
            mean_net_return: item.mean_net_return,
            follow_expectancy: item.follow_expectancy,
            fade_expectancy: item.fade_expectancy,
            wait_expectancy: item.wait_expectancy,
            mean_mfe: item.mean_mfe,
            mean_mae: item.mean_mae,
            follow_through_rate: item.follow_through_rate,
            invalidation_rate: item.invalidation_rate,
            structure_retention_rate: item.structure_retention_rate,
            mean_convergence_score: item.mean_convergence_score,
            mean_external_delta: item.mean_external_delta,
            external_follow_through_rate: item.external_follow_through_rate,
        }
    }

    fn contextual_row(
        snapshot_id: &str,
        tick_number: u64,
        recorded_at: OffsetDateTime,
        window_size: usize,
        bucket: &str,
        rank: usize,
        item: &ContextualLineageOutcome,
    ) -> Self {
        Self {
            row_id: format!("{}:{}:{}", snapshot_id, bucket, rank),
            snapshot_id: snapshot_id.into(),
            tick_number,
            recorded_at,
            window_size,
            bucket: bucket.into(),
            rank,
            label: item.label.clone(),
            family: Some(item.family.clone()),
            session: Some(item.session.clone()),
            market_regime: Some(item.market_regime.clone()),
            total: item.total,
            resolved: item.resolved,
            hits: item.hits,
            hit_rate: item.hit_rate,
            mean_return: item.mean_return,
            mean_net_return: item.mean_net_return,
            follow_expectancy: item.follow_expectancy,
            fade_expectancy: item.fade_expectancy,
            wait_expectancy: item.wait_expectancy,
            mean_mfe: item.mean_mfe,
            mean_mae: item.mean_mae,
            follow_through_rate: item.follow_through_rate,
            invalidation_rate: item.invalidation_rate,
            structure_retention_rate: item.structure_retention_rate,
            mean_convergence_score: item.mean_convergence_score,
            mean_external_delta: item.mean_external_delta,
            external_follow_through_rate: item.external_follow_through_rate,
        }
    }

    fn family_context_row(
        snapshot_id: &str,
        tick_number: u64,
        recorded_at: OffsetDateTime,
        window_size: usize,
        bucket: &str,
        rank: usize,
        item: &FamilyContextLineageOutcome,
    ) -> Self {
        Self {
            row_id: format!("{}:{}:{}", snapshot_id, bucket, rank),
            snapshot_id: snapshot_id.into(),
            tick_number,
            recorded_at,
            window_size,
            bucket: bucket.into(),
            rank,
            label: item.family.clone(),
            family: Some(item.family.clone()),
            session: Some(item.session.clone()),
            market_regime: Some(item.market_regime.clone()),
            total: item.total,
            resolved: item.resolved,
            hits: item.hits,
            hit_rate: item.hit_rate,
            mean_return: item.mean_return,
            mean_net_return: item.mean_net_return,
            follow_expectancy: item.follow_expectancy,
            fade_expectancy: item.fade_expectancy,
            wait_expectancy: item.wait_expectancy,
            mean_mfe: item.mean_mfe,
            mean_mae: item.mean_mae,
            follow_through_rate: item.follow_through_rate,
            invalidation_rate: item.invalidation_rate,
            structure_retention_rate: item.structure_retention_rate,
            mean_convergence_score: item.mean_convergence_score,
            mean_external_delta: item.mean_external_delta,
            external_follow_through_rate: item.external_follow_through_rate,
        }
    }
}

pub fn rows_from_lineage_stats(
    snapshot_id: &str,
    tick_number: u64,
    recorded_at: OffsetDateTime,
    window_size: usize,
    stats: &LineageStats,
) -> Vec<LineageMetricRowRecord> {
    let mut rows = Vec::new();

    rows.extend(
        stats
            .promoted_outcomes
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                LineageMetricRowRecord::outcome_row(
                    snapshot_id,
                    tick_number,
                    recorded_at,
                    window_size,
                    "promoted_outcomes",
                    idx,
                    item,
                )
            }),
    );
    rows.extend(
        stats
            .blocked_outcomes
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                LineageMetricRowRecord::outcome_row(
                    snapshot_id,
                    tick_number,
                    recorded_at,
                    window_size,
                    "blocked_outcomes",
                    idx,
                    item,
                )
            }),
    );
    rows.extend(
        stats
            .falsified_outcomes
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                LineageMetricRowRecord::outcome_row(
                    snapshot_id,
                    tick_number,
                    recorded_at,
                    window_size,
                    "falsified_outcomes",
                    idx,
                    item,
                )
            }),
    );
    rows.extend(
        stats
            .promoted_contexts
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                LineageMetricRowRecord::contextual_row(
                    snapshot_id,
                    tick_number,
                    recorded_at,
                    window_size,
                    "promoted_contexts",
                    idx,
                    item,
                )
            }),
    );
    rows.extend(
        stats
            .blocked_contexts
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                LineageMetricRowRecord::contextual_row(
                    snapshot_id,
                    tick_number,
                    recorded_at,
                    window_size,
                    "blocked_contexts",
                    idx,
                    item,
                )
            }),
    );
    rows.extend(
        stats
            .falsified_contexts
            .iter()
            .enumerate()
            .map(|(idx, item)| {
                LineageMetricRowRecord::contextual_row(
                    snapshot_id,
                    tick_number,
                    recorded_at,
                    window_size,
                    "falsified_contexts",
                    idx,
                    item,
                )
            }),
    );
    rows.extend(stats.family_contexts.iter().enumerate().map(|(idx, item)| {
        LineageMetricRowRecord::family_context_row(
            snapshot_id,
            tick_number,
            recorded_at,
            window_size,
            "family_contexts",
            idx,
            item,
        )
    }));

    rows
}

pub fn row_matches_filters(row: &LineageMetricRowRecord, filters: &LineageFilters) -> bool {
    matches_bucket(filters.bucket.as_deref(), &row.bucket)
        && matches_text(filters.label.as_deref(), &row.label)
        && matches_text(
            filters.family.as_deref(),
            row.family.as_deref().unwrap_or(""),
        )
        && matches_text(
            filters.session.as_deref(),
            row.session.as_deref().unwrap_or(""),
        )
        && matches_text(
            filters.market_regime.as_deref(),
            row.market_regime.as_deref().unwrap_or(""),
        )
}

pub fn snapshot_records_from_rows(
    rows: &[LineageMetricRowRecord],
    filters: &LineageFilters,
    latest_only: bool,
) -> Vec<LineageSnapshotRecord> {
    let mut grouped = std::collections::HashMap::<String, Vec<LineageMetricRowRecord>>::new();
    let mut snapshot_order = Vec::<String>::new();

    for row in rows.iter().filter(|row| row_matches_filters(row, filters)) {
        let entry = grouped.entry(row.snapshot_id.clone()).or_insert_with(|| {
            snapshot_order.push(row.snapshot_id.clone());
            Vec::new()
        });
        entry.push(row.clone());
    }

    let mut snapshots = snapshot_order
        .into_iter()
        .filter_map(|snapshot_id| {
            let rows = grouped.remove(&snapshot_id)?;
            let first = rows.first()?.clone();
            let stats = lineage_stats_from_rows(&rows);
            (!stats.is_empty()).then_some(LineageSnapshotRecord {
                snapshot_id: first.snapshot_id,
                tick_number: first.tick_number,
                recorded_at: first.recorded_at,
                window_size: first.window_size,
                stats,
            })
        })
        .collect::<Vec<_>>();

    if latest_only {
        snapshots.truncate(1);
    }

    snapshots
}

fn lineage_stats_from_rows(rows: &[LineageMetricRowRecord]) -> LineageStats {
    let mut stats = LineageStats::default();

    for row in rows {
        match row.bucket.as_str() {
            "promoted_outcomes" => stats.promoted_outcomes.push(parse_outcome_row(row)),
            "blocked_outcomes" => stats.blocked_outcomes.push(parse_outcome_row(row)),
            "falsified_outcomes" => stats.falsified_outcomes.push(parse_outcome_row(row)),
            "promoted_contexts" => stats.promoted_contexts.push(parse_contextual_row(row)),
            "blocked_contexts" => stats.blocked_contexts.push(parse_contextual_row(row)),
            "falsified_contexts" => stats.falsified_contexts.push(parse_contextual_row(row)),
            "family_contexts" => stats.family_contexts.push(parse_family_context_row(row)),
            _ => {}
        }
    }

    stats
}

fn parse_outcome_row(row: &LineageMetricRowRecord) -> LineageOutcome {
    LineageOutcome {
        label: row.label.clone(),
        total: row.total,
        resolved: row.resolved,
        hits: row.hits,
        hit_rate: row.hit_rate,
        mean_return: row.mean_return,
        mean_net_return: row.mean_net_return,
        follow_expectancy: row.follow_expectancy,
        fade_expectancy: row.fade_expectancy,
        wait_expectancy: row.wait_expectancy,
        mean_mfe: row.mean_mfe,
        mean_mae: row.mean_mae,
        follow_through_rate: row.follow_through_rate,
        invalidation_rate: row.invalidation_rate,
        structure_retention_rate: row.structure_retention_rate,
        mean_convergence_score: row.mean_convergence_score,
        mean_external_delta: row.mean_external_delta,
        external_follow_through_rate: row.external_follow_through_rate,
    }
}

fn parse_contextual_row(row: &LineageMetricRowRecord) -> ContextualLineageOutcome {
    ContextualLineageOutcome {
        label: row.label.clone(),
        family: row.family.clone().unwrap_or_default(),
        session: row.session.clone().unwrap_or_default(),
        market_regime: row.market_regime.clone().unwrap_or_default(),
        total: row.total,
        resolved: row.resolved,
        hits: row.hits,
        hit_rate: row.hit_rate,
        mean_return: row.mean_return,
        mean_net_return: row.mean_net_return,
        follow_expectancy: row.follow_expectancy,
        fade_expectancy: row.fade_expectancy,
        wait_expectancy: row.wait_expectancy,
        mean_mfe: row.mean_mfe,
        mean_mae: row.mean_mae,
        follow_through_rate: row.follow_through_rate,
        invalidation_rate: row.invalidation_rate,
        structure_retention_rate: row.structure_retention_rate,
        mean_convergence_score: row.mean_convergence_score,
        mean_external_delta: row.mean_external_delta,
        external_follow_through_rate: row.external_follow_through_rate,
    }
}

fn parse_family_context_row(row: &LineageMetricRowRecord) -> FamilyContextLineageOutcome {
    FamilyContextLineageOutcome {
        family: row.family.clone().unwrap_or_default(),
        session: row.session.clone().unwrap_or_default(),
        market_regime: row.market_regime.clone().unwrap_or_default(),
        total: row.total,
        resolved: row.resolved,
        hits: row.hits,
        hit_rate: row.hit_rate,
        mean_return: row.mean_return,
        mean_net_return: row.mean_net_return,
        mean_mfe: row.mean_mfe,
        mean_mae: row.mean_mae,
        follow_through_rate: row.follow_through_rate,
        invalidation_rate: row.invalidation_rate,
        structure_retention_rate: row.structure_retention_rate,
        mean_convergence_score: row.mean_convergence_score,
        mean_external_delta: row.mean_external_delta,
        external_follow_through_rate: row.external_follow_through_rate,
        follow_expectancy: row.follow_expectancy,
        fade_expectancy: row.fade_expectancy,
        wait_expectancy: row.wait_expectancy,
    }
}

fn matches_text(filter: Option<&str>, value: &str) -> bool {
    match filter {
        None => true,
        Some(filter) => value
            .to_ascii_lowercase()
            .contains(&filter.to_ascii_lowercase()),
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;

    #[test]
    fn rows_from_lineage_stats_flattens_outcomes_and_contexts() {
        let stats = LineageStats {
            promoted_outcomes: vec![LineageOutcome {
                label: "review -> enter".into(),
                total: 1,
                resolved: 1,
                hits: 1,
                hit_rate: dec!(1),
                mean_return: dec!(0.02),
                mean_net_return: dec!(0.015),
                follow_expectancy: dec!(0.015),
                fade_expectancy: dec!(-0.015),
                wait_expectancy: dec!(0),
                mean_mfe: dec!(0.03),
                mean_mae: dec!(-0.01),
                follow_through_rate: dec!(1),
                invalidation_rate: dec!(0),
                structure_retention_rate: dec!(1),
                mean_convergence_score: dec!(0.78),
                mean_external_delta: dec!(0.05),
                external_follow_through_rate: dec!(1),
            }],
            promoted_contexts: vec![ContextualLineageOutcome {
                label: "review -> enter".into(),
                family: "Directed Flow".into(),
                session: "opening".into(),
                market_regime: "risk_on".into(),
                total: 1,
                resolved: 1,
                hits: 1,
                hit_rate: dec!(1),
                mean_return: dec!(0.02),
                mean_net_return: dec!(0.015),
                follow_expectancy: dec!(0.015),
                fade_expectancy: dec!(-0.015),
                wait_expectancy: dec!(0),
                mean_mfe: dec!(0.03),
                mean_mae: dec!(-0.01),
                follow_through_rate: dec!(1),
                invalidation_rate: dec!(0),
                structure_retention_rate: dec!(1),
                mean_convergence_score: dec!(0.78),
                mean_external_delta: dec!(0.05),
                external_follow_through_rate: dec!(1),
            }],
            ..LineageStats::default()
        };

        let rows =
            rows_from_lineage_stats("lineage:42:50", 42, OffsetDateTime::UNIX_EPOCH, 50, &stats);

        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].bucket, "promoted_outcomes");
        assert_eq!(rows[1].bucket, "promoted_contexts");
        assert_eq!(rows[1].family.as_deref(), Some("Directed Flow"));
        assert_eq!(rows[0].follow_expectancy, dec!(0.015));
        assert_eq!(rows[0].fade_expectancy, dec!(-0.015));
    }

    #[test]
    fn snapshot_records_from_rows_filters_and_groups() {
        let stats = LineageStats {
            promoted_contexts: vec![ContextualLineageOutcome {
                label: "review -> enter".into(),
                family: "Directed Flow".into(),
                session: "opening".into(),
                market_regime: "risk_on".into(),
                total: 1,
                resolved: 1,
                hits: 1,
                hit_rate: dec!(1),
                mean_return: dec!(0.02),
                mean_net_return: dec!(0.015),
                follow_expectancy: dec!(0.015),
                fade_expectancy: dec!(-0.015),
                wait_expectancy: dec!(0),
                mean_mfe: dec!(0.03),
                mean_mae: dec!(-0.01),
                follow_through_rate: dec!(1),
                invalidation_rate: dec!(0),
                structure_retention_rate: dec!(1),
                mean_convergence_score: dec!(0.78),
                mean_external_delta: dec!(0.05),
                external_follow_through_rate: dec!(1),
            }],
            ..LineageStats::default()
        };

        let rows = rows_from_lineage_stats(
            "lineage:100:50",
            100,
            OffsetDateTime::UNIX_EPOCH,
            50,
            &stats,
        );
        let snapshots = snapshot_records_from_rows(
            &rows,
            &LineageFilters {
                bucket: Some("promoted_contexts".into()),
                family: Some("flow".into()),
                ..LineageFilters::default()
            },
            true,
        );

        assert_eq!(snapshots.len(), 1);
        assert_eq!(snapshots[0].tick_number, 100);
        assert_eq!(snapshots[0].stats.promoted_contexts.len(), 1);
    }

    #[test]
    fn rows_from_lineage_stats_includes_family_context_bucket() {
        let stats = LineageStats {
            family_contexts: vec![FamilyContextLineageOutcome {
                family: "Breakout Contagion".into(),
                session: "midday".into(),
                market_regime: "stress-dominant".into(),
                total: 4,
                resolved: 3,
                hits: 1,
                hit_rate: dec!(0.3333),
                mean_return: dec!(-0.01),
                mean_net_return: dec!(-0.012),
                mean_mfe: dec!(0.01),
                mean_mae: dec!(-0.03),
                follow_through_rate: dec!(0.2),
                invalidation_rate: dec!(0.7),
                structure_retention_rate: dec!(0.3),
                mean_convergence_score: dec!(0.41),
                mean_external_delta: dec!(-0.02),
                external_follow_through_rate: dec!(0.1),
                follow_expectancy: dec!(-0.012),
                fade_expectancy: dec!(0.024),
                wait_expectancy: dec!(0),
            }],
            ..LineageStats::default()
        };

        let rows =
            rows_from_lineage_stats("lineage:9:50", 9, OffsetDateTime::UNIX_EPOCH, 50, &stats);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].bucket, "family_contexts");
        assert_eq!(rows[0].family.as_deref(), Some("Breakout Contagion"));
        assert_eq!(rows[0].fade_expectancy, dec!(0.024));

        let snapshots = snapshot_records_from_rows(&rows, &LineageFilters::default(), true);
        assert_eq!(snapshots[0].stats.family_contexts.len(), 1);
        assert_eq!(
            snapshots[0].stats.family_contexts[0].fade_expectancy,
            dec!(0.024)
        );
    }
}
