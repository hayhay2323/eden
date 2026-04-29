# HK Minute-Level Backtest Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** A CLI binary that reads 2 years of HK minute-level candlestick data, runs Eden's predicate-state-mechanism pipeline on synthetic ticks, validates mechanism direction judgments against future price movement, and outputs per-mechanism hit rates at multiple time horizons.

**Architecture:** `src/bin/backtest.rs` reads chunk JSON files from disk, constructs synthetic `PredicateInputs` from rolling OHLCV windows, calls `derive_atomic_predicates` + `build_reasoning_profile`, records each judgment, then checks future bars for directional accuracy. A `src/backtest/` module provides the data loader, OHLCV-to-signal adapter, and result aggregation.

**Tech Stack:** Rust, serde_json for data loading, existing `eden` pipeline crate. No new dependencies.

---

## File Structure

| File | Role |
|------|------|
| Create: `src/backtest/mod.rs` | Module root |
| Create: `src/backtest/loader.rs` | Read chunk JSON files, merge + sort by timestamp |
| Create: `src/backtest/adapter.rs` | Convert OHLCV window into `PredicateInputs` |
| Create: `src/backtest/runner.rs` | Run pipeline on each tick, record judgments, validate against future bars |
| Create: `src/backtest/report.rs` | Aggregate results and print formatted report |
| Create: `src/bin/backtest.rs` | CLI entry point |
| Modify: `src/lib.rs` | Add `pub mod backtest;` |

---

### Task 1: Data loader

**Files:**
- Create: `src/backtest/mod.rs`
- Create: `src/backtest/loader.rs`
- Modify: `src/lib.rs`

- [ ] **Step 1: Create module structure**

`src/backtest/mod.rs`:
```rust
pub mod loader;
```

Add to `src/lib.rs`:
```rust
pub mod backtest;
```

- [ ] **Step 2: Implement the loader**

`src/backtest/loader.rs` needs:

```rust
use serde::Deserialize;
use std::path::Path;

#[derive(Debug, Clone, Deserialize)]
pub struct Bar {
    pub symbol: String,
    pub ts: i64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: u64,
    pub turnover: f64,
}

/// Load all chunk files for a symbol directory, merge and sort by timestamp.
pub fn load_symbol_bars(symbol_dir: &Path) -> Result<Vec<Bar>, String> {
    // Read all chunk_NNNN.json files
    // Parse each as Vec<Bar>
    // Concatenate all bars
    // Sort by ts ascending
    // Deduplicate by ts (keep first)
    // Return
}

/// Load bars for multiple symbols from the cache root directory.
/// Returns a map of symbol -> sorted bars.
pub fn load_symbols(
    cache_dir: &Path,
    symbols: &[&str],
) -> Result<std::collections::HashMap<String, Vec<Bar>>, String> {
    // For each symbol, convert "700.HK" -> "700_HK" directory name
    // Call load_symbol_bars
    // Collect into HashMap
}
```

Data lives at: `/Volumes/LaCie 1/eden-data/cache_1m/{symbol_dir}/chunk_NNNN.json`

Directory naming: `700.HK` -> `700_HK` (dot replaced with underscore).

Each chunk is a JSON array of Bar objects. Chunks are numbered but NOT necessarily in chronological order (chunk_0000 has the most recent data, chunk_0124 has the oldest). Must sort after merging.

- [ ] **Step 3: Add a unit test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bars_are_sorted_after_load() {
        // Only runs if LaCie is mounted
        let dir = Path::new("/Volumes/LaCie 1/eden-data/cache_1m/700_HK");
        if !dir.exists() {
            eprintln!("Skipping: LaCie not mounted");
            return;
        }
        let bars = load_symbol_bars(dir).unwrap();
        assert!(!bars.is_empty());
        for window in bars.windows(2) {
            assert!(window[0].ts <= window[1].ts, "bars not sorted");
        }
    }
}
```

- [ ] **Step 4: Verify**

Run: `cargo check --tests 2>&1 | tail -5`
Run: `cargo test --lib backtest::loader -q 2>&1 | tail -5`

- [ ] **Step 5: Commit**

```bash
git add src/backtest/ src/lib.rs
git commit -m "feat(backtest): add minute-level OHLCV loader for LaCie cache"
```

---

### Task 2: OHLCV-to-signal adapter

**Files:**
- Create: `src/backtest/adapter.rs`
- Modify: `src/backtest/mod.rs`

- [ ] **Step 1: Add module**

Add to `src/backtest/mod.rs`:
```rust
pub mod adapter;
```

- [ ] **Step 2: Implement the adapter**

`src/backtest/adapter.rs` converts a window of `Bar` into the `LiveSignal`, `LivePressure`, `LiveStressSnapshot`, `LiveTacticalCase`, and `LiveMarketRegime` that `PredicateInputs` needs.

Key derivations from a window of N bars (e.g., 30 bars = 30 minutes):

```rust
use rust_decimal::Decimal;
use crate::live_snapshot::*;
use crate::backtest::loader::Bar;

pub struct SyntheticTick {
    pub case: LiveTacticalCase,
    pub signal: LiveSignal,
    pub pressure: LivePressure,
    pub stress: LiveStressSnapshot,
    pub regime: LiveMarketRegime,
    /// The direction implied by the signal: +1 bullish, -1 bearish, 0 neutral.
    pub direction: i8,
    /// Timestamp of the last bar in the window.
    pub timestamp: i64,
}

/// Build a synthetic tick from a rolling window of bars for one symbol.
/// `window`: the most recent N bars for this symbol.
/// `sector`: the sector string for this symbol (from ObjectStore).
/// `market_bars`: bars from ALL symbols in the same time range (for stress computation).
pub fn build_synthetic_tick(
    symbol: &str,
    sector: &str,
    window: &[Bar],
    market_bars: &[(String, Vec<Bar>)],  // other symbols' bars for cross-sectional stress
) -> Option<SyntheticTick> {
    if window.len() < 5 { return None; }
    let current = window.last()?;
    let first = window.first()?;

    // ── Derive signal dimensions ──
    // capital_flow_direction: net buy pressure from volume-weighted price movement
    //   positive = more volume on up-bars, negative = more volume on down-bars
    let (up_vol, down_vol) = window.iter().fold((0.0_f64, 0.0_f64), |(up, down), bar| {
        if bar.close >= bar.open { (up + bar.volume as f64, down) }
        else { (up, down + bar.volume as f64) }
    });
    let total_vol = up_vol + down_vol;
    let capital_flow_direction = if total_vol > 0.0 {
        clamp((up_vol - down_vol) / total_vol)  // [-1, 1]
    } else { 0.0 };

    // price_momentum: return over the window
    let prev_close = first.open;
    let price_momentum = if prev_close > 0.0 {
        clamp(((current.close - prev_close) / prev_close) * 20.0)  // scale: 5% move = 1.0
    } else { 0.0 };

    // volume_profile: current volume vs window average
    let avg_vol = total_vol / window.len() as f64;
    let recent_vol = window[window.len().saturating_sub(5)..].iter()
        .map(|b| b.volume as f64).sum::<f64>() / 5.0;
    let volume_profile = if avg_vol > 0.0 {
        clamp((recent_vol / avg_vol - 1.0) * 2.0)  // 50% above avg = 1.0
    } else { 0.0 };

    let composite = capital_flow_direction * 0.4 + price_momentum * 0.4 + volume_profile * 0.2;

    // ── Derive pressure ──
    // Look at consecutive bar direction to estimate pressure duration
    let pressure_duration = window.iter().rev()
        .take_while(|b| (b.close >= b.open) == (current.close >= current.open))
        .count() as u64;
    let prev_5_pressure = if window.len() >= 10 {
        let prev_window = &window[window.len()-10..window.len()-5];
        let (pu, pd) = prev_window.iter().fold((0.0, 0.0), |(u, d), b| {
            if b.close >= b.open { (u + b.volume as f64, d) } else { (u, d + b.volume as f64) }
        });
        let pt = pu + pd;
        if pt > 0.0 { (pu - pd) / pt } else { 0.0 }
    } else { 0.0 };
    let pressure_delta = capital_flow_direction - prev_5_pressure;
    let accelerating = pressure_delta.abs() > 0.1 && pressure_delta.signum() == capital_flow_direction.signum();

    // ── Derive stress (cross-sectional) ──
    // Compute how synchronized the market is — high sync in negative direction = stress
    // This requires market_bars from other symbols
    let composite_stress = compute_cross_sectional_stress(current.ts, market_bars);

    // ── Direction ──
    let direction = if composite > 0.1 { 1i8 } else if composite < -0.1 { -1 } else { 0 };

    // ── Confidence ──
    let confidence = composite.abs().min(1.0);

    // ... construct and return SyntheticTick with all Live* types
    // using Decimal::try_from(f64) or rust_decimal_macros for conversion
}

fn compute_cross_sectional_stress(current_ts: i64, market_bars: &[(String, Vec<Bar>)]) -> f64 {
    // For each symbol, find bars near current_ts (within 60s)
    // Compute each symbol's recent return
    // Stress = proportion of symbols with negative returns * magnitude
    // High negative synchrony = high stress
    // ...
}

fn clamp(v: f64) -> f64 { v.max(-1.0).min(1.0) }
```

The adapter should convert f64 values to `Decimal` when constructing the Live* types.

- [ ] **Step 3: Add unit test**

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn make_bars(prices: &[(f64, f64)]) -> Vec<Bar> {
        prices.iter().enumerate().map(|(i, (open, close))| Bar {
            symbol: "700.HK".into(),
            ts: 1700000000 + (i as i64) * 60,
            open: *open, high: close.max(*open) + 0.5,
            low: close.min(*open) - 0.5, close: *close,
            volume: 100000, turnover: 100000.0 * close,
        }).collect()
    }

    #[test]
    fn uptrend_produces_positive_direction() {
        // Steadily rising prices
        let bars = make_bars(&[
            (100.0, 101.0), (101.0, 102.0), (102.0, 103.0),
            (103.0, 104.0), (104.0, 105.0), (105.0, 106.0),
        ]);
        let tick = build_synthetic_tick("700.HK", "tech", &bars, &[]).unwrap();
        assert_eq!(tick.direction, 1);
    }

    #[test]
    fn downtrend_produces_negative_direction() {
        let bars = make_bars(&[
            (106.0, 105.0), (105.0, 104.0), (104.0, 103.0),
            (103.0, 102.0), (102.0, 101.0), (101.0, 100.0),
        ]);
        let tick = build_synthetic_tick("700.HK", "tech", &bars, &[]).unwrap();
        assert_eq!(tick.direction, -1);
    }

    #[test]
    fn flat_produces_neutral() {
        let bars = make_bars(&[
            (100.0, 100.1), (100.1, 100.0), (100.0, 100.1),
            (100.1, 100.0), (100.0, 100.1), (100.1, 100.0),
        ]);
        let tick = build_synthetic_tick("700.HK", "tech", &bars, &[]).unwrap();
        assert_eq!(tick.direction, 0);
    }
}
```

- [ ] **Step 4: Verify**

Run: `cargo test --lib backtest::adapter -q 2>&1 | tail -5`
Expected: 3 passed

- [ ] **Step 5: Commit**

```bash
git add src/backtest/adapter.rs src/backtest/mod.rs
git commit -m "feat(backtest): add OHLCV-to-signal adapter for synthetic ticks"
```

---

### Task 3: Pipeline runner and judgment recording

**Files:**
- Create: `src/backtest/runner.rs`
- Modify: `src/backtest/mod.rs`

- [ ] **Step 1: Add module**

Add to `src/backtest/mod.rs`:
```rust
pub mod runner;
```

- [ ] **Step 2: Implement the runner**

`src/backtest/runner.rs`:

```rust
use rust_decimal::Decimal;
use crate::ontology::mechanisms::MechanismCandidateKind;
use crate::pipeline::mechanism_inference::build_reasoning_profile;
use crate::pipeline::predicate_engine::{derive_atomic_predicates, PredicateInputs};
use crate::backtest::adapter::SyntheticTick;
use crate::backtest::loader::Bar;

#[derive(Debug, Clone)]
pub struct Judgment {
    pub timestamp: i64,
    pub symbol: String,
    pub mechanism: MechanismCandidateKind,
    pub mechanism_label: String,
    pub direction: i8,       // +1 or -1
    pub confidence: Decimal,
    pub score: Decimal,
}

#[derive(Debug, Clone)]
pub struct ValidatedJudgment {
    pub judgment: Judgment,
    pub outcomes: Vec<HorizonOutcome>,
}

#[derive(Debug, Clone, Copy)]
pub struct HorizonOutcome {
    pub horizon_bars: usize,
    pub horizon_label: &'static str,
    pub future_return: Decimal,
    pub hit: bool,  // direction matches
}

pub const HORIZONS: &[(usize, &str)] = &[
    (5, "5m"),
    (30, "30m"),
    (60, "1h"),
    (390, "1d"),
];

/// Run the pipeline on a synthetic tick and return a Judgment if a mechanism fires.
pub fn evaluate_tick(tick: &SyntheticTick) -> Option<Judgment> {
    if tick.direction == 0 { return None; }  // Skip neutral — nothing to validate

    let inputs = PredicateInputs {
        tactical_case: &tick.case,
        active_positions: &[],
        chain: None,           // No backward chain from OHLCV
        pressure: Some(&tick.pressure),
        signal: Some(&tick.signal),
        causal: None,          // No causal leader from OHLCV
        track: None,           // No hypothesis track from OHLCV
        stress: &tick.stress,
        market_regime: &tick.regime,
        all_signals: std::slice::from_ref(&tick.signal),
        all_pressures: std::slice::from_ref(&tick.pressure),
        events: &[],           // No events from OHLCV
        cross_market_signals: &[],
        cross_market_anomalies: &[],
    };

    let predicates = derive_atomic_predicates(&inputs);
    let profile = build_reasoning_profile(&predicates, &[], None);
    let primary = profile.primary_mechanism?;

    Some(Judgment {
        timestamp: tick.timestamp,
        symbol: tick.case.symbol.clone(),
        mechanism: primary.kind,
        mechanism_label: primary.label,
        direction: tick.direction,
        confidence: primary.score,
        score: primary.score,
    })
}

/// Validate a judgment against future bars.
pub fn validate_judgment(
    judgment: &Judgment,
    future_bars: &[Bar],
    reference_price: f64,
) -> ValidatedJudgment {
    let outcomes = HORIZONS.iter().map(|(horizon_bars, label)| {
        let future_price = future_bars.get(*horizon_bars)
            .map(|b| b.close)
            .unwrap_or(reference_price);
        let future_return = if reference_price > 0.0 {
            Decimal::try_from((future_price - reference_price) / reference_price)
                .unwrap_or(Decimal::ZERO)
        } else {
            Decimal::ZERO
        };
        let hit = if judgment.direction > 0 {
            future_return > Decimal::ZERO
        } else {
            future_return < Decimal::ZERO
        };
        HorizonOutcome {
            horizon_bars: *horizon_bars,
            horizon_label: label,
            future_return,
            hit,
        }
    }).collect();

    ValidatedJudgment {
        judgment: judgment.clone(),
        outcomes,
    }
}
```

- [ ] **Step 3: Add unit test**

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn bullish_judgment_hits_on_price_increase() {
        let judgment = Judgment {
            timestamp: 1700000000,
            symbol: "700.HK".into(),
            mechanism: MechanismCandidateKind::MechanicalExecutionSignature,
            mechanism_label: "Mechanical Execution Signature".into(),
            direction: 1,
            confidence: dec!(0.7),
            score: dec!(0.7),
        };
        let future_bars: Vec<Bar> = (0..400).map(|i| Bar {
            symbol: "700.HK".into(), ts: 1700000000 + i * 60,
            open: 100.0 + i as f64 * 0.01, high: 100.5 + i as f64 * 0.01,
            low: 99.5, close: 100.0 + i as f64 * 0.02,
            volume: 100000, turnover: 10000000.0,
        }).collect();
        let validated = validate_judgment(&judgment, &future_bars, 100.0);
        assert!(validated.outcomes.iter().all(|o| o.hit));
    }
}
```

- [ ] **Step 4: Verify**

Run: `cargo test --lib backtest::runner -q 2>&1 | tail -5`

- [ ] **Step 5: Commit**

```bash
git add src/backtest/runner.rs src/backtest/mod.rs
git commit -m "feat(backtest): add pipeline runner with multi-horizon validation"
```

---

### Task 4: Report aggregation

**Files:**
- Create: `src/backtest/report.rs`
- Modify: `src/backtest/mod.rs`

- [ ] **Step 1: Add module**

Add to `src/backtest/mod.rs`:
```rust
pub mod report;
```

- [ ] **Step 2: Implement report aggregation**

`src/backtest/report.rs`:

```rust
use std::collections::HashMap;
use crate::backtest::runner::{ValidatedJudgment, HORIZONS};

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
    pub hit_rates: Vec<f64>,  // one per horizon
}

pub struct TotalRow {
    pub judgments: usize,
    pub hit_rates: Vec<f64>,
}

pub fn aggregate(results: &[ValidatedJudgment]) -> BacktestReport {
    let mut by_mechanism: HashMap<String, Vec<&ValidatedJudgment>> = HashMap::new();
    for result in results {
        by_mechanism.entry(result.judgment.mechanism_label.clone())
            .or_default()
            .push(result);
    }

    let mut mechanism_rows: Vec<MechanismRow> = by_mechanism.iter().map(|(mech, judgments)| {
        let hit_rates = HORIZONS.iter().enumerate().map(|(hi, _)| {
            let hits = judgments.iter().filter(|j| j.outcomes.get(hi).map(|o| o.hit).unwrap_or(false)).count();
            if judgments.is_empty() { 0.0 } else { hits as f64 / judgments.len() as f64 * 100.0 }
        }).collect();
        MechanismRow { mechanism: mech.clone(), judgments: judgments.len(), hit_rates }
    }).collect();
    mechanism_rows.sort_by(|a, b| b.judgments.cmp(&a.judgments));

    let total_hit_rates = HORIZONS.iter().enumerate().map(|(hi, _)| {
        let hits = results.iter().filter(|j| j.outcomes.get(hi).map(|o| o.hit).unwrap_or(false)).count();
        if results.is_empty() { 0.0 } else { hits as f64 / results.len() as f64 * 100.0 }
    }).collect();

    let symbols_count = {
        let mut syms: Vec<&str> = results.iter().map(|r| r.judgment.symbol.as_str()).collect();
        syms.sort(); syms.dedup(); syms.len()
    };

    BacktestReport {
        total_judgments: results.len(),
        mechanism_rows,
        total_row: TotalRow { judgments: results.len(), hit_rates: total_hit_rates },
        date_range: (String::new(), String::new()),  // filled by caller
        symbols_count,
    }
}

pub fn print_report(report: &BacktestReport) {
    println!();
    println!("═══════════════════════════════════════════════════════════════");
    println!("  Eden HK Backtest — {} to {}", report.date_range.0, report.date_range.1);
    println!("  Symbols: {} | Judgments: {}", report.symbols_count, report.total_judgments);
    println!("═══════════════════════════════════════════════════════════════");
    println!();
    println!("  {:<30} {:>6}  {:>6} {:>6} {:>6} {:>6}",
        "Mechanism", "Count", "5m", "30m", "1h", "1d");
    println!("  {}", "─".repeat(72));
    for row in &report.mechanism_rows {
        println!("  {:<30} {:>6}  {:>5.1}% {:>5.1}% {:>5.1}% {:>5.1}%",
            row.mechanism, row.judgments,
            row.hit_rates[0], row.hit_rates[1], row.hit_rates[2], row.hit_rates[3]);
    }
    println!("  {}", "─".repeat(72));
    println!("  {:<30} {:>6}  {:>5.1}% {:>5.1}% {:>5.1}% {:>5.1}%",
        "TOTAL", report.total_row.judgments,
        report.total_row.hit_rates[0], report.total_row.hit_rates[1],
        report.total_row.hit_rates[2], report.total_row.hit_rates[3]);
    println!("  {:<30} {:>6}  {:>5.1}% {:>5.1}% {:>5.1}% {:>5.1}%",
        "Baseline (random)", "", 50.0, 50.0, 50.0, 50.0);
    println!();
}
```

- [ ] **Step 3: Verify**

Run: `cargo check --tests 2>&1 | tail -5`

- [ ] **Step 4: Commit**

```bash
git add src/backtest/report.rs src/backtest/mod.rs
git commit -m "feat(backtest): add report aggregation and pretty-printing"
```

---

### Task 5: CLI binary and main loop

**Files:**
- Create: `src/bin/backtest.rs`

- [ ] **Step 1: Implement the binary**

`src/bin/backtest.rs`:

```rust
use std::collections::HashMap;
use std::path::Path;

use eden::backtest::adapter::build_synthetic_tick;
use eden::backtest::loader::load_symbols;
use eden::backtest::report::{aggregate, print_report};
use eden::backtest::runner::{evaluate_tick, validate_judgment};

/// Symbols to backtest — subset of the HK watchlist.
const BACKTEST_SYMBOLS: &[&str] = &[
    "700.HK", "9988.HK", "3690.HK", "9618.HK", "1810.HK",
    "981.HK", "2259.HK", "268.HK", "9999.HK", "1024.HK",
    "992.HK", "2382.HK", "285.HK", "3888.HK", "9626.HK",
    // Finance for rotation
    "1398.HK", "3988.HK", "939.HK", "2628.HK", "2318.HK",
    // Property for rotation
    "1109.HK", "688.HK", "16.HK",
];

/// Simple symbol-to-sector mapping for backtest.
fn symbol_sector(symbol: &str) -> &'static str {
    match symbol {
        "700.HK" | "9988.HK" | "3690.HK" | "9618.HK" | "1810.HK"
        | "268.HK" | "9999.HK" | "1024.HK" | "3888.HK" | "9626.HK" => "tech",
        "981.HK" | "2382.HK" | "285.HK" | "992.HK" | "2018.HK" => "semiconductor",
        "1398.HK" | "3988.HK" | "939.HK" | "2628.HK" | "2318.HK" => "finance",
        "1109.HK" | "688.HK" | "16.HK" => "property",
        "2259.HK" => "mining",
        _ => "other",
    }
}

/// Window size in bars (30 bars = 30 minutes at 1m frequency).
const WINDOW_SIZE: usize = 30;
/// Step size: how many bars to advance between ticks.
/// 30 = non-overlapping windows (one judgment per 30 minutes).
const STEP_SIZE: usize = 30;
/// Minimum future bars needed for validation (must cover longest horizon = 390 bars = 1 day).
const MIN_FUTURE_BARS: usize = 400;

fn main() {
    let cache_dir = std::env::args().nth(1)
        .unwrap_or_else(|| "/Volumes/LaCie 1/eden-data/cache_1m".into());

    println!("Loading data from {}...", cache_dir);
    let all_bars = load_symbols(Path::new(&cache_dir), BACKTEST_SYMBOLS)
        .expect("failed to load data");

    println!("Loaded {} symbols", all_bars.len());
    for (sym, bars) in &all_bars {
        println!("  {} — {} bars", sym, bars.len());
    }

    // Build market-level bar index for cross-sectional stress
    let market_bars: Vec<(String, Vec<eden::backtest::loader::Bar>)> = all_bars
        .iter()
        .map(|(sym, bars)| (sym.clone(), bars.clone()))
        .collect();

    let mut all_results = Vec::new();
    let mut judgment_count = 0_usize;

    for (symbol, bars) in &all_bars {
        let sector = symbol_sector(symbol);
        let mut offset = 0;

        while offset + WINDOW_SIZE + MIN_FUTURE_BARS < bars.len() {
            let window = &bars[offset..offset + WINDOW_SIZE];
            let future_bars = &bars[offset + WINDOW_SIZE..];

            if let Some(tick) = build_synthetic_tick(symbol, sector, window, &market_bars) {
                if let Some(judgment) = evaluate_tick(&tick) {
                    let reference_price = window.last().map(|b| b.close).unwrap_or(0.0);
                    let validated = validate_judgment(&judgment, future_bars, reference_price);
                    all_results.push(validated);
                    judgment_count += 1;
                }
            }

            offset += STEP_SIZE;
        }
    }

    println!("\nTotal judgments: {}", judgment_count);

    // Determine date range
    let mut report = aggregate(&all_results);
    if let (Some(first), Some(last)) = (
        all_results.iter().map(|r| r.judgment.timestamp).min(),
        all_results.iter().map(|r| r.judgment.timestamp).max(),
    ) {
        use chrono::{DateTime, Utc};
        let first_dt = DateTime::<Utc>::from_timestamp(first, 0)
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        let last_dt = DateTime::<Utc>::from_timestamp(last, 0)
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_default();
        report.date_range = (first_dt, last_dt);
    }

    print_report(&report);
}
```

Note: The binary uses `chrono` for date formatting — `chrono` is already a dependency in Cargo.toml. If `DateTime::from_timestamp` is not available in the project's chrono version, use `NaiveDateTime::from_timestamp_opt` instead.

- [ ] **Step 2: Verify compilation**

Run: `cargo check --bin backtest 2>&1 | tail -5`

- [ ] **Step 3: Test run with small subset**

Run: `cargo run --bin backtest -- "/Volumes/LaCie 1/eden-data/cache_1m" 2>&1 | tail -30`

This should load data and print results. First run may take a few minutes to process ~125K bars × 23 symbols.

- [ ] **Step 4: Commit**

```bash
git add src/bin/backtest.rs
git commit -m "feat(backtest): add CLI binary for HK minute-level mechanism backtest"
```

---

### Task 6: First run and tuning

**Files:**
- Potentially modify: `src/backtest/adapter.rs` (signal scaling adjustments)

- [ ] **Step 1: Run full backtest**

```bash
cargo run --release --bin backtest -- "/Volumes/LaCie 1/eden-data/cache_1m" 2>&1 | tee data/backtest_results.txt
```

Use `--release` for speed since we're processing millions of bars.

- [ ] **Step 2: Analyze results**

Check the output. Key questions:
- Is the total hit rate above 50%? (If yes, the pipeline has directional signal)
- Which mechanisms have highest hit rate? (These are the most reliable)
- Which horizons work best? (Tells you the optimal holding period)
- Are any mechanisms at exactly 50%? (These are noise — no signal)
- Are any mechanisms BELOW 50%? (These are inverse signals — also useful info)

- [ ] **Step 3: Save results and commit**

```bash
git add data/backtest_results.txt
git commit -m "data: first HK backtest results"
```
