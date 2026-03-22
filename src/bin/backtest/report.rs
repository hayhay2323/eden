use std::collections::HashMap;
use super::runner::{ValidatedJudgment, HORIZONS};

// ── types ────────────────────────────────────────────────────────────────────

pub struct BacktestReport {
    pub total_judgments: usize,
    pub mechanism_rows: Vec<MechanismRow>,
    pub total_row: TotalRow,
    pub date_range: (String, String),
    pub symbols_count: usize,
}

pub struct MechanismRow {
    pub mechanism: String,
    pub judgments: usize,
    pub hit_rates: Vec<f64>, // one per horizon (4 entries)
}

pub struct TotalRow {
    pub judgments: usize,
    pub hit_rates: Vec<f64>,
}

// ── aggregation ──────────────────────────────────────────────────────────────

pub fn aggregate(results: &[ValidatedJudgment]) -> BacktestReport {
    // Group by mechanism_label
    let mut groups: HashMap<String, Vec<&ValidatedJudgment>> = HashMap::new();
    for r in results {
        groups
            .entry(r.judgment.mechanism_label.clone())
            .or_default()
            .push(r);
    }

    // Build mechanism rows
    let mut mechanism_rows: Vec<MechanismRow> = groups
        .into_iter()
        .map(|(mechanism, judgments)| {
            let count = judgments.len();
            let hit_rates: Vec<f64> = (0..HORIZONS.len())
                .map(|hi| {
                    let hits = judgments
                        .iter()
                        .filter(|j| j.outcomes.get(hi).map_or(false, |o| o.hit))
                        .count();
                    if count > 0 {
                        hits as f64 / count as f64 * 100.0
                    } else {
                        0.0
                    }
                })
                .collect();
            MechanismRow {
                mechanism,
                judgments: count,
                hit_rates,
            }
        })
        .collect();

    // Sort by judgment count descending
    mechanism_rows.sort_by(|a, b| b.judgments.cmp(&a.judgments));

    // Total row
    let total_judgments = results.len();
    let total_hit_rates: Vec<f64> = (0..HORIZONS.len())
        .map(|hi| {
            let hits = results
                .iter()
                .filter(|j| j.outcomes.get(hi).map_or(false, |o| o.hit))
                .count();
            if total_judgments > 0 {
                hits as f64 / total_judgments as f64 * 100.0
            } else {
                0.0
            }
        })
        .collect();

    let total_row = TotalRow {
        judgments: total_judgments,
        hit_rates: total_hit_rates,
    };

    // Count unique symbols
    let mut symbols: std::collections::HashSet<&str> = std::collections::HashSet::new();
    for r in results {
        symbols.insert(&r.judgment.symbol);
    }

    BacktestReport {
        total_judgments,
        mechanism_rows,
        total_row,
        date_range: (String::new(), String::new()),
        symbols_count: symbols.len(),
    }
}

// ── display ──────────────────────────────────────────────────────────────────

pub fn print_report(report: &BacktestReport) {
    let bar = "=".repeat(65);
    let thin = "-".repeat(60);

    println!();
    println!("  {}", bar);
    println!(
        "  Eden HK Backtest  --  {} to {}",
        report.date_range.0, report.date_range.1
    );
    println!(
        "  Symbols: {} | Judgments: {}",
        report.symbols_count, report.total_judgments
    );
    println!("  {}", bar);
    println!();

    // Header
    println!(
        "  {:<30} {:>5}   {:>5} {:>5} {:>5} {:>5}",
        "Mechanism", "Count", "5m", "30m", "1h", "1d"
    );
    println!("  {}", thin);

    // Mechanism rows
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

    // Total row
    println!(
        "  {:<30} {:>5}  {:>5.1}% {:>5.1}% {:>5.1}% {:>5.1}%",
        "TOTAL",
        report.total_row.judgments,
        report.total_row.hit_rates.first().copied().unwrap_or(0.0),
        report.total_row.hit_rates.get(1).copied().unwrap_or(0.0),
        report.total_row.hit_rates.get(2).copied().unwrap_or(0.0),
        report.total_row.hit_rates.get(3).copied().unwrap_or(0.0),
    );

    // Baseline
    println!(
        "  {:<30} {:>5}  {:>5.1}% {:>5.1}% {:>5.1}% {:>5.1}%",
        "Baseline (random)", "", 50.0, 50.0, 50.0, 50.0
    );

    println!();
}

fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max - 3])
    }
}
