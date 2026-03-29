use std::collections::HashMap;

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::{
    EvidencePolarity, Hypothesis, InvestigationSelection, PropagationPath, ReasoningEvidence,
    ReasoningEvidenceKind, ReasoningScope, TacticalSetup,
};
use crate::us::graph::decision::UsMarketRegimeBias;
use crate::us::graph::graph::UsGraph;
use crate::us::graph::propagation::CrossMarketSignal;
use crate::us::temporal::lineage::UsLineageStats;

use super::signals::{
    UsDerivedSignalSnapshot, UsEventSnapshot, UsSignalScope,
};

#[path = "reasoning/propagation.rs"]
mod propagation;
#[path = "reasoning/support.rs"]
mod support;
#[path = "reasoning/policy.rs"]
mod policy;
#[path = "reasoning/synthesis.rs"]
mod synthesis;
use policy::{derive_investigation_selections, derive_tactical_setups};
use propagation::derive_diffusion_propagation_paths;
use synthesis::derive_hypotheses;

// ── Hypothesis template keys ──

const TEMPLATE_PRE_MARKET_POSITIONING: &str = "pre_market_positioning";
const TEMPLATE_CROSS_MARKET_ARBITRAGE: &str = "cross_market_arbitrage";
const TEMPLATE_MOMENTUM_CONTINUATION: &str = "momentum_continuation";
const TEMPLATE_SECTOR_ROTATION: &str = "sector_rotation";
const TEMPLATE_STRUCTURAL_DIFFUSION: &str = "structural_diffusion";

// ── Template definition ──

struct HypothesisTemplate {
    key: &'static str,
    family_label: &'static str,
    thesis: &'static str,
    invalidation: &'static str,
    expected_observations: &'static [&'static str],
}

const TEMPLATES: &[HypothesisTemplate] = &[
    HypothesisTemplate {
        key: TEMPLATE_PRE_MARKET_POSITIONING,
        family_label: "Pre-Market Positioning",
        thesis: "pre-market move reflects institutional positioning before regular hours",
        invalidation: "capital flow during regular hours moves opposite to pre-market direction",
        expected_observations: &[
            "gap should hold through first 30 minutes",
            "volume should confirm direction",
        ],
    },
    HypothesisTemplate {
        key: TEMPLATE_CROSS_MARKET_ARBITRAGE,
        family_label: "Cross-Market Arbitrage",
        thesis: "may follow HK counterpart's institutional-driven move",
        invalidation: "US capital flow moves opposite to HK signal",
        expected_observations: &[
            "price should converge toward HK-implied level",
            "arbitrage spread should narrow",
        ],
    },
    HypothesisTemplate {
        key: TEMPLATE_MOMENTUM_CONTINUATION,
        family_label: "Momentum Continuation",
        thesis: "capital flow momentum suggests continuation",
        invalidation: "valuation extreme reached or flow direction reverses",
        expected_observations: &[
            "flow direction should persist",
            "volume should remain elevated",
        ],
    },
    HypothesisTemplate {
        key: TEMPLATE_SECTOR_ROTATION,
        family_label: "Sector Rotation",
        thesis: "sector is gaining/losing relative to the broader market",
        invalidation: "individual stock diverges strongly from sector trend",
        expected_observations: &[
            "multiple stocks in the sector should move together",
            "sector ETF should confirm direction",
        ],
    },
    HypothesisTemplate {
        key: TEMPLATE_STRUCTURAL_DIFFUSION,
        family_label: "Structural Diffusion",
        thesis: "structural change may be diffusing through connected graph nodes",
        invalidation: "connected nodes absorb the move or the diffusion path breaks",
        expected_observations: &[
            "connected nodes should start repricing in sequence",
            "receiving nodes should react before the move is fully absorbed",
        ],
    },
];

// ── Public snapshot ──

#[derive(Debug, Clone)]
pub struct UsReasoningSnapshot {
    pub timestamp: OffsetDateTime,
    pub hypotheses: Vec<Hypothesis>,
    pub propagation_paths: Vec<PropagationPath>,
    pub investigation_selections: Vec<InvestigationSelection>,
    pub tactical_setups: Vec<TacticalSetup>,
    pub hypothesis_tracks: Vec<crate::ontology::reasoning::HypothesisTrack>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UsStructuralRankMetrics {
    pub composite_delta: Decimal,
    pub composite_acceleration: Decimal,
    pub capital_flow_delta: Decimal,
    pub flow_persistence: u64,
    pub flow_reversal: bool,
}

impl UsReasoningSnapshot {
    pub fn derive(
        events: &UsEventSnapshot,
        derived_signals: &UsDerivedSignalSnapshot,
        previous_setups: &[TacticalSetup],
        previous_tracks: &[crate::ontology::reasoning::HypothesisTrack],
    ) -> Self {
        Self::derive_with_policy(
            events,
            derived_signals,
            previous_setups,
            previous_tracks,
            None,
            None,
            None,
        )
    }

    pub fn derive_with_policy(
        events: &UsEventSnapshot,
        derived_signals: &UsDerivedSignalSnapshot,
        previous_setups: &[TacticalSetup],
        previous_tracks: &[crate::ontology::reasoning::HypothesisTrack],
        market_regime: Option<UsMarketRegimeBias>,
        lineage_stats: Option<&UsLineageStats>,
        structural_metrics: Option<&HashMap<Symbol, UsStructuralRankMetrics>>,
    ) -> Self {
        let propagation_paths = Vec::new();
        let hypotheses = derive_hypotheses(events, derived_signals, &propagation_paths);
        let investigation_selections = derive_investigation_selections(
            &hypotheses,
            previous_setups,
            events.timestamp,
            market_regime,
            lineage_stats,
            structural_metrics,
        );
        let tactical_setups = derive_tactical_setups(
            &hypotheses,
            &investigation_selections,
            previous_setups,
            lineage_stats,
        );
        let hypothesis_tracks = crate::pipeline::reasoning::derive_hypothesis_tracks(
            events.timestamp,
            &tactical_setups,
            previous_setups,
            previous_tracks,
        );

        Self {
            timestamp: events.timestamp,
            hypotheses,
            propagation_paths,
            investigation_selections,
            tactical_setups,
            hypothesis_tracks,
        }
    }

    pub fn derive_with_diffusion(
        events: &UsEventSnapshot,
        derived_signals: &UsDerivedSignalSnapshot,
        previous_setups: &[TacticalSetup],
        previous_tracks: &[crate::ontology::reasoning::HypothesisTrack],
        market_regime: Option<UsMarketRegimeBias>,
        lineage_stats: Option<&UsLineageStats>,
        structural_metrics: Option<&HashMap<Symbol, UsStructuralRankMetrics>>,
        graph: &UsGraph,
        cross_market_signals: &[CrossMarketSignal],
    ) -> Self {
        let propagation_paths =
            derive_diffusion_propagation_paths(graph, structural_metrics, cross_market_signals);
        let hypotheses = derive_hypotheses(events, derived_signals, &propagation_paths);
        let investigation_selections = derive_investigation_selections(
            &hypotheses,
            previous_setups,
            events.timestamp,
            market_regime,
            lineage_stats,
            structural_metrics,
        );
        let tactical_setups = derive_tactical_setups(
            &hypotheses,
            &investigation_selections,
            previous_setups,
            lineage_stats,
        );
        let hypothesis_tracks = crate::pipeline::reasoning::derive_hypothesis_tracks(
            events.timestamp,
            &tactical_setups,
            previous_setups,
            previous_tracks,
        );

        Self {
            timestamp: events.timestamp,
            hypotheses,
            propagation_paths,
            investigation_selections,
            tactical_setups,
            hypothesis_tracks,
        }
    }
}

// ── Helpers ──

fn convert_scope(scope: &UsSignalScope) -> ReasoningScope {
    match scope {
        UsSignalScope::Market => ReasoningScope::market(),
        UsSignalScope::Symbol(s) => ReasoningScope::Symbol(s.clone()),
        UsSignalScope::Sector(s) => ReasoningScope::Sector(s.clone()),
    }
}

fn scope_matches(a: &ReasoningScope, b: &ReasoningScope) -> bool {
    a == b
}

fn path_relevant_to_scope(path: &PropagationPath, scope: &ReasoningScope) -> bool {
    path.steps
        .iter()
        .any(|step| step.from == *scope || step.to == *scope)
}

fn scope_id(scope: &ReasoningScope) -> String {
    match scope {
        ReasoningScope::Market(_) => "market".into(),
        ReasoningScope::Symbol(s) => s.0.clone(),
        ReasoningScope::Sector(s) => format!("sector:{}", s),
        ReasoningScope::Institution(s) => format!("inst:{}", s),
        ReasoningScope::Theme(s) => format!("theme:{}", s),
        ReasoningScope::Region(s) => format!("region:{}", s),
        ReasoningScope::Custom(s) => format!("custom:{}", s),
    }
}

fn scope_label(scope: &ReasoningScope) -> String {
    match scope {
        ReasoningScope::Market(_) => "US market".into(),
        ReasoningScope::Symbol(s) => s.0.clone(),
        ReasoningScope::Sector(s) => format!("sector {}", s),
        ReasoningScope::Institution(s) => s.to_string(),
        ReasoningScope::Theme(s) => s.to_string(),
        ReasoningScope::Region(s) => s.to_string(),
        ReasoningScope::Custom(s) => s.to_string(),
    }
}

struct EvidenceSummary {
    local_support: Decimal,
    local_contradict: Decimal,
    propagated_support: Decimal,
    propagated_contradict: Decimal,
}

fn summarize_evidence(evidence: &[ReasoningEvidence]) -> EvidenceSummary {
    let mut summary = EvidenceSummary {
        local_support: Decimal::ZERO,
        local_contradict: Decimal::ZERO,
        propagated_support: Decimal::ZERO,
        propagated_contradict: Decimal::ZERO,
    };
    for item in evidence {
        match (item.polarity, item.kind) {
            (EvidencePolarity::Supports, ReasoningEvidenceKind::PropagatedPath) => {
                summary.propagated_support += item.weight;
            }
            (EvidencePolarity::Contradicts, ReasoningEvidenceKind::PropagatedPath) => {
                summary.propagated_contradict += item.weight;
            }
            (EvidencePolarity::Supports, _) => {
                summary.local_support += item.weight;
            }
            (EvidencePolarity::Contradicts, _) => {
                summary.local_contradict += item.weight;
            }
        }
    }
    summary
}

fn competing_confidence(evidence: &[ReasoningEvidence]) -> Decimal {
    // Same formula as HK: (support - contradict) / total, mapped to [0, 1].
    // No artificial prior — confidence is purely data-driven.
    // Differentiation comes from confidence_gap between competing hypotheses.
    let summary = summarize_evidence(evidence);
    let total_support = summary.local_support + summary.propagated_support;
    let total_contradict = summary.local_contradict + summary.propagated_contradict;
    let total = total_support + total_contradict;
    if total == Decimal::ZERO {
        return Decimal::ZERO;
    }
    (((total_support - total_contradict) / total + Decimal::ONE) / Decimal::TWO)
        .clamp(Decimal::ZERO, Decimal::ONE)
}

// ── Tests ──

#[cfg(test)]
#[path = "reasoning_tests.rs"]
mod tests;
