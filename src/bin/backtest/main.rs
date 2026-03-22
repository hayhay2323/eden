mod adapter;
mod loader;
mod report;
mod runner;

use adapter::{build_market_contexts, build_synthetic_tick};
use loader::load_symbols;
use report::{build_deep_report, print_deep_report, write_deep_report_files};
use runner::{evaluate_tick, validate_judgment};
use std::path::Path;

const BACKTEST_SYMBOLS: &[&str] = &[
    // Tech
    "700.HK", "9988.HK", "3690.HK", "9618.HK", "1810.HK", "268.HK", "9999.HK", "1024.HK", "3888.HK",
    "9626.HK", // Semiconductor
    "981.HK", "2382.HK", "285.HK", "992.HK", // Finance (for rotation detection)
    "1398.HK", "3988.HK", "939.HK", "2628.HK", "2318.HK",
    // Property (for rotation detection)
    "1109.HK", "688.HK", "16.HK", // Mining
    "2259.HK",
];

const WINDOW_SIZE: usize = 30; // 30 bars = 30 minutes
const STEP_SIZE: usize = 30; // non-overlapping windows
const MIN_FUTURE_BARS: usize = 400; // need 390+ bars for 1-day horizon

fn symbol_sector(symbol: &str) -> &'static str {
    match symbol {
        "700.HK" | "9988.HK" | "3690.HK" | "9618.HK" | "1810.HK" | "268.HK" | "9999.HK"
        | "1024.HK" | "3888.HK" | "9626.HK" => "tech",
        "981.HK" | "2382.HK" | "285.HK" | "992.HK" => "semiconductor",
        "1398.HK" | "3988.HK" | "939.HK" | "2628.HK" | "2318.HK" => "finance",
        "1109.HK" | "688.HK" | "16.HK" => "property",
        "2259.HK" => "mining",
        _ => "other",
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let cache_dir = args
        .next()
        .unwrap_or_else(|| "/Volumes/LaCie 1/eden-data/cache_1m".to_string());
    let output_dir = args.next().unwrap_or_else(|| "data".to_string());
    let cache_path = Path::new(&cache_dir);
    let output_path = Path::new(&output_dir);

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

    println!("Precomputing market context ...");
    let market_contexts = build_market_contexts(&symbol_bars);
    println!("Computed {} market snapshots.", market_contexts.len());
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

            let market_context = market_contexts.get(&window.last().unwrap().ts);
            if let Some(tick) = build_synthetic_tick(symbol, sector, window, market_context) {
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

    let report = build_deep_report(&all_results, STEP_SIZE == WINDOW_SIZE);
    print_deep_report(&report);
    match write_deep_report_files(&report, output_path) {
        Ok((json_path, csv_path)) => {
            println!("  Wrote JSON report to {}", json_path);
            println!("  Wrote CSV report to {}", csv_path);
        }
        Err(error) => {
            eprintln!("Failed to write backtest reports: {}", error);
        }
    }
}
