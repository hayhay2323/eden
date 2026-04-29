#[cfg(feature = "persistence")]
use surrealdb::engine::local::{Db, RocksDb};
#[cfg(feature = "persistence")]
use surrealdb::Surreal;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    #[cfg(not(feature = "persistence"))]
    {
        eprintln!("analyze requires building with `--features persistence`");
        return Ok(());
    }

    #[cfg(feature = "persistence")]
    {
        let db: Surreal<Db> = Surreal::new::<RocksDb>("data/eden.db").await?;
        db.use_ns("eden").use_db("market").await?;
        println!("Connected.\n");

        // Check signals across multiple ticks
        for tick_num in [1, 10, 50, 100, 500, 792] {
            let mut res = db
                .query("SELECT tick_number, signals FROM tick_record WHERE tick_number = $tick_num")
                .bind(("tick_num", tick_num))
                .await?;
            let raw: Vec<serde_json::Value> = res.take(0)?;
            if let Some(record) = raw.first() {
                let sig_count = record
                    .get("signals")
                    .and_then(|s| s.as_object())
                    .map(|o| o.len())
                    .unwrap_or(0);
                println!("Tick {:4}: signals has {} symbols", tick_num, sig_count);

                // Show first signal if exists
                if sig_count > 0 {
                    if let Some(signals) = record.get("signals").and_then(|s| s.as_object()) {
                        if let Some((sym, val)) = signals.iter().next() {
                            let s = serde_json::to_string(val).unwrap();
                            println!("  sample {}: {}...", sym, &s[..s.len().min(150)]);
                        }
                    }
                }
            } else {
                println!("Tick {:4}: not found", tick_num);
            }
        }

        println!("\nKnowledge event state counts:");
        for market in ["us", "hk"] {
            let mut res = db
                .query("SELECT count() AS count FROM knowledge_event_state WHERE market = $market")
                .bind(("market", market.to_string()))
                .await?;
            let raw: Vec<serde_json::Value> = res.take(0)?;
            let count = raw
                .first()
                .and_then(|row| row.get("count"))
                .and_then(|value| value.as_u64())
                .unwrap_or(0);
            println!("  {} state rows: {}", market, count);
        }

        let mut event_state = db
            .query(
                "SELECT event_id, kind, subject_node_id, object_node_id, latest_tick_number \
             FROM knowledge_event_state WHERE market = 'us' \
             ORDER BY latest_tick_number DESC LIMIT 10",
            )
            .await?;
        let state_rows: Vec<serde_json::Value> = event_state.take(0)?;
        println!("\nLatest US knowledge_event_state rows:");
        for row in state_rows {
            println!(
                "  {}  kind={}  subj={}  obj={}  tick={}",
                row.get("event_id").and_then(|v| v.as_str()).unwrap_or("?"),
                row.get("kind").and_then(|v| v.as_str()).unwrap_or("?"),
                row.get("subject_node_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?"),
                row.get("object_node_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-"),
                row.get("latest_tick_number")
                    .and_then(|v| v.as_i64())
                    .unwrap_or_default()
            );
        }

        let mut event_history = db
            .query(
                "SELECT event_id, kind, subject_node_id, object_node_id, tick_number \
             FROM knowledge_event_history WHERE market = 'us' \
             ORDER BY tick_number DESC LIMIT 10",
            )
            .await?;
        let history_rows: Vec<serde_json::Value> = event_history.take(0)?;
        println!("\nLatest US knowledge_event_history rows:");
        for row in history_rows {
            println!(
                "  {}  kind={}  subj={}  obj={}  tick={}",
                row.get("event_id").and_then(|v| v.as_str()).unwrap_or("?"),
                row.get("kind").and_then(|v| v.as_str()).unwrap_or("?"),
                row.get("subject_node_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("?"),
                row.get("object_node_id")
                    .and_then(|v| v.as_str())
                    .unwrap_or("-"),
                row.get("tick_number")
                    .and_then(|v| v.as_i64())
                    .unwrap_or_default()
            );
        }

        println!("\nDone.");
        Ok(())
    }
}
