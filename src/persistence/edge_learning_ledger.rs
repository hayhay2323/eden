use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::graph::edge_learning::{EdgeCredit, EdgeKey, EdgeLearningLedger};
use crate::ontology::objects::{InstitutionId, SectorId, Symbol};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeLearningLedgerRecord {
    pub ledger_id: String,
    pub market: String,
    pub entries: Vec<EdgeLearningEntryRecord>,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EdgeLearningEntryRecord {
    pub edge_id: String,
    pub key: EdgeLearningKeyRecord,
    pub total_credit: Decimal,
    pub sample_count: u32,
    pub mean_credit: Decimal,
    pub last_updated: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EdgeLearningKeyRecord {
    InstitutionToStock { institution_id: i32, symbol: String },
    StockToStock { a: String, b: String },
    StockToSector { symbol: String, sector_id: String },
}

impl EdgeLearningLedgerRecord {
    pub fn ledger_id_for(market: &str) -> String {
        format!("edge-ledger:{market}")
    }

    pub fn from_ledger(market: &str, ledger: &EdgeLearningLedger, now: OffsetDateTime) -> Self {
        Self {
            ledger_id: Self::ledger_id_for(market),
            market: market.to_string(),
            entries: ledger
                .iter()
                .map(|(key, credit)| EdgeLearningEntryRecord::from_parts(key, credit))
                .collect(),
            updated_at: format_time(now),
        }
    }

    pub fn into_ledger(self) -> EdgeLearningLedger {
        EdgeLearningLedger::from_entries(self.entries.into_iter().filter_map(|entry| {
            let key = entry.key.into_edge_key()?;
            let last_updated = parse_time(&entry.last_updated)?;
            Some((
                key,
                EdgeCredit {
                    total_credit: entry.total_credit,
                    sample_count: entry.sample_count,
                    mean_credit: entry.mean_credit,
                    last_updated,
                },
            ))
        }))
    }

    pub fn record_id(&self) -> &str {
        &self.ledger_id
    }
}

impl EdgeLearningEntryRecord {
    fn from_parts(key: &EdgeKey, credit: &EdgeCredit) -> Self {
        Self {
            edge_id: edge_key_id(key),
            key: EdgeLearningKeyRecord::from_edge_key(key),
            total_credit: credit.total_credit,
            sample_count: credit.sample_count,
            mean_credit: credit.mean_credit,
            last_updated: format_time(credit.last_updated),
        }
    }
}

impl EdgeLearningKeyRecord {
    fn from_edge_key(key: &EdgeKey) -> Self {
        match key {
            EdgeKey::InstitutionToStock {
                institution_id,
                symbol,
            } => Self::InstitutionToStock {
                institution_id: institution_id.0,
                symbol: symbol.0.clone(),
            },
            EdgeKey::StockToStock { a, b } => Self::StockToStock {
                a: a.0.clone(),
                b: b.0.clone(),
            },
            EdgeKey::StockToSector { symbol, sector_id } => Self::StockToSector {
                symbol: symbol.0.clone(),
                sector_id: sector_id.0.clone(),
            },
        }
    }

    fn into_edge_key(self) -> Option<EdgeKey> {
        Some(match self {
            Self::InstitutionToStock {
                institution_id,
                symbol,
            } => EdgeKey::InstitutionToStock {
                institution_id: InstitutionId(institution_id),
                symbol: Symbol(symbol),
            },
            Self::StockToStock { a, b } => EdgeKey::StockToStock {
                a: Symbol(a),
                b: Symbol(b),
            },
            Self::StockToSector { symbol, sector_id } => EdgeKey::StockToSector {
                symbol: Symbol(symbol),
                sector_id: SectorId(sector_id),
            },
        })
    }
}

fn edge_key_id(key: &EdgeKey) -> String {
    match key {
        EdgeKey::InstitutionToStock {
            institution_id,
            symbol,
        } => format!("inst:{}:{}", institution_id.0, symbol.0),
        EdgeKey::StockToStock { a, b } => format!("stock:{}:{}", a.0, b.0),
        EdgeKey::StockToSector { symbol, sector_id } => {
            format!("sector:{}:{}", symbol.0, sector_id.0)
        }
    }
}

fn format_time(timestamp: OffsetDateTime) -> String {
    timestamp
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_else(|_| timestamp.to_string())
}

fn parse_time(value: &str) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(value, &time::format_description::well_known::Rfc3339).ok()
}
