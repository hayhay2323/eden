use surrealdb::engine::local::{Db, RocksDb};
use surrealdb::Surreal;

use crate::ontology::links::CrossStockPresence;
use crate::ontology::objects::Symbol;
use crate::temporal::record::TickRecord;

use super::schema;

#[derive(Clone)]
pub struct EdenStore {
    db: Surreal<Db>,
}

impl EdenStore {
    /// Open or create the SurrealDB database at the given path.
    pub async fn open(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let db = Surreal::new::<RocksDb>(path).await?;
        db.use_ns("eden").use_db("market").await?;
        db.query(schema::SCHEMA).await?;
        Ok(Self { db })
    }

    /// Persist a tick record.
    pub async fn write_tick(&self, record: &TickRecord) -> Result<(), Box<dyn std::error::Error>> {
        let id = format!("tick_{}_{}", record.timestamp.unix_timestamp(), record.tick_number);
        let _: Option<serde_json::Value> = self
            .db
            .upsert(("tick_record", &id))
            .content(record.clone())
            .await?;
        Ok(())
    }

    /// Persist institution cross-stock presences in a single batch query.
    pub async fn write_institution_states(
        &self,
        presences: &[CrossStockPresence],
        timestamp: time::OffsetDateTime,
    ) -> Result<(), Box<dyn std::error::Error>> {
        if presences.is_empty() {
            return Ok(());
        }

        let ts_str = timestamp
            .format(&time::format_description::well_known::Rfc3339)
            .unwrap_or_default();

        // Build all records as a single batch
        let records: Vec<serde_json::Value> = presences
            .iter()
            .map(|p| {
                serde_json::json!({
                    "institution_id": p.institution_id.0,
                    "timestamp": ts_str,
                    "symbols": p.symbols.iter().map(|s| &s.0).collect::<Vec<_>>(),
                    "ask_symbols": p.ask_symbols.iter().map(|s| &s.0).collect::<Vec<_>>(),
                    "bid_symbols": p.bid_symbols.iter().map(|s| &s.0).collect::<Vec<_>>(),
                    "seat_count": p.symbols.len(),
                })
            })
            .collect();

        // Single INSERT with all records — one DB round-trip instead of O(n)
        self.db
            .query("INSERT INTO institution_state $records")
            .bind(("records", records))
            .await?;

        Ok(())
    }

    /// Query recent tick records for a symbol.
    pub async fn recent_ticks(
        &self,
        symbol: &Symbol,
        limit: usize,
    ) -> Result<Vec<TickRecord>, Box<dyn std::error::Error>> {
        let query = format!(
            "SELECT * FROM tick_record WHERE signals.`{sym}`.composite != NONE ORDER BY tick_number DESC LIMIT {limit}",
            sym = symbol.0,
            limit = limit,
        );
        let mut result = self.db.query(&query).await?;
        let records: Vec<TickRecord> = result.take(0)?;
        Ok(records)
    }
}
