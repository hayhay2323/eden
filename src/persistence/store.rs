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

    /// Persist institution cross-stock presences for tracking over time.
    pub async fn write_institution_states(
        &self,
        presences: &[CrossStockPresence],
        timestamp: time::OffsetDateTime,
    ) -> Result<(), Box<dyn std::error::Error>> {
        for p in presences {
            let id = format!("inst_{}_{}", p.institution_id.0, timestamp.unix_timestamp());
            let record = serde_json::json!({
                "institution_id": p.institution_id.0,
                "timestamp": timestamp.format(&time::format_description::well_known::Rfc3339).unwrap_or_default(),
                "symbols": p.symbols.iter().map(|s| &s.0).collect::<Vec<_>>(),
                "ask_symbols": p.ask_symbols.iter().map(|s| &s.0).collect::<Vec<_>>(),
                "bid_symbols": p.bid_symbols.iter().map(|s| &s.0).collect::<Vec<_>>(),
                "seat_count": p.symbols.len(),
            });
            let _: Option<serde_json::Value> = self
                .db
                .upsert(("institution_state", &id))
                .content(record)
                .await?;
        }
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
