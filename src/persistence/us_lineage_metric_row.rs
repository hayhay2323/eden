use serde::{Deserialize, Serialize};
use rust_decimal::Decimal;
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::persistence::us_lineage_snapshot::UsLineageSnapshotRecord;
use crate::us::temporal::lineage::{UsLineageContextStats, UsLineageStats};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsLineageMetricRowRecord {
    pub row_id: String,
    pub snapshot_id: String,
    pub tick_number: u64,
    #[serde(with = "rfc3339")]
    pub recorded_at: OffsetDateTime,
    pub window_size: usize,
    pub resolution_lag: u64,
    pub bucket: String,
    pub rank: usize,
    pub template: String,
    pub session: Option<String>,
    pub market_regime: Option<String>,
    pub total: usize,
    pub resolved: usize,
    pub hits: usize,
    pub hit_rate: Decimal,
    pub mean_return: Decimal,
    #[serde(default)]
    pub follow_expectancy: Decimal,
    #[serde(default)]
    pub fade_expectancy: Decimal,
    #[serde(default)]
    pub wait_expectancy: Decimal,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct UsLineageFilters {
    pub template: Option<String>,
    pub bucket: Option<String>,
    pub session: Option<String>,
    pub market_regime: Option<String>,
}

impl UsLineageMetricRowRecord {
    fn from_stat(
        snapshot_id: &str,
        tick_number: u64,
        recorded_at: OffsetDateTime,
        window_size: usize,
        resolution_lag: u64,
        bucket: &str,
        rank: usize,
        item: &UsLineageContextStats,
    ) -> Self {
        Self {
            row_id: format!("{}:{}:{}", snapshot_id, bucket, rank),
            snapshot_id: snapshot_id.into(),
            tick_number,
            recorded_at,
            window_size,
            resolution_lag,
            bucket: bucket.into(),
            rank,
            template: item.template.clone(),
            session: (!item.session.is_empty()).then_some(item.session.clone()),
            market_regime: (!item.market_regime.is_empty()).then_some(item.market_regime.clone()),
            total: item.total,
            resolved: item.resolved,
            hits: item.hits,
            hit_rate: item.hit_rate,
            mean_return: item.mean_return,
            follow_expectancy: item.follow_expectancy,
            fade_expectancy: item.fade_expectancy,
            wait_expectancy: item.wait_expectancy,
        }
    }
}

pub fn rows_from_us_lineage_stats(
    snapshot_id: &str,
    tick_number: u64,
    recorded_at: OffsetDateTime,
    window_size: usize,
    resolution_lag: u64,
    stats: &UsLineageStats,
) -> Vec<UsLineageMetricRowRecord> {
    let mut rows = Vec::new();
    rows.extend(stats.by_template.iter().enumerate().map(|(idx, item)| {
        UsLineageMetricRowRecord::from_stat(
            snapshot_id,
            tick_number,
            recorded_at,
            window_size,
            resolution_lag,
            "by_template",
            idx,
            item,
        )
    }));
    rows.extend(stats.by_context.iter().enumerate().map(|(idx, item)| {
        UsLineageMetricRowRecord::from_stat(
            snapshot_id,
            tick_number,
            recorded_at,
            window_size,
            resolution_lag,
            "by_context",
            idx,
            item,
        )
    }));
    rows
}

pub fn us_row_matches_filters(row: &UsLineageMetricRowRecord, filters: &UsLineageFilters) -> bool {
    matches_bucket(filters.bucket.as_deref(), &row.bucket)
        && matches_text(filters.template.as_deref(), &row.template)
        && matches_text(
            filters.session.as_deref(),
            row.session.as_deref().unwrap_or(""),
        )
        && matches_text(
            filters.market_regime.as_deref(),
            row.market_regime.as_deref().unwrap_or(""),
        )
}

pub fn us_snapshot_records_from_rows(
    rows: &[UsLineageMetricRowRecord],
    filters: &UsLineageFilters,
    latest_only: bool,
) -> Vec<UsLineageSnapshotRecord> {
    let mut grouped = Vec::<(String, Vec<UsLineageMetricRowRecord>)>::new();

    for row in rows
        .iter()
        .filter(|row| us_row_matches_filters(row, filters))
    {
        if let Some((_, bucket)) = grouped
            .iter_mut()
            .find(|(snapshot_id, _)| snapshot_id == &row.snapshot_id)
        {
            bucket.push(row.clone());
        } else {
            grouped.push((row.snapshot_id.clone(), vec![row.clone()]));
        }
    }

    let mut snapshots = grouped
        .into_iter()
        .filter_map(|(_, rows)| {
            let first = rows.first()?.clone();
            let stats = us_lineage_stats_from_rows(&rows);
            (!stats.is_empty()).then_some(UsLineageSnapshotRecord {
                snapshot_id: first.snapshot_id,
                tick_number: first.tick_number,
                recorded_at: first.recorded_at,
                window_size: first.window_size,
                resolution_lag: first.resolution_lag,
                stats,
            })
        })
        .collect::<Vec<_>>();

    if latest_only {
        snapshots.truncate(1);
    }

    snapshots
}

fn us_lineage_stats_from_rows(rows: &[UsLineageMetricRowRecord]) -> UsLineageStats {
    let mut stats = UsLineageStats::default();
    for row in rows {
        let item = UsLineageContextStats {
            template: row.template.clone(),
            session: row.session.clone().unwrap_or_default(),
            market_regime: row.market_regime.clone().unwrap_or_default(),
            total: row.total,
            resolved: row.resolved,
            hits: row.hits,
            hit_rate: row.hit_rate,
            mean_return: row.mean_return,
            follow_expectancy: row.follow_expectancy,
            fade_expectancy: row.fade_expectancy,
            wait_expectancy: row.wait_expectancy,
        };
        match row.bucket.as_str() {
            "by_template" => stats.by_template.push(item),
            "by_context" => stats.by_context.push(item),
            _ => {}
        }
    }
    stats
}

fn matches_text(filter: Option<&str>, value: &str) -> bool {
    match filter {
        None => true,
        Some(filter) => value
            .to_ascii_lowercase()
            .contains(&filter.to_ascii_lowercase()),
    }
}

fn matches_bucket(filter: Option<&str>, bucket: &str) -> bool {
    match filter {
        None => true,
        Some(filter) => filter.eq_ignore_ascii_case(bucket),
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    use super::*;

    #[test]
    fn us_rows_roundtrip_preserves_action_expectancies() {
        let stats = UsLineageStats {
            by_template: vec![UsLineageContextStats {
                template: "momentum_continuation".into(),
                session: String::new(),
                market_regime: String::new(),
                total: 3,
                resolved: 3,
                hits: 2,
                hit_rate: dec!(0.6667),
                mean_return: dec!(0.012),
                follow_expectancy: dec!(0.012),
                fade_expectancy: dec!(0.021),
                wait_expectancy: dec!(0),
            }],
            by_context: vec![],
        };

        let rows = rows_from_us_lineage_stats(
            "us_lineage:10:20",
            10,
            OffsetDateTime::UNIX_EPOCH,
            20,
            5,
            &stats,
        );
        assert_eq!(rows[0].follow_expectancy, dec!(0.012));
        assert_eq!(rows[0].fade_expectancy, dec!(0.021));

        let snapshots = us_snapshot_records_from_rows(&rows, &UsLineageFilters::default(), true);
        assert_eq!(snapshots.len(), 1);
        assert_eq!(
            snapshots[0].stats.by_template[0].follow_expectancy,
            dec!(0.012)
        );
        assert_eq!(
            snapshots[0].stats.by_template[0].fade_expectancy,
            dec!(0.021)
        );
    }
}
