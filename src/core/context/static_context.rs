use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Context that is computed once at startup and rarely changes.
///
/// Captures the structural environment of the trading session: which market
/// we are operating in, the symbol universe, and sector organisation.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticContext {
    /// Market identifier (e.g. "HK", "US").
    pub market: String,
    /// The full set of symbols tracked in this session.
    pub symbol_universe: Vec<String>,
    /// Sector -> member symbols mapping.
    pub sector_mappings: HashMap<String, Vec<String>>,
    /// ISO-8601 date string for the current trading session.
    pub session_date: String,
}

impl StaticContext {
    /// Build a new static context from startup parameters.
    pub fn new(
        market: String,
        symbol_universe: Vec<String>,
        sector_mappings: HashMap<String, Vec<String>>,
        session_date: String,
    ) -> Self {
        Self {
            market,
            symbol_universe,
            sector_mappings,
            session_date,
        }
    }

    /// Number of symbols in the universe.
    pub fn universe_size(&self) -> usize {
        self.symbol_universe.len()
    }

    /// Symbols belonging to the given sector, if known.
    pub fn sector_symbols(&self, sector: &str) -> Option<&Vec<String>> {
        self.sector_mappings.get(sector)
    }

    /// All sector names.
    pub fn sectors(&self) -> Vec<&String> {
        self.sector_mappings.keys().collect()
    }
}
