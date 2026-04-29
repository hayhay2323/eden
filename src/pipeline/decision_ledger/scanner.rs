//! File scanner for decision JSON files.
//!
//! Walks `decisions/YYYY/MM/DD/*.json` excluding `index.jsonl`,
//! `session-recap.json`, and the `schemas/` subtree. Parses each file,
//! filters by market, and inserts into the DecisionLedger.
//!
//! Error handling is log-and-skip: bad JSON / unknown action / schema
//! mismatch produces a warning and the file is marked skipped but does
//! not halt the scan.

use std::fs;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Datelike, NaiveDate, Utc};
use serde::Deserialize;

use crate::ontology::objects::{Market, Symbol};
use crate::pipeline::decision_ledger::{
    DecisionAction, DecisionLedger, DecisionRecord, OutcomeSummary, SymbolDecisionSummary,
    TradeDirection,
};

#[derive(Deserialize)]
struct DecisionJson {
    schema_version: u32,
    decision_id: String,
    timestamp: DateTime<Utc>,
    market: String,
    symbol: String,
    action: String,
    direction: Option<String>,
    claude: ClaudeJson,
    execution: Option<ExecutionJson>,
    outcome: Option<OutcomeJson>,
    retrospective: Option<RetrospectiveJson>,
    metadata: MetadataJson,
}

#[derive(Deserialize)]
struct ClaudeJson {
    confidence: f64,
}

#[derive(Deserialize)]
struct ExecutionJson {
    #[serde(default)]
    linked_entry_id: Option<String>,
}

#[derive(Deserialize)]
struct OutcomeJson {
    pnl_bps: f64,
    hold_duration_sec: u64,
    closing_reason: String,
}

#[derive(Deserialize)]
struct RetrospectiveJson {
    #[serde(default)]
    eden_gap: Option<String>,
}

#[derive(Deserialize)]
struct MetadataJson {
    backfilled: bool,
}

/// Full-tree scan of `decisions/` — used at startup.
pub fn scan_directory(root: &Path, ledger: &mut DecisionLedger) {
    if !root.is_dir() {
        eprintln!(
            "[decisions] no decisions directory at {}; starting empty",
            root.display()
        );
        return;
    }
    walk_decision_files(root, ledger);
    ledger.set_last_scan_ts(Utc::now());
    rebuild_all_summaries(ledger);
    eprintln!(
        "[decisions] ingested {} for market={:?} (skipped {})",
        ledger.ingested_count(),
        ledger.market(),
        ledger.skipped_count(),
    );
}

/// Incremental rescan — only today + yesterday subtrees; idempotent.
pub fn rescan_recent(root: &Path, ledger: &mut DecisionLedger, now: DateTime<Utc>) -> usize {
    if !root.is_dir() {
        return 0;
    }
    let before = ledger.ingested_count();
    let today = now.date_naive();
    let yesterday = today.pred_opt().unwrap_or(today);
    for date in &[yesterday, today] {
        let day_dir = daily_directory(root, *date);
        if day_dir.is_dir() {
            walk_day_directory(&day_dir, ledger);
        }
    }
    ledger.set_last_scan_ts(now);
    let new_records = ledger.ingested_count() - before;
    if new_records > 0 {
        rebuild_all_summaries(ledger);
        eprintln!(
            "[decisions] rescan: {} new ({} total, market={:?})",
            new_records,
            ledger.ingested_count(),
            ledger.market(),
        );
    }
    new_records
}

fn daily_directory(root: &Path, date: NaiveDate) -> PathBuf {
    root.join(format!("{:04}", date.year()))
        .join(format!("{:02}", date.month()))
        .join(format!("{:02}", date.day()))
}

fn walk_decision_files(root: &Path, ledger: &mut DecisionLedger) {
    let Ok(year_entries) = fs::read_dir(root) else {
        return;
    };
    for year_entry in year_entries.flatten() {
        let year_path = year_entry.path();
        if !year_path.is_dir() {
            continue;
        }
        if year_path.file_name().and_then(|n| n.to_str()) == Some("schemas") {
            continue;
        }
        walk_nested_months(&year_path, ledger);
    }
}

fn walk_nested_months(year_path: &Path, ledger: &mut DecisionLedger) {
    let Ok(month_entries) = fs::read_dir(year_path) else {
        return;
    };
    for month_entry in month_entries.flatten() {
        let month_path = month_entry.path();
        if !month_path.is_dir() {
            continue;
        }
        let Ok(day_entries) = fs::read_dir(&month_path) else {
            continue;
        };
        for day_entry in day_entries.flatten() {
            let day_path = day_entry.path();
            if !day_path.is_dir() {
                continue;
            }
            walk_day_directory(&day_path, ledger);
        }
    }
}

fn walk_day_directory(day_dir: &Path, ledger: &mut DecisionLedger) {
    let Ok(entries) = fs::read_dir(day_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string();
        if file_name == "session-recap.json" {
            continue;
        }
        if ledger.already_ingested(&path) {
            continue;
        }
        ingest_file(&path, ledger);
    }
}

fn ingest_file(path: &Path, ledger: &mut DecisionLedger) {
    let raw = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("[decisions] read failed {}: {}", path.display(), e);
            ledger.mark_skipped(path.to_path_buf());
            return;
        }
    };
    let parsed: DecisionJson = match serde_json::from_str(&raw) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("[decisions] json parse failed {}: {}", path.display(), e);
            ledger.mark_skipped(path.to_path_buf());
            return;
        }
    };
    if parsed.schema_version != 1 {
        eprintln!(
            "[decisions] unsupported schema_version {} at {}; skipping",
            parsed.schema_version,
            path.display()
        );
        ledger.mark_skipped(path.to_path_buf());
        return;
    }

    let decision_market = match parsed.market.as_str() {
        "HK" => Market::Hk,
        "US" => Market::Us,
        other => {
            eprintln!(
                "[decisions] unknown market {} at {}; skipping",
                other,
                path.display()
            );
            ledger.mark_skipped(path.to_path_buf());
            return;
        }
    };
    if decision_market != ledger.market() {
        // Silently skip — not this market's record.
        ledger.mark_skipped(path.to_path_buf());
        return;
    }

    let action = match DecisionAction::from_str(&parsed.action) {
        Some(a) => a,
        None => {
            eprintln!(
                "[decisions] unknown action {} at {}; skipping",
                parsed.action,
                path.display()
            );
            ledger.mark_skipped(path.to_path_buf());
            return;
        }
    };
    let direction = parsed
        .direction
        .as_deref()
        .and_then(TradeDirection::from_str);
    let outcome = parsed.outcome.map(|o| OutcomeSummary {
        pnl_bps: o.pnl_bps,
        hold_duration_sec: o.hold_duration_sec,
        closing_reason: o.closing_reason,
    });
    let linked_entry_id = parsed.execution.and_then(|e| e.linked_entry_id);
    let eden_gap = parsed
        .retrospective
        .and_then(|r| r.eden_gap)
        .filter(|s| !s.is_empty());

    let record = DecisionRecord {
        decision_id: parsed.decision_id,
        timestamp: parsed.timestamp,
        symbol: Symbol(parsed.symbol),
        action,
        direction,
        confidence: parsed.claude.confidence,
        linked_entry_id,
        outcome,
        eden_gap,
        backfilled: parsed.metadata.backfilled,
    };

    ledger.insert_record_raw(path.to_path_buf(), record);
}

fn rebuild_all_summaries(ledger: &mut DecisionLedger) {
    let symbols: Vec<Symbol> = ledger.symbols_with_decisions().cloned().collect();
    for symbol in &symbols {
        let records = ledger.decisions_for(symbol).to_vec();
        let summary = summarize(&records);
        ledger.set_summary_raw(symbol.clone(), summary);
    }
}

/// Build a SymbolDecisionSummary from a symbol's records (chronological
/// within the summary build, not requiring input to be sorted). eden_gap
/// values are de-duped and kept most-recent-first, capped at 3.
pub(super) fn summarize(records: &[DecisionRecord]) -> SymbolDecisionSummary {
    let mut s = SymbolDecisionSummary::default();
    let mut sorted = records.to_vec();
    sorted.sort_by_key(|r| r.timestamp);
    // Walk in chronological order to produce net_pnl + last_*.
    for r in &sorted {
        s.total_decisions += 1;
        match r.action {
            DecisionAction::Entry => s.entries += 1,
            DecisionAction::Exit => s.exits += 1,
            DecisionAction::Skip => s.skips += 1,
            DecisionAction::SizeChange => s.size_changes += 1,
        }
        if let Some(o) = &r.outcome {
            s.net_pnl_bps += o.pnl_bps;
            s.last_pnl_bps = Some(o.pnl_bps);
        }
        s.last_action = Some(r.action);
        s.last_timestamp = Some(r.timestamp);
    }
    // eden_gap dedup: iterate newest→oldest so the first sighting of each
    // unique gap is the most recent one. Cap total at 3.
    for r in sorted.iter().rev() {
        if let Some(gap) = &r.eden_gap {
            if s.unique_eden_gaps.iter().any(|g| g == gap) {
                continue;
            }
            s.unique_eden_gaps.push(gap.clone());
            if s.unique_eden_gaps.len() >= 3 {
                break;
            }
        }
    }
    s
}
