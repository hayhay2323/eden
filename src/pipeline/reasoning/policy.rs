use std::collections::HashMap;

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::graph::decision::{MarketRegimeFilter, OrderDirection};
use crate::ontology::reasoning::{
    DecisionLineage, HorizonPolicyVerdict, HypothesisTrack, HypothesisTrackStatus,
    PolicyVerdictKind, PolicyVerdictSummary, ReviewReasonCode, TacticalSetup,
};
#[cfg(feature = "persistence")]
use crate::persistence::case_reasoning_assessment::CaseReasoningAssessmentRecord;
use crate::temporal::lineage::FamilyContextLineageOutcome;

use super::support::{hk_session_label, track_id_for_scope};

#[derive(Debug, Clone, Default)]
pub struct ReviewerDoctrinePressure {
    pub updates: usize,
    pub reflexive_correction_rate: Decimal,
    pub narrative_failure_rate: Decimal,
    pub dominant_mechanism: Option<String>,
    pub dominant_rejection_reason: Option<String>,
    pub family_pressure_overrides: HashMap<String, Decimal>,
}

impl ReviewerDoctrinePressure {
    pub(crate) fn strictness_score(&self) -> Decimal {
        let mut score = Decimal::ZERO;
        if self.updates >= 5 {
            score += self.reflexive_correction_rate;
            score += self.narrative_failure_rate;
            if matches!(
                self.dominant_rejection_reason.as_deref(),
                Some("Evidence Too Weak" | "Mechanism Mismatch" | "Timing Mismatch")
            ) {
                score += Decimal::new(15, 2);
            }
        }
        score
    }

    pub(crate) fn is_active(&self) -> bool {
        self.updates >= 5 && self.strictness_score() >= Decimal::new(35, 2)
    }

    pub(crate) fn pressure_for_family(&self, family: Option<&str>) -> Decimal {
        if !self.is_active() {
            return Decimal::ZERO;
        }

        let base = self.strictness_score().clamp(Decimal::ZERO, Decimal::ONE);
        let Some(family) = family else {
            return base;
        };
        let family = normalized_family_key(family);
        if let Some(override_pressure) = self.family_pressure_overrides.get(&family) {
            return (*override_pressure).clamp(Decimal::ZERO, Decimal::new(15, 1));
        }
        let mut pressure = base;

        if let Some(reason) = self.dominant_rejection_reason.as_deref() {
            pressure += reason_family_pressure(reason, &family);
        }
        if let Some(mechanism) = self.dominant_mechanism.as_deref() {
            pressure += mechanism_family_pressure(mechanism, &family);
        }

        pressure.clamp(Decimal::ZERO, Decimal::new(15, 1))
    }
}

#[cfg(feature = "persistence")]
impl ReviewerDoctrinePressure {
    pub fn from_assessments(assessments: &[CaseReasoningAssessmentRecord]) -> Self {
        let mut updates = 0usize;
        let mut reflexive_corrections = 0usize;
        let mut narrative_failures = 0usize;
        let mut mechanisms: HashMap<String, usize> = HashMap::new();
        let mut rejection_reasons: HashMap<String, usize> = HashMap::new();
        let mut family_updates: HashMap<String, usize> = HashMap::new();
        let mut family_reflexive: HashMap<String, usize> = HashMap::new();
        let mut family_narrative: HashMap<String, usize> = HashMap::new();
        let mut family_mechanisms: HashMap<String, HashMap<String, usize>> = HashMap::new();
        let mut family_reasons: HashMap<String, HashMap<String, usize>> = HashMap::new();

        for assessment in assessments
            .iter()
            .filter(|item| item.source == "workflow_update" || item.source == "outcome_auto")
        {
            updates += 1;
            let family_key = assessment
                .family_label
                .as_deref()
                .map(normalized_family_key);
            if assessment
                .composite_state_kinds
                .iter()
                .any(|item| item == "Reflexive Correction")
            {
                reflexive_corrections += 1;
                if let Some(family_key) = family_key.as_ref() {
                    *family_reflexive.entry(family_key.clone()).or_insert(0) += 1;
                }
            }
            if assessment.primary_mechanism_kind.as_deref() == Some("Narrative Failure") {
                narrative_failures += 1;
                if let Some(family_key) = family_key.as_ref() {
                    *family_narrative.entry(family_key.clone()).or_insert(0) += 1;
                }
            }
            if let Some(mechanism) = assessment.primary_mechanism_kind.as_ref() {
                *mechanisms.entry(mechanism.clone()).or_insert(0) += 1;
                if let Some(family_key) = family_key.as_ref() {
                    *family_mechanisms
                        .entry(family_key.clone())
                        .or_default()
                        .entry(mechanism.clone())
                        .or_insert(0) += 1;
                }
            }
            if let Some(family_key) = family_key.as_ref() {
                *family_updates.entry(family_key.clone()).or_insert(0) += 1;
            }
            if let Some(review) = assessment.reasoning_profile.human_review.as_ref() {
                for reason in &review.reasons {
                    *rejection_reasons.entry(reason.label.clone()).or_insert(0) += 1;
                    if let Some(family_key) = family_key.as_ref() {
                        *family_reasons
                            .entry(family_key.clone())
                            .or_default()
                            .entry(reason.label.clone())
                            .or_insert(0) += 1;
                    }
                }
            }
        }

        let reflexive_correction_rate = if updates > 0 {
            Decimal::from(reflexive_corrections as i64) / Decimal::from(updates as i64)
        } else {
            Decimal::ZERO
        };
        let narrative_failure_rate = if updates > 0 {
            Decimal::from(narrative_failures as i64) / Decimal::from(updates as i64)
        } else {
            Decimal::ZERO
        };
        let dominant_rejection_reason = rejection_reasons
            .into_iter()
            .max_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)))
            .map(|(label, _)| label);
        let dominant_mechanism = mechanisms
            .into_iter()
            .max_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)))
            .map(|(label, _)| label);
        let mut family_pressure_overrides = HashMap::new();
        for (family, family_update_count) in family_updates {
            if family_update_count < 3 {
                continue;
            }
            let family_reflexive_rate =
                Decimal::from(family_reflexive.remove(&family).unwrap_or_default() as i64)
                    / Decimal::from(family_update_count as i64);
            let family_narrative_rate =
                Decimal::from(family_narrative.remove(&family).unwrap_or_default() as i64)
                    / Decimal::from(family_update_count as i64);
            let family_dominant_reason = family_reasons.remove(&family).and_then(|counts| {
                counts
                    .into_iter()
                    .max_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)))
                    .map(|(label, _)| label)
            });
            let family_dominant_mechanism = family_mechanisms.remove(&family).and_then(|counts| {
                counts
                    .into_iter()
                    .max_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)))
                    .map(|(label, _)| label)
            });
            let mut family_pressure = Decimal::ZERO;
            if family_update_count >= 5 {
                family_pressure += family_reflexive_rate;
                family_pressure += family_narrative_rate;
            }
            if let Some(reason) = family_dominant_reason.as_deref() {
                family_pressure += reason_family_pressure(reason, &family);
            }
            if let Some(mechanism) = family_dominant_mechanism.as_deref() {
                family_pressure += mechanism_family_pressure(mechanism, &family);
            }
            if family_pressure > Decimal::ZERO {
                family_pressure_overrides.insert(
                    family,
                    family_pressure.clamp(Decimal::ZERO, Decimal::new(15, 1)),
                );
            }
        }

        Self {
            updates,
            reflexive_correction_rate,
            narrative_failure_rate,
            dominant_mechanism,
            dominant_rejection_reason,
            family_pressure_overrides,
        }
    }
}

fn family_contains_any(family: &str, needles: &[&str]) -> bool {
    let normalized_family = normalized_family_key(family);
    needles
        .iter()
        .any(|needle| normalized_family.contains(&needle.to_ascii_lowercase()))
}

fn normalized_family_key(family: &str) -> String {
    family
        .to_ascii_lowercase()
        .replace('_', "-")
        .replace(' ', "-")
}

fn reason_family_pressure(reason: &str, family: &str) -> Decimal {
    match reason {
        "Evidence Too Weak" => {
            if family_contains_any(
                family,
                &[
                    "propagation",
                    "cross",
                    "catalyst",
                    "risk",
                    "stress",
                    "relay",
                    "breakout",
                    "sector",
                ],
            ) {
                Decimal::new(35, 2)
            } else {
                Decimal::ZERO
            }
        }
        "Mechanism Mismatch" => {
            if family_contains_any(
                family,
                &["propagation", "cross", "catalyst", "risk", "rotation"],
            ) {
                Decimal::new(30, 2)
            } else {
                Decimal::new(10, 2)
            }
        }
        "Timing Mismatch" => {
            if family_contains_any(
                family,
                &[
                    "momentum",
                    "breakout",
                    "pre-market",
                    "cross-market",
                    "rotation",
                    "catalyst",
                ],
            ) {
                Decimal::new(35, 2)
            } else {
                Decimal::ZERO
            }
        }
        "Risk Too High" => {
            if family_contains_any(
                family,
                &[
                    "risk",
                    "stress",
                    "cross-mechanism",
                    "propagation",
                    "contagion",
                ],
            ) {
                Decimal::new(25, 2)
            } else {
                Decimal::new(10, 2)
            }
        }
        "Event Risk" => {
            if family_contains_any(
                family,
                &["catalyst", "pre-market", "cross-market", "risk", "event"],
            ) {
                Decimal::new(25, 2)
            } else {
                Decimal::ZERO
            }
        }
        "Execution Constraint" => {
            if family_contains_any(family, &["flow", "liquidity", "pre-market", "breakout"]) {
                Decimal::new(25, 2)
            } else {
                Decimal::ZERO
            }
        }
        _ => Decimal::ZERO,
    }
}

fn mechanism_family_pressure(mechanism: &str, family: &str) -> Decimal {
    match mechanism {
        "Narrative Failure" => {
            if family_contains_any(
                family,
                &[
                    "catalyst",
                    "risk",
                    "cross-mechanism",
                    "cross-market",
                    "propagation",
                ],
            ) {
                Decimal::new(20, 2)
            } else {
                Decimal::ZERO
            }
        }
        "Mechanical Execution Signature" => {
            if family_contains_any(family, &["flow", "liquidity", "pre-market", "breakout"]) {
                Decimal::new(20, 2)
            } else {
                Decimal::ZERO
            }
        }
        "Capital Rotation" => {
            if family_contains_any(family, &["rotation", "sector"]) {
                Decimal::new(20, 2)
            } else {
                Decimal::ZERO
            }
        }
        "Arbitrage Convergence" => {
            if family_contains_any(family, &["arbitrage", "cross-market"]) {
                Decimal::new(20, 2)
            } else {
                Decimal::ZERO
            }
        }
        "Event-driven Dislocation" => {
            if family_contains_any(family, &["catalyst", "pre-market", "cross-market", "risk"]) {
                Decimal::new(20, 2)
            } else {
                Decimal::ZERO
            }
        }
        "Liquidity Trap" => {
            if family_contains_any(family, &["liquidity", "flow"]) {
                Decimal::new(15, 2)
            } else {
                Decimal::ZERO
            }
        }
        _ => Decimal::ZERO,
    }
}

pub fn derive_hypothesis_tracks(
    timestamp: OffsetDateTime,
    current_setups: &[TacticalSetup],
    previous_setups: &[TacticalSetup],
    previous_tracks: &[HypothesisTrack],
) -> Vec<HypothesisTrack> {
    let previous_setup_map = previous_setups
        .iter()
        .map(|setup| (track_id_for_scope(&setup.scope), setup))
        .collect::<HashMap<_, _>>();
    let previous_track_map = previous_tracks
        .iter()
        .map(|track| (track.track_id.clone(), track))
        .collect::<HashMap<_, _>>();
    let mut tracks = Vec::new();

    for setup in current_setups {
        let track_id = track_id_for_scope(&setup.scope);
        let previous_setup = previous_setup_map.get(&track_id).copied();
        let previous_track = previous_track_map.get(&track_id).copied();

        let previous_confidence = previous_setup.map(|item| item.confidence);
        let previous_gap = previous_setup.map(|item| item.confidence_gap);
        let confidence_change = previous_confidence
            .map(|value| setup.confidence - value)
            .unwrap_or(Decimal::ZERO);
        let confidence_gap_change = previous_gap
            .map(|value| setup.confidence_gap - value)
            .unwrap_or(Decimal::ZERO);
        let status = previous_setup
            .map(|previous| track_status(previous, setup))
            .unwrap_or(HypothesisTrackStatus::New);
        let status_streak = previous_track
            .filter(|track| {
                track.status == status
                    && track.hypothesis_id == setup.hypothesis_id
                    && track.runner_up_hypothesis_id == setup.runner_up_hypothesis_id
            })
            .map(|track| track.status_streak + 1)
            .unwrap_or(1);
        let policy_reason = policy_reason_for_setup(setup, status, status_streak);
        let transition_reason = transition_reason_for_setup(setup, previous_track, &policy_reason);

        tracks.push(HypothesisTrack {
            track_id,
            setup_id: setup.setup_id.clone(),
            hypothesis_id: setup.hypothesis_id.clone(),
            runner_up_hypothesis_id: setup.runner_up_hypothesis_id.clone(),
            scope: setup.scope.clone(),
            title: setup.title.clone(),
            action: setup.action.clone(),
            status,
            age_ticks: previous_track.map(|track| track.age_ticks + 1).unwrap_or(1),
            status_streak,
            confidence: setup.confidence,
            previous_confidence,
            confidence_change,
            confidence_gap: setup.confidence_gap,
            previous_confidence_gap: previous_gap,
            confidence_gap_change,
            heuristic_edge: setup.heuristic_edge,
            policy_reason,
            transition_reason,
            first_seen_at: previous_track
                .map(|track| track.first_seen_at)
                .unwrap_or(timestamp),
            last_updated_at: timestamp,
            invalidated_at: None,
        });
    }

    for previous_setup in previous_setups {
        let track_id = track_id_for_scope(&previous_setup.scope);
        if current_setups
            .iter()
            .any(|setup| track_id_for_scope(&setup.scope) == track_id)
        {
            continue;
        }

        let previous_track = previous_track_map.get(&track_id).copied();
        tracks.push(HypothesisTrack {
            track_id,
            setup_id: previous_setup.setup_id.clone(),
            hypothesis_id: previous_setup.hypothesis_id.clone(),
            runner_up_hypothesis_id: previous_setup.runner_up_hypothesis_id.clone(),
            scope: previous_setup.scope.clone(),
            title: previous_setup.title.clone(),
            action: previous_setup.action.clone(),
            status: HypothesisTrackStatus::Invalidated,
            age_ticks: previous_track.map(|track| track.age_ticks).unwrap_or(1),
            status_streak: previous_track
                .map(|track| track.status_streak + 1)
                .unwrap_or(1),
            confidence: previous_setup.confidence,
            previous_confidence: Some(previous_setup.confidence),
            confidence_change: -previous_setup.confidence,
            confidence_gap: previous_setup.confidence_gap,
            previous_confidence_gap: Some(previous_setup.confidence_gap),
            confidence_gap_change: -previous_setup.confidence_gap,
            heuristic_edge: previous_setup.heuristic_edge,
            policy_reason: "current tick no longer supports the prior leading case".into(),
            transition_reason: Some(format!(
                "downgraded from {} because the leading case invalidated",
                previous_setup.action
            )),
            first_seen_at: previous_track
                .map(|track| track.first_seen_at)
                .unwrap_or(timestamp),
            last_updated_at: timestamp,
            invalidated_at: Some(timestamp),
        });
    }

    tracks.sort_by(|a, b| {
        track_status_priority(a.status)
            .cmp(&track_status_priority(b.status))
            .then_with(|| b.confidence_gap.cmp(&a.confidence_gap))
            .then_with(|| b.confidence.cmp(&a.confidence))
            .then_with(|| a.track_id.cmp(&b.track_id))
    });
    tracks
}

pub fn apply_track_action_policy(
    setups: &[TacticalSetup],
    tracks: &[HypothesisTrack],
    previous_tracks: &[HypothesisTrack],
    timestamp: OffsetDateTime,
    market_regime: &MarketRegimeFilter,
    lineage_priors: &[FamilyContextLineageOutcome],
    multi_horizon_gate: Option<&crate::temporal::lineage::MultiHorizonGate>,
    reviewer_doctrine: Option<&ReviewerDoctrinePressure>,
    family_boost: &super::context::FamilyBoostLedger,
) -> Vec<TacticalSetup> {
    let track_map = tracks
        .iter()
        .map(|track| (track.track_id.as_str(), track))
        .collect::<HashMap<_, _>>();
    let previous_track_map = previous_tracks
        .iter()
        .map(|track| (track.track_id.as_str(), track))
        .collect::<HashMap<_, _>>();

    setups
        .iter()
        .map(|setup| {
            let track_id = track_id_for_scope(&setup.scope);
            let Some(track) = track_map.get(track_id.as_str()).copied() else {
                let mut fallback = setup.clone();
                fallback
                    .risk_notes
                    .insert(0, "policy_gate: missing_track_context".into());
                return fallback;
            };
            let previous_track = previous_track_map.get(track_id.as_str()).copied();
            let decision = decide_track_action(
                setup,
                track,
                previous_track,
                timestamp,
                market_regime,
                lineage_priors,
                multi_horizon_gate,
                reviewer_doctrine,
                family_boost,
            );

            let mut updated = setup.clone();
            updated.action = decision.action.into();
            updated.review_reason_code = decision.review_reason_code;
            updated.lineage = decision.lineage.clone();
            updated.policy_verdict = Some(decision.policy_verdict.clone());
            let mut provenance = updated
                .provenance
                .clone()
                .with_trace_id(updated.setup_id.clone());
            provenance.note = Some(decision.reason.clone());
            if let Some(transition_reason) = &decision.transition_reason {
                let mut inputs = provenance.inputs.clone();
                inputs.push(transition_reason.clone());
                provenance.inputs = inputs;
            }
            updated.provenance = provenance;
            if updated.action == "enter" {
                updated.entry_rationale = decision.reason.clone();
            } else {
                updated
                    .risk_notes
                    .insert(0, format!("policy_gate: {}", decision.reason));
            }
            if let Some(transition_reason) = decision.transition_reason {
                updated
                    .risk_notes
                    .insert(0, format!("policy_transition: {}", transition_reason));
            }
            updated
        })
        .collect()
}

pub fn apply_case_budget(
    mut setups: Vec<TacticalSetup>,
    tracks: &[HypothesisTrack],
    previous_tracks: &[HypothesisTrack],
) -> Vec<TacticalSetup> {
    const MAX_NEW_ENTERS_PER_TICK: usize = 2;
    const MAX_TOTAL_ATTENTION_CASES: usize = 6;

    let track_map = tracks
        .iter()
        .map(|track| (track.track_id.as_str(), track))
        .collect::<HashMap<_, _>>();
    let previous_track_map = previous_tracks
        .iter()
        .map(|track| (track.track_id.as_str(), track))
        .collect::<HashMap<_, _>>();

    let mut new_enter_indices = setups
        .iter()
        .enumerate()
        .filter_map(|(index, setup)| {
            let track_id = track_id_for_scope(&setup.scope);
            let previous_action = previous_track_map
                .get(track_id.as_str())
                .map(|track| track.action.as_str());
            (setup.action == "enter" && previous_action != Some("enter")).then_some(index)
        })
        .collect::<Vec<_>>();
    new_enter_indices.sort_by(|left, right| {
        compare_attention_priority(
            &setups[*left],
            &setups[*right],
            &track_map,
            &previous_track_map,
        )
    });
    for index in new_enter_indices.iter().skip(MAX_NEW_ENTERS_PER_TICK) {
        demote_setup_for_budget(
            &mut setups[*index],
            "review",
            "new-enter budget reached; only highest-conviction promotions advance this tick",
        );
    }

    let preserved_enter_count = setups
        .iter()
        .filter(|setup| {
            if setup.action != "enter" {
                return false;
            }
            let track_id = track_id_for_scope(&setup.scope);
            previous_track_map
                .get(track_id.as_str())
                .map(|track| track.action.as_str() == "enter")
                .unwrap_or(false)
        })
        .count();
    let remaining_attention_slots = MAX_TOTAL_ATTENTION_CASES.saturating_sub(preserved_enter_count);

    let mut attention_indices = setups
        .iter()
        .enumerate()
        .filter_map(|(index, setup)| {
            if !matches!(setup.action.as_str(), "enter" | "review") {
                return None;
            }
            let track_id = track_id_for_scope(&setup.scope);
            let previous_action = previous_track_map
                .get(track_id.as_str())
                .map(|track| track.action.as_str());
            (previous_action != Some("enter")).then_some(index)
        })
        .collect::<Vec<_>>();
    attention_indices.sort_by(|left, right| {
        compare_attention_priority(
            &setups[*left],
            &setups[*right],
            &track_map,
            &previous_track_map,
        )
    });
    for index in attention_indices.iter().skip(remaining_attention_slots) {
        demote_setup_for_budget(
            &mut setups[*index],
            "observe",
            "attention budget reached; lower-priority cases stay backgrounded this tick",
        );
    }

    prune_stale_tactical_setups(setups, previous_tracks)
}

pub(crate) fn prune_stale_tactical_setups(
    setups: Vec<TacticalSetup>,
    previous_tracks: &[HypothesisTrack],
) -> Vec<TacticalSetup> {
    let previous_track_map = previous_tracks
        .iter()
        .map(|track| (track.track_id.clone(), track))
        .collect::<HashMap<_, _>>();

    setups
        .into_iter()
        .filter(|setup| {
            let track_id = track_id_for_scope(&setup.scope);
            let Some(previous_track) = previous_track_map.get(&track_id) else {
                return true;
            };
            !should_expire_observe_case(setup, previous_track)
        })
        .collect()
}

fn should_expire_observe_case(setup: &TacticalSetup, previous_track: &HypothesisTrack) -> bool {
    if setup.action != "observe" || previous_track.action != "observe" {
        return false;
    }

    // Aggressive TTL: order cases get 8 ticks, non-order get 4
    let ttl_ticks = if setup.workflow_id.is_some() { 8 } else { 4 };

    // Fast-expire: very low confidence gap AND low edge → not worth keeping
    let low_quality =
        setup.confidence_gap < Decimal::new(10, 2) && setup.heuristic_edge <= Decimal::new(5, 2);
    if low_quality && previous_track.age_ticks >= 2 {
        return true;
    }

    if previous_track.age_ticks < ttl_ticks {
        return false;
    }

    let stable_confidence = previous_track
        .previous_confidence
        .map(|previous| (setup.confidence - previous).abs() <= Decimal::new(5, 2))
        .unwrap_or(true);
    let no_edge = setup.heuristic_edge <= Decimal::new(10, 2);
    let not_strengthening = !matches!(previous_track.status, HypothesisTrackStatus::Strengthening);

    // Only need two of the three signals to expire (relaxed from requiring all)
    let signals = [stable_confidence, no_edge, not_strengthening];
    let active_signals = signals.iter().filter(|&&v| v).count();
    active_signals >= 2
}

const PROPAGATION_FAMILIES: &[&str] = &[
    "propagation chain",
    "sector rotation spillover",
    "sector-symbol spillover",
    "cross-mechanism chain",
    "catalyst repricing",
];

fn is_propagation_family(family: &str) -> bool {
    let lower = family.to_ascii_lowercase();
    PROPAGATION_FAMILIES
        .iter()
        .any(|propagation_family| lower.contains(propagation_family))
}

pub(crate) fn demote_on_propagation_absence(
    setups: Vec<TacticalSetup>,
    absence_sectors: &[crate::ontology::objects::SectorId],
) -> Vec<TacticalSetup> {
    if absence_sectors.is_empty() {
        return setups;
    }
    setups
        .into_iter()
        .map(|mut setup| {
            let dominated_by_absence = setup_family_key(&setup)
                .map(is_propagation_family)
                .unwrap_or(false)
                && setup_touches_sector(&setup, absence_sectors);
            if dominated_by_absence && setup.action != "observe" {
                setup.risk_notes.insert(
                    0,
                    "propagation_absence: sector peers silent, demoting".into(),
                );
                setup.action = "observe".into();
                setup
                    .lineage
                    .blocked_by
                    .push("demoted because propagation absence detected for this sector".into());
                if let Some(verdict) = setup.policy_verdict.as_mut() {
                    verdict.primary = PolicyVerdictKind::AttentionCapped;
                    verdict.rationale =
                        "propagation_absence: expected sector move not confirmed".into();
                }
            }
            setup
        })
        .collect()
}

pub(crate) fn apply_convergence_policy(mut setups: Vec<TacticalSetup>) -> Vec<TacticalSetup> {
    for setup in &mut setups {
        let Some(detail) = setup.convergence_detail.as_ref() else {
            continue;
        };
        let spread = detail.component_spread.unwrap_or(Decimal::ZERO);

        // Rule 1: Strong consensus promotes observe → review
        let strong_consensus = detail.institutional_alignment.abs() > Decimal::new(5, 1)
            && spread < Decimal::new(3, 1);
        if strong_consensus && setup.action == "observe" {
            setup.action = "review".into();
            setup
                .risk_notes
                .push("promoted: strong institutional consensus".into());
        }

        // Rule 2: High disagreement demotes enter → review
        let high_disagreement = spread > Decimal::new(6, 1);
        if high_disagreement && setup.action == "enter" {
            setup.action = "review".into();
            setup.review_reason_code = Some(ReviewReasonCode::ConvergenceDisagreement);
            setup
                .risk_notes
                .push("demoted: convergence components disagree".into());
        }
    }
    setups
}

pub(crate) fn apply_midflight_health_check(
    setups: Vec<TacticalSetup>,
    previous_tracks: &[HypothesisTrack],
) -> Vec<TacticalSetup> {
    let prev_map: HashMap<&str, &HypothesisTrack> = previous_tracks
        .iter()
        .map(|t| (t.setup_id.as_str(), t))
        .collect();
    let conf_drop_threshold = Decimal::new(-5, 2);
    let gap_shrink_threshold = Decimal::new(-8, 2);
    setups
        .into_iter()
        .map(|mut setup| {
            if setup.action != "enter" && setup.action != "review" {
                return setup;
            }
            let Some(prev) = prev_map.get(setup.setup_id.as_str()) else {
                return setup;
            };
            if prev.action != "enter" && prev.action != "review" {
                return setup;
            }
            let conf_delta = setup.confidence - prev.confidence;
            let gap_delta =
                setup.confidence_gap - prev.previous_confidence_gap.unwrap_or(prev.confidence_gap);

            let conf_deteriorated = conf_delta < conf_drop_threshold;
            let gap_collapsed = gap_delta < gap_shrink_threshold;

            if !conf_deteriorated && !gap_collapsed {
                return setup;
            }

            let mut reasons = Vec::new();
            if conf_deteriorated {
                reasons.push(format!(
                    "confidence dropped {:+.1}% since promotion",
                    conf_delta * Decimal::new(100, 0)
                ));
            }
            if gap_collapsed {
                reasons.push(format!(
                    "gap shrank {:+.1}% since promotion",
                    gap_delta * Decimal::new(100, 0)
                ));
            }
            let health_msg = format!("midflight_health: {}", reasons.join("; "));
            setup.risk_notes.insert(0, health_msg.clone());
            if setup.action == "enter" {
                setup.action = "review".into();
                setup
                    .lineage
                    .blocked_by
                    .push("demoted from enter: evidence deteriorated after promotion".into());
            }
            if let Some(verdict) = setup.policy_verdict.as_mut() {
                verdict.rationale = health_msg;
            }
            setup
        })
        .collect()
}

fn setup_touches_sector(
    setup: &TacticalSetup,
    sectors: &[crate::ontology::objects::SectorId],
) -> bool {
    match &setup.scope {
        crate::ontology::reasoning::ReasoningScope::Sector(sector_id) => {
            sectors.iter().any(|s| s == sector_id)
        }
        _ => setup
            .risk_notes
            .iter()
            .any(|note| sectors.iter().any(|sector_id| note.contains(&sector_id.0))),
    }
}

fn track_status(previous: &TacticalSetup, current: &TacticalSetup) -> HypothesisTrackStatus {
    let confidence_delta = current.confidence - previous.confidence;
    let gap_delta = current.confidence_gap - previous.confidence_gap;
    let net_delta = confidence_delta + gap_delta;
    let threshold = Decimal::new(1, 3);

    if net_delta > threshold {
        HypothesisTrackStatus::Strengthening
    } else if net_delta < -threshold {
        HypothesisTrackStatus::Weakening
    } else {
        HypothesisTrackStatus::Stable
    }
}

fn track_status_priority(status: HypothesisTrackStatus) -> i32 {
    match status {
        HypothesisTrackStatus::Strengthening => 0,
        HypothesisTrackStatus::New => 1,
        HypothesisTrackStatus::Stable => 2,
        HypothesisTrackStatus::Weakening => 3,
        HypothesisTrackStatus::Invalidated => 4,
    }
}

struct TrackActionDecision {
    action: &'static str,
    reason: String,
    review_reason_code: Option<ReviewReasonCode>,
    transition_reason: Option<String>,
    lineage: DecisionLineage,
    policy_verdict: PolicyVerdictSummary,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[cfg_attr(test, allow(dead_code))]
pub(super) enum PriorSignal {
    Positive,
    Negative,
    Neutral,
}

fn decide_track_action(
    setup: &TacticalSetup,
    track: &HypothesisTrack,
    previous_track: Option<&HypothesisTrack>,
    timestamp: OffsetDateTime,
    market_regime: &MarketRegimeFilter,
    lineage_priors: &[FamilyContextLineageOutcome],
    multi_horizon_gate: Option<&crate::temporal::lineage::MultiHorizonGate>,
    reviewer_doctrine: Option<&ReviewerDoctrinePressure>,
    family_boost: &super::context::FamilyBoostLedger,
) -> TrackActionDecision {
    let doctrine_pressure = reviewer_doctrine
        .map(|item| item.pressure_for_family(setup_family_key(setup)))
        .unwrap_or(Decimal::ZERO);
    let doctrine_active = doctrine_pressure > Decimal::ZERO;
    let lineage_prior = best_lineage_prior(setup, timestamp, market_regime, lineage_priors);
    let prior_signal = lineage_prior
        .map(classify_lineage_prior)
        .unwrap_or(PriorSignal::Neutral);
    let alpha_boost = lineage_prior
        .map(|p| compute_alpha_boost(p))
        .unwrap_or(Decimal::ZERO);
    let boost_factor = family_boost.boost_for_family(
        setup_family_key(setup).unwrap_or("unknown"),
    );
    let boost_edge_reduction = (boost_factor - Decimal::ONE) * Decimal::new(2, 2);
    let min_enter_edge = Decimal::new(3, 2) + doctrine_pressure * Decimal::new(2, 2)
        - alpha_boost * Decimal::new(1, 2)
        - boost_edge_reduction;
    let min_enter_gap = Decimal::new(15, 2) + doctrine_pressure * Decimal::new(5, 2)
        - alpha_boost * Decimal::new(3, 2);
    let min_enter_local_support = Decimal::new(30, 2) + doctrine_pressure * Decimal::new(10, 2)
        - alpha_boost * Decimal::new(5, 2);
    let min_hold_edge = Decimal::new(15, 3);
    let min_strengthening_streak = if doctrine_pressure >= Decimal::new(35, 2) {
        3
    } else if alpha_boost >= Decimal::new(5, 1) {
        1
    } else {
        2
    };
    let is_order_case = setup.workflow_id.is_some();
    let previous_action = previous_track.map(|item| item.action.as_str());
    let blocked_by_regime =
        setup_order_direction(setup).and_then(|direction| market_regime.gate_reason(direction));
    let prior_reason = lineage_prior
        .map(describe_lineage_prior)
        .filter(|reason| !reason.is_empty());
    let recent_invalidation = previous_track
        .map(|item| {
            item.status == HypothesisTrackStatus::Invalidated || item.invalidated_at.is_some()
        })
        .unwrap_or(false);
    let in_refractory_window = recently_invalidated(previous_track, timestamp);
    let fresh_symbol_confirmation = fresh_symbol_confirmation_from_notes(setup);
    let directional_conflict_present = directional_conflict_present_from_notes(setup);
    let directional_support = directional_support_from_notes(setup);
    let directional_conflict = directional_conflict_from_notes(setup);
    let symbol_event_count = symbol_event_count_from_notes(setup);
    let multi_horizon_supported = setup_family_key(setup)
        .map(|family| {
            multi_horizon_gate
                .map(|gate| gate.allows(family))
                .unwrap_or(true)
        })
        .unwrap_or(true);

    let (action, reason) = if !is_order_case {
        if prior_signal == PriorSignal::Negative {
            (
                "observe",
                format!(
                    "suppressed scope case because lineage prior is weak for this context ({})",
                    prior_reason
                        .clone()
                        .unwrap_or_else(|| "no reliable edge".into())
                ),
            )
        } else if track.status == HypothesisTrackStatus::Strengthening
            && track.status_streak >= 3
            && setup.confidence >= Decimal::new(74, 2)
            && setup.confidence_gap >= Decimal::new(20, 2)
        {
            (
                "review",
                format!(
                    "scope case strengthened for {} ticks with gap {}",
                    track.status_streak,
                    setup.confidence_gap.round_dp(3)
                ),
            )
        } else {
            (
                "observe",
                format!(
                    "scope case remains background-only; status={} gap={}",
                    track.status,
                    setup.confidence_gap.round_dp(3)
                ),
            )
        }
    } else {
        if matches!(track.status, HypothesisTrackStatus::Invalidated) {
            (
                "review",
                "leading hypothesis invalidated; manual review required".into(),
            )
        } else if matches!(track.status, HypothesisTrackStatus::Weakening) {
            if matches!(previous_action, Some("enter" | "review")) {
                (
                    "review",
                    format!(
                        "confidence or gap weakened (d_conf={} d_gap={})",
                        track.confidence_change.round_dp(3),
                        track.confidence_gap_change.round_dp(3)
                    ),
                )
            } else {
                (
                    "observe",
                    format!(
                        "weakening detected but no active escalation exists yet (d_conf={} d_gap={})",
                        track.confidence_change.round_dp(3),
                        track.confidence_gap_change.round_dp(3)
                    ),
                )
            }
        } else if directional_conflict_present {
            if matches!(previous_action, Some("enter" | "review")) {
                (
                    "review",
                    format!(
                        "symbol-level confirmation now conflicts with the case (support={} conflict={} events={})",
                        directional_support.round_dp(3),
                        directional_conflict.round_dp(3),
                        symbol_event_count,
                    ),
                )
            } else {
                (
                    "observe",
                    format!(
                        "suppressed because symbol-level confirmation conflicts with the case (support={} conflict={} events={})",
                        directional_support.round_dp(3),
                        directional_conflict.round_dp(3),
                        symbol_event_count,
                    ),
                )
            }
        } else if !fresh_symbol_confirmation {
            if previous_action == Some("enter") {
                (
                    "review",
                    format!(
                        "enter became stale because no fresh symbol-level confirmation remains (support={} conflict={} events={})",
                        directional_support.round_dp(3),
                        directional_conflict.round_dp(3),
                        symbol_event_count,
                    ),
                )
            } else {
                (
                    "observe",
                    format!(
                        "waiting for fresh symbol-level confirmation before escalation (support={} conflict={} events={})",
                        directional_support.round_dp(3),
                        directional_conflict.round_dp(3),
                        symbol_event_count,
                    ),
                )
            }
        } else if let Some(ref reason) = blocked_by_regime {
            ("review", reason.clone())
        } else if prior_signal == PriorSignal::Negative && previous_action != Some("enter") {
            (
                "observe",
                format!(
                    "suppressed because lineage prior is unfavorable in this context ({})",
                    prior_reason
                        .clone()
                        .unwrap_or_else(|| "no reliable edge".into())
                ),
            )
        } else if in_refractory_window && previous_action != Some("enter") {
            (
                "observe",
                "suppressed by refractory window after recent invalidation/weakening".into(),
            )
        } else if !multi_horizon_supported && previous_action != Some("enter") {
            (
                "observe",
                "suppressed because this family has not yet shown positive edge beyond the short tick horizon".into(),
            )
        } else {
            match track.status {
                HypothesisTrackStatus::Strengthening
                    if track.status_streak >= min_strengthening_streak
                        && setup.confidence >= Decimal::new(64, 2)
                        && setup.confidence_gap >= min_enter_gap
                        && track.confidence_change >= Decimal::ZERO
                        && track.confidence_gap_change >= Decimal::ZERO
                        && setup.heuristic_edge >= min_enter_edge
                        && local_support_from_reason(setup) >= min_enter_local_support
                        && fresh_symbol_confirmation
                        && !directional_conflict_present
                        && !recent_invalidation
                        && prior_signal != PriorSignal::Negative =>
                {
                    (
                        "enter",
                        format!(
                            "promoted by strengthening streak={} with widening gap {} and local support {}{}",
                            track.status_streak,
                            setup.confidence_gap.round_dp(3),
                            local_support_from_reason(setup).round_dp(3),
                            if doctrine_active {
                                " under stricter reviewer doctrine"
                            } else if alpha_boost > Decimal::ZERO {
                                " with alpha boost from proven lineage"
                            } else {
                                ""
                            },
                        ),
                    )
                }
                HypothesisTrackStatus::Stable
                    if previous_action == Some("enter")
                        && setup.confidence >= Decimal::new(58, 2)
                        && setup.confidence_gap >= Decimal::new(10, 2)
                        && setup.heuristic_edge >= min_hold_edge
                        && fresh_symbol_confirmation
                        && !directional_conflict_present
                        && prior_signal != PriorSignal::Negative =>
                {
                    (
                        "enter",
                        format!(
                            "holding enter because confidence, gap, and edge remain above maintenance thresholds (edge={} local={})",
                            setup.heuristic_edge.round_dp(3),
                            local_support_from_reason(setup).round_dp(3),
                        ),
                    )
                }
                HypothesisTrackStatus::Strengthening
                    if setup.confidence >= Decimal::new(70, 2)
                        && setup.confidence_gap >= Decimal::new(18, 2)
                        && prior_signal != PriorSignal::Negative =>
                {
                    (
                        "review",
                        format!(
                            "high-quality strengthening detected but persistence is still building (streak={})",
                            track.status_streak
                        ),
                    )
                }
                HypothesisTrackStatus::New
                    if setup.confidence >= Decimal::new(74, 2)
                        && setup.confidence_gap >= Decimal::new(20, 2)
                        && prior_signal == PriorSignal::Positive =>
                {
                    (
                        "review",
                        "fresh case meets structural thresholds but still needs persistence confirmation".into(),
                    )
                }
                HypothesisTrackStatus::Stable | HypothesisTrackStatus::New => (
                    "observe",
                    format!(
                        "waiting for stronger persistence before escalation; status={} streak={}",
                        track.status, track.status_streak
                    ),
                ),
                HypothesisTrackStatus::Strengthening => (
                    "review",
                    format!(
                        "strengthening detected but streak={} is below the promote threshold",
                        track.status_streak
                    ),
                ),
                HypothesisTrackStatus::Invalidated | HypothesisTrackStatus::Weakening => {
                    unreachable!("covered above")
                }
            }
        }
    };

    let transition_reason = previous_action.and_then(|previous| {
        if previous == action {
            None
        } else if action_priority(action) < action_priority(previous) {
            Some(format!(
                "promoted from {} to {} because {}",
                previous, action, reason
            ))
        } else {
            Some(format!(
                "downgraded from {} to {} because {}",
                previous, action, reason
            ))
        }
    });

    let full_reason = if let Some(prior_reason) = &prior_reason {
        format!("{} | {}", reason, prior_reason)
    } else {
        reason
    };
    let review_reason_code = if action == "review" {
        if matches!(track.status, HypothesisTrackStatus::Invalidated) {
            Some(ReviewReasonCode::LeadingInvalidated)
        } else if matches!(track.status, HypothesisTrackStatus::Weakening) {
            Some(ReviewReasonCode::Weakening)
        } else if directional_conflict_present {
            Some(ReviewReasonCode::DirectionalConflict)
        } else if !fresh_symbol_confirmation && previous_action == Some("enter") {
            Some(ReviewReasonCode::StaleSymbolConfirmation)
        } else if blocked_by_regime.is_some() {
            Some(ReviewReasonCode::RegimeBlocked)
        } else if matches!(
            track.status,
            HypothesisTrackStatus::Strengthening | HypothesisTrackStatus::New
        ) {
            Some(ReviewReasonCode::PersistenceBuilding)
        } else {
            None
        }
    } else {
        None
    };

    TrackActionDecision {
        action,
        reason: full_reason.clone(),
        review_reason_code,
        transition_reason,
        policy_verdict: build_policy_verdict(
            setup,
            track,
            previous_action,
            action,
            &full_reason,
            review_reason_code,
            prior_signal,
            blocked_by_regime.as_deref(),
            recent_invalidation || in_refractory_window,
        ),
        lineage: decision_lineage(
            setup,
            track,
            previous_action,
            action,
            timestamp,
            lineage_prior,
            blocked_by_regime.as_deref(),
        ),
    }
}

fn setup_family_key<'a>(setup: &'a TacticalSetup) -> Option<&'a str> {
    setup
        .risk_notes
        .iter()
        .find_map(|note| note.strip_prefix("family="))
}

fn decision_lineage(
    setup: &TacticalSetup,
    track: &HypothesisTrack,
    previous_action: Option<&str>,
    action: &str,
    timestamp: OffsetDateTime,
    lineage_prior: Option<&FamilyContextLineageOutcome>,
    blocked_reason: Option<&str>,
) -> DecisionLineage {
    let mut lineage = DecisionLineage {
        based_on: vec![
            setup.hypothesis_id.clone(),
            format!("confidence_gap={}", setup.confidence_gap.round_dp(4)),
            format!("heuristic_edge={}", setup.heuristic_edge.round_dp(4)),
            format!("track_status={}", track.status),
        ],
        blocked_by: Vec::new(),
        promoted_by: Vec::new(),
        falsified_by: Vec::new(),
    };

    if let Some(reason) = blocked_reason {
        lineage.blocked_by.push(reason.to_string());
    }
    if let Some(prior) = lineage_prior {
        let context_label = format!(
            "family_context={} session={} regime={} resolved={} net={} follow={} invalidation={}",
            prior.family,
            prior.session,
            prior.market_regime,
            prior.resolved,
            prior.mean_net_return.round_dp(4),
            prior.follow_through_rate.round_dp(3),
            prior.invalidation_rate.round_dp(3),
        );
        lineage.based_on.push(context_label.clone());
        match classify_lineage_prior(prior) {
            PriorSignal::Positive if action == "enter" => {
                lineage.promoted_by.push(context_label);
            }
            PriorSignal::Negative if action != "enter" => {
                lineage.blocked_by.push(context_label);
            }
            _ => {}
        }
        lineage
            .based_on
            .push(format!("session={}", hk_session_label(timestamp)));
    }
    if let Some(previous_action) = previous_action {
        if previous_action != action && action_priority(action) < action_priority(previous_action) {
            lineage
                .promoted_by
                .push(format!("{} -> {}", previous_action, action));
        } else if previous_action != action {
            lineage
                .blocked_by
                .push(format!("{} -> {}", previous_action, action));
        }
    }
    if let Some(reason) = setup
        .risk_notes
        .iter()
        .find_map(|note| note.strip_prefix("invalidates_on="))
    {
        lineage.falsified_by.push(reason.to_string());
    }

    lineage
}

fn build_policy_verdict(
    setup: &TacticalSetup,
    track: &HypothesisTrack,
    previous_action: Option<&str>,
    action: &str,
    reason: &str,
    review_reason_code: Option<ReviewReasonCode>,
    prior_signal: PriorSignal,
    blocked_reason: Option<&str>,
    recent_negative: bool,
) -> PolicyVerdictSummary {
    let primary = primary_policy_verdict(
        track,
        previous_action,
        action,
        prior_signal,
        blocked_reason,
        recent_negative,
    );
    let conflict_reason = if let Some(blocked_reason) = blocked_reason {
        Some(format!(
            "market regime blocks escalation: {}",
            blocked_reason
        ))
    } else if primary == PolicyVerdictKind::LineageConflict {
        Some(
            "live setup remains structurally healthy, but lineage prior is unfavorable for this context"
                .into(),
        )
    } else {
        None
    };

    PolicyVerdictSummary {
        primary,
        rationale: reason.to_string(),
        review_reason_code,
        conflict_reason: conflict_reason.clone(),
        horizons: horizon_policy_verdicts(
            primary,
            track,
            reason,
            conflict_reason.as_deref(),
            setup,
        ),
    }
}

fn primary_policy_verdict(
    track: &HypothesisTrack,
    previous_action: Option<&str>,
    action: &str,
    prior_signal: PriorSignal,
    blocked_reason: Option<&str>,
    recent_negative: bool,
) -> PolicyVerdictKind {
    if blocked_reason.is_some() || recent_negative {
        return PolicyVerdictKind::Avoid;
    }

    match action {
        "enter" if previous_action == Some("enter") => PolicyVerdictKind::Active,
        "enter" => PolicyVerdictKind::EnterReady,
        "review"
            if matches!(
                track.status,
                HypothesisTrackStatus::Invalidated | HypothesisTrackStatus::Weakening
            ) =>
        {
            PolicyVerdictKind::ExitRequired
        }
        "review" => PolicyVerdictKind::ReviewRequired,
        "observe"
            if prior_signal == PriorSignal::Negative
                && matches!(
                    track.status,
                    HypothesisTrackStatus::Stable | HypothesisTrackStatus::Strengthening
                ) =>
        {
            PolicyVerdictKind::LineageConflict
        }
        "observe"
            if matches!(
                track.status,
                HypothesisTrackStatus::New
                    | HypothesisTrackStatus::Stable
                    | HypothesisTrackStatus::Strengthening
            ) =>
        {
            PolicyVerdictKind::PersistenceBuilding
        }
        _ => PolicyVerdictKind::Avoid,
    }
}

fn horizon_policy_verdicts(
    primary: PolicyVerdictKind,
    track: &HypothesisTrack,
    reason: &str,
    conflict_reason: Option<&str>,
    setup: &TacticalSetup,
) -> Vec<HorizonPolicyVerdict> {
    let short = match primary {
        PolicyVerdictKind::EnterReady => PolicyVerdictKind::EnterReady,
        PolicyVerdictKind::Active => PolicyVerdictKind::Active,
        PolicyVerdictKind::ExitRequired => PolicyVerdictKind::ExitRequired,
        PolicyVerdictKind::ReviewRequired => PolicyVerdictKind::PersistenceBuilding,
        PolicyVerdictKind::LineageConflict => PolicyVerdictKind::Avoid,
        PolicyVerdictKind::PersistenceBuilding => PolicyVerdictKind::Avoid,
        PolicyVerdictKind::AttentionCapped => PolicyVerdictKind::Avoid,
        PolicyVerdictKind::Avoid => PolicyVerdictKind::Avoid,
    };
    let medium = match primary {
        PolicyVerdictKind::EnterReady => PolicyVerdictKind::Active,
        PolicyVerdictKind::Active => PolicyVerdictKind::Active,
        PolicyVerdictKind::ExitRequired => PolicyVerdictKind::Avoid,
        PolicyVerdictKind::ReviewRequired => PolicyVerdictKind::ReviewRequired,
        PolicyVerdictKind::LineageConflict => PolicyVerdictKind::ReviewRequired,
        PolicyVerdictKind::PersistenceBuilding => PolicyVerdictKind::PersistenceBuilding,
        PolicyVerdictKind::AttentionCapped => PolicyVerdictKind::ReviewRequired,
        PolicyVerdictKind::Avoid => PolicyVerdictKind::Avoid,
    };
    let long = match primary {
        PolicyVerdictKind::EnterReady | PolicyVerdictKind::Active => {
            PolicyVerdictKind::PersistenceBuilding
        }
        PolicyVerdictKind::ExitRequired => PolicyVerdictKind::Avoid,
        PolicyVerdictKind::ReviewRequired => PolicyVerdictKind::PersistenceBuilding,
        PolicyVerdictKind::LineageConflict => PolicyVerdictKind::Avoid,
        PolicyVerdictKind::PersistenceBuilding => PolicyVerdictKind::Avoid,
        PolicyVerdictKind::AttentionCapped => PolicyVerdictKind::PersistenceBuilding,
        PolicyVerdictKind::Avoid => PolicyVerdictKind::Avoid,
    };

    let medium_reason = match primary {
        PolicyVerdictKind::LineageConflict => conflict_reason
            .map(|reason| {
                format!(
                    "medium horizon keeps the case alive despite conflict: {}",
                    reason
                )
            })
            .unwrap_or_else(|| reason.to_string()),
        PolicyVerdictKind::PersistenceBuilding => format!(
            "medium horizon keeps watching while persistence builds (status={} streak={})",
            track.status, track.status_streak
        ),
        PolicyVerdictKind::EnterReady => format!(
            "medium horizon can carry the case after entry (conf={} gap={})",
            setup.confidence.round_dp(3),
            setup.confidence_gap.round_dp(3)
        ),
        _ => reason.to_string(),
    };

    vec![
        HorizonPolicyVerdict {
            horizon: "15t".into(),
            verdict: short,
            rationale: reason.to_string(),
        },
        HorizonPolicyVerdict {
            horizon: "50t".into(),
            verdict: medium,
            rationale: medium_reason,
        },
        HorizonPolicyVerdict {
            horizon: "150t".into(),
            verdict: long,
            rationale: reason.to_string(),
        },
    ]
}

fn best_lineage_prior<'a>(
    setup: &TacticalSetup,
    timestamp: OffsetDateTime,
    market_regime: &MarketRegimeFilter,
    lineage_priors: &'a [FamilyContextLineageOutcome],
) -> Option<&'a FamilyContextLineageOutcome> {
    let family = setup_family(setup)?;
    let session = hk_session_label(timestamp);
    let regime = market_regime.bias.as_str();

    let best = |items: Vec<&'a FamilyContextLineageOutcome>| {
        items.into_iter().max_by(|left, right| {
            left.resolved
                .cmp(&right.resolved)
                .then_with(|| left.mean_net_return.cmp(&right.mean_net_return))
                .then_with(|| left.follow_through_rate.cmp(&right.follow_through_rate))
        })
    };

    best(
        lineage_priors
            .iter()
            .filter(|item| {
                item.family == family && item.session == session && item.market_regime == regime
            })
            .collect(),
    )
    .or_else(|| {
        best(
            lineage_priors
                .iter()
                .filter(|item| item.family == family && item.session == session)
                .collect(),
        )
    })
    .or_else(|| {
        best(
            lineage_priors
                .iter()
                .filter(|item| item.family == family)
                .collect(),
        )
    })
}

fn classify_lineage_prior(prior: &FamilyContextLineageOutcome) -> PriorSignal {
    if prior.resolved < 5 {
        return PriorSignal::Neutral;
    }

    // Tier 1: catastrophic — any two strongly negative dimensions
    let catastrophic = prior.mean_net_return < Decimal::new(-1, 2)
        && prior.follow_through_rate < Decimal::new(30, 2)
        && prior.invalidation_rate > Decimal::new(60, 2);
    if catastrophic {
        return PriorSignal::Negative;
    }

    // Tier 2: sustained underperformance with enough data
    // 30+ resolved, negative net, AND poor follow-through → Negative
    let sustained_underperformance = prior.resolved >= 30
        && prior.mean_net_return < Decimal::ZERO
        && prior.follow_through_rate < Decimal::new(25, 2);
    if sustained_underperformance {
        return PriorSignal::Negative;
    }

    // Tier 3: large sample, net clearly negative even with decent follow-through
    let large_sample_negative =
        prior.resolved >= 100 && prior.mean_net_return < Decimal::new(-2, 2);
    if large_sample_negative {
        return PriorSignal::Negative;
    }

    if prior.mean_net_return > Decimal::ZERO && prior.follow_through_rate >= Decimal::new(45, 2) {
        PriorSignal::Positive
    } else {
        PriorSignal::Neutral
    }
}

/// Positive feedback: families with proven alpha get easier promotion thresholds.
/// Returns 0.0 (no boost) to 1.0 (maximum boost).
/// Mirror of `should_block_family_alpha` — that kills losers, this amplifies winners.
fn compute_alpha_boost(prior: &FamilyContextLineageOutcome) -> Decimal {
    if prior.resolved < 15 {
        return Decimal::ZERO;
    }
    let has_positive_return = prior.mean_net_return > Decimal::new(5, 3);
    let has_good_follow = prior.follow_through_rate >= Decimal::new(40, 2);
    let has_low_invalidation = prior.invalidation_rate < Decimal::new(30, 2);
    if !has_positive_return || !has_good_follow {
        return Decimal::ZERO;
    }
    let mut boost = Decimal::new(2, 1);
    if prior.resolved >= 30 && has_low_invalidation {
        boost = Decimal::new(5, 1);
    }
    if prior.resolved >= 60
        && prior.mean_net_return > Decimal::new(1, 2)
        && prior.follow_through_rate >= Decimal::new(50, 2)
    {
        boost = Decimal::new(8, 1);
    }
    if prior.resolved >= 100
        && prior.mean_net_return > Decimal::new(15, 3)
        && prior.follow_through_rate >= Decimal::new(55, 2)
    {
        boost = Decimal::ONE;
    }
    boost
}

#[cfg(test)]
pub fn compute_alpha_boost_for_test(prior: &FamilyContextLineageOutcome) -> Decimal {
    compute_alpha_boost(prior)
}

fn describe_lineage_prior(prior: &FamilyContextLineageOutcome) -> String {
    format!(
        "lineage prior family={} session={} regime={} resolved={} net={} follow={} invalidation={}",
        prior.family,
        prior.session,
        prior.market_regime,
        prior.resolved,
        prior.mean_net_return.round_dp(4),
        prior.follow_through_rate.round_dp(3),
        prior.invalidation_rate.round_dp(3),
    )
}

fn setup_family(setup: &TacticalSetup) -> Option<&str> {
    setup
        .risk_notes
        .iter()
        .find_map(|note| note.strip_prefix("family="))
}

fn compare_attention_priority(
    left: &TacticalSetup,
    right: &TacticalSetup,
    track_map: &HashMap<&str, &HypothesisTrack>,
    previous_track_map: &HashMap<&str, &HypothesisTrack>,
) -> std::cmp::Ordering {
    let left_track_id = track_id_for_scope(&left.scope);
    let right_track_id = track_id_for_scope(&right.scope);
    let left_track = track_map.get(left_track_id.as_str()).copied();
    let right_track = track_map.get(right_track_id.as_str()).copied();
    let left_previous = previous_track_map.get(left_track_id.as_str()).copied();
    let right_previous = previous_track_map.get(right_track_id.as_str()).copied();

    previous_enter_priority(left_previous)
        .cmp(&previous_enter_priority(right_previous))
        .then_with(|| {
            action_budget_priority(left.action.as_str())
                .cmp(&action_budget_priority(right.action.as_str()))
        })
        .then_with(|| track_priority(left_track).cmp(&track_priority(right_track)))
        .then_with(|| streak_priority(left_track).cmp(&streak_priority(right_track)))
        .then_with(|| left.heuristic_edge.cmp(&right.heuristic_edge))
        .then_with(|| left.confidence_gap.cmp(&right.confidence_gap))
        .then_with(|| left.confidence.cmp(&right.confidence))
        .reverse()
}

fn previous_enter_priority(previous_track: Option<&HypothesisTrack>) -> i32 {
    if previous_track.map(|track| track.action.as_str()) == Some("enter") {
        1
    } else {
        0
    }
}

fn action_budget_priority(action: &str) -> i32 {
    match action {
        "enter" => 2,
        "review" => 1,
        _ => 0,
    }
}

fn track_priority(track: Option<&HypothesisTrack>) -> i32 {
    match track.map(|track| track.status) {
        Some(HypothesisTrackStatus::Strengthening) => 4,
        Some(HypothesisTrackStatus::Stable) => 3,
        Some(HypothesisTrackStatus::New) => 2,
        Some(HypothesisTrackStatus::Weakening) => 1,
        Some(HypothesisTrackStatus::Invalidated) => 0,
        None => 0,
    }
}

fn streak_priority(track: Option<&HypothesisTrack>) -> u64 {
    track.map(|track| track.status_streak).unwrap_or(0)
}

fn demote_setup_for_budget(setup: &mut TacticalSetup, target_action: &str, reason: &str) {
    if setup.action == target_action {
        return;
    }
    let previous_action = setup.action.clone();
    setup.action = target_action.into();
    if target_action == "review" {
        setup.review_reason_code = Some(ReviewReasonCode::AttentionCapped);
    }
    let mut provenance = setup
        .provenance
        .clone()
        .with_trace_id(setup.setup_id.clone());
    provenance.note = Some(reason.to_string());
    setup.provenance = provenance;
    setup.lineage.blocked_by.push(format!(
        "case_budget {} -> {} because {}",
        previous_action, target_action, reason
    ));
    setup
        .risk_notes
        .insert(0, format!("policy_gate: {}", reason));
    if let Some(verdict) = setup.policy_verdict.as_mut() {
        verdict.primary = PolicyVerdictKind::AttentionCapped;
        verdict.rationale = reason.to_string();
        verdict.review_reason_code = Some(ReviewReasonCode::AttentionCapped);
        verdict.conflict_reason = Some(
            "attention budget capped a live case; keep it visible without promoting it".into(),
        );
        verdict.horizons = vec![
            HorizonPolicyVerdict {
                horizon: "15t".into(),
                verdict: PolicyVerdictKind::Avoid,
                rationale: reason.to_string(),
            },
            HorizonPolicyVerdict {
                horizon: "50t".into(),
                verdict: PolicyVerdictKind::ReviewRequired,
                rationale: reason.to_string(),
            },
            HorizonPolicyVerdict {
                horizon: "150t".into(),
                verdict: PolicyVerdictKind::PersistenceBuilding,
                rationale: reason.to_string(),
            },
        ];
    }
}

fn recently_invalidated(
    previous_track: Option<&HypothesisTrack>,
    timestamp: OffsetDateTime,
) -> bool {
    const REFRACTORY_WINDOW_SECS: i64 = 90;
    previous_track
        .and_then(|track| {
            track
                .invalidated_at
                .or_else(|| {
                    matches!(track.status, HypothesisTrackStatus::Weakening)
                        .then_some(track.last_updated_at)
                })
                .map(|last_negative_at| {
                    timestamp.unix_timestamp() - last_negative_at.unix_timestamp()
                })
        })
        .map(|delta| delta >= 0 && delta < REFRACTORY_WINDOW_SECS)
        .unwrap_or(false)
}

fn local_support_from_reason(setup: &TacticalSetup) -> Decimal {
    setup
        .risk_notes
        .iter()
        .find_map(|note| note.strip_prefix("local_support="))
        .and_then(|value| value.parse::<Decimal>().ok())
        .unwrap_or(Decimal::ZERO)
}

fn directional_support_from_notes(setup: &TacticalSetup) -> Decimal {
    note_decimal(setup, "directional_support=").unwrap_or(Decimal::ZERO)
}

fn directional_conflict_from_notes(setup: &TacticalSetup) -> Decimal {
    note_decimal(setup, "directional_conflict=").unwrap_or(Decimal::ZERO)
}

fn fresh_symbol_confirmation_from_notes(setup: &TacticalSetup) -> bool {
    note_bool(setup, "fresh_symbol_confirmation=").unwrap_or(true)
}

fn directional_conflict_present_from_notes(setup: &TacticalSetup) -> bool {
    note_bool(setup, "directional_conflict_present=").unwrap_or(false)
}

fn symbol_event_count_from_notes(setup: &TacticalSetup) -> usize {
    setup
        .risk_notes
        .iter()
        .find_map(|note| note.strip_prefix("symbol_event_count="))
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(0)
}

fn note_decimal(setup: &TacticalSetup, prefix: &str) -> Option<Decimal> {
    setup
        .risk_notes
        .iter()
        .find_map(|note| note.strip_prefix(prefix))
        .and_then(|value| value.parse::<Decimal>().ok())
}

fn note_bool(setup: &TacticalSetup, prefix: &str) -> Option<bool> {
    setup
        .risk_notes
        .iter()
        .find_map(|note| note.strip_prefix(prefix))
        .and_then(|value| value.parse::<bool>().ok())
}

fn setup_order_direction(setup: &TacticalSetup) -> Option<OrderDirection> {
    if let Some(workflow_id) = setup.workflow_id.as_deref() {
        if workflow_id.ends_with(":buy") {
            return Some(OrderDirection::Buy);
        }
        if workflow_id.ends_with(":sell") {
            return Some(OrderDirection::Sell);
        }
    }

    if setup.title.starts_with("Long ") {
        Some(OrderDirection::Buy)
    } else if setup.title.starts_with("Short ") {
        Some(OrderDirection::Sell)
    } else {
        None
    }
}

fn setup_policy_reason_override(setup: &TacticalSetup) -> Option<String> {
    setup.risk_notes.iter().find_map(|note| {
        note.strip_prefix("policy_gate: ")
            .map(std::borrow::ToOwned::to_owned)
    })
}

fn setup_transition_reason_override(setup: &TacticalSetup) -> Option<String> {
    setup.risk_notes.iter().find_map(|note| {
        note.strip_prefix("policy_transition: ")
            .map(std::borrow::ToOwned::to_owned)
    })
}

fn policy_reason_for_setup(
    setup: &TacticalSetup,
    status: HypothesisTrackStatus,
    status_streak: u64,
) -> String {
    if let Some(reason) = setup_policy_reason_override(setup) {
        return reason;
    }

    if setup.workflow_id.is_none() {
        return format!(
            "scope-level case status={} streak={} gap={}",
            status,
            status_streak,
            setup.confidence_gap.round_dp(3)
        );
    }

    match status {
        HypothesisTrackStatus::Invalidated => {
            "current tick no longer supports the prior leading case".into()
        }
        HypothesisTrackStatus::Weakening => format!(
            "case weakened; confidence={} gap={}",
            setup.confidence.round_dp(3),
            setup.confidence_gap.round_dp(3)
        ),
        HypothesisTrackStatus::Strengthening => format!(
            "case strengthened for {} ticks with gap {} and edge {}",
            status_streak,
            setup.confidence_gap.round_dp(3),
            setup.heuristic_edge.round_dp(3)
        ),
        HypothesisTrackStatus::Stable => format!(
            "case remains stable with confidence {} and gap {}",
            setup.confidence.round_dp(3),
            setup.confidence_gap.round_dp(3)
        ),
        HypothesisTrackStatus::New => format!(
            "new case seeded with confidence {} and gap {}",
            setup.confidence.round_dp(3),
            setup.confidence_gap.round_dp(3)
        ),
    }
}

fn transition_reason_for_setup(
    setup: &TacticalSetup,
    previous_track: Option<&HypothesisTrack>,
    policy_reason: &str,
) -> Option<String> {
    if let Some(reason) = setup_transition_reason_override(setup) {
        return Some(reason);
    }

    let previous_action = previous_track.map(|track| track.action.as_str())?;
    if previous_action == setup.action {
        None
    } else if action_priority(&setup.action) < action_priority(previous_action) {
        Some(format!(
            "promoted from {} to {} because {}",
            previous_action, setup.action, policy_reason
        ))
    } else {
        Some(format!(
            "downgraded from {} to {} because {}",
            previous_action, setup.action, policy_reason
        ))
    }
}

pub(super) fn action_priority(action: &str) -> i32 {
    match action {
        "enter" => 0,
        "review" => 1,
        "observe" => 2,
        _ => 3,
    }
}

#[cfg(test)]
pub fn classify_lineage_prior_for_test(prior: &FamilyContextLineageOutcome) -> PriorSignal {
    classify_lineage_prior(prior)
}
