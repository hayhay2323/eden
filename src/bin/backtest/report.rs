use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::path::Path;

use serde::Serialize;
use super::runner::{ValidatedJudgment, HORIZONS};

#[derive(Clone, Serialize)]
pub struct BacktestReport {
    pub total_judgments: usize,
    pub mechanism_rows: Vec<MechanismRow>,
    pub total_row: TotalRow,
    pub date_range: (String, String),
    pub symbols_count: usize,
}

#[derive(Clone, Serialize)]
pub struct MechanismRow {
    pub mechanism: String,
    pub judgments: usize,
    pub hit_rates: Vec<f64>,
}

#[derive(Clone, Serialize)]
pub struct TotalRow {
    pub judgments: usize,
    pub hit_rates: Vec<f64>,
}

#[derive(Clone, Serialize)]
pub struct SectionReport {
    pub title: String,
    pub report: BacktestReport,
}

#[derive(Clone, Serialize)]
pub struct BootstrapSummary {
    pub horizon_label: &'static str,
    pub mean: f64,
    pub lower: f64,
    pub upper: f64,
}

#[derive(Clone, Serialize)]
pub struct CrossSliceRow {
    pub mechanism: String,
    pub slice: String,
    pub judgments: usize,
    pub hit_rates: Vec<f64>,
    pub contrarian_hit_rates: Vec<f64>,
}

#[derive(Clone, Serialize)]
pub struct DeepReport {
    pub overall: BacktestReport,
    pub out_of_sample: Option<SectionReport>,
    pub by_session: Vec<SectionReport>,
    pub by_regime: Vec<SectionReport>,
    pub mechanism_by_session: Vec<CrossSliceRow>,
    pub mechanism_by_regime: Vec<CrossSliceRow>,
    pub contrarian_bootstrap: Vec<BootstrapSummary>,
    pub non_overlapping: bool,
    pub out_of_sample_start: Option<String>,
}

pub fn aggregate(results: &[ValidatedJudgment]) -> BacktestReport {
    let mut groups: HashMap<String, Vec<&ValidatedJudgment>> = HashMap::new();
    for result in results {
        groups
            .entry(result.judgment.mechanism_label.clone())
            .or_default()
            .push(result);
    }

    let mut mechanism_rows: Vec<MechanismRow> = groups
        .into_iter()
        .map(|(mechanism, judgments)| {
            let count = judgments.len();
            let hit_rates = horizon_hit_rates(&judgments, false);
            MechanismRow {
                mechanism,
                judgments: count,
                hit_rates,
            }
        })
        .collect();

    mechanism_rows.sort_by(|a, b| b.judgments.cmp(&a.judgments));

    let total_judgments = results.len();
    let total_row = TotalRow {
        judgments: total_judgments,
        hit_rates: horizon_hit_rates(&results.iter().collect::<Vec<_>>(), false),
    };

    let mut symbols: BTreeSet<&str> = BTreeSet::new();
    for result in results {
        symbols.insert(&result.judgment.symbol);
    }

    BacktestReport {
        total_judgments,
        mechanism_rows,
        total_row,
        date_range: (String::new(), String::new()),
        symbols_count: symbols.len(),
    }
}

pub fn build_deep_report(results: &[ValidatedJudgment], non_overlapping: bool) -> DeepReport {
    let mut overall = aggregate(results);
    overall.date_range = date_range(results);

    let out_of_sample = build_out_of_sample(results);
    let by_session = slice_reports(results, |result| result.judgment.session.to_string());
    let by_regime = slice_reports(results, |result| result.judgment.regime.clone());
    let mechanism_by_session =
        cross_slice_rows(results, |result| result.judgment.session.to_string());
    let mechanism_by_regime = cross_slice_rows(results, |result| result.judgment.regime.clone());
    let contrarian_bootstrap = bootstrap_contrarian(results, 400);
    let out_of_sample_start = out_of_sample
        .as_ref()
        .map(|section| section.report.date_range.0.clone());

    DeepReport {
        overall,
        out_of_sample,
        by_session,
        by_regime,
        mechanism_by_session,
        mechanism_by_regime,
        contrarian_bootstrap,
        non_overlapping,
        out_of_sample_start,
    }
}

pub fn print_report(report: &BacktestReport) {
    print_section(report, "Overall");
}

pub fn print_deep_report(report: &DeepReport) {
    print_section(&report.overall, "Overall");

    if let Some(start) = &report.out_of_sample_start {
        println!("  Out-of-sample split starts at {}", start);
    }
    println!(
        "  Windowing: {}",
        if report.non_overlapping {
            "non-overlapping"
        } else {
            "overlapping"
        }
    );
    println!();

    if let Some(section) = &report.out_of_sample {
        print_section(&section.report, &section.title);
    }

    print_slice_table("By Session", &report.by_session);
    print_slice_table("By Regime", &report.by_regime);
    print_cross_table("Mechanism x Session", &report.mechanism_by_session);
    print_cross_table("Mechanism x Regime", &report.mechanism_by_regime);
    print_bootstrap(&report.contrarian_bootstrap);
}

pub fn write_deep_report_files(report: &DeepReport, output_dir: &Path) -> Result<(String, String), String> {
    std::fs::create_dir_all(output_dir)
        .map_err(|error| format!("failed to create {:?}: {}", output_dir, error))?;

    let json_path = output_dir.join("backtest_report.json");
    let csv_path = output_dir.join("backtest_report.csv");

    let json = serde_json::to_string_pretty(report)
        .map_err(|error| format!("failed to serialize report JSON: {}", error))?;
    std::fs::write(&json_path, json)
        .map_err(|error| format!("failed to write {:?}: {}", json_path, error))?;

    let csv = build_csv(report);
    std::fs::write(&csv_path, csv)
        .map_err(|error| format!("failed to write {:?}: {}", csv_path, error))?;

    Ok((
        json_path.to_string_lossy().to_string(),
        csv_path.to_string_lossy().to_string(),
    ))
}

fn build_out_of_sample(results: &[ValidatedJudgment]) -> Option<SectionReport> {
    if results.len() < 10 {
        return None;
    }

    let mut ordered = results.iter().collect::<Vec<_>>();
    ordered.sort_by_key(|result| result.judgment.timestamp);
    let split_idx = ((ordered.len() as f64) * 0.7).floor() as usize;
    if split_idx >= ordered.len() {
        return None;
    }
    let split_ts = ordered[split_idx].judgment.timestamp;
    let oos = results
        .iter()
        .filter(|result| result.judgment.timestamp >= split_ts)
        .cloned()
        .collect::<Vec<_>>();
    if oos.is_empty() {
        return None;
    }
    let mut report = aggregate(&oos);
    report.date_range = date_range(&oos);
    Some(SectionReport {
        title: "Out-of-Sample (last 30%)".into(),
        report,
    })
}

fn slice_reports<F>(results: &[ValidatedJudgment], key_fn: F) -> Vec<SectionReport>
where
    F: Fn(&ValidatedJudgment) -> String,
{
    let mut buckets: BTreeMap<String, Vec<ValidatedJudgment>> = BTreeMap::new();
    for result in results {
        buckets
            .entry(key_fn(result))
            .or_default()
            .push(result.clone());
    }

    buckets
        .into_iter()
        .filter_map(|(label, bucket)| {
            if bucket.is_empty() {
                return None;
            }
            let mut report = aggregate(&bucket);
            report.date_range = date_range(&bucket);
            Some(SectionReport {
                title: label,
                report,
            })
        })
        .collect()
}

fn cross_slice_rows<F>(results: &[ValidatedJudgment], key_fn: F) -> Vec<CrossSliceRow>
where
    F: Fn(&ValidatedJudgment) -> String,
{
    let mut buckets: BTreeMap<(String, String), Vec<&ValidatedJudgment>> = BTreeMap::new();
    for result in results {
        buckets
            .entry((result.judgment.mechanism_label.clone(), key_fn(result)))
            .or_default()
            .push(result);
    }

    buckets
        .into_iter()
        .map(|((mechanism, slice), judgments)| {
            let hit_rates = horizon_hit_rates(&judgments, false);
            let contrarian_hit_rates = horizon_hit_rates(&judgments, true);
            CrossSliceRow {
                mechanism,
                slice,
                judgments: judgments.len(),
                hit_rates,
                contrarian_hit_rates,
            }
        })
        .collect()
}

fn bootstrap_contrarian(results: &[ValidatedJudgment], iterations: usize) -> Vec<BootstrapSummary> {
    #[derive(Default, Clone, Copy)]
    struct BlockTotals {
        judgments: usize,
        hits: [usize; 4],
    }

    let mut blocks: Vec<BlockTotals> = Vec::new();
    let mut by_key: BTreeMap<String, BlockTotals> = BTreeMap::new();
    for result in results {
        let key = format!(
            "{}:{}",
            result.judgment.symbol,
            hkt_date(result.judgment.timestamp)
        );
        let entry = by_key.entry(key).or_default();
        entry.judgments += 1;
        for (idx, outcome) in result.outcomes.iter().enumerate() {
            if !outcome.hit {
                entry.hits[idx] += 1; // contrarian hit = inverted directional hit
            }
        }
    }
    blocks.extend(by_key.into_values());
    if blocks.is_empty() {
        return Vec::new();
    }

    let mut samples: Vec<Vec<f64>> = vec![Vec::with_capacity(iterations); HORIZONS.len()];
    let n = blocks.len();
    for iter in 0..iterations {
        let mut judgments = 0usize;
        let mut hits = [0usize; 4];
        let mut rng_state = splitmix64_seed(iter as u64 + 1);
        for offset in 0..n {
            let idx = (next_u64(&mut rng_state).wrapping_add(offset as u64) as usize) % n;
            let block = blocks[idx];
            judgments += block.judgments;
            for hi in 0..HORIZONS.len() {
                hits[hi] += block.hits[hi];
            }
        }
        if judgments == 0 {
            continue;
        }
        for hi in 0..HORIZONS.len() {
            samples[hi].push(hits[hi] as f64 / judgments as f64 * 100.0);
        }
    }

    samples
        .into_iter()
        .enumerate()
        .map(|(idx, mut values)| {
            values.sort_by(|a, b| a.partial_cmp(b).unwrap());
            let mean = values.iter().sum::<f64>() / values.len().max(1) as f64;
            let lower_idx = ((values.len() as f64) * 0.025).floor() as usize;
            let upper_idx = ((values.len() as f64) * 0.975).floor() as usize;
            BootstrapSummary {
                horizon_label: HORIZONS[idx].1,
                mean,
                lower: values.get(lower_idx).copied().unwrap_or(mean),
                upper: values
                    .get(upper_idx.min(values.len().saturating_sub(1)))
                    .copied()
                    .unwrap_or(mean),
            }
        })
        .collect()
}

fn splitmix64_seed(seed: u64) -> u64 {
    seed.wrapping_add(0x9E3779B97F4A7C15)
}

fn next_u64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E3779B97F4A7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58476D1CE4E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D049BB133111EB);
    z ^ (z >> 31)
}

fn horizon_hit_rates(results: &[&ValidatedJudgment], invert: bool) -> Vec<f64> {
    (0..HORIZONS.len())
        .map(|hi| {
            let hits = results
                .iter()
                .filter(|result| {
                    result.outcomes.get(hi).map_or(false, |outcome| {
                        if invert {
                            !outcome.hit
                        } else {
                            outcome.hit
                        }
                    })
                })
                .count();
            if results.is_empty() {
                0.0
            } else {
                hits as f64 / results.len() as f64 * 100.0
            }
        })
        .collect()
}

fn date_range(results: &[ValidatedJudgment]) -> (String, String) {
    let min_ts = results
        .iter()
        .map(|result| result.judgment.timestamp)
        .min()
        .unwrap_or(0);
    let max_ts = results
        .iter()
        .map(|result| result.judgment.timestamp)
        .max()
        .unwrap_or(0);
    (date_string(min_ts), date_string(max_ts))
}

fn date_string(timestamp: i64) -> String {
    let dt = time::OffsetDateTime::from_unix_timestamp(timestamp)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH)
        .to_offset(time::UtcOffset::from_hms(8, 0, 0).unwrap());
    format!("{}-{:02}-{:02}", dt.year(), dt.month() as u8, dt.day())
}

fn hkt_date(timestamp: i64) -> String {
    date_string(timestamp)
}

fn print_section(report: &BacktestReport, title: &str) {
    let bar = "=".repeat(65);
    let thin = "-".repeat(60);

    println!();
    println!("  {}", bar);
    println!(
        "  {}  --  {} to {}",
        title, report.date_range.0, report.date_range.1
    );
    println!(
        "  Symbols: {} | Judgments: {}",
        report.symbols_count, report.total_judgments
    );
    println!("  {}", bar);
    println!();

    println!(
        "  {:<30} {:>5}   {:>5} {:>5} {:>5} {:>5}",
        "Mechanism", "Count", "5m", "30m", "1h", "1d"
    );
    println!("  {}", thin);

    for row in &report.mechanism_rows {
        println!(
            "  {:<30} {:>5}  {:>5.1}% {:>5.1}% {:>5.1}% {:>5.1}%",
            truncate(&row.mechanism, 30),
            row.judgments,
            row.hit_rates.first().copied().unwrap_or(0.0),
            row.hit_rates.get(1).copied().unwrap_or(0.0),
            row.hit_rates.get(2).copied().unwrap_or(0.0),
            row.hit_rates.get(3).copied().unwrap_or(0.0),
        );
    }

    println!("  {}", thin);
    println!(
        "  {:<30} {:>5}  {:>5.1}% {:>5.1}% {:>5.1}% {:>5.1}%",
        "TOTAL",
        report.total_row.judgments,
        report.total_row.hit_rates.first().copied().unwrap_or(0.0),
        report.total_row.hit_rates.get(1).copied().unwrap_or(0.0),
        report.total_row.hit_rates.get(2).copied().unwrap_or(0.0),
        report.total_row.hit_rates.get(3).copied().unwrap_or(0.0),
    );

    let inv_rates: Vec<f64> = report
        .total_row
        .hit_rates
        .iter()
        .map(|rate| 100.0 - rate)
        .collect();
    println!(
        "  {:<30} {:>5}  {:>5.1}% {:>5.1}% {:>5.1}% {:>5.1}%",
        "CONTRARIAN (inverted)",
        report.total_row.judgments,
        inv_rates.first().copied().unwrap_or(0.0),
        inv_rates.get(1).copied().unwrap_or(0.0),
        inv_rates.get(2).copied().unwrap_or(0.0),
        inv_rates.get(3).copied().unwrap_or(0.0),
    );
    println!(
        "  {:<30} {:>5}  {:>5.1}% {:>5.1}% {:>5.1}% {:>5.1}%",
        "Baseline (random)", "", 50.0, 50.0, 50.0, 50.0
    );
    println!();
}

fn print_slice_table(title: &str, sections: &[SectionReport]) {
    if sections.is_empty() {
        return;
    }

    println!("  {}", title);
    println!("  {}", "-".repeat(65));
    println!(
        "  {:<20} {:>7}   {:>5} {:>5} {:>5} {:>5}",
        "Slice", "Count", "5m", "30m", "1h", "1d"
    );
    for section in sections {
        let contrarian = section
            .report
            .total_row
            .hit_rates
            .iter()
            .map(|rate| 100.0 - rate)
            .collect::<Vec<_>>();
        println!(
            "  {:<20} {:>7}  {:>5.1}% {:>5.1}% {:>5.1}% {:>5.1}%",
            truncate(&section.title, 20),
            section.report.total_row.judgments,
            contrarian.first().copied().unwrap_or(0.0),
            contrarian.get(1).copied().unwrap_or(0.0),
            contrarian.get(2).copied().unwrap_or(0.0),
            contrarian.get(3).copied().unwrap_or(0.0),
        );
    }
    println!();
}

fn print_cross_table(title: &str, rows: &[CrossSliceRow]) {
    if rows.is_empty() {
        return;
    }

    println!("  {}", title);
    println!("  {}", "-".repeat(90));
    println!(
        "  {:<28} {:<16} {:>7}   {:>5} {:>5} {:>5} {:>5}",
        "Mechanism", "Slice", "Count", "5m", "30m", "1h", "1d"
    );
    for row in rows {
        println!(
            "  {:<28} {:<16} {:>7}  {:>5.1}% {:>5.1}% {:>5.1}% {:>5.1}%",
            truncate(&row.mechanism, 28),
            truncate(&row.slice, 16),
            row.judgments,
            row.contrarian_hit_rates.first().copied().unwrap_or(0.0),
            row.contrarian_hit_rates.get(1).copied().unwrap_or(0.0),
            row.contrarian_hit_rates.get(2).copied().unwrap_or(0.0),
            row.contrarian_hit_rates.get(3).copied().unwrap_or(0.0),
        );
    }
    println!();
}

fn print_bootstrap(samples: &[BootstrapSummary]) {
    if samples.is_empty() {
        return;
    }
    println!("  Contrarian 95% block-bootstrap CI (symbol-day)");
    println!("  {}", "-".repeat(65));
    for sample in samples {
        println!(
            "  {:<6} mean={:>5.1}%  ci=[{:>5.1}%, {:>5.1}%]",
            sample.horizon_label, sample.mean, sample.lower, sample.upper
        );
    }
    println!();
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}

fn build_csv(report: &DeepReport) -> String {
    let mut csv = String::from(
        "section,row_type,label,slice,count,hit_5m,hit_30m,hit_1h,hit_1d,contrarian_5m,contrarian_30m,contrarian_1h,contrarian_1d\n",
    );

    append_report_rows(&mut csv, "overall", &report.overall);
    if let Some(section) = &report.out_of_sample {
        append_report_rows(&mut csv, "out_of_sample", &section.report);
    }
    append_slice_rows(&mut csv, "session", &report.by_session);
    append_slice_rows(&mut csv, "regime", &report.by_regime);
    append_cross_rows(&mut csv, "mechanism_x_session", &report.mechanism_by_session);
    append_cross_rows(&mut csv, "mechanism_x_regime", &report.mechanism_by_regime);

    for sample in &report.contrarian_bootstrap {
        let _ = writeln!(
            csv,
            "bootstrap,ci,{},{},,,,,{:.4},{:.4},{:.4},",
            sample.horizon_label, 0, sample.mean, sample.lower, sample.upper
        );
    }

    csv
}

fn append_report_rows(csv: &mut String, section: &str, report: &BacktestReport) {
    for row in &report.mechanism_rows {
        append_row(
            csv,
            section,
            "mechanism",
            &row.mechanism,
            "",
            row.judgments,
            &row.hit_rates,
        );
    }
    append_row(
        csv,
        section,
        "total",
        "TOTAL",
        "",
        report.total_row.judgments,
        &report.total_row.hit_rates,
    );
}

fn append_slice_rows(csv: &mut String, section: &str, slices: &[SectionReport]) {
    for slice in slices {
        append_row(
            csv,
            section,
            "slice_total",
            &slice.title,
            &slice.title,
            slice.report.total_row.judgments,
            &slice.report.total_row.hit_rates,
        );
    }
}

fn append_cross_rows(csv: &mut String, section: &str, rows: &[CrossSliceRow]) {
    for row in rows {
        append_row(
            csv,
            section,
            "cross",
            &row.mechanism,
            &row.slice,
            row.judgments,
            &row.hit_rates,
        );
    }
}

fn append_row(
    csv: &mut String,
    section: &str,
    row_type: &str,
    label: &str,
    slice: &str,
    count: usize,
    hit_rates: &[f64],
) {
    let contrarian = hit_rates.iter().map(|rate| 100.0 - rate).collect::<Vec<_>>();
    let _ = writeln!(
        csv,
        "{section},{row_type},\"{}\",\"{}\",{count},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4},{:.4}",
        label.replace('"', "\"\""),
        slice.replace('"', "\"\""),
        hit_rates.first().copied().unwrap_or(0.0),
        hit_rates.get(1).copied().unwrap_or(0.0),
        hit_rates.get(2).copied().unwrap_or(0.0),
        hit_rates.get(3).copied().unwrap_or(0.0),
        contrarian.first().copied().unwrap_or(0.0),
        contrarian.get(1).copied().unwrap_or(0.0),
        contrarian.get(2).copied().unwrap_or(0.0),
        contrarian.get(3).copied().unwrap_or(0.0),
    );
}
