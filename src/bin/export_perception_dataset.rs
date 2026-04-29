#[cfg(feature = "persistence")]
use std::io::Write;

#[cfg(feature = "persistence")]
use surrealdb::engine::local::{Db, RocksDb};
#[cfg(feature = "persistence")]
use surrealdb::Surreal;

#[cfg(feature = "persistence")]
use rust_decimal::Decimal;
#[cfg(feature = "persistence")]
use serde_json::Value;

#[cfg(feature = "persistence")]
use eden::pipeline::state_labeler::{label_outcome, OutcomeLabelInput, StateLabel};

#[cfg(feature = "persistence")]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportFormat {
    Csv,
    Jsonl,
}

#[cfg(feature = "persistence")]
impl ExportFormat {
    fn parse(raw: Option<&str>) -> Self {
        match raw.unwrap_or("csv").to_ascii_lowercase().as_str() {
            "jsonl" => Self::Jsonl,
            _ => Self::Csv,
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Csv => "csv",
            Self::Jsonl => "jsonl",
        }
    }
}

#[cfg(feature = "persistence")]
#[derive(Debug, Clone, serde::Serialize)]
struct PerceptionDatasetRow {
    assessment_id: String,
    setup_id: String,
    workflow_id: Option<String>,
    market: String,
    symbol: String,
    recorded_at: String,
    source: String,
    family_label: Option<String>,
    sector: Option<String>,
    recommended_action: String,
    workflow_state: String,
    market_regime_bias: Option<String>,
    market_regime_confidence: Option<String>,
    freshness_state: Option<String>,
    timing_state: Option<String>,
    timing_position_in_range: Option<String>,
    local_state: Option<String>,
    local_state_confidence: Option<String>,
    actionability_score: Option<String>,
    actionability_state: Option<String>,
    state_persistence_ticks: Option<u16>,
    direction_stability_rounds: Option<u16>,
    state_reason_codes: String,
    state_label: Option<String>,
    state_label_confidence: Option<String>,
    state_label_horizon: Option<String>,
    ticks_to_resolution: Option<u64>,
    state_label_reason_codes: String,
    primary_mechanism_kind: Option<String>,
    primary_mechanism_score: Option<String>,
    law_kinds: String,
    predicate_kinds: String,
    composite_state_kinds: String,
    outcome_present: bool,
    outcome_net_return: Option<String>,
    outcome_return_pct: Option<String>,
    outcome_followed_through: Option<bool>,
    outcome_invalidated: Option<bool>,
    outcome_structure_retained: Option<bool>,
    outcome_resolved_at: Option<String>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Clone, serde::Serialize)]
struct ActionabilityBucketStat {
    bucket: String,
    samples: usize,
    mean_score: String,
    mean_net_return: String,
    hit_rate: String,
    invalidation_rate: String,
    stale_ratio: String,
    low_information_ratio: String,
    actionable_ratio: String,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Clone, serde::Serialize)]
struct ActionabilityCalibrationReport {
    market: String,
    rows_scanned: usize,
    latest_rows: usize,
    rows_with_outcomes: usize,
    bucket_width: String,
    buckets: Vec<ActionabilityBucketStat>,
    state_label_stats: Vec<StateLabelStat>,
}

#[cfg(feature = "persistence")]
#[derive(Debug, Clone, serde::Serialize)]
struct StateLabelStat {
    label: String,
    horizon: String,
    samples: usize,
    mean_confidence: String,
    mean_net_return: String,
    hit_rate: String,
    invalidation_rate: String,
}

#[cfg(feature = "persistence")]
impl PerceptionDatasetRow {
    fn csv_header() -> &'static [&'static str] {
        &[
            "assessment_id",
            "setup_id",
            "workflow_id",
            "market",
            "symbol",
            "recorded_at",
            "source",
            "family_label",
            "sector",
            "recommended_action",
            "workflow_state",
            "market_regime_bias",
            "market_regime_confidence",
            "freshness_state",
            "timing_state",
            "timing_position_in_range",
            "local_state",
            "local_state_confidence",
            "actionability_score",
            "actionability_state",
            "state_persistence_ticks",
            "direction_stability_rounds",
            "state_reason_codes",
            "state_label",
            "state_label_confidence",
            "state_label_horizon",
            "ticks_to_resolution",
            "state_label_reason_codes",
            "primary_mechanism_kind",
            "primary_mechanism_score",
            "law_kinds",
            "predicate_kinds",
            "composite_state_kinds",
            "outcome_present",
            "outcome_net_return",
            "outcome_return_pct",
            "outcome_followed_through",
            "outcome_invalidated",
            "outcome_structure_retained",
            "outcome_resolved_at",
        ]
    }

    fn csv_values(&self) -> Vec<String> {
        vec![
            self.assessment_id.clone(),
            self.setup_id.clone(),
            self.workflow_id.clone().unwrap_or_default(),
            self.market.clone(),
            self.symbol.clone(),
            self.recorded_at.clone(),
            self.source.clone(),
            self.family_label.clone().unwrap_or_default(),
            self.sector.clone().unwrap_or_default(),
            self.recommended_action.clone(),
            self.workflow_state.clone(),
            self.market_regime_bias.clone().unwrap_or_default(),
            self.market_regime_confidence.clone().unwrap_or_default(),
            self.freshness_state.clone().unwrap_or_default(),
            self.timing_state.clone().unwrap_or_default(),
            self.timing_position_in_range.clone().unwrap_or_default(),
            self.local_state.clone().unwrap_or_default(),
            self.local_state_confidence.clone().unwrap_or_default(),
            self.actionability_score.clone().unwrap_or_default(),
            self.actionability_state.clone().unwrap_or_default(),
            self.state_persistence_ticks
                .map(|value| value.to_string())
                .unwrap_or_default(),
            self.direction_stability_rounds
                .map(|value| value.to_string())
                .unwrap_or_default(),
            self.state_reason_codes.clone(),
            self.state_label.clone().unwrap_or_default(),
            self.state_label_confidence.clone().unwrap_or_default(),
            self.state_label_horizon.clone().unwrap_or_default(),
            self.ticks_to_resolution
                .map(|value| value.to_string())
                .unwrap_or_default(),
            self.state_label_reason_codes.clone(),
            self.primary_mechanism_kind.clone().unwrap_or_default(),
            self.primary_mechanism_score.clone().unwrap_or_default(),
            self.law_kinds.clone(),
            self.predicate_kinds.clone(),
            self.composite_state_kinds.clone(),
            self.outcome_present.to_string(),
            self.outcome_net_return.clone().unwrap_or_default(),
            self.outcome_return_pct.clone().unwrap_or_default(),
            self.outcome_followed_through
                .map(|value| value.to_string())
                .unwrap_or_default(),
            self.outcome_invalidated
                .map(|value| value.to_string())
                .unwrap_or_default(),
            self.outcome_structure_retained
                .map(|value| value.to_string())
                .unwrap_or_default(),
            self.outcome_resolved_at.clone().unwrap_or_default(),
        ]
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    #[cfg(not(feature = "persistence"))]
    {
        eprintln!("export_perception_dataset requires building with `--features persistence`");
        return Ok(());
    }

    #[cfg(feature = "persistence")]
    {
        let args = std::env::args().collect::<Vec<_>>();
        let market = parse_flag_str(&args, "--market").unwrap_or_else(|| "hk".into());
        let limit: usize = parse_flag(&args, "--limit").unwrap_or(5000);
        let format = ExportFormat::parse(parse_flag_str(&args, "--format").as_deref());
        let output = parse_flag_str(&args, "--output").unwrap_or_else(|| {
            format!("data/perception_dataset_{}.{}", market, format.extension())
        });
        let report_output = parse_flag_str(&args, "--report-output");

        let db: Surreal<Db> = Surreal::new::<RocksDb>("data/eden.db").await?;
        db.use_ns("eden").use_db("market").await?;

        let mut assessment_res = db
            .query(
                "SELECT \
                    assessment_id, \
                    setup_id, \
                    workflow_id, \
                    market, \
                    symbol, \
                    recorded_at, \
                    source, \
                    family_label, \
                    sector, \
                    recommended_action, \
                    workflow_state, \
                    market_regime_bias, \
                    market_regime_confidence, \
                    freshness_state, \
                    timing_state, \
                    timing_position_in_range, \
                    local_state, \
                    local_state_confidence, \
                    actionability_score, \
                    actionability_state, \
                    state_persistence_ticks, \
                    direction_stability_rounds, \
                    state_reason_codes, \
                    primary_mechanism_kind, \
                    primary_mechanism_score, \
                    law_kinds, \
                    predicate_kinds, \
                    composite_state_kinds \
                 FROM case_reasoning_assessment \
                 WHERE market = $market \
                 ORDER BY recorded_at DESC LIMIT $limit",
            )
            .bind(("market", market.clone()))
            .bind(("limit", limit))
            .await?;
        let assessment_rows: Vec<Value> = assessment_res.take(0)?;

        let mut outcome_res = db
            .query(
                "SELECT \
                    setup_id, \
                    entry_tick, \
                    resolved_tick, \
                    net_return, \
                    return_pct, \
                    max_favorable_excursion, \
                    max_adverse_excursion, \
                    followed_through, \
                    invalidated, \
                    structure_retained, \
                    resolved_at \
                 FROM case_realized_outcome \
                 WHERE market = $market \
                 ORDER BY resolved_at DESC LIMIT $limit",
            )
            .bind(("market", market.clone()))
            .bind(("limit", limit))
            .await?;
        let outcome_rows: Vec<Value> = outcome_res.take(0)?;

        let outcome_by_setup = outcome_rows
            .iter()
            .filter_map(|item| {
                item.get("setup_id")
                    .and_then(|value| value.as_str())
                    .map(|setup_id| (setup_id.to_string(), item))
            })
            .collect::<std::collections::HashMap<_, _>>();

        let mut rows = assessment_rows
            .iter()
            .filter(|record| record_has_perception_fields(record))
            .filter_map(|record| {
                let setup_id = record.get("setup_id").and_then(|value| value.as_str())?;
                Some(row_from_values(
                    record,
                    outcome_by_setup.get(setup_id).copied(),
                ))
            })
            .collect::<Vec<_>>();
        rows.sort_by(|a, b| a.recorded_at.cmp(&b.recorded_at));

        write_rows(&output, format, &rows)?;
        if let Some(report_output) = report_output {
            let report = build_actionability_calibration_report(&market, &rows);
            write_calibration_report(&report_output, &report)?;
            eprintln!("wrote actionability report to {}", report_output);
        }
        eprintln!(
            "exported {} perception rows for market={} to {}",
            rows.len(),
            market,
            output
        );
        Ok(())
    }
}

#[cfg(feature = "persistence")]
fn row_from_values(record: &Value, outcome: Option<&Value>) -> PerceptionDatasetRow {
    let derived_state_label = outcome.and_then(derive_state_label);
    PerceptionDatasetRow {
        assessment_id: string_field(record, "assessment_id"),
        setup_id: string_field(record, "setup_id"),
        workflow_id: optional_string_field(record, "workflow_id"),
        market: string_field(record, "market"),
        symbol: string_field(record, "symbol"),
        recorded_at: string_field(record, "recorded_at"),
        source: string_field(record, "source"),
        family_label: optional_string_field(record, "family_label"),
        sector: optional_string_field(record, "sector"),
        recommended_action: string_field(record, "recommended_action"),
        workflow_state: string_field(record, "workflow_state"),
        market_regime_bias: optional_string_field(record, "market_regime_bias"),
        market_regime_confidence: optional_numeric_string_field(record, "market_regime_confidence"),
        freshness_state: optional_string_field(record, "freshness_state"),
        timing_state: optional_string_field(record, "timing_state"),
        timing_position_in_range: optional_numeric_string_field(record, "timing_position_in_range"),
        local_state: optional_string_field(record, "local_state"),
        local_state_confidence: optional_numeric_string_field(record, "local_state_confidence"),
        actionability_score: optional_numeric_string_field(record, "actionability_score"),
        actionability_state: optional_string_field(record, "actionability_state"),
        state_persistence_ticks: optional_u16_field(record, "state_persistence_ticks"),
        direction_stability_rounds: optional_u16_field(record, "direction_stability_rounds"),
        state_reason_codes: string_array_field(record, "state_reason_codes").join("|"),
        state_label: derived_state_label
            .as_ref()
            .map(|label| label.label.as_str().to_string()),
        state_label_confidence: derived_state_label
            .as_ref()
            .map(|label| label.confidence.round_dp(4).to_string()),
        state_label_horizon: derived_state_label
            .as_ref()
            .map(|label| label.horizon.as_str().to_string()),
        ticks_to_resolution: derived_state_label
            .as_ref()
            .map(|label| label.ticks_to_resolution),
        state_label_reason_codes: derived_state_label
            .as_ref()
            .map(|label| label.reason_codes.join("|"))
            .unwrap_or_default(),
        primary_mechanism_kind: optional_string_field(record, "primary_mechanism_kind"),
        primary_mechanism_score: optional_numeric_string_field(record, "primary_mechanism_score"),
        law_kinds: string_array_field(record, "law_kinds").join("|"),
        predicate_kinds: string_array_field(record, "predicate_kinds").join("|"),
        composite_state_kinds: string_array_field(record, "composite_state_kinds").join("|"),
        outcome_present: outcome.is_some(),
        outcome_net_return: outcome.and_then(|v| optional_numeric_string_field(v, "net_return")),
        outcome_return_pct: outcome.and_then(|v| optional_numeric_string_field(v, "return_pct")),
        outcome_followed_through: outcome
            .and_then(|v| v.get("followed_through"))
            .and_then(|v| v.as_bool()),
        outcome_invalidated: outcome
            .and_then(|v| v.get("invalidated"))
            .and_then(|v| v.as_bool()),
        outcome_structure_retained: outcome
            .and_then(|v| v.get("structure_retained"))
            .and_then(|v| v.as_bool()),
        outcome_resolved_at: outcome.and_then(|v| optional_string_field(v, "resolved_at")),
    }
}

#[cfg(feature = "persistence")]
fn string_field(value: &Value, field: &str) -> String {
    value
        .get(field)
        .and_then(|item| item.as_str())
        .unwrap_or_default()
        .to_string()
}

#[cfg(feature = "persistence")]
fn optional_string_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(|item| item.as_str())
        .map(str::to_string)
}

#[cfg(feature = "persistence")]
fn optional_numeric_string_field(value: &Value, field: &str) -> Option<String> {
    value.get(field).and_then(|item| match item {
        Value::String(raw) => Some(raw.clone()),
        Value::Number(raw) => Some(raw.to_string()),
        _ => None,
    })
}

#[cfg(feature = "persistence")]
fn optional_u16_field(value: &Value, field: &str) -> Option<u16> {
    value
        .get(field)
        .and_then(|item| item.as_u64())
        .map(|v| v as u16)
}

#[cfg(feature = "persistence")]
fn optional_u64_field(value: &Value, field: &str) -> Option<u64> {
    value.get(field).and_then(|item| item.as_u64())
}

#[cfg(feature = "persistence")]
fn string_array_field(value: &Value, field: &str) -> Vec<String> {
    value
        .get(field)
        .and_then(|item| item.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_string))
                .collect()
        })
        .unwrap_or_default()
}

#[cfg(feature = "persistence")]
fn record_has_perception_fields(record: &Value) -> bool {
    optional_string_field(record, "local_state").is_some()
        || optional_string_field(record, "actionability_state").is_some()
        || optional_numeric_string_field(record, "actionability_score").is_some()
        || optional_string_field(record, "freshness_state").is_some()
        || optional_string_field(record, "timing_state").is_some()
}

#[cfg(feature = "persistence")]
fn derive_state_label(outcome: &Value) -> Option<StateLabel> {
    let entry_tick = optional_u64_field(outcome, "entry_tick")?;
    let resolved_tick = optional_u64_field(outcome, "resolved_tick")?;
    let net_return = optional_numeric_string_field(outcome, "net_return")
        .as_deref()
        .and_then(parse_decimal_string)?;
    let max_favorable_excursion = optional_numeric_string_field(outcome, "max_favorable_excursion")
        .as_deref()
        .and_then(parse_decimal_string)?;
    let max_adverse_excursion = optional_numeric_string_field(outcome, "max_adverse_excursion")
        .as_deref()
        .and_then(parse_decimal_string)?;
    let followed_through = outcome.get("followed_through")?.as_bool()?;
    let invalidated = outcome.get("invalidated")?.as_bool()?;
    let structure_retained = outcome.get("structure_retained")?.as_bool()?;
    Some(label_outcome(OutcomeLabelInput {
        entry_tick,
        resolved_tick,
        net_return,
        max_favorable_excursion,
        max_adverse_excursion,
        followed_through,
        invalidated,
        structure_retained,
    }))
}

#[cfg(feature = "persistence")]
fn write_rows(
    output: &str,
    format: ExportFormat,
    rows: &[PerceptionDatasetRow],
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(parent) = std::path::Path::new(output).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    match format {
        ExportFormat::Jsonl => {
            let mut file = std::fs::File::create(output)?;
            for row in rows {
                writeln!(file, "{}", serde_json::to_string(row)?)?;
            }
        }
        ExportFormat::Csv => {
            let mut file = std::fs::File::create(output)?;
            writeln!(
                file,
                "{}",
                PerceptionDatasetRow::csv_header()
                    .iter()
                    .map(|item| csv_escape(item))
                    .collect::<Vec<_>>()
                    .join(",")
            )?;
            for row in rows {
                writeln!(
                    file,
                    "{}",
                    row.csv_values()
                        .into_iter()
                        .map(|item| csv_escape(&item))
                        .collect::<Vec<_>>()
                        .join(",")
                )?;
            }
        }
    }
    Ok(())
}

#[cfg(feature = "persistence")]
fn csv_escape(value: &str) -> String {
    if value.contains(',') || value.contains('"') || value.contains('\n') {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_string()
    }
}

#[cfg(feature = "persistence")]
fn build_actionability_calibration_report(
    market: &str,
    rows: &[PerceptionDatasetRow],
) -> ActionabilityCalibrationReport {
    let latest_rows = latest_runtime_rows_by_setup(rows);
    let rows_with_outcomes = latest_rows
        .iter()
        .filter(|row| row.outcome_present)
        .cloned()
        .collect::<Vec<_>>();
    let bucket_width = Decimal::new(2, 1); // 0.2
    let bucket_edges = [
        Decimal::ZERO,
        Decimal::new(2, 1),
        Decimal::new(4, 1),
        Decimal::new(6, 1),
        Decimal::new(8, 1),
        Decimal::ONE,
    ];

    let mut buckets = Vec::new();
    for window in bucket_edges.windows(2) {
        let lower = window[0];
        let upper = window[1];
        let members = rows_with_outcomes
            .iter()
            .filter(|row| {
                let Some(score) = row
                    .actionability_score
                    .as_deref()
                    .and_then(parse_decimal_string)
                else {
                    return false;
                };
                if upper == Decimal::ONE {
                    score >= lower && score <= upper
                } else {
                    score >= lower && score < upper
                }
            })
            .cloned()
            .collect::<Vec<_>>();
        if members.is_empty() {
            continue;
        }

        let mean_score = decimal_mean(
            members
                .iter()
                .filter_map(|row| {
                    row.actionability_score
                        .as_deref()
                        .and_then(parse_decimal_string)
                })
                .sum(),
            members.len(),
        );
        let mean_net_return = decimal_mean(
            members
                .iter()
                .filter_map(|row| {
                    row.outcome_net_return
                        .as_deref()
                        .and_then(parse_decimal_string)
                })
                .sum(),
            members.len(),
        );
        let hit_rate = ratio(
            members
                .iter()
                .filter(|row| row.outcome_followed_through == Some(true))
                .count(),
            members.len(),
        );
        let invalidation_rate = ratio(
            members
                .iter()
                .filter(|row| row.outcome_invalidated == Some(true))
                .count(),
            members.len(),
        );
        let stale_ratio = ratio(
            members
                .iter()
                .filter(|row| row.local_state.as_deref() == Some("stale"))
                .count(),
            members.len(),
        );
        let low_information_ratio = ratio(
            members
                .iter()
                .filter(|row| row.local_state.as_deref() == Some("low_information"))
                .count(),
            members.len(),
        );
        let actionable_ratio = ratio(
            members
                .iter()
                .filter(|row| row.actionability_state.as_deref() == Some("actionable"))
                .count(),
            members.len(),
        );

        buckets.push(ActionabilityBucketStat {
            bucket: bucket_label(lower, upper),
            samples: members.len(),
            mean_score: mean_score.round_dp(4).to_string(),
            mean_net_return: mean_net_return.round_dp(4).to_string(),
            hit_rate: hit_rate.round_dp(4).to_string(),
            invalidation_rate: invalidation_rate.round_dp(4).to_string(),
            stale_ratio: stale_ratio.round_dp(4).to_string(),
            low_information_ratio: low_information_ratio.round_dp(4).to_string(),
            actionable_ratio: actionable_ratio.round_dp(4).to_string(),
        });
    }

    ActionabilityCalibrationReport {
        market: market.to_string(),
        rows_scanned: rows.len(),
        latest_rows: latest_rows.len(),
        rows_with_outcomes: rows_with_outcomes.len(),
        bucket_width: bucket_width.to_string(),
        buckets,
        state_label_stats: build_state_label_stats(&rows_with_outcomes),
    }
}

#[cfg(feature = "persistence")]
fn build_state_label_stats(rows: &[PerceptionDatasetRow]) -> Vec<StateLabelStat> {
    let mut grouped =
        std::collections::BTreeMap::<(String, String), Vec<&PerceptionDatasetRow>>::new();
    for row in rows
        .iter()
        .filter(|row| row.state_label.is_some() && row.state_label_horizon.is_some())
    {
        grouped
            .entry((
                row.state_label.clone().unwrap_or_default(),
                row.state_label_horizon.clone().unwrap_or_default(),
            ))
            .or_default()
            .push(row);
    }

    grouped
        .into_iter()
        .map(|((label, horizon), members)| {
            let samples = members.len();
            let mean_confidence = decimal_mean(
                members
                    .iter()
                    .filter_map(|row| {
                        row.state_label_confidence
                            .as_deref()
                            .and_then(parse_decimal_string)
                    })
                    .sum(),
                samples,
            );
            let mean_net_return = decimal_mean(
                members
                    .iter()
                    .filter_map(|row| {
                        row.outcome_net_return
                            .as_deref()
                            .and_then(parse_decimal_string)
                    })
                    .sum(),
                samples,
            );
            let hit_rate = ratio(
                members
                    .iter()
                    .filter(|row| row.outcome_followed_through == Some(true))
                    .count(),
                samples,
            );
            let invalidation_rate = ratio(
                members
                    .iter()
                    .filter(|row| row.outcome_invalidated == Some(true))
                    .count(),
                samples,
            );
            StateLabelStat {
                label,
                horizon,
                samples,
                mean_confidence: mean_confidence.round_dp(4).to_string(),
                mean_net_return: mean_net_return.round_dp(4).to_string(),
                hit_rate: hit_rate.round_dp(4).to_string(),
                invalidation_rate: invalidation_rate.round_dp(4).to_string(),
            }
        })
        .collect()
}

#[cfg(feature = "persistence")]
fn latest_runtime_rows_by_setup(rows: &[PerceptionDatasetRow]) -> Vec<PerceptionDatasetRow> {
    let mut by_setup = std::collections::BTreeMap::<String, &PerceptionDatasetRow>::new();
    for row in rows.iter().filter(|row| row.source != "outcome_auto") {
        match by_setup.get(&row.setup_id) {
            Some(existing) if existing.recorded_at >= row.recorded_at => {}
            _ => {
                by_setup.insert(row.setup_id.clone(), row);
            }
        }
    }
    by_setup.into_values().cloned().collect()
}

#[cfg(feature = "persistence")]
fn decimal_mean(total: Decimal, count: usize) -> Decimal {
    if count == 0 {
        Decimal::ZERO
    } else {
        total / Decimal::from(count as i64)
    }
}

#[cfg(feature = "persistence")]
fn ratio(numerator: usize, denominator: usize) -> Decimal {
    if denominator == 0 {
        Decimal::ZERO
    } else {
        Decimal::from(numerator as i64) / Decimal::from(denominator as i64)
    }
}

#[cfg(feature = "persistence")]
fn parse_decimal_string(raw: &str) -> Option<Decimal> {
    raw.parse::<Decimal>().ok()
}

#[cfg(feature = "persistence")]
fn bucket_label(lower: Decimal, upper: Decimal) -> String {
    if upper == Decimal::ONE {
        format!("[{}, {}]", lower.round_dp(1), upper.round_dp(1))
    } else {
        format!("[{}, {})", lower.round_dp(1), upper.round_dp(1))
    }
}

#[cfg(feature = "persistence")]
fn write_calibration_report(
    output: &str,
    report: &ActionabilityCalibrationReport,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if let Some(parent) = std::path::Path::new(output).parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let mut file = std::fs::File::create(output)?;
    writeln!(
        file,
        "# Actionability Calibration Report — {}",
        report.market
    )?;
    writeln!(file)?;
    writeln!(file, "- rows_scanned: {}", report.rows_scanned)?;
    writeln!(file, "- latest_rows: {}", report.latest_rows)?;
    writeln!(file, "- rows_with_outcomes: {}", report.rows_with_outcomes)?;
    writeln!(file, "- bucket_width: {}", report.bucket_width)?;
    writeln!(file)?;
    writeln!(
        file,
        "| bucket | samples | mean_score | mean_net_return | hit_rate | invalidation_rate | stale_ratio | low_information_ratio | actionable_ratio |"
    )?;
    writeln!(file, "|---|---:|---:|---:|---:|---:|---:|---:|---:|")?;
    for bucket in &report.buckets {
        writeln!(
            file,
            "| {} | {} | {} | {} | {} | {} | {} | {} | {} |",
            bucket.bucket,
            bucket.samples,
            bucket.mean_score,
            bucket.mean_net_return,
            bucket.hit_rate,
            bucket.invalidation_rate,
            bucket.stale_ratio,
            bucket.low_information_ratio,
            bucket.actionable_ratio,
        )?;
    }
    writeln!(file)?;
    writeln!(file, "## State Labels")?;
    writeln!(file)?;
    writeln!(
        file,
        "| label | horizon | samples | mean_confidence | mean_net_return | hit_rate | invalidation_rate |"
    )?;
    writeln!(file, "|---|---|---:|---:|---:|---:|---:|")?;
    for item in &report.state_label_stats {
        writeln!(
            file,
            "| {} | {} | {} | {} | {} | {} | {} |",
            item.label,
            item.horizon,
            item.samples,
            item.mean_confidence,
            item.mean_net_return,
            item.hit_rate,
            item.invalidation_rate,
        )?;
    }
    Ok(())
}

#[cfg(feature = "persistence")]
fn parse_flag<T: std::str::FromStr>(args: &[String], flag: &str) -> Option<T> {
    let idx = args.iter().position(|arg| arg == flag)?;
    args.get(idx + 1)?.parse().ok()
}

#[cfg(feature = "persistence")]
fn parse_flag_str(args: &[String], flag: &str) -> Option<String> {
    let idx = args.iter().position(|arg| arg == flag)?;
    args.get(idx + 1).cloned()
}

#[cfg(all(test, feature = "persistence"))]
mod tests {
    use super::*;

    #[test]
    fn csv_escape_wraps_commas_and_quotes() {
        assert_eq!(csv_escape("plain"), "plain");
        assert_eq!(csv_escape("a,b"), "\"a,b\"");
        assert_eq!(csv_escape("a\"b"), "\"a\"\"b\"");
    }

    // NOTE: a previous `row_from_assessment_carries_perception_and_outcome`
    // test lived here but referenced `row_from_assessment(...)` which has
    // been refactored away in favour of `row_from_values(&Value, Option<&Value>)`.
    // The test was left type-drifted and broke `cargo test --features
    // persistence,coordinator` compilation. Deleted during 2026-04-19
    // audit cleanup. A replacement test that exercises row_from_values
    // against serde_json::Value input is future work.
}
