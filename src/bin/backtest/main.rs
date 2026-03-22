mod loader;
mod adapter;
mod runner;
mod report;

use std::path::Path;
use loader::load_symbols;
use adapter::build_synthetic_tick;
use runner::{evaluate_tick, validate_judgment};
use report::{aggregate, print_report};

const BACKTEST_SYMBOLS: &[&str] = &[
    // Tech
    "700.HK", "9988.HK", "3690.HK", "9618.HK", "1810.HK",
    "268.HK", "9999.HK", "1024.HK", "3888.HK", "9626.HK",
    // Semiconductor
    "981.HK", "2382.HK", "285.HK", "992.HK",
    // Finance (for rotation detection)
    "1398.HK", "3988.HK", "939.HK", "2628.HK", "2318.HK",
    // Property (for rotation detection)
    "1109.HK", "688.HK", "16.HK",
    // Mining
    "2259.HK",
];

const WINDOW_SIZE: usize = 30;   // 30 bars = 30 minutes
const STEP_SIZE: usize = 30;     // non-overlapping windows
const MIN_FUTURE_BARS: usize = 400; // need 390+ bars for 1-day horizon

fn symbol_sector(symbol: &str) -> &'static str {
    match symbol {
        "700.HK" | "9988.HK" | "3690.HK" | "9618.HK" | "1810.HK"
        | "268.HK" | "9999.HK" | "1024.HK" | "3888.HK" | "9626.HK" => "tech",
        "981.HK" | "2382.HK" | "285.HK" | "992.HK" => "semiconductor",
        "1398.HK" | "3988.HK" | "939.HK" | "2628.HK" | "2318.HK" => "finance",
        "1109.HK" | "688.HK" | "16.HK" => "property",
        "2259.HK" => "mining",
        _ => "other",
    }
}

fn format_date(ts: i64) -> String {
    let dt = time::OffsetDateTime::from_unix_timestamp(ts)
        .unwrap_or(time::OffsetDateTime::UNIX_EPOCH);
    format!("{}-{:02}-{:02}", dt.year(), dt.month() as u8, dt.day())
}

fn main() {
    let cache_dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/Volumes/LaCie 1/eden-data/cache_1m".to_string());
    let cache_path = Path::new(&cache_dir);

    println!("Eden HK Backtest");
    println!("Loading bars from {:?} ...", cache_path);

    let symbol_bars = match load_symbols(cache_path, BACKTEST_SYMBOLS) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Failed to load symbols: {}", e);
            return;
        }
    };

    // Print loading summary
    println!("Loaded {} symbols:", symbol_bars.len());
    for (sym, bars) in &symbol_bars {
        println!("  {} : {} bars", sym, bars.len());
    }
    println!();

    // Collect all validated judgments
    let mut all_results: Vec<runner::ValidatedJudgment> = Vec::new();
    let mut judgment_count: usize = 0;

    for &symbol in BACKTEST_SYMBOLS {
        let bars = match symbol_bars.get(symbol) {
            Some(b) if b.len() >= WINDOW_SIZE + MIN_FUTURE_BARS => b,
            _ => continue,
        };

        let sector = symbol_sector(symbol);
        let max_start = bars.len().saturating_sub(WINDOW_SIZE + MIN_FUTURE_BARS);

        let mut offset = 0;
        while offset <= max_start {
            let window = &bars[offset..offset + WINDOW_SIZE];
            let future_bars = &bars[offset + WINDOW_SIZE..];
            let reference_price = window.last().unwrap().close;

            // Pass empty slice for cross-sectional stress (skip O(n^2) for now)
            if let Some(tick) = build_synthetic_tick(symbol, sector, window, &[]) {
                if let Some(judgment) = evaluate_tick(&tick) {
                    let validated = validate_judgment(&judgment, future_bars, reference_price);
                    all_results.push(validated);
                    judgment_count += 1;

                    if judgment_count % 1000 == 0 {
                        println!("  ... {} judgments so far", judgment_count);
                    }
                }
            }

            offset += STEP_SIZE;
        }
    }

    println!();
    println!("Total judgments collected: {}", all_results.len());

    if all_results.is_empty() {
        println!("No judgments to report.");
        return;
    }

    // Aggregate and set date range
    let mut report = aggregate(&all_results);

    let min_ts = all_results.iter().map(|r| r.judgment.timestamp).min().unwrap_or(0);
    let max_ts = all_results.iter().map(|r| r.judgment.timestamp).max().unwrap_or(0);
    report.date_range = (format_date(min_ts), format_date(max_ts));

    print_report(&report);
}
