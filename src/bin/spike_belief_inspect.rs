//! One-off spike tool for the Polymarket Phase 1 feasibility memo.
//! Counts belief_snapshot rows + dumps time range / per-market split.
//!
//! Usage:
//!   cargo run --bin spike_belief_inspect --features persistence -- data/eden-hk.db
//!   cargo run --bin spike_belief_inspect --features persistence -- data/eden-us.db
//!
//! Delete after Phase 1 spike completes.

#[cfg(not(feature = "persistence"))]
fn main() {
    eprintln!("spike_belief_inspect requires --features persistence");
}

#[cfg(feature = "persistence")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use surrealdb::engine::local::RocksDb;
    use surrealdb::Surreal;

    let path = std::env::args()
        .nth(1)
        .ok_or("usage: spike_belief_inspect <db-path>")?;

    let db = Surreal::new::<RocksDb>(&path).await?;
    db.use_ns("eden").use_db("market").await?;

    println!("=== {} ===\n", path);

    for table in [
        "belief_snapshot",
        "intent_belief_snapshot",
        "broker_archetype_snapshot",
        "regime_fingerprint_snapshot",
        "tick_record",
        "us_tick_record",
    ] {
        let q = format!("SELECT count() FROM {} GROUP ALL", table);
        let count: Option<i64> = match db.query(&q).await {
            Ok(mut r) => r.take((0, "count")).unwrap_or(None),
            Err(e) => {
                println!("{:<32} ERROR: {}", table, e);
                continue;
            }
        };
        let count = count.unwrap_or(0);
        if count == 0 {
            println!("{:<32} {:>8} rows", table, count);
            continue;
        }

        let ts_field = if table.ends_with("tick_record") {
            "timestamp"
        } else {
            "snapshot_ts"
        };

        let q_first = format!(
            "SELECT {0} as ts FROM {1} ORDER BY {0} ASC LIMIT 1",
            ts_field, table
        );
        let q_last = format!(
            "SELECT {0} as ts FROM {1} ORDER BY {0} DESC LIMIT 1",
            ts_field, table
        );

        let first: Option<serde_json::Value> = db
            .query(&q_first)
            .await?
            .take::<Vec<serde_json::Value>>(0)?
            .into_iter()
            .next();
        let last: Option<serde_json::Value> = db
            .query(&q_last)
            .await?
            .take::<Vec<serde_json::Value>>(0)?
            .into_iter()
            .next();

        let first_ts = first
            .as_ref()
            .and_then(|v| v.get("ts"))
            .and_then(|v| v.as_str().map(String::from).or_else(|| Some(v.to_string())))
            .unwrap_or_else(|| "?".into());
        let last_ts = last
            .as_ref()
            .and_then(|v| v.get("ts"))
            .and_then(|v| v.as_str().map(String::from).or_else(|| Some(v.to_string())))
            .unwrap_or_else(|| "?".into());

        println!(
            "{:<32} {:>8} rows  [{} -> {}]",
            table, count, first_ts, last_ts
        );

        // Distinct ts count + 5 sample timestamps
        let q_sample = format!(
            "SELECT {} as ts FROM {} GROUP BY ts ORDER BY ts ASC LIMIT 12",
            ts_field, table
        );
        if let Ok(mut r) = db.query(&q_sample).await {
            let rows: Vec<serde_json::Value> = r.take(0).unwrap_or_default();
            let n_distinct = rows.len();
            let preview: Vec<String> = rows
                .iter()
                .filter_map(|v| v.get("ts").and_then(|t| t.as_str().map(String::from)))
                .collect();
            println!(
                "  └─ distinct ts (capped 12): {}  preview: {:?}",
                n_distinct,
                preview
                    .iter()
                    .take(5)
                    .chain(preview.iter().rev().take(2))
                    .collect::<Vec<_>>()
            );
        }
    }

    Ok(())
}
