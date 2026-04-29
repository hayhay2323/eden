//! Persistent decision ledger — Eden reads Claude Code's own decisions
//! from `decisions/YYYY/MM/DD/*.json` into a per-symbol indexed struct.
//!
//! This spec is observation-only: decisions flow IN to Eden for wake
//! emission and query, but do NOT update the belief field (that's A2.5).
//!
//! See docs/superpowers/specs/2026-04-19-decisions-ingestor-design.md.

use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

use chrono::{DateTime, Utc};

use crate::ontology::objects::{Market, Symbol};

pub mod scanner;
pub mod wake_format;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecisionAction {
    Entry,
    Exit,
    Skip,
    SizeChange,
}

impl DecisionAction {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Entry => "entry",
            Self::Exit => "exit",
            Self::Skip => "skip",
            Self::SizeChange => "size_change",
        }
    }

    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "entry" => Some(Self::Entry),
            "exit" => Some(Self::Exit),
            "skip" => Some(Self::Skip),
            "size_change" => Some(Self::SizeChange),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeDirection {
    Long,
    Short,
}

impl TradeDirection {
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "long" => Some(Self::Long),
            "short" => Some(Self::Short),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct OutcomeSummary {
    pub pnl_bps: f64,
    pub hold_duration_sec: u64,
    pub closing_reason: String,
}

#[derive(Debug, Clone)]
pub struct DecisionRecord {
    pub decision_id: String,
    pub timestamp: DateTime<Utc>,
    pub symbol: Symbol,
    pub action: DecisionAction,
    pub direction: Option<TradeDirection>,
    pub confidence: f64,
    pub linked_entry_id: Option<String>,
    pub outcome: Option<OutcomeSummary>,
    pub eden_gap: Option<String>,
    pub backfilled: bool,
}

#[derive(Debug, Clone, Default)]
pub struct SymbolDecisionSummary {
    pub total_decisions: usize,
    pub entries: usize,
    pub exits: usize,
    pub skips: usize,
    pub size_changes: usize,
    pub net_pnl_bps: f64,
    pub last_action: Option<DecisionAction>,
    pub last_timestamp: Option<DateTime<Utc>>,
    pub last_pnl_bps: Option<f64>,
    /// Unique, de-duped eden_gap values, most-recent first. Cap 3.
    pub unique_eden_gaps: Vec<String>,
}

/// Cross-tick persistent ledger of Claude Code's decisions for a single
/// market. Populated from `decisions/YYYY/MM/DD/*.json` via scanner.
pub struct DecisionLedger {
    per_symbol: HashMap<Symbol, Vec<DecisionRecord>>,
    summaries: HashMap<Symbol, SymbolDecisionSummary>,
    market: Market,
    last_scan_ts: Option<DateTime<Utc>>,
    ingested_paths: HashSet<PathBuf>,
    ingested_count: usize,
    skipped_count: usize,
}

impl DecisionLedger {
    pub fn new(market: Market) -> Self {
        Self {
            per_symbol: HashMap::new(),
            summaries: HashMap::new(),
            market,
            last_scan_ts: None,
            ingested_paths: HashSet::new(),
            ingested_count: 0,
            skipped_count: 0,
        }
    }

    pub fn market(&self) -> Market {
        self.market
    }

    pub fn ingested_count(&self) -> usize {
        self.ingested_count
    }

    pub fn skipped_count(&self) -> usize {
        self.skipped_count
    }

    pub fn last_scan_ts(&self) -> Option<DateTime<Utc>> {
        self.last_scan_ts
    }

    pub fn set_last_scan_ts(&mut self, ts: DateTime<Utc>) {
        self.last_scan_ts = Some(ts);
    }

    pub fn decisions_for(&self, symbol: &Symbol) -> &[DecisionRecord] {
        self.per_symbol
            .get(symbol)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    pub fn summary_for(&self, symbol: &Symbol) -> Option<&SymbolDecisionSummary> {
        self.summaries.get(symbol)
    }

    pub fn symbols_with_decisions(&self) -> impl Iterator<Item = &Symbol> {
        self.per_symbol.keys()
    }

    pub fn total_symbols(&self) -> usize {
        self.per_symbol.len()
    }

    /// Raw insert — used by scanner + integration tests. Caller must have
    /// checked `already_ingested` first in production code; integration
    /// tests can pass any unique path. This bypasses summary rebuild;
    /// integration tests should call `set_summary_raw` if they need the
    /// summary populated, or go through the scanner's public APIs.
    pub fn insert_record_raw(&mut self, path: PathBuf, record: DecisionRecord) {
        let symbol = record.symbol.clone();
        self.per_symbol
            .entry(symbol.clone())
            .or_default()
            .push(record);
        self.ingested_paths.insert(path);
        self.ingested_count += 1;
        // Summary rebuilt by scanner after batch via rebuild_all_summaries.
    }

    pub(super) fn mark_skipped(&mut self, path: PathBuf) {
        self.ingested_paths.insert(path);
        self.skipped_count += 1;
    }

    pub(super) fn already_ingested(&self, path: &PathBuf) -> bool {
        self.ingested_paths.contains(path)
    }

    pub(super) fn set_summary_raw(&mut self, symbol: Symbol, summary: SymbolDecisionSummary) {
        self.summaries.insert(symbol, summary);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_ledger_is_empty() {
        let ledger = DecisionLedger::new(Market::Hk);
        assert_eq!(ledger.total_symbols(), 0);
        assert_eq!(ledger.ingested_count(), 0);
        assert_eq!(ledger.skipped_count(), 0);
        assert_eq!(ledger.market(), Market::Hk);
        assert!(ledger.last_scan_ts().is_none());
    }

    #[test]
    fn decision_action_roundtrips() {
        for a in [
            DecisionAction::Entry,
            DecisionAction::Exit,
            DecisionAction::Skip,
            DecisionAction::SizeChange,
        ] {
            let s = a.as_str();
            assert_eq!(DecisionAction::from_str(s), Some(a));
        }
        assert_eq!(DecisionAction::from_str("not_a_real_action"), None);
    }

    #[test]
    fn scan_real_2026_04_15_us_session_ingests_three_decisions() {
        use std::path::Path;
        let mut ledger = DecisionLedger::new(Market::Us);
        scanner::scan_directory(Path::new("decisions"), &mut ledger);
        assert_eq!(
            ledger.ingested_count(),
            3,
            "expected 3 US decisions in 2026-04-15"
        );

        let kc = Symbol("KC.US".to_string());
        let hubs = Symbol("HUBS.US".to_string());
        assert_eq!(ledger.decisions_for(&kc).len(), 2);
        assert_eq!(ledger.decisions_for(&hubs).len(), 1);
    }

    #[test]
    fn scan_filters_by_market_hk_ledger_gets_zero_from_us_backfill() {
        use std::path::Path;
        let mut ledger = DecisionLedger::new(Market::Hk);
        scanner::scan_directory(Path::new("decisions"), &mut ledger);
        assert_eq!(ledger.ingested_count(), 0);
    }

    #[test]
    fn summary_from_kc_entry_exit_has_correct_pnl() {
        use std::path::Path;
        let mut ledger = DecisionLedger::new(Market::Us);
        scanner::scan_directory(Path::new("decisions"), &mut ledger);

        let kc = Symbol("KC.US".to_string());
        let summary = ledger.summary_for(&kc).expect("KC summary exists");
        assert_eq!(summary.total_decisions, 2);
        assert_eq!(summary.entries, 1);
        assert_eq!(summary.exits, 1);
        assert_eq!(summary.skips, 0);
        assert!(
            (summary.net_pnl_bps - (-18.0)).abs() < 1e-6,
            "expected net_pnl -18 bps, got {}",
            summary.net_pnl_bps
        );
        assert_eq!(summary.last_action, Some(DecisionAction::Exit));
        assert_eq!(summary.last_pnl_bps, Some(-18.0));
        assert_eq!(summary.unique_eden_gaps.len(), 1);
    }

    #[test]
    fn rescan_on_same_tree_is_idempotent() {
        use chrono::TimeZone;
        use std::path::Path;
        let mut ledger = DecisionLedger::new(Market::Us);
        scanner::scan_directory(Path::new("decisions"), &mut ledger);
        let first_count = ledger.ingested_count();
        assert_eq!(first_count, 3);

        // Rescan with "now" = 2026-04-16 so today/yesterday cover the data.
        let now = Utc.with_ymd_and_hms(2026, 4, 16, 0, 0, 0).unwrap();
        let added = scanner::rescan_recent(Path::new("decisions"), &mut ledger, now);
        assert_eq!(added, 0, "rescan should not re-add ingested paths");
        assert_eq!(ledger.ingested_count(), first_count);
    }

    #[test]
    fn summary_eden_gap_dedup_respects_cap() {
        use scanner::summarize;
        let s = Symbol("T.T".to_string());
        let records: Vec<DecisionRecord> = (0..5)
            .map(|i| DecisionRecord {
                decision_id: format!("id{}", i),
                timestamp: Utc::now() + chrono::Duration::seconds(i),
                symbol: s.clone(),
                action: DecisionAction::Entry,
                direction: None,
                confidence: 0.5,
                linked_entry_id: None,
                outcome: None,
                eden_gap: Some(format!("gap{}", i)),
                backfilled: false,
            })
            .collect();
        let summary = summarize(&records);
        assert_eq!(summary.unique_eden_gaps.len(), 3, "capped at 3");
        // Most recent first (gap4, gap3, gap2).
        assert_eq!(summary.unique_eden_gaps[0], "gap4");
        assert_eq!(summary.unique_eden_gaps[1], "gap3");
        assert_eq!(summary.unique_eden_gaps[2], "gap2");
    }

    #[test]
    fn summary_eden_gap_dedup_collapses_same_gap() {
        use scanner::summarize;
        let s = Symbol("T.T".to_string());
        let records: Vec<DecisionRecord> = (0..5)
            .map(|i| DecisionRecord {
                decision_id: format!("id{}", i),
                timestamp: Utc::now() + chrono::Duration::seconds(i),
                symbol: s.clone(),
                action: DecisionAction::Entry,
                direction: None,
                confidence: 0.5,
                linked_entry_id: None,
                outcome: None,
                eden_gap: Some("same_gap".to_string()),
                backfilled: false,
            })
            .collect();
        let summary = summarize(&records);
        assert_eq!(summary.unique_eden_gaps.len(), 1);
        assert_eq!(summary.unique_eden_gaps[0], "same_gap");
    }
}
