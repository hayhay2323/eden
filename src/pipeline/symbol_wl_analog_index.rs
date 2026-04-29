//! Per-symbol structural analog index keyed on WL graph signature.
//!
//! Builds on `wl_graph_signature` — for each (symbol, snapshot) pair we
//! compute a `signature_hash` over the typed sub-KG, then look up which
//! prior (symbol, snapshot) pairs shared that exact signature.
//!
//! Why exact match: two sub-KGs with the same h-WL signature are
//! h-WL-equivalent (depth-h structurally indistinguishable). With h=2
//! that's a strong claim — same node types, same value buckets, same
//! 2-hop neighborhoods. When exact match fires, the structures really
//! ARE the same shape.
//!
//! V1 = exact match only. Approximate (jaccard-similarity-based)
//! nearest-neighbor lookup is deferred to a future iteration —
//! comparing all 500 symbols' histograms vs all stored histograms each
//! tick is ~M⋅N work that needs an index (LSH on bag-of-labels). For
//! now, exact match gives the strongest signal and is O(1) per query.
//!
//! In-memory storage cap: last `MAX_HISTORY_PER_SIG` visits per
//! signature, last `MAX_TOTAL_SIGNATURES` distinct signatures total
//! (LRU eviction by signature first-seen time). Bounded memory.
//!
//! Output: `.run/eden-wl-analog-{market}.ndjson` — one row per symbol
//! per snapshot tick when there's at least one prior match.

use std::collections::{HashMap, VecDeque};
use std::io::Write;

use chrono::{DateTime, Utc};
use serde::Serialize;

/// Cap visits per signature_hash. When exceeded, oldest entry pops.
pub const MAX_HISTORY_PER_SIG: usize = 100;

/// Cap total distinct signatures held. When exceeded, the
/// least-recently-touched signature is evicted entirely.
pub const MAX_TOTAL_SIGNATURES: usize = 50_000;

#[derive(Debug, Clone, Serialize)]
pub struct WLVisit {
    pub ts: DateTime<Utc>,
    pub symbol: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct AnalogMatch {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub symbol: String,
    pub signature_hash: String,
    /// Total prior visits to this signature (across all symbols).
    pub historical_visits: usize,
    /// How many distinct symbols have ever produced this signature.
    pub distinct_symbol_count: usize,
    /// Last time this signature was seen (any symbol).
    pub last_seen_ts: Option<DateTime<Utc>>,
    pub last_seen_symbol: Option<String>,
    /// Up to 5 most-recent matches for operator inspection.
    pub recent_matches: Vec<WLVisit>,
}

#[derive(Debug, Default)]
pub struct SymbolWlAnalogIndex {
    /// signature_hash → ordered (oldest first) list of visits.
    history: HashMap<String, VecDeque<WLVisit>>,
    /// Insertion order tracking for LRU eviction at the signature
    /// level. Oldest first.
    sig_order: VecDeque<String>,
}

impl SymbolWlAnalogIndex {
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a (symbol, signature) visit. Returns the analog match
    /// summary BEFORE this visit is added (so historical_visits
    /// reflects prior visits, not including the current one).
    pub fn record(
        &mut self,
        market: &str,
        symbol: &str,
        signature_hash: &str,
        ts: DateTime<Utc>,
    ) -> AnalogMatch {
        // Build summary from current state BEFORE mutating.
        let history_for_sig = self.history.get(signature_hash);
        let historical_visits = history_for_sig.map(|d| d.len()).unwrap_or(0);
        let distinct_symbol_count = history_for_sig
            .map(|d| {
                let mut s: std::collections::HashSet<&str> = std::collections::HashSet::new();
                for v in d.iter() {
                    s.insert(v.symbol.as_str());
                }
                s.len()
            })
            .unwrap_or(0);
        let last = history_for_sig.and_then(|d| d.back());
        let last_seen_ts = last.map(|v| v.ts);
        let last_seen_symbol = last.map(|v| v.symbol.clone());
        let mut recent_matches: Vec<WLVisit> = history_for_sig
            .map(|d| d.iter().rev().take(5).cloned().collect())
            .unwrap_or_default();
        // Reverse so most-recent is last (more natural for ndjson reading).
        recent_matches.reverse();

        let summary = AnalogMatch {
            ts,
            market: market.to_string(),
            symbol: symbol.to_string(),
            signature_hash: signature_hash.to_string(),
            historical_visits,
            distinct_symbol_count,
            last_seen_ts,
            last_seen_symbol,
            recent_matches,
        };

        // Now mutate: append the new visit.
        let visit = WLVisit {
            ts,
            symbol: symbol.to_string(),
        };
        let entry = self
            .history
            .entry(signature_hash.to_string())
            .or_insert_with(|| {
                self.sig_order.push_back(signature_hash.to_string());
                VecDeque::new()
            });
        entry.push_back(visit);
        while entry.len() > MAX_HISTORY_PER_SIG {
            entry.pop_front();
        }

        // LRU eviction at signature level.
        while self.sig_order.len() > MAX_TOTAL_SIGNATURES {
            if let Some(evict) = self.sig_order.pop_front() {
                self.history.remove(&evict);
            }
        }

        summary
    }

    pub fn distinct_signatures(&self) -> usize {
        self.history.len()
    }

    pub fn total_visits(&self) -> usize {
        self.history.values().map(|d| d.len()).sum()
    }
}

/// Append AnalogMatch rows to ndjson — only those with at least one
/// historical visit (no point logging "first-time-ever" sigs which
/// dominate early-session output).
pub fn write_matches(market: &str, matches: &[AnalogMatch]) -> std::io::Result<usize> {
    let path = format!(".run/eden-wl-analog-{}.ndjson", market);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let mut written = 0;
    for m in matches {
        if m.historical_visits == 0 {
            continue;
        }
        let line = serde_json::to_string(m)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(file, "{}", line)?;
        written += 1;
    }
    Ok(written)
}

// ---------------- Tests ----------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_visit_zero_history() {
        let mut idx = SymbolWlAnalogIndex::new();
        let m = idx.record("us", "AAPL.US", "abc123", Utc::now());
        assert_eq!(m.historical_visits, 0);
        assert_eq!(m.distinct_symbol_count, 0);
        assert!(m.last_seen_ts.is_none());
    }

    #[test]
    fn second_visit_sees_first() {
        let mut idx = SymbolWlAnalogIndex::new();
        idx.record("us", "AAPL.US", "abc", Utc::now());
        let m = idx.record("us", "AAPL.US", "abc", Utc::now());
        assert_eq!(m.historical_visits, 1);
        assert_eq!(m.distinct_symbol_count, 1);
        assert_eq!(m.last_seen_symbol.as_deref(), Some("AAPL.US"));
    }

    #[test]
    fn cross_symbol_match_counted() {
        let mut idx = SymbolWlAnalogIndex::new();
        // AAPL produces signature "X" first.
        idx.record("us", "AAPL.US", "X", Utc::now());
        // NVDA later produces same signature → distinct_symbol_count = 2.
        let m = idx.record("us", "NVDA.US", "X", Utc::now());
        assert_eq!(m.historical_visits, 1);
        assert_eq!(m.distinct_symbol_count, 1);
        assert_eq!(m.last_seen_symbol.as_deref(), Some("AAPL.US"));
    }

    #[test]
    fn different_signatures_isolated() {
        let mut idx = SymbolWlAnalogIndex::new();
        idx.record("us", "AAPL.US", "X", Utc::now());
        let m = idx.record("us", "AAPL.US", "Y", Utc::now());
        assert_eq!(m.historical_visits, 0, "different sig is fresh");
        assert_eq!(idx.distinct_signatures(), 2);
    }

    #[test]
    fn history_bounded_by_max_per_sig() {
        let mut idx = SymbolWlAnalogIndex::new();
        for i in 0..(MAX_HISTORY_PER_SIG + 50) {
            idx.record("us", &format!("S{}", i), "X", Utc::now());
        }
        assert_eq!(idx.history.get("X").unwrap().len(), MAX_HISTORY_PER_SIG);
    }

    #[test]
    fn write_matches_skips_zero_history() {
        let mut idx = SymbolWlAnalogIndex::new();
        let m = idx.record("us", "AAPL.US", "X", Utc::now());
        // historical_visits == 0 → skipped on write.
        let path = format!(".run/eden-wl-analog-test.ndjson");
        let _ = std::fs::remove_file(&path);
        let written = write_matches("test", &[m]).unwrap();
        assert_eq!(written, 0);
        let _ = std::fs::remove_file(&path);
    }
}
