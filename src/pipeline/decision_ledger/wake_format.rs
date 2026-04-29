//! Format a SymbolDecisionSummary as a single wake line.

use chrono::{DateTime, Utc};

use crate::ontology::objects::Symbol;
use crate::pipeline::decision_ledger::{DecisionAction, SymbolDecisionSummary};

/// Format a `prior decisions:` wake line for a notable symbol.
///
/// Shape examples:
///   prior decisions: KC.US 2 (exit @2026-04-15 -18bps)
///   prior decisions: KC.US 2 (exit @2026-04-15 -18bps); eden_gap: roster churn
///   prior decisions: 3690.HK 1 (skip @2026-04-18)
pub fn format_prior_decisions_line(symbol: &Symbol, summary: &SymbolDecisionSummary) -> String {
    let mut line = format!("prior decisions: {} {}", symbol.0, summary.total_decisions);

    if let (Some(action), Some(ts)) = (summary.last_action, summary.last_timestamp) {
        line.push_str(" (");
        line.push_str(action_verb(action));
        line.push_str(" @");
        line.push_str(&format_date(ts));
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
        let line =
            format_prior_decisions_line(&Symbol("KC.US".to_string()), &summary_exit_with_pnl());
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
        let line = format_prior_decisions_line(&Symbol("3690.HK".to_string()), &summary_skip());
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
