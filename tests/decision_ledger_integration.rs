//! End-to-end: scan the repo's real `decisions/` tree and verify the
//! three 2026-04-15 US backfills land in the expected shape.

use std::path::Path;

use eden::ontology::objects::{Market, Symbol};
use eden::pipeline::decision_ledger::{scanner, DecisionAction, DecisionLedger};

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
    let line =
        eden::pipeline::decision_ledger::wake_format::format_prior_decisions_line(&kc, summary);

    assert!(
        line.starts_with("prior decisions: KC.US 2"),
        "line: {}",
        line
    );
    assert!(line.contains("exit @2026-04-15"), "line: {}", line);
    assert!(line.contains("-18bps"), "line: {}", line);
    assert!(line.contains("eden_gap:"), "line: {}", line);
}
