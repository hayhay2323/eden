# Decisions Ingestor Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rust-side DecisionLedger that reads `decisions/YYYY/MM/DD/*.json` into a per-symbol indexed structure, rescanned every 60s, and emits `prior decisions:` wake lines for symbols already marked notable by the belief field.

**Architecture:** New `pipeline::decision_ledger` module owning the in-memory ledger plus submodules for scanner + wake formatter. HK and US runtimes each own an instance (symmetric with belief_field, independent). No belief_field modification — pure observation, interpretation deferred.

**Tech Stack:** Rust + serde (JSON parse) + chrono (timestamps) + existing Eden ontology types (Symbol, Market). No new deps.

**Spec:** `docs/superpowers/specs/2026-04-19-decisions-ingestor-design.md`

---

## File Structure

**New files:**
- `src/pipeline/decision_ledger.rs` (~300 LOC) — types + struct + query API + tests
- `src/pipeline/decision_ledger/scanner.rs` (~150 LOC) — file glob + JSON parse
- `src/pipeline/decision_ledger/wake_format.rs` (~80 LOC) — format helper
- `tests/decision_ledger_integration.rs` (~100 LOC) — end-to-end

**Modified files:**
- `src/pipeline/mod.rs` — `pub mod decision_ledger;`
- `src/hk/runtime.rs` — init ledger at startup + per-tick wake emit + 60s rescan
- `src/us/runtime.rs` — symmetric

**Existing types referenced** (verified):
- `Symbol(String)` — `src/ontology/objects.rs:16`
- `Market::{Hk, Us}` — `src/ontology/objects.rs:20`
- Belief wake integration point: the `top_notable_beliefs(5)` loop already present in both HK and US runtimes (added by belief persistence A1)

---

## Branch

Commits go on current branch (`codex/polymarket-convergence`). Additive + contained to listed files.

**Verification commands:**
```bash
export CARGO_TARGET_DIR=/tmp/eden-target
cargo check --lib -q
cargo check --lib --features persistence -q
cargo check --lib --no-default-features -q
cargo test --lib -q decision_ledger
cargo test --test decision_ledger_integration -q
```

---

## Task 1: Core types + DecisionLedger skeleton

**Files:**
- Create: `src/pipeline/decision_ledger.rs`
- Modify: `src/pipeline/mod.rs`

- [ ] **Step 1: Create module skeleton with types**

Create `src/pipeline/decision_ledger.rs`:

```rust
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

    /// Raw insert — used by scanner.rs. Bypasses dedup check; callers
    /// must have already consulted `ingested_paths`.
    pub(super) fn insert_record_raw(&mut self, path: PathBuf, record: DecisionRecord) {
        let symbol = record.symbol.clone();
        self.per_symbol
            .entry(symbol.clone())
            .or_default()
            .push(record);
        self.ingested_paths.insert(path);
        self.ingested_count += 1;
        // Summary will be recomputed by rebuild_summary_for(&symbol).
    }

    pub(super) fn mark_skipped(&mut self, path: PathBuf) {
        self.ingested_paths.insert(path);
        self.skipped_count += 1;
    }

    pub(super) fn already_ingested(&self, path: &PathBuf) -> bool {
        self.ingested_paths.contains(path)
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
}
```

- [ ] **Step 2: Wire into pipeline mod.rs**

Read `src/pipeline/mod.rs`, add alphabetically after `belief_field`:

```rust
pub mod decision_ledger;
```

- [ ] **Step 3: Create empty submodule files**

Create placeholder files so the `pub mod scanner` and `pub mod wake_format` in decision_ledger.rs compile. These get filled by Task 2 and Task 4.

`src/pipeline/decision_ledger/scanner.rs`:
```rust
//! File scanner for decision JSON files. Populated in Task 2.
```

`src/pipeline/decision_ledger/wake_format.rs`:
```rust
//! Wake formatter for prior decisions. Populated in Task 4.
```

- [ ] **Step 4: Compile + run tests**

Run: `cargo check --lib -q && cargo test --lib -q decision_ledger`
Expected: compiles clean, 2 tests pass.

- [ ] **Step 5: Commit**

```bash
git add src/pipeline/decision_ledger.rs src/pipeline/decision_ledger/ src/pipeline/mod.rs
git commit -m "$(cat <<'EOF'
feat(decision_ledger): scaffold core types + DecisionLedger struct

DecisionAction / TradeDirection / OutcomeSummary / DecisionRecord /
SymbolDecisionSummary value types. DecisionLedger with per_symbol +
summaries HashMaps, ingested_paths dedup, counters. No scanner/wake
yet — those are Task 2/4.

Spec: docs/superpowers/specs/2026-04-19-decisions-ingestor-design.md

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 2: JSON scanner — parse + ingest

**Files:**
- Modify: `src/pipeline/decision_ledger/scanner.rs`

- [ ] **Step 1: Write failing test in decision_ledger.rs tests mod**

Append to `src/pipeline/decision_ledger.rs` tests mod:

```rust
    #[test]
    fn scan_real_2026_04_15_us_session_ingests_three_decisions() {
        use std::path::Path;
        let mut ledger = DecisionLedger::new(Market::Us);
        scanner::scan_directory(
            Path::new("decisions"),
            &mut ledger,
        );
        assert_eq!(ledger.ingested_count(), 3, "expected 3 US decisions in 2026-04-15");
        assert_eq!(ledger.skipped_count(), 0);

        let kc = Symbol("KC.US".to_string());
        let hubs = Symbol("HUBS.US".to_string());
        assert_eq!(ledger.decisions_for(&kc).len(), 2);
        assert_eq!(ledger.decisions_for(&hubs).len(), 1);
    }

    #[test]
    fn scan_filters_by_market_hk_ledger_gets_zero_from_us_backfill() {
        use std::path::Path;
        let mut ledger = DecisionLedger::new(Market::Hk);
        scanner::scan_directory(
            Path::new("decisions"),
            &mut ledger,
        );
        assert_eq!(ledger.ingested_count(), 0);
    }
```

- [ ] **Step 2: Run — expect FAIL**

Run: `cargo test --lib -q decision_ledger`
Expected: compile errors on `scanner::scan_directory`.

- [ ] **Step 3: Implement scanner.rs**

Replace `src/pipeline/decision_ledger/scanner.rs` with:

```rust
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

use chrono::{DateTime, NaiveDate, Utc};
use serde::Deserialize;

use crate::ontology::objects::{Market, Symbol};
use crate::pipeline::decision_ledger::{
    DecisionAction, DecisionLedger, DecisionRecord, OutcomeSummary, TradeDirection,
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
    eden_gap: Option<String>,
}

#[derive(Deserialize)]
struct MetadataJson {
    backfilled: bool,
}

/// Full-tree scan of `decisions/` — used at startup.
pub fn scan_directory(root: &Path, ledger: &mut DecisionLedger) {
    if !root.is_dir() {
        tracing::info!(
            target: "decisions",
            path = %root.display(),
            "no decisions directory yet; starting empty"
        );
        return;
    }
    walk_decision_files(root, ledger);
    ledger.set_last_scan_ts(Utc::now());
    rebuild_all_summaries(ledger);
    tracing::info!(
        target: "decisions",
        ingested = ledger.ingested_count(),
        skipped = ledger.skipped_count(),
        market = ?ledger.market(),
        "ingested decisions"
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
        tracing::info!(
            target: "decisions",
            new = new_records,
            total = ledger.ingested_count(),
            market = ?ledger.market(),
            "rescan added new decisions"
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
    let Ok(year_entries) = fs::read_dir(root) else { return };
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
    let Ok(month_entries) = fs::read_dir(year_path) else { return };
    for month_entry in month_entries.flatten() {
        let month_path = month_entry.path();
        if !month_path.is_dir() {
            continue;
        }
        let Ok(day_entries) = fs::read_dir(&month_path) else { continue };
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
    let Ok(entries) = fs::read_dir(day_dir) else { return };
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
            tracing::warn!(target: "decisions", path = %path.display(), err = %e, "read failed");
            ledger.mark_skipped(path.to_path_buf());
            return;
        }
    };
    let parsed: DecisionJson = match serde_json::from_str(&raw) {
        Ok(p) => p,
        Err(e) => {
            tracing::warn!(target: "decisions", path = %path.display(), err = %e, "json parse failed");
            ledger.mark_skipped(path.to_path_buf());
            return;
        }
    };
    if parsed.schema_version != 1 {
        tracing::warn!(
            target: "decisions",
            path = %path.display(),
            version = parsed.schema_version,
            "unsupported schema_version; skipping"
        );
        ledger.mark_skipped(path.to_path_buf());
        return;
    }

    let decision_market = match parsed.market.as_str() {
        "HK" => Market::Hk,
        "US" => Market::Us,
        other => {
            tracing::warn!(
                target: "decisions",
                path = %path.display(),
                market = %other,
                "unknown market; skipping"
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
            tracing::warn!(
                target: "decisions",
                path = %path.display(),
                action = %parsed.action,
                "unknown action; skipping"
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

pub(super) fn summarize(records: &[DecisionRecord]) -> crate::pipeline::decision_ledger::SymbolDecisionSummary {
    use crate::pipeline::decision_ledger::SymbolDecisionSummary;
    let mut s = SymbolDecisionSummary::default();
    let mut sorted = records.to_vec();
    sorted.sort_by_key(|r| r.timestamp);
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
        if let Some(gap) = &r.eden_gap {
            if !s.unique_eden_gaps.iter().any(|g| g == gap) {
                s.unique_eden_gaps.push(gap.clone());
            }
        }
    }
    // Cap eden_gaps to most-recent 3 (sorted already chronological, take last 3 in reverse).
    if s.unique_eden_gaps.len() > 3 {
        let start = s.unique_eden_gaps.len() - 3;
        s.unique_eden_gaps = s.unique_eden_gaps.split_off(start);
    }
    s.unique_eden_gaps.reverse();
    s
}

// chrono::Datelike brings year/month/day into scope.
use chrono::Datelike;
```

- [ ] **Step 4: Add `set_summary_raw` helper on DecisionLedger**

In `src/pipeline/decision_ledger.rs`, append to `impl DecisionLedger`:

```rust
    pub(super) fn set_summary_raw(
        &mut self,
        symbol: Symbol,
        summary: SymbolDecisionSummary,
    ) {
        self.summaries.insert(symbol, summary);
    }
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib -q decision_ledger`
Expected: 4 tests pass.

If the real-file test fails with "expected 3 US decisions" check count: 0, the test CWD isn't repo root. Leave the test using `Path::new("decisions")` — cargo test runs with repo root as CWD by default.

- [ ] **Step 6: Commit**

```bash
git add src/pipeline/decision_ledger.rs src/pipeline/decision_ledger/scanner.rs
git commit -m "$(cat <<'EOF'
feat(decision_ledger): scanner + full-tree scan + rescan_recent

scan_directory walks decisions/YYYY/MM/DD/*.json excluding schemas/,
index.jsonl, session-recap.json. rescan_recent only touches today +
yesterday via chrono::Datelike. JSON parse errors / unknown
action / schema mismatch / wrong-market records all skip with
warn-log, never halting the scan.

Summary rebuild after each ingest batch. Two unit tests verify real
2026-04-15 ingest (3 US decisions, 0 for HK ledger).

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 3: Summary correctness + eden_gap dedup + idempotent rescan tests

**Files:**
- Modify: `src/pipeline/decision_ledger.rs`

- [ ] **Step 1: Write failing tests**

Append to tests mod:

```rust
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
        // KC exit has eden_gap filled; entry doesn't.
        assert_eq!(summary.unique_eden_gaps.len(), 1);
    }

    #[test]
    fn rescan_on_same_tree_is_idempotent() {
        use std::path::Path;
        use chrono::TimeZone;
        let mut ledger = DecisionLedger::new(Market::Us);
        scanner::scan_directory(Path::new("decisions"), &mut ledger);
        let first_count = ledger.ingested_count();
        assert_eq!(first_count, 3);

        // Rescan with "now" = 2026-04-15 so today/yesterday cover the data.
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
                // Same gap text for all → dedupes to one entry.
                eden_gap: Some("same_gap".to_string()),
                backfilled: false,
            })
            .collect();
        let summary = summarize(&records);
        assert_eq!(summary.unique_eden_gaps.len(), 1);
        assert_eq!(summary.unique_eden_gaps[0], "same_gap");
    }
```

- [ ] **Step 2: Run tests — expect PASS**

All tests depend only on Task 1/2 code. No new implementation needed — Task 2's `summarize` already handles dedup + cap.

Run: `cargo test --lib -q decision_ledger`
Expected: 8 tests pass (2 from Task 1 + 2 from Task 2 + 4 new).

If `summary_eden_gap_dedup_respects_cap` fails because the cap-3 logic in `summarize` retains oldest-first instead of newest-first, fix the summarize function accordingly — the test encodes the correct behavior (most-recent 3, newest first).

- [ ] **Step 3: Commit**

```bash
git add src/pipeline/decision_ledger.rs
git commit -m "$(cat <<'EOF'
test(decision_ledger): summary correctness + rescan idempotency + gap dedup

Four new unit tests:
- summary_from_kc_entry_exit_has_correct_pnl: real backfill data
- rescan_on_same_tree_is_idempotent
- summary_eden_gap_dedup_respects_cap (cap 3, newest first)
- summary_eden_gap_dedup_collapses_same_gap

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 4: Wake formatter

**Files:**
- Modify: `src/pipeline/decision_ledger/wake_format.rs`

- [ ] **Step 1: Write failing tests inline**

Replace `src/pipeline/decision_ledger/wake_format.rs` with:

```rust
//! Format a SymbolDecisionSummary as a single wake line.

use chrono::{DateTime, Utc};

use crate::ontology::objects::Symbol;
use crate::pipeline::decision_ledger::{DecisionAction, SymbolDecisionSummary};

/// Format a `prior decisions:` wake line for a notable symbol.
///
/// Shape examples:
///   prior decisions: KC.US 2 (exit @2026-04-15 -18bps)
///   prior decisions: KC.US 2 (exit @2026-04-15 -18bps); eden_gap: roster churn ≠ signal fade
///   prior decisions: 3690.HK 1 (skip @2026-04-18)
pub fn format_prior_decisions_line(
    symbol: &Symbol,
    summary: &SymbolDecisionSummary,
) -> String {
    let mut line = format!(
        "prior decisions: {} {}",
        symbol.0, summary.total_decisions
    );

    if let (Some(action), Some(ts)) = (summary.last_action, summary.last_timestamp) {
        let date = format_date(ts);
        line.push_str(" (");
        line.push_str(action_verb(action));
        line.push_str(" @");
        line.push_str(&date);
        if let Some(pnl) = summary.last_pnl_bps {
            line.push_str(&format!(" {:+}bps", pnl as i64));
        }
        line.push(')');
    }

    if let Some(gap) = summary.unique_eden_gaps.first() {
        line.push_str("; eden_gap: ");
        line.push_str(gap);
    }

    line
}

fn action_verb(action: DecisionAction) -> &'static str {
    match action {
        DecisionAction::Entry => "entry",
        DecisionAction::Exit => "exit",
        DecisionAction::Skip => "skip",
        DecisionAction::SizeChange => "size_change",
    }
}

fn format_date(ts: DateTime<Utc>) -> String {
    ts.format("%Y-%m-%d").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;

    fn summary_entry_no_outcome() -> SymbolDecisionSummary {
        let mut s = SymbolDecisionSummary::default();
        s.total_decisions = 1;
        s.entries = 1;
        s.last_action = Some(DecisionAction::Entry);
        s.last_timestamp = Some(Utc.with_ymd_and_hms(2026, 4, 15, 16, 46, 0).unwrap());
        s
    }

    fn summary_exit_with_pnl() -> SymbolDecisionSummary {
        let mut s = SymbolDecisionSummary::default();
        s.total_decisions = 2;
        s.entries = 1;
        s.exits = 1;
        s.net_pnl_bps = -18.0;
        s.last_action = Some(DecisionAction::Exit);
        s.last_timestamp = Some(Utc.with_ymd_and_hms(2026, 4, 15, 16, 49, 0).unwrap());
        s.last_pnl_bps = Some(-18.0);
        s
    }

    fn summary_skip() -> SymbolDecisionSummary {
        let mut s = SymbolDecisionSummary::default();
        s.total_decisions = 1;
        s.skips = 1;
        s.last_action = Some(DecisionAction::Skip);
        s.last_timestamp = Some(Utc.with_ymd_and_hms(2026, 4, 18, 11, 55, 0).unwrap());
        s
    }

    #[test]
    fn format_shows_entry_no_pnl() {
        let line = format_prior_decisions_line(
            &Symbol("0700.HK".to_string()),
            &summary_entry_no_outcome(),
        );
        assert_eq!(line, "prior decisions: 0700.HK 1 (entry @2026-04-15)");
    }

    #[test]
    fn format_shows_exit_with_pnl() {
        let line = format_prior_decisions_line(
            &Symbol("KC.US".to_string()),
            &summary_exit_with_pnl(),
        );
        assert_eq!(line, "prior decisions: KC.US 2 (exit @2026-04-15 -18bps)");
    }

    #[test]
    fn format_appends_top_eden_gap() {
        let mut s = summary_exit_with_pnl();
        s.unique_eden_gaps = vec!["roster churn != signal fade".to_string()];
        let line = format_prior_decisions_line(&Symbol("KC.US".to_string()), &s);
        assert_eq!(
            line,
            "prior decisions: KC.US 2 (exit @2026-04-15 -18bps); eden_gap: roster churn != signal fade"
        );
    }

    #[test]
    fn format_shows_skip_without_pnl() {
        let line = format_prior_decisions_line(
            &Symbol("3690.HK".to_string()),
            &summary_skip(),
        );
        assert_eq!(line, "prior decisions: 3690.HK 1 (skip @2026-04-18)");
    }

    #[test]
    fn format_handles_empty_summary_gracefully() {
        let line = format_prior_decisions_line(
            &Symbol("X.X".to_string()),
            &SymbolDecisionSummary::default(),
        );
        assert_eq!(line, "prior decisions: X.X 0");
    }
}
```

- [ ] **Step 2: Run tests**

Run: `cargo test --lib -q decision_ledger::wake_format`
Expected: 5 tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/pipeline/decision_ledger/wake_format.rs
git commit -m "$(cat <<'EOF'
feat(decision_ledger): wake_format::format_prior_decisions_line

format_prior_decisions_line(symbol, summary) produces a single-line
wake entry. Optional pnl in bps, optional eden_gap suffix (top 1 from
unique list). 5 unit tests cover entry-no-pnl, exit-with-pnl, gap
suffix, skip, empty.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 5: HK runtime integration

**Files:**
- Modify: `src/hk/runtime.rs`

- [ ] **Step 1: Add ledger construction near belief_field init**

Find the block added by A1 that constructs `belief_field` (after `pressure_field` creation, around line ~248 after A1 commits). Append a similar block for the ledger:

```rust
    // DecisionLedger: Eden reads Claude Code's own decision history from
    // the decisions/ tree. Startup scan builds full index; per-tick rescan
    // (piggybacking belief snapshot cadence) picks up new decisions.
    let mut decision_ledger =
        eden::pipeline::decision_ledger::DecisionLedger::new(
            eden::ontology::objects::Market::Hk,
        );
    {
        use std::path::Path;
        eden::pipeline::decision_ledger::scanner::scan_directory(
            Path::new("decisions"),
            &mut decision_ledger,
        );
    }
```

Insert this **after** the belief_field construction block (which ends with `let mut lifecycle_tracker = ...`). To find the exact spot, grep:

```bash
grep -n "lifecycle_tracker = eden::pipeline::pressure::reasoning::LifecycleTracker::default()" src/hk/runtime.rs | head -1
```

The ledger construction must be immediately after that line.

- [ ] **Step 2: Emit wake lines in the belief notable loop + rescan**

Find the block that emits belief wake lines:

```bash
grep -n 'format_wake_line(&notable)' src/hk/runtime.rs | head -1
```

This lands on the belief-layer wake emission. Modify to also emit prior-decisions line per notable symbol:

Before:
```rust
                for notable in belief_field.top_notable_beliefs(5) {
                    artifact_projection
                        .agent_snapshot
                        .wake
                        .reasons
                        .push(eden::pipeline::belief_field::format_wake_line(&notable));
                }
```

After:
```rust
                for notable in belief_field.top_notable_beliefs(5) {
                    let symbol_for_decisions = match &notable {
                        eden::pipeline::belief_field::NotableBelief::Gaussian { symbol, .. }
                        | eden::pipeline::belief_field::NotableBelief::Categorical { symbol, .. } => symbol.clone(),
                    };
                    artifact_projection
                        .agent_snapshot
                        .wake
                        .reasons
                        .push(eden::pipeline::belief_field::format_wake_line(&notable));
                    if let Some(summary) = decision_ledger.summary_for(&symbol_for_decisions) {
                        if summary.total_decisions >= 1 {
                            artifact_projection
                                .agent_snapshot
                                .wake
                                .reasons
                                .push(
                                    eden::pipeline::decision_ledger::wake_format::format_prior_decisions_line(
                                        &symbol_for_decisions,
                                        summary,
                                    ),
                                );
                        }
                    }
                }
```

Also, after the existing belief snapshot cadence block (still inside the `{ ... }` that holds the belief block), add a rescan trigger piggybacking on the same 60s cadence:

```rust
                // Piggyback: on every 60s belief snapshot moment, also
                // rescan decisions/ for new files written since startup.
                #[cfg(feature = "persistence")]
                {
                    // belief_field.last_snapshot_ts was just set above when
                    // a snapshot was written; re-use the "just-wrote" flag.
                    let should_rescan = match decision_ledger.last_scan_ts() {
                        None => true,
                        Some(prev) => (chrono::Utc::now() - prev).num_seconds() >= 60,
                    };
                    if should_rescan {
                        use std::path::Path;
                        eden::pipeline::decision_ledger::scanner::rescan_recent(
                            Path::new("decisions"),
                            &mut decision_ledger,
                            chrono::Utc::now(),
                        );
                    }
                }
```

The rescan block must be **inside** the same outer brace that holds the belief block, but **outside** the inner `#[cfg(feature = "persistence")]` that controls the snapshot write. See HK belief integration as a guide — the rescan can live in a separate `#[cfg(feature = "persistence")]` block directly below the snapshot block, still inside the outer brace.

Actually — rescan doesn't depend on persistence. Remove the cfg gate on the rescan (it only reads the filesystem):

```rust
                let should_rescan = match decision_ledger.last_scan_ts() {
                    None => true,
                    Some(prev) => (chrono::Utc::now() - prev).num_seconds() >= 60,
                };
                if should_rescan {
                    use std::path::Path;
                    eden::pipeline::decision_ledger::scanner::rescan_recent(
                        Path::new("decisions"),
                        &mut decision_ledger,
                        chrono::Utc::now(),
                    );
                }
```

- [ ] **Step 3: Compile both configs**

Run:
```bash
export CARGO_TARGET_DIR=/tmp/eden-target
cargo check --lib --features persistence -q
cargo check --lib --no-default-features -q
```

Expected: both compile clean.

- [ ] **Step 4: Re-run all belief tests to confirm 0 regression**

Run:
```bash
cargo test --lib -q belief_field belief_snapshot decision_ledger
```

Expected: all previously-passing tests still pass, plus the 13 decision_ledger tests.

- [ ] **Step 5: Commit**

```bash
git add src/hk/runtime.rs
git commit -m "$(cat <<'EOF'
feat(hk): integrate DecisionLedger into HK tick loop

Startup: scan decisions/ tree into Market::Hk ledger. Per-tick: for
each notable symbol from belief_field.top_notable_beliefs(5), if
the ledger has >=1 decision on that symbol, emit a
"prior decisions:" wake line via decision_ledger::wake_format.
60s rescan of decisions/YYYY/MM/DD/ for today + yesterday only.

No belief_field modification — pure observation (interpretation
deferred to A2.5 decision→belief spec). Compile clean both with and
without persistence feature; all prior tests pass.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 6: US runtime integration (symmetric)

**Files:**
- Modify: `src/us/runtime.rs`

- [ ] **Step 1: Mirror HK changes**

Apply the same structural edits to `src/us/runtime.rs`:

1. Ledger construction (`Market::Us`, `"us"` tag not needed — market passed directly):

   Grep for the US belief_field construction (`PressureBeliefField::new(crate::ontology::objects::Market::Us)`) and add immediately after the US `lifecycle_tracker` line:

   ```rust
       let mut decision_ledger =
           crate::pipeline::decision_ledger::DecisionLedger::new(
               crate::ontology::objects::Market::Us,
           );
       {
           use std::path::Path;
           crate::pipeline::decision_ledger::scanner::scan_directory(
               Path::new("decisions"),
               &mut decision_ledger,
           );
       }
   ```

   (Note `crate::` prefix because US runtime is under `src/us/` and uses crate-local paths, unlike HK runtime which uses `eden::` — mirror each runtime's existing style exactly. Check by grepping `eden::pipeline` vs `crate::pipeline` in each file.)

2. Wake-emission block in US tick loop — same pattern:

   Find the US belief `format_wake_line` call (`grep format_wake_line src/us/runtime.rs`) and modify similarly:

   ```rust
               for notable in belief_field.top_notable_beliefs(5) {
                   let symbol_for_decisions = match &notable {
                       crate::pipeline::belief_field::NotableBelief::Gaussian { symbol, .. }
                       | crate::pipeline::belief_field::NotableBelief::Categorical { symbol, .. } => symbol.clone(),
                   };
                   artifact_projection
                       .agent_snapshot
                       .wake
                       .reasons
                       .push(crate::pipeline::belief_field::format_wake_line(&notable));
                   if let Some(summary) = decision_ledger.summary_for(&symbol_for_decisions) {
                       if summary.total_decisions >= 1 {
                           artifact_projection
                               .agent_snapshot
                               .wake
                               .reasons
                               .push(
                                   crate::pipeline::decision_ledger::wake_format::format_prior_decisions_line(
                                       &symbol_for_decisions,
                                       summary,
                                   ),
                               );
                       }
                   }
               }
   ```

3. 60s rescan block same as HK. Inside the same outer belief `{ ... }` block:

   ```rust
               let should_rescan = match decision_ledger.last_scan_ts() {
                   None => true,
                   Some(prev) => (chrono::Utc::now() - prev).num_seconds() >= 60,
               };
               if should_rescan {
                   use std::path::Path;
                   crate::pipeline::decision_ledger::scanner::rescan_recent(
                       Path::new("decisions"),
                       &mut decision_ledger,
                       chrono::Utc::now(),
                   );
               }
   ```

- [ ] **Step 2: Compile + test**

Run:
```bash
export CARGO_TARGET_DIR=/tmp/eden-target
cargo check --lib --features persistence -q
cargo check --lib --no-default-features -q
cargo test --lib -q belief_field belief_snapshot decision_ledger
```

Expected: all compile clean, all tests pass.

- [ ] **Step 3: Commit**

```bash
git add src/us/runtime.rs
git commit -m "$(cat <<'EOF'
feat(us): integrate DecisionLedger into US tick loop (symmetric)

Mirror of HK integration using crate::pipeline::decision_ledger and
Market::Us. Real 2026-04-15 US decisions (KC entry/exit + HUBS
entry) appear in prior-decisions wake lines when those symbols land
in top_notable_beliefs.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Task 7: Integration test + final acceptance

**Files:**
- Create: `tests/decision_ledger_integration.rs`

- [ ] **Step 1: Write end-to-end integration test**

Create `tests/decision_ledger_integration.rs`:

```rust
//! End-to-end: scan the repo's real `decisions/` tree and verify the
//! three 2026-04-15 US backfills land in the expected shape.

use std::path::Path;

use eden::ontology::objects::{Market, Symbol};
use eden::pipeline::decision_ledger::{
    scanner, DecisionAction, DecisionLedger,
};

#[test]
fn scans_real_2026_04_15_us_session_end_to_end() {
    let mut ledger = DecisionLedger::new(Market::Us);
    scanner::scan_directory(Path::new("decisions"), &mut ledger);

    assert_eq!(ledger.ingested_count(), 3, "3 US decisions in 2026-04-15");

    let kc = Symbol("KC.US".to_string());
    let hubs = Symbol("HUBS.US".to_string());

    let kc_summary = ledger.summary_for(&kc).expect("KC summary exists");
    assert_eq!(kc_summary.total_decisions, 2);
    assert_eq!(kc_summary.entries, 1);
    assert_eq!(kc_summary.exits, 1);
    assert!(
        (kc_summary.net_pnl_bps - (-18.0)).abs() < 1e-6,
        "KC net_pnl: {}",
        kc_summary.net_pnl_bps
    );
    assert_eq!(kc_summary.last_action, Some(DecisionAction::Exit));
    assert!(
        !kc_summary.unique_eden_gaps.is_empty(),
        "KC exit retrospective should contribute an eden_gap"
    );

    let hubs_summary = ledger.summary_for(&hubs).expect("HUBS summary exists");
    assert_eq!(hubs_summary.total_decisions, 1);
    assert_eq!(hubs_summary.entries, 1);
    assert_eq!(hubs_summary.exits, 0);
    assert_eq!(hubs_summary.last_action, Some(DecisionAction::Entry));
    assert_eq!(hubs_summary.last_pnl_bps, None);
}

#[test]
fn hk_ledger_has_no_decisions_from_us_only_backfill_day() {
    let mut ledger = DecisionLedger::new(Market::Hk);
    scanner::scan_directory(Path::new("decisions"), &mut ledger);

    assert_eq!(ledger.ingested_count(), 0);
    assert_eq!(ledger.total_symbols(), 0);
}

#[test]
fn wake_line_for_kc_mentions_exit_and_gap() {
    let mut ledger = DecisionLedger::new(Market::Us);
    scanner::scan_directory(Path::new("decisions"), &mut ledger);

    let kc = Symbol("KC.US".to_string());
    let summary = ledger.summary_for(&kc).expect("KC summary exists");
    let line = eden::pipeline::decision_ledger::wake_format::format_prior_decisions_line(
        &kc, summary,
    );

    assert!(line.starts_with("prior decisions: KC.US 2"), "line: {}", line);
    assert!(line.contains("exit @2026-04-15"), "line: {}", line);
    assert!(line.contains("-18bps"), "line: {}", line);
    assert!(line.contains("eden_gap:"), "line: {}", line);
}
```

- [ ] **Step 2: Run integration test**

Run: `cargo test --test decision_ledger_integration -q`
Expected: 3 tests pass.

- [ ] **Step 3: Full acceptance run**

Run:
```bash
export CARGO_TARGET_DIR=/tmp/eden-target
echo "AC1 cargo check persistence:"; cargo check --lib --features persistence -q && echo PASS
echo "AC2 cargo check no-default:"; cargo check --lib --no-default-features -q && echo PASS
echo "AC3 unit tests:"; cargo test --lib -q decision_ledger 2>&1 | tail -3
echo "AC4 integration test:"; cargo test --test decision_ledger_integration -q 2>&1 | tail -3
echo "AC5 belief tests 0 regression:"; cargo test --lib -q belief_field belief_snapshot 2>&1 | tail -3
```

Expected: PASS on all AC lines; unit test count ≥ 13 (decision_ledger) + ≥ 5 (wake_format); integration count = 3.

- [ ] **Step 4: Commit**

```bash
git add tests/decision_ledger_integration.rs
git commit -m "$(cat <<'EOF'
test(decision_ledger): end-to-end integration with real 2026-04-15 data

Three integration tests:
- scans_real_2026_04_15_us_session_end_to_end: 3 decisions, KC
  summary has correct pnl, HUBS entry-only
- hk_ledger_has_no_decisions_from_us_only_backfill_day: market split
- wake_line_for_kc_mentions_exit_and_gap: format wiring

Closes A2 implementation.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```

---

## Self-Review

**Spec coverage:**

| Spec requirement | Task |
|------------------|------|
| DecisionLedger struct + types | Task 1 |
| Startup full-tree scan | Task 2 |
| 60s rescan today + yesterday | Task 2 (`rescan_recent`) + Task 5 (wired in HK) + Task 6 (US) |
| Market-field split HK vs US | Task 2 (scanner filter) + Task 5/6 (per-market construction) |
| Summary computation (eden_gap dedup cap-3, newest-first) | Task 2 (`summarize`) + Task 3 (tests) |
| Ingested-paths dedup | Task 1 (`already_ingested`) + Task 2 (scanner uses it) |
| Error handling: bad JSON / schema mismatch / unknown action | Task 2 (`ingest_file` warn-and-skip) |
| Wake format `prior decisions:` line | Task 4 |
| Wake emission bound to belief top_notable_beliefs(5) | Task 5 (HK) + Task 6 (US) |
| Eden_gap appended to wake line | Task 4 |
| HK runtime integration | Task 5 |
| US runtime integration | Task 6 |
| Integration test (3 real decisions) | Task 7 |
| Acceptance AC1 cargo check persistence | Task 7 step 3 |
| Acceptance AC2 cargo check no-default | Task 7 step 3 |
| Acceptance AC3 unit tests | Task 7 step 3 |
| Acceptance AC4 integration test | Task 7 step 3 |
| Acceptance AC5 belief_field tests 0 regression | Task 7 step 3 |
| Observation only (no belief field mod) | Enforced by file list — no belief_field.rs edits anywhere |

All spec requirements have tasks.

**Placeholder scan:** No "TBD", no "implement later", no "similar to Task N", no missing code blocks. Each task has concrete code. Task 2 and Task 3 both reference `scanner::summarize`; Task 3 step 2 notes the summarize cap-3 behavior is an invariant, not an ambiguity.

**Type consistency:**
- `DecisionRecord { decision_id, timestamp, symbol, action, direction, confidence, linked_entry_id, outcome, eden_gap, backfilled }` — consistent across Task 1, 2, 3
- `SymbolDecisionSummary` fields consistent across Task 1 (definition), Task 2 (`summarize` populates), Task 3 (tests), Task 4 (wake format reads)
- `DecisionLedger::already_ingested` / `insert_record_raw` / `mark_skipped` / `set_summary_raw` / `summary_for` / `decisions_for` — signatures consistent
- `scanner::scan_directory(&Path, &mut DecisionLedger)` — consistent from Task 2 definition through Task 5/6/7 call sites
- `scanner::rescan_recent(&Path, &mut DecisionLedger, DateTime<Utc>) -> usize` — consistent
- `wake_format::format_prior_decisions_line(&Symbol, &SymbolDecisionSummary) -> String` — consistent

No signature drift detected.
