#[cfg(not(feature = "persistence"))]
fn main() {
    eprintln!("dbpeek requires building with --features persistence");
}

#[cfg(feature = "persistence")]
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use surrealdb::engine::local::RocksDb;
    use surrealdb::Surreal;

    let mut args = std::env::args().skip(1);
    let path = args.next().ok_or("usage: dbpeek <db-path> <workflow-id>")?;
    let workflow_id = args.next().ok_or("usage: dbpeek <db-path> <workflow-id>")?;

    let db = Surreal::new::<RocksDb>(&path).await?;
    db.use_ns("eden").use_db("market").await?;

    let mut result = db
        .query(
            "SELECT * FROM action_workflow_event WHERE workflow_id = $workflow_id ORDER BY recorded_at DESC LIMIT 10",
        )
        .bind(("workflow_id", workflow_id))
        .await?;
    let rows: Vec<serde_json::Value> = result.take(0)?;
    println!("{}", serde_json::to_string_pretty(&rows)?);
    Ok(())
}
