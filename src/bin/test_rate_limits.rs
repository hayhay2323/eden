//! Longport API Rate Limit Stress Test
//!
//! Tests the actual rate limits by progressively increasing request frequency.
//! Measures: max sustained req/s, burst capacity, error codes, recovery time.
//!
//! Run: cargo run --bin test_rate_limits
//!
//! Findings are printed as a summary table at the end.

use longport::quote::{CalcIndex, QuoteContext, SubFlags};
use longport::Config;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
struct TestResult {
    test_name: String,
    total_requests: u64,
    success_count: u64,
    error_count: u64,
    elapsed_ms: u64,
    effective_rps: f64,
    first_error_at_request: Option<u64>,
    error_messages: Vec<String>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();
    let config = Arc::new(Config::from_env()?);
    let (ctx, _receiver) = QuoteContext::try_new(config).await?;

    println!("╔══════════════════════════════════════════════════════════╗");
    println!("║       Longport API Rate Limit Stress Test               ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    let mut results: Vec<TestResult> = Vec::new();

    // ═══════════════════════════════════════════════════════════════
    // Test 1: Sequential single-symbol quote — find max req/s
    // ═══════════════════════════════════════════════════════════════
    println!("━━━ Test 1: Sequential quote (single symbol) ━━━");
    {
        let mut success = 0u64;
        let mut errors = 0u64;
        let mut first_error_at = None;
        let mut error_msgs = Vec::new();
        let start = Instant::now();
        let target = 30; // 30 sequential calls

        for i in 0..target {
            match ctx.quote(["700.HK"]).await {
                Ok(_) => success += 1,
                Err(e) => {
                    errors += 1;
                    if first_error_at.is_none() {
                        first_error_at = Some(i + 1);
                    }
                    let msg = format!("{e}");
                    if !error_msgs.contains(&msg) {
                        error_msgs.push(msg);
                    }
                }
            }
            print!("  [{}/{}] ok={} err={}\r", i + 1, target, success, errors);
        }
        let elapsed = start.elapsed();
        println!(
            "  [{}/{}] ok={} err={} in {:.1}s ({:.1} req/s)",
            target,
            target,
            success,
            errors,
            elapsed.as_secs_f64(),
            success as f64 / elapsed.as_secs_f64()
        );
        results.push(TestResult {
            test_name: "Sequential quote (1 sym)".into(),
            total_requests: target,
            success_count: success,
            error_count: errors,
            elapsed_ms: elapsed.as_millis() as u64,
            effective_rps: success as f64 / elapsed.as_secs_f64(),
            first_error_at_request: first_error_at,
            error_messages: error_msgs,
        });
    }

    tokio::time::sleep(Duration::from_secs(2)).await;

    // ═══════════════════════════════════════════════════════════════
    // Test 2: Batch quote — how many symbols per batch call?
    // ═══════════════════════════════════════════════════════════════
    println!("\n━━━ Test 2: Batch quote (increasing batch size) ━━━");
    {
        let all_symbols: Vec<String> = eden::hk::watchlist::WATCHLIST
            .iter()
            .map(|s| s.to_string())
            .collect();

        for batch_size in [10, 50, 100, 200, 500] {
            let batch: Vec<String> = all_symbols.iter().take(batch_size).cloned().collect();
            let start = Instant::now();
            match ctx.quote(batch).await {
                Ok(quotes) => {
                    let elapsed = start.elapsed();
                    println!(
                        "  batch={}: {} quotes in {:.0}ms",
                        batch_size,
                        quotes.len(),
                        elapsed.as_millis()
                    );
                }
                Err(e) => {
                    println!("  batch={}: ERROR {}", batch_size, e);
                }
            }
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }

    tokio::time::sleep(Duration::from_secs(2)).await;

    // ═══════════════════════════════════════════════════════════════
    // Test 3: Concurrent requests — find max concurrency
    // ═══════════════════════════════════════════════════════════════
    println!("\n━━━ Test 3: Concurrent requests (increasing parallelism) ━━━");
    for concurrency in [1, 2, 3, 5, 8, 10, 15, 20] {
        let success = Arc::new(AtomicU64::new(0));
        let errors = Arc::new(AtomicU64::new(0));
        let start = Instant::now();

        let mut handles = Vec::new();
        for _ in 0..concurrency {
            let ctx = ctx.clone();
            let success = success.clone();
            let errors = errors.clone();
            handles.push(tokio::spawn(async move {
                match ctx.quote(["700.HK"]).await {
                    Ok(_) => success.fetch_add(1, Ordering::Relaxed),
                    Err(_) => errors.fetch_add(1, Ordering::Relaxed),
                };
            }));
        }
        for h in handles {
            let _ = h.await;
        }
        let elapsed = start.elapsed();
        let ok = success.load(Ordering::Relaxed);
        let err = errors.load(Ordering::Relaxed);
        println!(
            "  concurrency={:2}: ok={} err={} in {:.0}ms",
            concurrency,
            ok,
            err,
            elapsed.as_millis()
        );
        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    tokio::time::sleep(Duration::from_secs(2)).await;

    // ═══════════════════════════════════════════════════════════════
    // Test 4: Per-symbol endpoints burst — capital_flow, depth, brokers
    // ═══════════════════════════════════════════════════════════════
    println!("\n━━━ Test 4: Per-symbol endpoint burst (capital_flow) ━━━");
    {
        let symbols: Vec<String> = eden::hk::watchlist::WATCHLIST
            .iter()
            .take(50)
            .map(|s| s.to_string())
            .collect();

        for concurrency in [1, 2, 4, 8] {
            let success = Arc::new(AtomicU64::new(0));
            let errors = Arc::new(AtomicU64::new(0));
            let error_msgs: Arc<tokio::sync::Mutex<Vec<String>>> =
                Arc::new(tokio::sync::Mutex::new(Vec::new()));
            let start = Instant::now();

            let mut handles = Vec::new();
            for sym in &symbols {
                let ctx = ctx.clone();
                let sym = sym.clone();
                let success = success.clone();
                let errors = errors.clone();
                let error_msgs = error_msgs.clone();

                // Limit concurrency with a semaphore
                handles.push(tokio::spawn(async move {
                    match ctx.capital_flow(sym).await {
                        Ok(_) => {
                            success.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(e) => {
                            errors.fetch_add(1, Ordering::Relaxed);
                            let msg = format!("{e}");
                            let mut msgs = error_msgs.lock().await;
                            if msgs.len() < 5 && !msgs.contains(&msg) {
                                msgs.push(msg);
                            }
                        }
                    }
                }));

                // Control concurrency: wait every N spawns
                if handles.len() >= concurrency {
                    for h in handles.drain(..) {
                        let _ = h.await;
                    }
                }
            }
            for h in handles {
                let _ = h.await;
            }
            let elapsed = start.elapsed();
            let ok = success.load(Ordering::Relaxed);
            let err = errors.load(Ordering::Relaxed);
            let msgs = error_msgs.lock().await;
            println!(
                "  capital_flow x50, concurrency={}: ok={} err={} in {:.1}s ({:.1} req/s){}",
                concurrency,
                ok,
                err,
                elapsed.as_secs_f64(),
                ok as f64 / elapsed.as_secs_f64(),
                if msgs.is_empty() {
                    String::new()
                } else {
                    format!(" errors: {:?}", &msgs[..1])
                }
            );
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    tokio::time::sleep(Duration::from_secs(2)).await;

    // ═══════════════════════════════════════════════════════════════
    // Test 5: Depth burst
    // ═══════════════════════════════════════════════════════════════
    println!("\n━━━ Test 5: Depth endpoint burst ━━━");
    {
        let symbols: Vec<String> = eden::hk::watchlist::WATCHLIST
            .iter()
            .take(30)
            .map(|s| s.to_string())
            .collect();

        for concurrency in [1, 2, 4, 8] {
            let success = Arc::new(AtomicU64::new(0));
            let errors = Arc::new(AtomicU64::new(0));
            let start = Instant::now();

            let mut handles = Vec::new();
            for sym in &symbols {
                let ctx = ctx.clone();
                let sym = sym.clone();
                let success = success.clone();
                let errors = errors.clone();

                handles.push(tokio::spawn(async move {
                    match ctx.depth(sym).await {
                        Ok(_) => {
                            success.fetch_add(1, Ordering::Relaxed);
                        }
                        Err(_) => {
                            errors.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }));

                if handles.len() >= concurrency {
                    for h in handles.drain(..) {
                        let _ = h.await;
                    }
                }
            }
            for h in handles {
                let _ = h.await;
            }
            let elapsed = start.elapsed();
            let ok = success.load(Ordering::Relaxed);
            let err = errors.load(Ordering::Relaxed);
            println!(
                "  depth x30, concurrency={}: ok={} err={} in {:.1}s ({:.1} req/s)",
                concurrency,
                ok,
                err,
                elapsed.as_secs_f64(),
                ok as f64 / elapsed.as_secs_f64()
            );
            tokio::time::sleep(Duration::from_secs(2)).await;
        }
    }

    tokio::time::sleep(Duration::from_secs(2)).await;

    // ═══════════════════════════════════════════════════════════════
    // Test 6: calc_indexes batch — the most efficient endpoint
    // ═══════════════════════════════════════════════════════════════
    println!("\n━━━ Test 6: calc_indexes batch efficiency ━━━");
    {
        let all_hk: Vec<String> = eden::hk::watchlist::WATCHLIST
            .iter()
            .map(|s| s.to_string())
            .collect();
        let calc_fields = vec![
            CalcIndex::TurnoverRate,
            CalcIndex::VolumeRatio,
            CalcIndex::CapitalFlow,
            CalcIndex::ChangeRate,
            CalcIndex::TotalMarketValue,
            CalcIndex::FiveMinutesChangeRate,
        ];

        for batch_size in [50, 100, 200, 500] {
            let batch: Vec<String> = all_hk.iter().take(batch_size).cloned().collect();
            let start = Instant::now();
            match ctx.calc_indexes(batch, calc_fields.clone()).await {
                Ok(indexes) => {
                    let elapsed = start.elapsed();
                    println!(
                        "  calc_indexes batch={}: {} results in {:.0}ms",
                        batch_size,
                        indexes.len(),
                        elapsed.as_millis()
                    );
                }
                Err(e) => {
                    println!("  calc_indexes batch={}: ERROR {}", batch_size, e);
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }
    }

    tokio::time::sleep(Duration::from_secs(2)).await;

    // ═══════════════════════════════════════════════════════════════
    // Test 7: Sustained mixed workload — simulates real tick cycle
    // ═══════════════════════════════════════════════════════════════
    println!("\n━━━ Test 7: Simulated tick cycle (5 cycles) ━━━");
    {
        let all_hk: Vec<String> = eden::hk::watchlist::WATCHLIST
            .iter()
            .take(500)
            .map(|s| s.to_string())
            .collect();
        let top_80: Vec<String> = all_hk.iter().take(80).cloned().collect();
        let calc_fields = vec![
            CalcIndex::TurnoverRate,
            CalcIndex::VolumeRatio,
            CalcIndex::CapitalFlow,
            CalcIndex::ChangeRate,
            CalcIndex::TotalMarketValue,
        ];

        for cycle in 1..=5 {
            let cycle_start = Instant::now();
            let mut cycle_ok = 0u64;
            let mut cycle_err = 0u64;

            // 1. Batch quote (500 symbols, 1 request)
            match ctx.quote(all_hk.clone()).await {
                Ok(_) => cycle_ok += 1,
                Err(e) => {
                    cycle_err += 1;
                    eprintln!("  cycle {}: quote err: {}", cycle, e);
                }
            }

            // 2. Batch calc_indexes (500 symbols, 1 request)
            match ctx.calc_indexes(all_hk.clone(), calc_fields.clone()).await {
                Ok(_) => cycle_ok += 1,
                Err(e) => {
                    cycle_err += 1;
                    eprintln!("  cycle {}: calc err: {}", cycle, e);
                }
            }

            // 3. Per-symbol capital_flow for top 80 (concurrency 4)
            let mut handles = Vec::new();
            let ok_counter = Arc::new(AtomicU64::new(0));
            let err_counter = Arc::new(AtomicU64::new(0));
            for sym in &top_80 {
                let ctx = ctx.clone();
                let sym = sym.clone();
                let ok_counter = ok_counter.clone();
                let err_counter = err_counter.clone();
                handles.push(tokio::spawn(async move {
                    match ctx.capital_flow(sym).await {
                        Ok(_) => ok_counter.fetch_add(1, Ordering::Relaxed),
                        Err(_) => err_counter.fetch_add(1, Ordering::Relaxed),
                    };
                }));
                if handles.len() >= 4 {
                    for h in handles.drain(..) {
                        let _ = h.await;
                    }
                }
            }
            for h in handles {
                let _ = h.await;
            }
            cycle_ok += ok_counter.load(Ordering::Relaxed);
            cycle_err += err_counter.load(Ordering::Relaxed);

            // 4. Per-symbol capital_distribution for top 80 (concurrency 4)
            let mut handles = Vec::new();
            let ok_counter = Arc::new(AtomicU64::new(0));
            let err_counter = Arc::new(AtomicU64::new(0));
            for sym in &top_80 {
                let ctx = ctx.clone();
                let sym = sym.clone();
                let ok_counter = ok_counter.clone();
                let err_counter = err_counter.clone();
                handles.push(tokio::spawn(async move {
                    match ctx.capital_distribution(sym).await {
                        Ok(_) => ok_counter.fetch_add(1, Ordering::Relaxed),
                        Err(_) => err_counter.fetch_add(1, Ordering::Relaxed),
                    };
                }));
                if handles.len() >= 4 {
                    for h in handles.drain(..) {
                        let _ = h.await;
                    }
                }
            }
            for h in handles {
                let _ = h.await;
            }
            cycle_ok += ok_counter.load(Ordering::Relaxed);
            cycle_err += err_counter.load(Ordering::Relaxed);

            let elapsed = cycle_start.elapsed();
            let total_reqs = cycle_ok + cycle_err;
            println!(
                "  cycle {}: {} reqs (ok={} err={}) in {:.1}s ({:.1} req/s)",
                cycle,
                total_reqs,
                cycle_ok,
                cycle_err,
                elapsed.as_secs_f64(),
                total_reqs as f64 / elapsed.as_secs_f64()
            );

            // Wait before next cycle (simulate rest_refresh_secs interval)
            if cycle < 5 {
                tokio::time::sleep(Duration::from_secs(3)).await;
            }
        }
    }

    tokio::time::sleep(Duration::from_secs(2)).await;

    // ═══════════════════════════════════════════════════════════════
    // Test 8: WebSocket subscription limit
    // ═══════════════════════════════════════════════════════════════
    println!("\n━━━ Test 8: WebSocket subscription capacity ━━━");
    {
        // Try subscribing to increasing numbers of symbols
        let all_hk: Vec<&str> = eden::hk::watchlist::WATCHLIST.to_vec();

        for count in [100, 200, 300, 400, 500] {
            let batch: Vec<&str> = all_hk.iter().take(count).copied().collect();
            let start = Instant::now();
            match ctx
                .subscribe(&batch, SubFlags::QUOTE | SubFlags::TRADE)
                .await
            {
                Ok(_) => {
                    let elapsed = start.elapsed();
                    println!(
                        "  subscribe {} symbols (QUOTE+TRADE): OK in {:.0}ms",
                        count,
                        elapsed.as_millis()
                    );
                }
                Err(e) => {
                    println!("  subscribe {} symbols: ERROR {}", count, e);
                }
            }
            tokio::time::sleep(Duration::from_millis(500)).await;
        }

        // Try all 4 channels
        println!("  Trying full 4-channel subscription (500 symbols)...");
        let batch500: Vec<&str> = all_hk.iter().take(500).copied().collect();
        match ctx
            .subscribe(
                &batch500,
                SubFlags::QUOTE | SubFlags::TRADE | SubFlags::DEPTH | SubFlags::BROKER,
            )
            .await
        {
            Ok(_) => println!("  subscribe 500 x 4 channels: OK"),
            Err(e) => println!("  subscribe 500 x 4 channels: ERROR {}", e),
        }

        // Try >500
        if all_hk.len() > 500 {
            println!("  Trying 501 symbols...");
            let batch501: Vec<&str> = all_hk.iter().take(501).copied().collect();
            match ctx.subscribe(&batch501, SubFlags::QUOTE).await {
                Ok(_) => println!("  subscribe 501 x QUOTE: OK (limit may be higher than 500)"),
                Err(e) => println!("  subscribe 501: ERROR {} (confirms 500 limit)", e),
            }
        }
    }

    // ═══════════════════════════════════════════════════════════════
    // Summary
    // ═══════════════════════════════════════════════════════════════
    println!("\n╔══════════════════════════════════════════════════════════╗");
    println!("║                     SUMMARY                              ║");
    println!("╚══════════════════════════════════════════════════════════╝\n");

    for r in &results {
        println!("  {}", r.test_name);
        println!(
            "    {} reqs in {}ms → {:.1} req/s (ok={} err={})",
            r.total_requests, r.elapsed_ms, r.effective_rps, r.success_count, r.error_count
        );
        if let Some(first_err) = r.first_error_at_request {
            println!("    First error at request #{}", first_err);
        }
        if !r.error_messages.is_empty() {
            println!("    Errors: {:?}", r.error_messages);
        }
    }

    println!("\n  Key Longport limits (from docs):");
    println!("    Quote API: 10 req/s, max 5 concurrent");
    println!("    Trade API: 30 req/30s");
    println!("    WebSocket: 500 symbols max, 1 connection/account");
    println!("    Batch endpoints (quote, calc_indexes): up to 500 symbols/request");
    println!(
        "\n  Current Eden config: HK={} symbols, US={} symbols",
        eden::hk::watchlist::WATCHLIST.len(),
        eden::us::watchlist::US_WATCHLIST.len()
    );

    Ok(())
}
