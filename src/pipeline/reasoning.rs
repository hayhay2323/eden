use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::graph::convergence::ConvergenceScore;
use crate::ontology::reasoning::{
    CaseCluster, Hypothesis, HypothesisTrack, InvestigationSelection, PropagationPath,
    TacticalSetup,
};

#[path = "reasoning/propagation.rs"]
mod propagation;
pub(crate) use propagation::derive_diffusion_propagation_paths;
pub use propagation::{mechanism_family, path_has_family, path_is_mixed_multi_hop};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ConvergenceDetail {
    pub institutional_alignment: Decimal,
    pub sector_coherence: Option<Decimal>,
    pub cross_stock_correlation: Decimal,
    pub component_spread: Option<Decimal>,
    pub edge_stability: Option<Decimal>,
}

impl ConvergenceDetail {
    pub fn from_convergence_score(score: &ConvergenceScore) -> Self {
        Self {
            institutional_alignment: score.institutional_alignment,
            sector_coherence: score.sector_coherence,
            cross_stock_correlation: score.cross_stock_correlation,
            component_spread: score.component_spread,
            edge_stability: score.edge_stability,
        }
    }

    pub fn from_us_convergence_score(
        score: &crate::us::graph::decision::UsConvergenceScore,
    ) -> Self {
        Self {
            institutional_alignment: score.dimension_composite,
            sector_coherence: score.sector_coherence,
            cross_stock_correlation: score.cross_stock_correlation,
            component_spread: None,
            edge_stability: None,
        }
    }
}

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
    pub fn empty(timestamp: OffsetDateTime) -> Self {
        Self {
            timestamp,
            hypotheses: Vec::new(),
            propagation_paths: Vec::new(),
            investigation_selections: Vec::new(),
            tactical_setups: Vec::new(),
            hypothesis_tracks: Vec::new(),
            case_clusters: Vec::new(),
        }
    }
}
