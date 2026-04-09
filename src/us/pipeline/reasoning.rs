use std::collections::HashMap;

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::{
    EvidencePolarity, Hypothesis, InvestigationSelection, PropagationPath, ReasoningEvidence,
    ReasoningEvidenceKind, ReasoningScope, TacticalSetup,
};
use crate::pipeline::reasoning::ReviewerDoctrinePressure;
use crate::us::graph::decision::UsMarketRegimeBias;
use crate::us::graph::graph::UsGraph;
use crate::us::graph::propagation::CrossMarketSignal;
use crate::us::temporal::lineage::{
    us_topology_hypothesis_matches_pattern, UsConvergenceSuccessPattern, UsLineageStats,
};

use super::signals::{UsDerivedSignalSnapshot, UsEventSnapshot, UsSignalScope};

#[path = "reasoning/policy.rs"]
mod policy;
#[path = "reasoning/propagation.rs"]
mod propagation;
#[path = "reasoning/support.rs"]
mod support;
#[path = "reasoning/synthesis.rs"]
mod synthesis;
#[path = "reasoning/vortex.rs"]
mod vortex;
use policy::{derive_investigation_selections, derive_tactical_setups, prune_us_stale_cases};
use propagation::derive_diffusion_propagation_paths;
use synthesis::derive_hypotheses;

// ── Hypothesis template keys ──

const TEMPLATE_PRE_MARKET_POSITIONING: &str = "pre_market_positioning";
const TEMPLATE_CROSS_MARKET_ARBITRAGE: &str = "cross_market_arbitrage";
const TEMPLATE_MOMENTUM_CONTINUATION: &str = "momentum_continuation";
const TEMPLATE_SECTOR_ROTATION: &str = "sector_rotation";
const TEMPLATE_STRUCTURAL_DIFFUSION: &str = "structural_diffusion";
const TEMPLATE_CROSS_MARKET_DIFFUSION: &str = "cross_market_diffusion";
const TEMPLATE_SECTOR_DIFFUSION: &str = "sector_diffusion";
const TEMPLATE_PEER_RELAY: &str = "peer_relay";
const TEMPLATE_CROSS_MECHANISM_CHAIN: &str = "cross_mechanism_chain";
const TEMPLATE_CATALYST_REPRICING: &str = "catalyst_repricing";

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
    HypothesisTemplate {
        key: TEMPLATE_CROSS_MARKET_DIFFUSION,
        family_label: "Cross-Market Diffusion",
        thesis: "cross-market lead-lag pressure may still be diffusing into US names",
        invalidation: "HK lead decays without US follow-through or the linkage snaps back",
        expected_observations: &[
            "linked US names should reprice in the same direction with a lag",
            "cross-market spread should compress as diffusion matures",
        ],
    },
    HypothesisTemplate {
        key: TEMPLATE_SECTOR_DIFFUSION,
        family_label: "Sector Diffusion Chain",
        thesis: "sector leadership may be propagating through connected members",
        invalidation: "sector leaders stall before followers confirm the move",
        expected_observations: &[
            "leaders should move first and sector peers should confirm sequentially",
            "sector impulse should broaden rather than stay isolated",
        ],
    },
    HypothesisTemplate {
        key: TEMPLATE_PEER_RELAY,
        family_label: "Peer Relay",
        thesis: "a peer-led move may be relaying through adjacent names before broad recognition",
        invalidation: "peer reinforcement fades or the relay stalls after the first hop",
        expected_observations: &[
            "adjacent peers should react in sequence rather than all at once",
            "the relay leader should stay ahead of second-order names",
        ],
    },
    HypothesisTemplate {
        key: TEMPLATE_CROSS_MECHANISM_CHAIN,
        family_label: "Cross-Mechanism Chain",
        thesis: "multiple transmission mechanisms may be converging into one live setup",
        invalidation: "one leg of the chain breaks and the multi-hop structure collapses",
        expected_observations: &[
            "different mechanisms should confirm the same directional story",
            "later hops should strengthen rather than dilute the original move",
        ],
    },
    HypothesisTemplate {
        key: TEMPLATE_CATALYST_REPRICING,
        family_label: "Catalyst Repricing",
        thesis: "a live catalyst may still be driving repricing in this scope",
        invalidation: "the catalyst fades before local price or flow confirms it",
        expected_observations: &[
            "flow or price should begin aligning with the catalyst direction",
            "linked symbols or sectors should start confirming the same theme",
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
            1,
            previous_setups,
            previous_tracks,
            None,
            None,
            None,
            None,
            None,
            None,
        )
    }

    pub fn derive_with_policy(
        events: &UsEventSnapshot,
        derived_signals: &UsDerivedSignalSnapshot,
        tick_number: u64,
        previous_setups: &[TacticalSetup],
        previous_tracks: &[crate::ontology::reasoning::HypothesisTrack],
        market_regime: Option<UsMarketRegimeBias>,
        lineage_stats: Option<&UsLineageStats>,
        multi_horizon_gate: Option<&crate::temporal::lineage::MultiHorizonGate>,
        structural_metrics: Option<&HashMap<Symbol, UsStructuralRankMetrics>>,
        convergence_scores: Option<
            &HashMap<Symbol, crate::us::graph::decision::UsConvergenceScore>,
        >,
        reviewer_doctrine: Option<&ReviewerDoctrinePressure>,
    ) -> Self {
        let propagation_paths = Vec::new();
        let family_gate = lineage_stats.map(|stats| {
            crate::pipeline::reasoning::family_gate::FamilyAlphaGate::from_us_lineage_stats(
                &stats.by_template,
            )
        });
        let hypotheses = derive_hypotheses(
            events,
            derived_signals,
            &propagation_paths,
            family_gate.as_ref(),
        );
        let investigation_selections = derive_investigation_selections(
            &hypotheses,
            tick_number,
            previous_setups,
            events.timestamp,
            market_regime,
            lineage_stats,
            multi_horizon_gate,
            structural_metrics,
            reviewer_doctrine,
        );
        let tactical_setups = derive_tactical_setups(
            &hypotheses,
            &investigation_selections,
            previous_setups,
            lineage_stats,
            convergence_scores,
        );
        let tactical_setups =
            prune_us_stale_cases(tactical_setups, previous_setups, previous_tracks);
        let tactical_setups = crate::pipeline::reasoning::apply_convergence_policy(tactical_setups);
        let tactical_setups = crate::pipeline::reasoning::cap_observe_budget(tactical_setups);
        let tactical_setups = crate::pipeline::reasoning::apply_midflight_health_check(
            tactical_setups,
            previous_tracks,
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
        tick_number: u64,
        previous_setups: &[TacticalSetup],
        previous_tracks: &[crate::ontology::reasoning::HypothesisTrack],
        market_regime: Option<UsMarketRegimeBias>,
        lineage_stats: Option<&UsLineageStats>,
        multi_horizon_gate: Option<&crate::temporal::lineage::MultiHorizonGate>,
        structural_metrics: Option<&HashMap<Symbol, UsStructuralRankMetrics>>,
        convergence_scores: Option<
            &HashMap<Symbol, crate::us::graph::decision::UsConvergenceScore>,
        >,
        graph: &UsGraph,
        cross_market_signals: &[CrossMarketSignal],
        reviewer_doctrine: Option<&ReviewerDoctrinePressure>,
    ) -> Self {
        let propagation_paths =
            derive_diffusion_propagation_paths(graph, structural_metrics, cross_market_signals);
        let family_gate = lineage_stats.map(|stats| {
            crate::pipeline::reasoning::family_gate::FamilyAlphaGate::from_us_lineage_stats(
                &stats.by_template,
            )
        });
        let hypotheses = derive_hypotheses(
            events,
            derived_signals,
            &propagation_paths,
            family_gate.as_ref(),
        );
        let investigation_selections = derive_investigation_selections(
            &hypotheses,
            tick_number,
            previous_setups,
            events.timestamp,
            market_regime,
            lineage_stats,
            multi_horizon_gate,
            structural_metrics,
            reviewer_doctrine,
        );
        let tactical_setups = derive_tactical_setups(
            &hypotheses,
            &investigation_selections,
            previous_setups,
            lineage_stats,
            convergence_scores,
        );
        let tactical_setups =
            prune_us_stale_cases(tactical_setups, previous_setups, previous_tracks);
        let tactical_setups = crate::pipeline::reasoning::apply_convergence_policy(tactical_setups);
        let tactical_setups = crate::pipeline::reasoning::cap_observe_budget(tactical_setups);
        let tactical_setups = crate::pipeline::reasoning::apply_midflight_health_check(
            tactical_setups,
            previous_tracks,
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

#[allow(clippy::too_many_arguments)]
pub fn apply_us_convergence_success_pattern_feedback(
    snapshot: &mut UsReasoningSnapshot,
    tick_number: u64,
    previous_setups: &[TacticalSetup],
    previous_tracks: &[crate::ontology::reasoning::HypothesisTrack],
    market_regime: Option<UsMarketRegimeBias>,
    lineage_stats: Option<&UsLineageStats>,
    multi_horizon_gate: Option<&crate::temporal::lineage::MultiHorizonGate>,
    structural_metrics: Option<&HashMap<Symbol, UsStructuralRankMetrics>>,
    reviewer_doctrine: Option<&ReviewerDoctrinePressure>,
    patterns: &[UsConvergenceSuccessPattern],
    convergence_scores: Option<&HashMap<Symbol, crate::us::graph::decision::UsConvergenceScore>>,
) -> bool {
    let mut boosted = HashMap::<String, (Decimal, String)>::new();

    for hypothesis in &mut snapshot.hypotheses {
        let Some(pattern) = patterns
            .iter()
            .filter(|pattern| {
                pattern.top_family == "Convergence Hypothesis"
                    || pattern.top_family == "Latent Vortex"
            })
            .filter(|pattern| us_topology_hypothesis_matches_pattern(hypothesis, pattern))
            .max_by(|left, right| {
                left.mean_net_return
                    .cmp(&right.mean_net_return)
                    .then_with(|| left.samples.cmp(&right.samples))
                    .then_with(|| left.mean_strength.cmp(&right.mean_strength))
            })
        else {
            continue;
        };
        let boost = learned_us_convergence_boost(pattern, hypothesis);
        if boost <= Decimal::ZERO {
            continue;
        }

        hypothesis.confidence = (hypothesis.confidence + boost)
            .clamp(Decimal::ZERO, Decimal::ONE)
            .round_dp(4);
        hypothesis.provenance = hypothesis
            .provenance
            .clone()
            .with_confidence(hypothesis.confidence)
            .with_note(append_us_note(
                hypothesis.provenance.note.as_deref(),
                format!(
                    "learned_convergence_boost={}; matched_success_pattern={}",
                    boost.round_dp(4),
                    pattern.channel_signature
                ),
            ));
        boosted.insert(
            hypothesis.hypothesis_id.clone(),
            (boost, pattern.channel_signature.clone()),
        );
    }

    if boosted.is_empty() {
        return false;
    }

    let mut investigation_selections = derive_investigation_selections(
        &snapshot.hypotheses,
        tick_number,
        previous_setups,
        snapshot.timestamp,
        market_regime,
        lineage_stats,
        multi_horizon_gate,
        structural_metrics,
        reviewer_doctrine,
    );
    for selection in &mut investigation_selections {
        if let Some((boost, signature)) = boosted.get(&selection.hypothesis_id) {
            selection.priority_score += *boost;
            selection
                .notes
                .push(format!("learned_convergence_boost={}", boost.round_dp(4)));
            selection
                .notes
                .push(format!("matched_success_pattern={}", signature));
        }
    }

    let tactical_setups = derive_tactical_setups(
        &snapshot.hypotheses,
        &investigation_selections,
        previous_setups,
        lineage_stats,
        convergence_scores,
    );
    let tactical_setups = prune_us_stale_cases(tactical_setups, previous_setups, previous_tracks);
    let tactical_setups = crate::pipeline::reasoning::cap_observe_budget(tactical_setups);
    let mut tactical_setups =
        crate::pipeline::reasoning::apply_midflight_health_check(tactical_setups, previous_tracks);
    for setup in &mut tactical_setups {
        if let Some((boost, signature)) = boosted.get(&setup.hypothesis_id) {
            setup.confidence = (setup.confidence + *boost)
                .clamp(Decimal::ZERO, Decimal::ONE)
                .round_dp(4);
            setup
                .risk_notes
                .push(format!("learned_convergence_boost={}", boost.round_dp(4)));
            setup
                .risk_notes
                .push(format!("matched_success_pattern={}", signature));
        }
    }
    let hypothesis_tracks = crate::pipeline::reasoning::derive_hypothesis_tracks(
        snapshot.timestamp,
        &tactical_setups,
        previous_setups,
        previous_tracks,
    );

    snapshot.investigation_selections = investigation_selections;
    snapshot.tactical_setups = tactical_setups;
    snapshot.hypothesis_tracks = hypothesis_tracks;
    true
}

fn learned_us_convergence_boost(
    pattern: &UsConvergenceSuccessPattern,
    hypothesis: &Hypothesis,
) -> Decimal {
    let note = hypothesis.provenance.note.as_deref().unwrap_or_default();
    let strength =
        extract_us_note_decimal(note, "vortex_strength").unwrap_or(hypothesis.confidence);
    let coherence = extract_us_note_decimal(note, "coherence").unwrap_or(hypothesis.confidence);
    let return_bonus =
        (pattern.mean_net_return.max(Decimal::ZERO) * Decimal::TWO).min(Decimal::new(8, 2));
    let sample_bonus = Decimal::from(pattern.samples.min(3) as i64) * Decimal::new(1, 2);
    let structural_bonus = ((strength + coherence) / Decimal::TWO) * Decimal::new(2, 2);

    (return_bonus + sample_bonus + structural_bonus)
        .clamp(Decimal::ZERO, Decimal::new(12, 2))
        .round_dp(4)
}

fn extract_us_note_decimal(note: &str, key: &str) -> Option<Decimal> {
    note.split(';').map(str::trim).find_map(|part| {
        let (found_key, value) = part.split_once('=')?;
        (found_key.trim() == key)
            .then(|| value.trim().parse::<Decimal>().ok())
            .flatten()
    })
}

fn append_us_note(existing: Option<&str>, next: String) -> String {
    match existing {
        Some(existing) if !existing.is_empty() => format!("{existing}; {next}"),
        _ => next,
    }
}

// ── Tests ──

#[cfg(test)]
#[path = "reasoning_tests.rs"]
mod tests;
