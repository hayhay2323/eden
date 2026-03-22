use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::{ActionDirection, ActionNode, ActionNodeStage, TacticalSetup};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::tracker::UsStructuralDegradation;

// ── Stage enum ──

/// Five-stage lifecycle for a US tactical action.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsActionStage {
    /// Setup has been generated but not yet reviewed.
    Suggested,
    /// Operator or engine has reviewed and agreed to act.
    Confirmed,
    /// Position has been entered at a known price.
    Executed,
    /// Position is open; degradation monitoring is active.
    Monitoring,
    /// Position has been closed and the outcome recorded.
    Reviewed,
}

impl UsActionStage {
    pub const ALL: [Self; 5] = [
        Self::Suggested,
        Self::Confirmed,
        Self::Executed,
        Self::Monitoring,
        Self::Reviewed,
    ];

    /// Advance to the next stage, if one exists.
    pub fn next(self) -> Option<Self> {
        match self {
            Self::Suggested => Some(Self::Confirmed),
            Self::Confirmed => Some(Self::Executed),
            Self::Executed => Some(Self::Monitoring),
            Self::Monitoring => Some(Self::Reviewed),
            Self::Reviewed => None,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Suggested => "suggested",
            Self::Confirmed => "confirmed",
            Self::Executed => "executed",
            Self::Monitoring => "monitoring",
            Self::Reviewed => "reviewed",
        }
    }
}

impl std::fmt::Display for UsActionStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── Workflow ──

/// Tracks the full lifecycle of a single US tactical action from suggestion to review.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsActionWorkflow {
    /// Unique identifier for this workflow instance.
    pub workflow_id: String,
    /// The stock being acted on.
    pub symbol: Symbol,
    /// Current stage in the lifecycle.
    pub stage: UsActionStage,
    /// The tactical setup that originated this workflow.
    pub setup_id: String,
    /// Tick counter at which this workflow was created.
    pub entry_tick: u64,
    /// Tick counter at which the current stage was entered.
    #[serde(default)]
    pub stage_entered_tick: u64,
    /// Price recorded when the position was executed, if applicable.
    pub entry_price: Option<Decimal>,
    /// Confidence score from the originating tactical setup.
    pub confidence_at_entry: Decimal,
    /// Most recently observed confidence for the underlying setup.
    pub current_confidence: Decimal,
    /// Unrealised or realised P&L (set once price is available).
    pub pnl: Option<Decimal>,
    /// Latest structural degradation snapshot (populated during Monitoring).
    pub degradation: Option<UsStructuralDegradation>,
    /// Freeform audit trail.
    pub notes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UsWorkflowTransitionError {
    action: &'static str,
    expected: UsActionStage,
    actual: UsActionStage,
}

impl std::fmt::Display for UsWorkflowTransitionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} requires stage {}, got {}",
            self.action, self.expected, self.actual
        )
    }
}

impl std::error::Error for UsWorkflowTransitionError {}

fn action_node_stage(stage: UsActionStage) -> ActionNodeStage {
    match stage {
        UsActionStage::Suggested => ActionNodeStage::Suggested,
        UsActionStage::Confirmed => ActionNodeStage::Confirmed,
        UsActionStage::Executed => ActionNodeStage::Executed,
        UsActionStage::Monitoring => ActionNodeStage::Monitoring,
        UsActionStage::Reviewed => ActionNodeStage::Reviewed,
    }
}

impl UsActionWorkflow {
    /// Create a new workflow from a tactical setup at a given tick and optional entry price.
    pub fn from_setup(setup: &TacticalSetup, tick: u64, price: Option<Decimal>) -> Self {
        let symbol = match &setup.scope {
            crate::ontology::reasoning::ReasoningScope::Symbol(s) => s.clone(),
            _ => Symbol(setup.setup_id.clone()),
        };

        Self {
            workflow_id: format!("wf:{}:{}", setup.setup_id, tick),
            symbol,
            stage: UsActionStage::Suggested,
            setup_id: setup.setup_id.clone(),
            entry_tick: tick,
            stage_entered_tick: tick,
            entry_price: price,
            confidence_at_entry: setup.confidence,
            current_confidence: setup.confidence,
            pnl: None,
            degradation: None,
            notes: vec![],
        }
    }

    fn ensure_stage(
        &self,
        expected: UsActionStage,
        action: &'static str,
    ) -> Result<(), UsWorkflowTransitionError> {
        if self.stage == expected {
            Ok(())
        } else {
            Err(UsWorkflowTransitionError {
                action,
                expected,
                actual: self.stage,
            })
        }
    }

    /// Advance from Suggested to Confirmed.
    pub fn confirm(&mut self, tick: u64) -> Result<(), UsWorkflowTransitionError> {
        self.ensure_stage(UsActionStage::Suggested, "confirm")?;
        self.stage = UsActionStage::Confirmed;
        self.stage_entered_tick = tick;
        self.notes.push("Workflow confirmed.".to_string());
        Ok(())
    }

    /// Advance from Confirmed to Executed and record the actual entry price.
    pub fn execute(&mut self, price: Decimal, tick: u64) -> Result<(), UsWorkflowTransitionError> {
        self.ensure_stage(UsActionStage::Confirmed, "execute")?;
        self.stage = UsActionStage::Executed;
        self.entry_price = Some(price);
        self.notes.push(format!("Position executed at {price}."));

        // Immediately advance to Monitoring since the position is now open.
        self.stage = UsActionStage::Monitoring;
        self.stage_entered_tick = tick;
        self.notes.push("Monitoring started.".to_string());
        Ok(())
    }

    /// Update monitoring state with the current price and structural degradation.
    pub fn update_monitoring(
        &mut self,
        current_price: Option<Decimal>,
        degradation: UsStructuralDegradation,
    ) -> Result<(), UsWorkflowTransitionError> {
        self.ensure_stage(UsActionStage::Monitoring, "update_monitoring")?;

        // Recompute P&L if both prices are known.
        if let (Some(entry), Some(current)) = (self.entry_price, current_price) {
            self.pnl = Some(current - entry);
        }

        if degradation.should_exit {
            self.notes.push(format!(
                "Exit signal triggered: composite_drift={}, flow_reversal={}, \
                 momentum_decay={}, volume_dry_up={}, ticks_held={}",
                degradation.composite_drift,
                degradation.capital_flow_reversal,
                degradation.momentum_decay,
                degradation.volume_dry_up,
                degradation.ticks_held,
            ));
        }

        self.degradation = Some(degradation);
        Ok(())
    }

    /// Advance from Monitoring to Reviewed and record the outcome.
    pub fn review(&mut self, outcome: &str, tick: u64) -> Result<(), UsWorkflowTransitionError> {
        self.ensure_stage(UsActionStage::Monitoring, "review")?;
        self.stage = UsActionStage::Reviewed;
        self.stage_entered_tick = tick;
        self.notes.push(format!("Review: {outcome}"));
        Ok(())
    }

    /// Returns true if the workflow has been in the current stage too long.
    /// The staleness timer is stage-local and resets on every successful transition.
    ///
    /// Staleness thresholds (in ticks):
    /// - Suggested  → stale after 100 ticks without confirmation
    /// - Confirmed  → stale after 50 ticks without execution
    /// - Monitoring → stale after 600 ticks without review
    /// - Reviewed   → never stale
    pub fn is_stale(&self, current_tick: u64) -> bool {
        let elapsed = current_tick.saturating_sub(self.stage_entered_tick);
        match self.stage {
            UsActionStage::Suggested => elapsed > 100,
            UsActionStage::Confirmed => elapsed > 50,
            UsActionStage::Executed => elapsed > 50,
            UsActionStage::Monitoring => elapsed > 600,
            UsActionStage::Reviewed => false,
        }
    }

    /// Serialize the workflow to a JSON snapshot suitable for the frontend.
    pub fn snapshot(&self) -> Value {
        json!({
            "workflow_id": self.workflow_id,
            "symbol": self.symbol.0,
            "stage": self.stage.as_str(),
            "setup_id": self.setup_id,
            "entry_tick": self.entry_tick,
            "stage_entered_tick": self.stage_entered_tick,
            "entry_price": self.entry_price,
            "confidence_at_entry": self.confidence_at_entry,
            "current_confidence": self.current_confidence,
            "pnl": self.pnl,
            "should_exit": self.degradation.as_ref().map(|d| d.should_exit),
            "composite_drift": self.degradation.as_ref().map(|d| d.composite_drift),
            "ticks_held": self.degradation.as_ref().map(|d| d.ticks_held),
            "notes": self.notes,
        })
    }
}

impl ActionNode {
    pub fn from_us_workflow(workflow: &UsActionWorkflow, current_tick: u64) -> Self {
        Self {
            workflow_id: workflow.workflow_id.clone(),
            symbol: workflow.symbol.clone(),
            market: workflow.symbol.market(),
            sector: None,
            stage: action_node_stage(workflow.stage),
            // US workflows do not currently store signed trade direction.
            // Keep this neutral until Phase 3d unifies workflow directionality.
            direction: ActionDirection::Neutral,
            entry_confidence: workflow.confidence_at_entry,
            current_confidence: workflow.current_confidence,
            entry_price: workflow.entry_price,
            pnl: workflow.pnl,
            age_ticks: current_tick.saturating_sub(workflow.entry_tick),
            degradation_score: workflow.degradation.as_ref().map(|degradation| {
                degradation
                    .composite_drift
                    .abs()
                    .max(degradation.momentum_decay)
                    .max(degradation.volume_dry_up)
            }),
            exit_forming: workflow
                .degradation
                .as_ref()
                .map(|degradation| degradation.should_exit)
                .unwrap_or(false),
        }
    }
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
    use crate::ontology::reasoning::{DecisionLineage, ReasoningScope};
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    fn make_setup(symbol: &str, confidence: Decimal) -> TacticalSetup {
        TacticalSetup {
            setup_id: format!("setup:{symbol}:enter"),
            hypothesis_id: format!("hyp:{symbol}:momentum"),
            runner_up_hypothesis_id: None,
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            ),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(Symbol(symbol.into())),
            title: format!("{symbol} Momentum Continuation"),
            action: "enter".into(),
            time_horizon: "intraday".into(),
            confidence,
            confidence_gap: dec!(0.2),
            heuristic_edge: confidence * dec!(0.2),
            workflow_id: None,
            entry_rationale: "capital flow momentum suggests continuation".into(),
            risk_notes: vec!["valuation extreme reached".into()],
        }
    }

    fn make_degradation(should_exit: bool) -> UsStructuralDegradation {
        UsStructuralDegradation {
            symbol: Symbol("AAPL.US".into()),
            composite_drift: if should_exit { dec!(-0.5) } else { dec!(0.05) },
            capital_flow_reversal: false,
            momentum_decay: dec!(0.0),
            volume_dry_up: dec!(0.0),
            ticks_held: 10,
            should_exit,
        }
    }

    // ── Stage progression ──

    #[test]
    fn stage_next_is_linear() {
        assert_eq!(
            UsActionStage::Suggested.next(),
            Some(UsActionStage::Confirmed)
        );
        assert_eq!(
            UsActionStage::Confirmed.next(),
            Some(UsActionStage::Executed)
        );
        assert_eq!(
            UsActionStage::Executed.next(),
            Some(UsActionStage::Monitoring)
        );
        assert_eq!(
            UsActionStage::Monitoring.next(),
            Some(UsActionStage::Reviewed)
        );
        assert_eq!(UsActionStage::Reviewed.next(), None);
    }

    #[test]
    fn stage_all_covers_all_variants() {
        assert_eq!(UsActionStage::ALL.len(), 5);
    }

    // ── from_setup ──

    #[test]
    fn from_setup_creates_suggested_workflow() {
        let setup = make_setup("NVDA.US", dec!(0.7));
        let wf = UsActionWorkflow::from_setup(&setup, 42, Some(dec!(120)));
        assert_eq!(wf.stage, UsActionStage::Suggested);
        assert_eq!(wf.symbol, Symbol("NVDA.US".into()));
        assert_eq!(wf.confidence_at_entry, dec!(0.7));
        assert_eq!(wf.current_confidence, dec!(0.7));
        assert_eq!(wf.entry_tick, 42);
        assert_eq!(wf.stage_entered_tick, 42);
        assert_eq!(wf.entry_price, Some(dec!(120)));
        assert!(wf.pnl.is_none());
        assert!(wf.notes.is_empty());
    }

    // ── confirm / execute ──

    #[test]
    fn confirm_advances_to_confirmed() {
        let setup = make_setup("AAPL.US", dec!(0.65));
        let mut wf = UsActionWorkflow::from_setup(&setup, 10, None);
        wf.confirm(10).unwrap();
        assert_eq!(wf.stage, UsActionStage::Confirmed);
        assert_eq!(wf.stage_entered_tick, 10);
        assert!(!wf.notes.is_empty());
    }

    #[test]
    fn execute_advances_to_monitoring_and_records_price() {
        let setup = make_setup("TSLA.US", dec!(0.72));
        let mut wf = UsActionWorkflow::from_setup(&setup, 5, None);
        wf.confirm(6).unwrap();
        wf.execute(dec!(215.50), 7).unwrap();
        assert_eq!(wf.stage, UsActionStage::Monitoring);
        assert_eq!(wf.stage_entered_tick, 7);
        assert_eq!(wf.entry_price, Some(dec!(215.50)));
    }

    // ── update_monitoring ──

    #[test]
    fn update_monitoring_computes_pnl() {
        let setup = make_setup("MSFT.US", dec!(0.6));
        let mut wf = UsActionWorkflow::from_setup(&setup, 0, Some(dec!(300)));
        wf.confirm(1).unwrap();
        wf.execute(dec!(300), 2).unwrap();
        wf.update_monitoring(Some(dec!(310)), make_degradation(false))
            .unwrap();
        assert_eq!(wf.pnl, Some(dec!(10)));
        assert!(wf.degradation.is_some());
    }

    #[test]
    fn update_monitoring_records_exit_signal_in_notes() {
        let setup = make_setup("BABA.US", dec!(0.55));
        let mut wf = UsActionWorkflow::from_setup(&setup, 0, Some(dec!(80)));
        wf.confirm(1).unwrap();
        wf.execute(dec!(80), 2).unwrap();
        wf.update_monitoring(Some(dec!(75)), make_degradation(true))
            .unwrap();
        assert!(wf.notes.iter().any(|n| n.contains("Exit signal triggered")));
    }

    // ── review ──

    #[test]
    fn review_advances_to_reviewed() {
        let setup = make_setup("NVDA.US", dec!(0.75));
        let mut wf = UsActionWorkflow::from_setup(&setup, 0, Some(dec!(500)));
        wf.confirm(1).unwrap();
        wf.execute(dec!(500), 2).unwrap();
        wf.update_monitoring(Some(dec!(510)), make_degradation(false))
            .unwrap();
        wf.review("closed with +10 profit", 3).unwrap();
        assert_eq!(wf.stage, UsActionStage::Reviewed);
        assert!(wf
            .notes
            .iter()
            .any(|n| n.contains("closed with +10 profit")));
    }

    // ── is_stale ──

    #[test]
    fn is_stale_suggested_after_100_ticks() {
        let setup = make_setup("AAPL.US", dec!(0.6));
        let wf = UsActionWorkflow::from_setup(&setup, 0, None);
        assert!(!wf.is_stale(100));
        assert!(wf.is_stale(101));
    }

    #[test]
    fn is_stale_monitoring_after_600_ticks() {
        let setup = make_setup("TSLA.US", dec!(0.7));
        let mut wf = UsActionWorkflow::from_setup(&setup, 0, Some(dec!(200)));
        wf.confirm(25).unwrap();
        wf.execute(dec!(200), 40).unwrap();
        assert!(!wf.is_stale(640));
        assert!(wf.is_stale(641));
    }

    #[test]
    fn reviewed_is_never_stale() {
        let setup = make_setup("MSFT.US", dec!(0.65));
        let mut wf = UsActionWorkflow::from_setup(&setup, 0, Some(dec!(300)));
        wf.confirm(1).unwrap();
        wf.execute(dec!(300), 2).unwrap();
        wf.update_monitoring(None, make_degradation(false)).unwrap();
        wf.review("exit", 3).unwrap();
        assert!(!wf.is_stale(999_999));
    }

    // ── snapshot ──

    #[test]
    fn snapshot_contains_required_fields() {
        let setup = make_setup("NVDA.US", dec!(0.8));
        let wf = UsActionWorkflow::from_setup(&setup, 10, Some(dec!(120)));
        let snap = wf.snapshot();
        assert_eq!(snap["symbol"], "NVDA.US");
        assert_eq!(snap["stage"], "suggested");
        assert_eq!(snap["entry_tick"], 10);
        assert_eq!(snap["confidence_at_entry"], dec!(0.8).to_string());
    }

    #[test]
    fn snapshot_pnl_present_after_monitoring() {
        let setup = make_setup("AAPL.US", dec!(0.65));
        let mut wf = UsActionWorkflow::from_setup(&setup, 0, Some(dec!(150)));
        wf.confirm(1).unwrap();
        wf.execute(dec!(150), 2).unwrap();
        wf.update_monitoring(Some(dec!(155)), make_degradation(false))
            .unwrap();
        let snap = wf.snapshot();
        assert!(!snap["pnl"].is_null());
    }

    #[test]
    fn invalid_transition_returns_error_instead_of_panicking() {
        let setup = make_setup("AMD.US", dec!(0.6));
        let mut wf = UsActionWorkflow::from_setup(&setup, 0, None);

        let err = wf.execute(dec!(100), 1).unwrap_err();
        assert_eq!(err.expected, UsActionStage::Confirmed);
        assert_eq!(err.actual, UsActionStage::Suggested);
    }

    #[test]
    fn action_node_from_us_workflow_preserves_market_and_age() {
        let setup = make_setup("NVDA.US", dec!(0.8));
        let wf = UsActionWorkflow::from_setup(&setup, 10, Some(dec!(120)));

        let node = ActionNode::from_us_workflow(&wf, 25);

        assert_eq!(node.market, crate::ontology::Market::Us);
        assert_eq!(node.stage, ActionNodeStage::Suggested);
        assert_eq!(node.direction, ActionDirection::Neutral);
        assert_eq!(node.age_ticks, 15);
    }
}
