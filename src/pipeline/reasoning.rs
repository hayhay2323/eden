use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::graph::decision::DecisionSnapshot;
use crate::graph::graph::BrainGraph;
use crate::graph::insights::GraphInsights;
use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::{
    CaseCluster, Hypothesis, HypothesisTrack, InvestigationSelection,
    PropagationPath, TacticalSetup,
};
use crate::temporal::lineage::FamilyContextLineageOutcome;

use super::signals::{DerivedSignalSnapshot, EventSnapshot};

#[path = "reasoning/propagation.rs"]
mod propagation;
#[path = "reasoning/support.rs"]
mod support;
#[path = "reasoning/policy.rs"]
mod policy;
#[path = "reasoning/synthesis.rs"]
mod synthesis;
#[path = "reasoning/clustering.rs"]
mod clustering;
use clustering::derive_case_clusters;
use propagation::{derive_diffusion_propagation_paths, derive_propagation_paths};
pub use propagation::{mechanism_family, path_has_family, path_is_mixed_multi_hop};
pub use policy::derive_hypothesis_tracks;
use policy::{apply_case_budget, apply_track_action_policy};
use synthesis::{derive_hypotheses, derive_investigation_selections, derive_tactical_setups};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningSnapshot {
    pub timestamp: OffsetDateTime,
    pub hypotheses: Vec<Hypothesis>,
    pub propagation_paths: Vec<PropagationPath>,
    pub investigation_selections: Vec<InvestigationSelection>,
    pub tactical_setups: Vec<TacticalSetup>,
    pub hypothesis_tracks: Vec<HypothesisTrack>,
    pub case_clusters: Vec<CaseCluster>,
}

impl ReasoningSnapshot {
    pub fn derive(
        events: &EventSnapshot,
        derived_signals: &DerivedSignalSnapshot,
        insights: &GraphInsights,
        decision: &DecisionSnapshot,
        previous_setups: &[TacticalSetup],
        previous_tracks: &[HypothesisTrack],
    ) -> Self {
        Self::derive_with_policy(
            events,
            derived_signals,
            insights,
            decision,
            previous_setups,
            previous_tracks,
            &[],
        )
    }

    pub fn derive_with_policy(
        events: &EventSnapshot,
        derived_signals: &DerivedSignalSnapshot,
        insights: &GraphInsights,
        decision: &DecisionSnapshot,
        previous_setups: &[TacticalSetup],
        previous_tracks: &[HypothesisTrack],
        lineage_priors: &[FamilyContextLineageOutcome],
    ) -> Self {
        let propagation_paths = derive_propagation_paths(insights, decision.timestamp);
        let hypotheses = derive_hypotheses(events, derived_signals, &propagation_paths);
        let investigation_selections = derive_investigation_selections(decision, &hypotheses);
        let baseline_setups =
            derive_tactical_setups(decision, &hypotheses, &investigation_selections);
        let baseline_tracks = derive_hypothesis_tracks(
            decision.timestamp,
            &baseline_setups,
            previous_setups,
            previous_tracks,
        );
        let tactical_setups = apply_track_action_policy(
            &baseline_setups,
            &baseline_tracks,
            previous_tracks,
            decision.timestamp,
            &decision.market_regime,
            lineage_priors,
        );
        let tactical_setups = apply_case_budget(tactical_setups, &baseline_tracks, previous_tracks);
        let hypothesis_tracks = derive_hypothesis_tracks(
            decision.timestamp,
            &tactical_setups,
            previous_setups,
            previous_tracks,
        );
        let case_clusters = derive_case_clusters(
            &hypotheses,
            &propagation_paths,
            &tactical_setups,
            &hypothesis_tracks,
        );

        Self {
            timestamp: decision.timestamp,
            hypotheses,
            propagation_paths,
            investigation_selections,
            tactical_setups,
            hypothesis_tracks,
            case_clusters,
        }
    }

    pub fn derive_with_diffusion(
        events: &EventSnapshot,
        derived_signals: &DerivedSignalSnapshot,
        _insights: &GraphInsights,
        decision: &DecisionSnapshot,
        previous_setups: &[TacticalSetup],
        previous_tracks: &[HypothesisTrack],
        lineage_priors: &[FamilyContextLineageOutcome],
        brain: &BrainGraph,
        stock_deltas: &HashMap<Symbol, Decimal>,
    ) -> Self {
        let propagation_paths =
            derive_diffusion_propagation_paths(brain, stock_deltas, decision.timestamp);
        let hypotheses = derive_hypotheses(events, derived_signals, &propagation_paths);
        let investigation_selections = derive_investigation_selections(decision, &hypotheses);
        let baseline_setups =
            derive_tactical_setups(decision, &hypotheses, &investigation_selections);
        let baseline_tracks = derive_hypothesis_tracks(
            decision.timestamp,
            &baseline_setups,
            previous_setups,
            previous_tracks,
        );
        let tactical_setups = apply_track_action_policy(
            &baseline_setups,
            &baseline_tracks,
            previous_tracks,
            decision.timestamp,
            &decision.market_regime,
            lineage_priors,
        );
        let tactical_setups = apply_case_budget(tactical_setups, &baseline_tracks, previous_tracks);
        let hypothesis_tracks = derive_hypothesis_tracks(
            decision.timestamp,
            &tactical_setups,
            previous_setups,
            previous_tracks,
        );
        let case_clusters = derive_case_clusters(
            &hypotheses,
            &propagation_paths,
            &tactical_setups,
            &hypothesis_tracks,
        );

        Self {
            timestamp: decision.timestamp,
            hypotheses,
            propagation_paths,
            investigation_selections,
            tactical_setups,
            hypothesis_tracks,
            case_clusters,
        }
    }
}

#[cfg(test)]
pub(crate) fn cluster_title(
    family_key: &str,
    linkage_key: &str,
    member_count: usize,
    path: Option<&PropagationPath>,
) -> String {
    clustering::cluster_title(family_key, linkage_key, member_count, path)
}

#[cfg(test)]
pub(crate) fn propagated_path_evidence(
    scope: &crate::ontology::reasoning::ReasoningScope,
    local_evidence: &[crate::ontology::reasoning::ReasoningEvidence],
    propagation_paths: &[PropagationPath],
) -> (Decimal, Vec<String>) {
    synthesis::propagated_path_evidence(scope, local_evidence, propagation_paths)
}


#[cfg(test)]
#[path = "reasoning/tests.rs"]
mod tests;
