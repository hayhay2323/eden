use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::graph::decision::DecisionSnapshot;
use crate::graph::graph::BrainGraph;
use crate::graph::insights::GraphInsights;
use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::{
    CaseCluster, Hypothesis, HypothesisTrack, InvestigationSelection, PropagationPath,
    ReviewReasonCode, TacticalSetup,
};
use crate::ontology::world::{
    BackwardInvestigation, BackwardReasoningSnapshot, CausalContestState, WorldStateSnapshot,
};
use crate::pipeline::dimensions::SymbolDimensions;
use crate::temporal::lineage::{FamilyContextLineageOutcome, VortexSuccessPattern};

use super::signals::{DerivedSignalSnapshot, EventSnapshot};

#[path = "reasoning/clustering.rs"]
mod clustering;
#[path = "reasoning/context.rs"]
mod context;
#[path = "reasoning/family_gate.rs"]
pub(crate) mod family_gate;
#[path = "reasoning/policy.rs"]
mod policy;
#[path = "reasoning/propagation.rs"]
mod propagation;
#[path = "reasoning/support.rs"]
mod support;
#[path = "reasoning/synthesis.rs"]
mod synthesis;
pub(crate) use clustering::derive_case_clusters;
pub use context::{AbsenceMemory, ConvergenceDetail, FamilyBoostLedger, ReasoningContext};
pub use family_gate::templates_from_candidate_mechanisms;
pub(crate) use policy::apply_convergence_policy;
pub(crate) use policy::apply_midflight_health_check;
pub use policy::derive_hypothesis_tracks;
pub(crate) use policy::ReviewerDoctrinePressure;
use policy::{apply_case_budget, apply_track_action_policy, prune_stale_tactical_setups};
use propagation::{derive_diffusion_propagation_paths, derive_propagation_paths};
pub use propagation::{mechanism_family, path_has_family, path_is_mixed_multi_hop};
pub use support::HypothesisTemplate;
pub(crate) use support::hk_session_label;
use family_gate::FamilyAlphaGate;
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
        let empty_convergence = HashMap::new();
        let absence_memory = AbsenceMemory::default();
        let family_boost = FamilyBoostLedger::default();
        let ctx = ReasoningContext {
            lineage_priors: &[],
            multi_horizon_gate: None,
            symbol_dimensions: None,
            reviewer_doctrine: None,
            convergence_components: &empty_convergence,
            market_regime: &decision.market_regime,
            world_state: None,
            absence_memory: &absence_memory,
            family_boost: &family_boost,
        };
        Self::derive_with_policy(
            events,
            derived_signals,
            insights,
            decision,
            previous_setups,
            previous_tracks,
            &ctx,
        )
    }

    pub fn derive_with_policy(
        events: &EventSnapshot,
        derived_signals: &DerivedSignalSnapshot,
        insights: &GraphInsights,
        decision: &DecisionSnapshot,
        previous_setups: &[TacticalSetup],
        previous_tracks: &[HypothesisTrack],
        ctx: &ReasoningContext<'_>,
    ) -> Self {
        let propagation_paths = derive_propagation_paths(insights, decision.timestamp);
        let family_gate = (!ctx.lineage_priors.is_empty()).then(|| {
            FamilyAlphaGate::from_lineage_priors(
                ctx.lineage_priors,
                hk_session_label(events.timestamp),
                ctx.market_regime.bias.as_str(),
            )
        });
        let hypotheses = derive_hypotheses(
            events,
            derived_signals,
            &propagation_paths,
            family_gate.as_ref(),
            ctx.absence_memory,
            ctx.world_state,
        );
        let mut investigation_selections = derive_investigation_selections(decision, &hypotheses);
        let baseline_setups = derive_tactical_setups(
            decision,
            &hypotheses,
            &investigation_selections,
            synthesis::SetupSupportContext {
                events,
                insights,
                symbol_dimensions: ctx.symbol_dimensions,
                convergence_components: &decision.convergence_scores,
            },
        );
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
            ctx.market_regime,
            ctx.lineage_priors,
            ctx.multi_horizon_gate,
            ctx.reviewer_doctrine,
            ctx.family_boost,
        );
        let tactical_setups = apply_convergence_policy(tactical_setups);
        let tactical_setups = apply_case_budget(tactical_setups, &baseline_tracks, previous_tracks);
        let tactical_setups = prune_stale_tactical_setups(tactical_setups, previous_tracks);
        let tactical_setups = cap_observe_budget(tactical_setups);
        let absence_sectors = propagation_absence_sectors(events);
        let tactical_setups =
            policy::demote_on_propagation_absence(tactical_setups, &absence_sectors);
        let tactical_setups = apply_midflight_health_check(tactical_setups, previous_tracks);
        sync_investigation_selections_from_setups(&mut investigation_selections, &tactical_setups);
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
        insights: &GraphInsights,
        decision: &DecisionSnapshot,
        previous_setups: &[TacticalSetup],
        previous_tracks: &[HypothesisTrack],
        ctx: &ReasoningContext<'_>,
        brain: &BrainGraph,
        stock_deltas: &HashMap<Symbol, Decimal>,
    ) -> Self {
        let propagation_paths =
            derive_diffusion_propagation_paths(brain, stock_deltas, decision.timestamp);
        let family_gate = (!ctx.lineage_priors.is_empty()).then(|| {
            FamilyAlphaGate::from_lineage_priors(
                ctx.lineage_priors,
                hk_session_label(events.timestamp),
                ctx.market_regime.bias.as_str(),
            )
        });
        let hypotheses = derive_hypotheses(
            events,
            derived_signals,
            &propagation_paths,
            family_gate.as_ref(),
            ctx.absence_memory,
            ctx.world_state,
        );
        let mut investigation_selections = derive_investigation_selections(decision, &hypotheses);
        let baseline_setups = derive_tactical_setups(
            decision,
            &hypotheses,
            &investigation_selections,
            synthesis::SetupSupportContext {
                events,
                insights,
                symbol_dimensions: ctx.symbol_dimensions,
                convergence_components: &decision.convergence_scores,
            },
        );
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
            ctx.market_regime,
            ctx.lineage_priors,
            ctx.multi_horizon_gate,
            ctx.reviewer_doctrine,
            ctx.family_boost,
        );
        let tactical_setups = apply_convergence_policy(tactical_setups);
        let tactical_setups = apply_case_budget(tactical_setups, &baseline_tracks, previous_tracks);
        let tactical_setups = prune_stale_tactical_setups(tactical_setups, previous_tracks);
        let tactical_setups = cap_observe_budget(tactical_setups);
        let absence_sectors = propagation_absence_sectors(events);
        let tactical_setups =
            policy::demote_on_propagation_absence(tactical_setups, &absence_sectors);
        let tactical_setups = apply_midflight_health_check(tactical_setups, previous_tracks);
        sync_investigation_selections_from_setups(&mut investigation_selections, &tactical_setups);
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

const MAX_OBSERVE_SETUPS: usize = 20;

pub(crate) fn cap_observe_budget(mut setups: Vec<TacticalSetup>) -> Vec<TacticalSetup> {
    let non_observe_count = setups.iter().filter(|s| s.action != "observe").count();
    let observe_count = setups.len() - non_observe_count;
    if observe_count <= MAX_OBSERVE_SETUPS {
        return setups;
    }

    // Partition: keep all non-observe, rank observe by quality
    let mut non_observe = Vec::with_capacity(non_observe_count);
    let mut observe = Vec::with_capacity(observe_count);
    for setup in setups.drain(..) {
        if setup.action == "observe" {
            observe.push(setup);
        } else {
            non_observe.push(setup);
        }
    }

    // Sort observe by quality: higher edge first, then higher gap
    observe.sort_by(|a, b| {
        b.heuristic_edge
            .cmp(&a.heuristic_edge)
            .then_with(|| b.confidence_gap.cmp(&a.confidence_gap))
    });
    observe.truncate(MAX_OBSERVE_SETUPS);

    non_observe.extend(observe);
    non_observe
}

pub(crate) fn propagation_absence_sectors(events: &EventSnapshot) -> Vec<crate::ontology::objects::SectorId> {
    use crate::pipeline::signals::{MarketEventKind, SignalScope};

    events
        .events
        .iter()
        .filter(|ev| ev.value.kind == MarketEventKind::PropagationAbsence)
        .filter_map(|ev| match &ev.value.scope {
            SignalScope::Sector(sector_id) => Some(sector_id.clone()),
            _ => None,
        })
        .collect()
}

pub fn apply_backward_confirmation_gate(
    reasoning_snapshot: &mut ReasoningSnapshot,
    previous_setups: &[TacticalSetup],
    previous_tracks: &[HypothesisTrack],
    backward_reasoning: &BackwardReasoningSnapshot,
) -> bool {
    let backward_by_symbol = backward_reasoning
        .investigations
        .iter()
        .filter_map(|investigation| match &investigation.leaf_scope {
            crate::ontology::ReasoningScope::Symbol(symbol) => {
                Some((symbol.clone(), investigation))
            }
            _ => None,
        })
        .collect::<HashMap<_, _>>();
    let mut changed = false;

    for setup in &mut reasoning_snapshot.tactical_setups {
        if setup.action != "enter" || setup.workflow_id.is_none() {
            continue;
        }
        let crate::ontology::ReasoningScope::Symbol(symbol) = &setup.scope else {
            continue;
        };

        let gated = backward_by_symbol
            .get(symbol)
            .map(|investigation| backward_gate_reason(setup, investigation))
            .unwrap_or_else(|| {
                Some((
                    ReviewReasonCode::BackwardMissing,
                    "no backward investigation is available to confirm this enter case".into(),
                ))
            });
        if let Some((review_reason_code, reason)) = gated {
            demote_matching_investigations(
                &mut reasoning_snapshot.investigation_selections,
                &setup.scope,
                review_reason_code,
                &reason,
            );
            demote_setup_for_backward_confirmation(setup, review_reason_code, &reason);
            changed = true;
        }
    }

    if changed {
        sync_investigation_selections_from_setups(
            &mut reasoning_snapshot.investigation_selections,
            &reasoning_snapshot.tactical_setups,
        );
        reasoning_snapshot.hypothesis_tracks = derive_hypothesis_tracks(
            reasoning_snapshot.timestamp,
            &reasoning_snapshot.tactical_setups,
            previous_setups,
            previous_tracks,
        );
        reasoning_snapshot.case_clusters = derive_case_clusters(
            &reasoning_snapshot.hypotheses,
            &reasoning_snapshot.propagation_paths,
            &reasoning_snapshot.tactical_setups,
            &reasoning_snapshot.hypothesis_tracks,
        );
    }
    changed
}

#[allow(clippy::too_many_arguments)]
pub fn apply_vortex_success_pattern_feedback(
    reasoning_snapshot: &mut ReasoningSnapshot,
    decision: &DecisionSnapshot,
    events: &EventSnapshot,
    insights: &GraphInsights,
    previous_setups: &[TacticalSetup],
    previous_tracks: &[HypothesisTrack],
    lineage_priors: &[FamilyContextLineageOutcome],
    multi_horizon_gate: Option<&crate::temporal::lineage::MultiHorizonGate>,
    symbol_dimensions: Option<&HashMap<Symbol, SymbolDimensions>>,
    reviewer_doctrine: Option<&ReviewerDoctrinePressure>,
    patterns: &[VortexSuccessPattern],
    world_state: &WorldStateSnapshot,
) -> bool {
    let boost_by_scope = world_state
        .vortices
        .iter()
        .filter_map(|vortex| {
            let best_pattern = patterns
                .iter()
                .filter(|pattern| pattern.top_family == "Convergence Hypothesis")
                .filter(|pattern| {
                    crate::temporal::lineage::vortex_matches_success_pattern(vortex, pattern)
                })
                .max_by(|left, right| {
                    left.mean_net_return
                        .cmp(&right.mean_net_return)
                        .then_with(|| left.samples.cmp(&right.samples))
                        .then_with(|| left.mean_strength.cmp(&right.mean_strength))
                })?;
            Some((
                vortex.center_scope.clone(),
                learned_vortex_confidence_boost(best_pattern, vortex),
            ))
        })
        .filter(|(_, boost)| *boost > Decimal::ZERO)
        .collect::<HashMap<_, _>>();

    if boost_by_scope.is_empty() {
        return false;
    }

    let matched_signature_by_scope = world_state
        .vortices
        .iter()
        .filter_map(|vortex| {
            patterns
                .iter()
                .filter(|pattern| pattern.top_family == "Convergence Hypothesis")
                .filter(|pattern| {
                    crate::temporal::lineage::vortex_matches_success_pattern(vortex, pattern)
                })
                .max_by(|left, right| {
                    left.mean_net_return
                        .cmp(&right.mean_net_return)
                        .then_with(|| left.samples.cmp(&right.samples))
                        .then_with(|| left.mean_strength.cmp(&right.mean_strength))
                })
                .map(|pattern| (vortex.center_scope.clone(), pattern.channel_signature.clone()))
        })
        .collect::<HashMap<_, _>>();

    let mut boosted_hypotheses = HashMap::<String, (Decimal, String)>::new();
    for hypothesis in &mut reasoning_snapshot.hypotheses {
        let Some(boost) = boost_by_scope.get(&hypothesis.scope).copied() else {
            continue;
        };
        if hypothesis.family_key != "convergence_hypothesis" {
            continue;
        }
        let matched_signature = matched_signature_by_scope
            .get(&hypothesis.scope)
            .cloned()
            .unwrap_or_default();

        hypothesis.confidence = (hypothesis.confidence + boost)
            .clamp(Decimal::ZERO, Decimal::ONE)
            .round_dp(4);
        hypothesis.provenance = hypothesis
            .provenance
            .clone()
            .with_confidence(hypothesis.confidence)
            .with_note(append_note(
                hypothesis.provenance.note.as_deref(),
                format!(
                    "learned_vortex_boost={}; matched_success_pattern={}",
                    boost.round_dp(4),
                    matched_signature
                ),
            ));
        boosted_hypotheses.insert(hypothesis.hypothesis_id.clone(), (boost, matched_signature));
    }

    if boosted_hypotheses.is_empty() {
        return false;
    }

    let mut investigation_selections =
        derive_investigation_selections(decision, &reasoning_snapshot.hypotheses);
    for selection in &mut investigation_selections {
        if let Some((boost, signature)) = boosted_hypotheses.get(&selection.hypothesis_id) {
            selection.priority_score += *boost;
            selection
                .notes
                .push(format!("learned_vortex_boost={}", boost.round_dp(4)));
            selection
                .notes
                .push(format!("matched_success_pattern={}", signature));
        }
    }

    let baseline_setups = derive_tactical_setups(
        decision,
        &reasoning_snapshot.hypotheses,
        &investigation_selections,
        synthesis::SetupSupportContext {
            events,
            insights,
            symbol_dimensions,
            convergence_components: &decision.convergence_scores,
        },
    );
    let baseline_tracks = derive_hypothesis_tracks(
        reasoning_snapshot.timestamp,
        &baseline_setups,
        previous_setups,
        previous_tracks,
    );
    let tactical_setups = apply_track_action_policy(
        &baseline_setups,
        &baseline_tracks,
        previous_tracks,
        reasoning_snapshot.timestamp,
        &decision.market_regime,
        lineage_priors,
        multi_horizon_gate,
        reviewer_doctrine,
        &FamilyBoostLedger::default(),
    );
    let tactical_setups = apply_convergence_policy(tactical_setups);
    let tactical_setups = apply_case_budget(tactical_setups, &baseline_tracks, previous_tracks);
    let tactical_setups = prune_stale_tactical_setups(tactical_setups, previous_tracks);
    let tactical_setups = cap_observe_budget(tactical_setups);
    let absence_sectors = propagation_absence_sectors(events);
    let mut tactical_setups =
        policy::demote_on_propagation_absence(tactical_setups, &absence_sectors);
    tactical_setups = apply_midflight_health_check(tactical_setups, previous_tracks);
    for setup in &mut tactical_setups {
        if let Some((boost, signature)) = boosted_hypotheses.get(&setup.hypothesis_id) {
            setup.confidence = (setup.confidence + *boost)
                .clamp(Decimal::ZERO, Decimal::ONE)
                .round_dp(4);
            setup
                .risk_notes
                .push(format!("learned_vortex_boost={}", boost.round_dp(4)));
            setup
                .risk_notes
                .push(format!("matched_success_pattern={}", signature));
        }
    }
    sync_investigation_selections_from_setups(&mut investigation_selections, &tactical_setups);
    let hypothesis_tracks = derive_hypothesis_tracks(
        reasoning_snapshot.timestamp,
        &tactical_setups,
        previous_setups,
        previous_tracks,
    );
    let case_clusters = derive_case_clusters(
        &reasoning_snapshot.hypotheses,
        &reasoning_snapshot.propagation_paths,
        &tactical_setups,
        &hypothesis_tracks,
    );

    reasoning_snapshot.investigation_selections = investigation_selections;
    reasoning_snapshot.tactical_setups = tactical_setups;
    reasoning_snapshot.hypothesis_tracks = hypothesis_tracks;
    reasoning_snapshot.case_clusters = case_clusters;
    true
}

fn sync_investigation_selections_from_setups(
    selections: &mut [InvestigationSelection],
    setups: &[TacticalSetup],
) {
    let setup_by_scope = setups
        .iter()
        .map(|setup| (&setup.scope, setup))
        .collect::<HashMap<_, _>>();

    for selection in selections {
        let Some(setup) = setup_by_scope.get(&selection.scope) else {
            continue;
        };
        if selection.attention_hint != setup.action {
            selection.notes.insert(
                0,
                format!(
                    "policy_sync_transition={} -> {}",
                    selection.attention_hint, setup.action
                ),
            );
            selection.attention_hint = setup.action.clone();
        }
        selection.review_reason_code = setup.review_reason_code;
        if let Some(code) = setup.review_reason_code {
            selection
                .notes
                .insert(0, format!("review_reason_code={}", code.as_str()));
        }
        if let Some(verdict) = setup.policy_verdict.as_ref() {
            selection.rationale = verdict.rationale.clone();
        } else if setup.action == "enter" {
            selection.rationale = setup.entry_rationale.clone();
        }
    }
}

fn learned_vortex_confidence_boost(
    pattern: &VortexSuccessPattern,
    vortex: &crate::ontology::world::Vortex,
) -> Decimal {
    let return_bonus =
        (pattern.mean_net_return.max(Decimal::ZERO) * Decimal::TWO).min(Decimal::new(8, 2));
    let sample_bonus = Decimal::from(pattern.samples.min(3) as i64) * Decimal::new(1, 2);
    let structure_bonus =
        ((vortex.strength + vortex.coherence) / Decimal::TWO) * Decimal::new(2, 2);

    (return_bonus + sample_bonus + structure_bonus)
        .clamp(Decimal::ZERO, Decimal::new(12, 2))
        .round_dp(4)
}

fn append_note(existing: Option<&str>, next: String) -> String {
    match existing {
        Some(existing) if !existing.is_empty() => format!("{existing}; {next}"),
        _ => next,
    }
}

fn backward_gate_reason(
    setup: &TacticalSetup,
    investigation: &BackwardInvestigation,
) -> Option<(ReviewReasonCode, String)> {
    if direction_conflicts_with_label(setup, &investigation.leaf_label) {
        return Some((
            ReviewReasonCode::BackwardDirectionConflict,
            format!(
                "backward leaf {} no longer matches the setup direction",
                investigation.leaf_label
            ),
        ));
    }
    if matches!(
        investigation.contest_state,
        CausalContestState::Contested | CausalContestState::Eroding | CausalContestState::Flipped
    ) {
        return Some((
            ReviewReasonCode::BackwardContested,
            format!(
                "backward contest state {} cannot sustain enter",
                investigation.contest_state
            ),
        ));
    }

    let Some(leading_cause) = investigation.leading_cause.as_ref() else {
        return Some((
            ReviewReasonCode::BackwardMissing,
            "backward investigation has no leading cause".into(),
        ));
    };
    if leading_cause.net_conviction < Decimal::new(10, 2) {
        return Some((
            ReviewReasonCode::BackwardWeakConviction,
            format!(
                "backward leading cause conviction is too weak ({})",
                leading_cause.net_conviction.round_dp(3)
            ),
        ));
    }
    if investigation
        .cause_gap
        .is_some_and(|gap| gap < Decimal::new(5, 2))
    {
        return Some((
            ReviewReasonCode::BackwardNarrowGap,
            format!(
                "backward cause gap is too narrow ({})",
                investigation.cause_gap.unwrap_or_default().round_dp(3)
            ),
        ));
    }

    None
}

fn direction_conflicts_with_label(setup: &TacticalSetup, leaf_label: &str) -> bool {
    let setup_is_long = setup.title.starts_with("Long ");
    let setup_is_short = setup.title.starts_with("Short ");
    let label_is_long = leaf_label.starts_with("Long ");
    let label_is_short = leaf_label.starts_with("Short ");

    (setup_is_long && label_is_short) || (setup_is_short && label_is_long)
}

fn demote_setup_for_backward_confirmation(
    setup: &mut TacticalSetup,
    review_reason_code: ReviewReasonCode,
    reason: &str,
) {
    if setup.action == "review" {
        return;
    }
    let previous_action = setup.action.clone();
    setup.action = "review".into();
    setup.review_reason_code = Some(review_reason_code);
    let mut provenance = setup
        .provenance
        .clone()
        .with_trace_id(setup.setup_id.clone());
    provenance.note = Some(reason.to_string());
    setup.provenance = provenance;
    setup.lineage.blocked_by.push(format!(
        "backward_confirmation {} -> review because {}",
        previous_action, reason
    ));
    setup
        .risk_notes
        .insert(0, format!("policy_gate: {}", reason));
    setup.risk_notes.insert(
        0,
        format!(
            "policy_transition: downgraded from {} to review because {}",
            previous_action, reason
        ),
    );
    if let Some(verdict) = setup.policy_verdict.as_mut() {
        verdict.primary = crate::ontology::PolicyVerdictKind::ReviewRequired;
        verdict.rationale = reason.to_string();
        verdict.review_reason_code = Some(review_reason_code);
        verdict.conflict_reason =
            Some("backward confirmation could not sustain an enter-ready case".into());
    }
}

fn demote_matching_investigations(
    selections: &mut [InvestigationSelection],
    scope: &crate::ontology::ReasoningScope,
    review_reason_code: ReviewReasonCode,
    reason: &str,
) {
    for selection in selections
        .iter_mut()
        .filter(|selection| &selection.scope == scope)
    {
        if selection.attention_hint != "review" {
            selection.notes.insert(
                0,
                format!(
                    "backward_confirmation_transition={} -> review",
                    selection.attention_hint
                ),
            );
            selection.attention_hint = "review".into();
        }
        selection.review_reason_code = Some(review_reason_code);
        selection
            .notes
            .insert(0, format!("backward_confirmation_gate={}", reason));
        selection.notes.insert(
            0,
            format!("review_reason_code={}", review_reason_code.as_str()),
        );
        if !selection.rationale.contains(reason) {
            selection.rationale = format!("{} | {}", selection.rationale, reason);
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
