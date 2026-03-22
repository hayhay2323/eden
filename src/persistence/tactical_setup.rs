use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::ontology::reasoning::TacticalSetup;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TacticalSetupRecord {
    pub setup_id: String,
    pub hypothesis_id: String,
    pub runner_up_hypothesis_id: Option<String>,
    pub scope_key: String,
    pub title: String,
    pub action: String,
    pub time_horizon: String,
    pub confidence: String,
    pub confidence_gap: String,
    pub heuristic_edge: String,
    pub workflow_id: Option<String>,
    pub entry_rationale: String,
    pub risk_notes: Vec<String>,
    pub based_on: Vec<String>,
    pub blocked_by: Vec<String>,
    pub promoted_by: Vec<String>,
    pub falsified_by: Vec<String>,
    #[serde(with = "rfc3339")]
    pub recorded_at: OffsetDateTime,
}

impl TacticalSetupRecord {
    pub fn from_setup(setup: &TacticalSetup, recorded_at: OffsetDateTime) -> Self {
        Self {
            setup_id: setup.setup_id.clone(),
            hypothesis_id: setup.hypothesis_id.clone(),
            runner_up_hypothesis_id: setup.runner_up_hypothesis_id.clone(),
            scope_key: format!("{:?}", setup.scope),
            title: setup.title.clone(),
            action: setup.action.clone(),
            time_horizon: setup.time_horizon.clone(),
            confidence: setup.confidence.to_string(),
            confidence_gap: setup.confidence_gap.to_string(),
            heuristic_edge: setup.heuristic_edge.to_string(),
            workflow_id: setup.workflow_id.clone(),
            entry_rationale: setup.entry_rationale.clone(),
            risk_notes: setup.risk_notes.clone(),
            based_on: setup.lineage.based_on.clone(),
            blocked_by: setup.lineage.blocked_by.clone(),
            promoted_by: setup.lineage.promoted_by.clone(),
            falsified_by: setup.lineage.falsified_by.clone(),
            recorded_at,
        }
    }

    pub fn record_id(&self) -> &str {
        &self.setup_id
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;
    use crate::ontology::reasoning::ReasoningScope;
    use crate::ontology::ProvenanceMetadata;
    use crate::ontology::ProvenanceSource;
    use crate::ontology::Symbol;

    #[test]
    fn tactical_setup_record_preserves_gap_and_runner_up() {
        let setup = TacticalSetup {
            setup_id: "setup:700.HK:enter".into(),
            hypothesis_id: "hyp:700.HK:flow".into(),
            runner_up_hypothesis_id: Some("hyp:700.HK:risk".into()),
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            )
            .with_trace_id("setup:700.HK:enter")
            .with_inputs(["hyp:700.HK:flow"]),
            lineage: crate::ontology::DecisionLineage {
                based_on: vec!["hyp:700.HK:flow".into()],
                blocked_by: vec![],
                promoted_by: vec!["review -> enter".into()],
                falsified_by: vec!["local flow flips negative".into()],
            },
            scope: ReasoningScope::Symbol(Symbol("700.HK".into())),
            title: "Long 700.HK".into(),
            action: "enter".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.62),
            confidence_gap: dec!(0.18),
            heuristic_edge: dec!(0.11),
            workflow_id: Some("order:700.HK:buy".into()),
            entry_rationale: "flow explanation leads".into(),
            risk_notes: vec!["runner-up remains close".into()],
        };

        let record = TacticalSetupRecord::from_setup(&setup, OffsetDateTime::UNIX_EPOCH);
        assert_eq!(
            record.runner_up_hypothesis_id.as_deref(),
            Some("hyp:700.HK:risk")
        );
        assert_eq!(record.confidence_gap, "0.18");
        assert_eq!(record.based_on, vec!["hyp:700.HK:flow"]);
        assert_eq!(record.promoted_by, vec!["review -> enter"]);
        assert_eq!(record.falsified_by, vec!["local flow flips negative"]);
    }
}
