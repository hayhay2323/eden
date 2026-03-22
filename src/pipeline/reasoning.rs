use std::collections::{HashMap, HashSet};

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::graph::decision::{DecisionSnapshot, MarketRegimeFilter, OrderDirection};
use crate::graph::insights::GraphInsights;
use crate::ontology::reasoning::{
    CaseCluster, DecisionLineage, EvidencePolarity, Hypothesis, HypothesisTrack,
    HypothesisTrackStatus, InvalidationCondition, PropagationPath, PropagationStep,
    ReasoningEvidence, ReasoningEvidenceKind, ReasoningScope, TacticalSetup,
};

use super::signals::{DerivedSignalSnapshot, EventSnapshot, MarketEventKind, SignalScope};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningSnapshot {
    pub timestamp: OffsetDateTime,
    pub hypotheses: Vec<Hypothesis>,
    pub propagation_paths: Vec<PropagationPath>,
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
        let propagation_paths = derive_propagation_paths(insights, decision.timestamp);
        let hypotheses = derive_hypotheses(events, derived_signals, &propagation_paths);
        let baseline_setups = derive_tactical_setups(decision, &hypotheses);
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
            &decision.market_regime,
        );
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
            tactical_setups,
            hypothesis_tracks,
            case_clusters,
        }
    }
}

fn derive_propagation_paths(
    insights: &GraphInsights,
    observed_at: OffsetDateTime,
) -> Vec<PropagationPath> {
    const MAX_ONE_HOP_PATHS: usize = 20;
    const MAX_TWO_HOP_PATHS: usize = 20;
    const MAX_THREE_HOP_SEEDS: usize = 8;
    const MAX_THREE_HOP_PATHS: usize = 12;
    let hop_decay_2 = Decimal::new(85, 2);
    let hop_decay_3 = Decimal::new(70, 2);

    let mut one_hop_paths = Vec::new();
    one_hop_paths.extend(rotation_one_hop_paths(insights, observed_at));
    one_hop_paths.extend(shared_holder_one_hop_paths(insights));
    one_hop_paths.extend(shared_holder_bridge_paths(insights));
    one_hop_paths.extend(market_stress_sector_paths(insights));

    let two_hop_paths = derive_two_hop_paths(&one_hop_paths, hop_decay_2)
        .into_iter()
        .take(MAX_TWO_HOP_PATHS)
        .collect::<Vec<_>>();
    let three_hop_paths = derive_three_hop_paths(
        &two_hop_paths
            .iter()
            .take(MAX_THREE_HOP_SEEDS)
            .cloned()
            .collect::<Vec<_>>(),
        &one_hop_paths,
        hop_decay_3,
    )
    .into_iter()
    .take(MAX_THREE_HOP_PATHS)
    .collect::<Vec<_>>();

    one_hop_paths.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.path_id.cmp(&b.path_id))
    });
    let mut paths = one_hop_paths
        .into_iter()
        .take(MAX_ONE_HOP_PATHS)
        .collect::<Vec<_>>();
    paths.extend(two_hop_paths);
    paths.extend(three_hop_paths);
    paths = canonicalize_paths(paths);
    paths.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.steps.len().cmp(&b.steps.len()))
            .then_with(|| a.path_id.cmp(&b.path_id))
    });
    paths
}

fn derive_hypotheses(
    events: &EventSnapshot,
    derived_signals: &DerivedSignalSnapshot,
    propagation_paths: &[PropagationPath],
) -> Vec<Hypothesis> {
    let mut scopes = events
        .events
        .iter()
        .map(|event| convert_scope(&event.value.scope))
        .collect::<Vec<_>>();
    for path in propagation_paths {
        for step in &path.steps {
            scopes.push(step.from.clone());
            scopes.push(step.to.clone());
        }
    }
    scopes.sort_by_key(scope_id);
    scopes.dedup();

    let mut hypotheses = Vec::new();

    for scope in scopes {
        let relevant_events = events
            .events
            .iter()
            .filter(|event| scope_matches_event(&scope, &event.value.scope))
            .collect::<Vec<_>>();
        let relevant_signals = derived_signals
            .signals
            .iter()
            .filter(|signal| scope_matches_signal_or_market(&scope, &signal.value.scope))
            .collect::<Vec<_>>();
        let relevant_paths = propagation_paths
            .iter()
            .filter(|path| path_relevant_to_scope(path, &scope))
            .collect::<Vec<_>>();
        let templates = hypothesis_templates(&relevant_events, &relevant_signals, &relevant_paths);
        for template in &templates {
            let mut evidence = Vec::new();

            for event in &relevant_events {
                if let Some(polarity) = event_polarity(template, &event.value.kind) {
                    evidence.push(ReasoningEvidence {
                        statement: event.value.summary.clone(),
                        kind: ReasoningEvidenceKind::LocalEvent,
                        polarity,
                        weight: event.value.magnitude.min(Decimal::ONE),
                        references: event.provenance.inputs.clone(),
                        provenance: event.provenance.clone(),
                    });
                }
            }

            for signal in &relevant_signals {
                if let Some(polarity) = signal_polarity(template, &signal.value.kind) {
                    evidence.push(ReasoningEvidence {
                        statement: signal.value.summary.clone(),
                        kind: ReasoningEvidenceKind::LocalSignal,
                        polarity,
                        weight: signal.value.strength.abs().min(Decimal::ONE),
                        references: signal.provenance.inputs.clone(),
                        provenance: signal.provenance.clone(),
                    });
                }
            }

            let (path_weight, path_ids) =
                propagated_path_evidence(&scope, &evidence, propagation_paths);
            if path_weight > Decimal::ZERO {
                let polarity = path_polarity(template);
                evidence.push(ReasoningEvidence {
                    statement: if polarity == EvidencePolarity::Supports {
                        format!("propagation paths align with {}", template.thesis)
                    } else {
                        format!("propagation paths do not support {}", template.thesis)
                    },
                    kind: ReasoningEvidenceKind::PropagatedPath,
                    polarity,
                    weight: path_weight,
                    references: path_ids.clone(),
                    provenance: derived_provenance(events.timestamp, path_weight, &path_ids),
                });
            }

            let evidence_summary = summarize_evidence_weights(&evidence);
            let support_count = evidence
                .iter()
                .filter(|item| item.polarity == EvidencePolarity::Supports)
                .count();
            if support_count == 0 {
                continue;
            }

            hypotheses.push(Hypothesis {
                hypothesis_id: format!("hyp:{}:{}", scope_id(&scope), template.key),
                family_key: template.key.clone(),
                family_label: template.family_label.clone(),
                provenance: hypothesis_provenance(
                    events.timestamp,
                    &format!("hyp:{}:{}", scope_id(&scope), template.key),
                    &template.family_label,
                    &evidence,
                    &path_ids,
                ),
                scope: scope.clone(),
                statement: template_statement(template, &scope),
                confidence: competing_hypothesis_confidence(&evidence),
                local_support_weight: evidence_summary.local_support,
                local_contradict_weight: evidence_summary.local_contradict,
                propagated_support_weight: evidence_summary.propagated_support,
                propagated_contradict_weight: evidence_summary.propagated_contradict,
                evidence,
                invalidation_conditions: template_invalidation(template),
                propagation_path_ids: path_ids.clone(),
                expected_observations: template_expected_observations(template),
            });
        }
    }

    hypotheses.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.hypothesis_id.cmp(&b.hypothesis_id))
    });
    hypotheses
}

fn derive_tactical_setups(
    decision: &DecisionSnapshot,
    hypotheses: &[Hypothesis],
) -> Vec<TacticalSetup> {
    let mut setups = decision
        .order_suggestions
        .iter()
        .map(|suggestion| {
            let scope = ReasoningScope::Symbol(suggestion.symbol.clone());
            let ranked_hypotheses = hypotheses
                .iter()
                .filter(|hypothesis| hypothesis.scope == scope)
                .collect::<Vec<_>>();
            let linked_hypothesis = ranked_hypotheses.first().map(|hypothesis| {
                (
                    hypothesis.hypothesis_id.clone(),
                    hypothesis.statement.clone(),
                    hypothesis.confidence,
                    hypothesis.local_support_weight,
                )
            });
            let runner_up_hypothesis = ranked_hypotheses
                .get(1)
                .map(|hypothesis| hypothesis.hypothesis_id.clone());

            let hypothesis_margin = if ranked_hypotheses.len() >= 2 {
                ranked_hypotheses[0].confidence - ranked_hypotheses[1].confidence
            } else {
                Decimal::ONE
            };

            let action =
                if suggestion.requires_confirmation || hypothesis_margin < Decimal::new(1, 1) {
                    "review"
                } else if suggestion.heuristic_edge > Decimal::ZERO {
                    "enter"
                } else {
                    "observe"
                };
            let title = format!(
                "{} {}",
                match suggestion.direction {
                    crate::graph::decision::OrderDirection::Buy => "Long",
                    crate::graph::decision::OrderDirection::Sell => "Short",
                },
                suggestion.symbol
            );

            TacticalSetup {
                setup_id: format!("setup:{}:{}", suggestion.symbol, action),
                hypothesis_id: linked_hypothesis
                    .as_ref()
                    .map(|(id, _, _, _)| id.clone())
                    .unwrap_or_else(|| format!("hyp:{}:convergence", suggestion.symbol)),
                runner_up_hypothesis_id: runner_up_hypothesis.clone(),
                provenance: setup_provenance(
                    decision.timestamp,
                    &format!("setup:{}:{}", suggestion.symbol, action),
                    linked_hypothesis.as_ref().map(|(id, _, _, _)| id.as_str()),
                    runner_up_hypothesis.as_deref(),
                    [
                        format!("order_suggestion:{}", suggestion.symbol),
                        format!("heuristic_edge:{}", suggestion.heuristic_edge.round_dp(4)),
                        format!("estimated_cost:{}", suggestion.estimated_cost.round_dp(4)),
                    ],
                ),
                lineage: DecisionLineage::default(),
                scope,
                title,
                action: action.into(),
                time_horizon: "intraday".into(),
                confidence: linked_hypothesis
                    .as_ref()
                    .map(|(_, _, confidence, _)| *confidence)
                    .unwrap_or(suggestion.effective_confidence),
                confidence_gap: hypothesis_margin,
                heuristic_edge: suggestion.heuristic_edge,
                workflow_id: Some(format!(
                    "order:{}:{}",
                    suggestion.symbol,
                    match suggestion.direction {
                        crate::graph::decision::OrderDirection::Buy => "buy",
                        crate::graph::decision::OrderDirection::Sell => "sell",
                    }
                )),
                entry_rationale: linked_hypothesis
                    .as_ref()
                    .map(|(_, statement, _, _)| statement.clone())
                    .unwrap_or_else(|| "structural convergence without explicit hypothesis".into()),
                risk_notes: vec![
                    format!(
                        "estimated execution cost={}",
                        suggestion.estimated_cost.round_dp(4)
                    ),
                    format!(
                        "convergence_score={}",
                        suggestion.convergence_score.round_dp(4)
                    ),
                    format!(
                        "effective_confidence={}",
                        suggestion.effective_confidence.round_dp(4)
                    ),
                    format!("hypothesis_margin={}", hypothesis_margin.round_dp(4)),
                    format!(
                        "local_support={}",
                        linked_hypothesis
                            .as_ref()
                            .map(|(_, _, _, local_support)| local_support.round_dp(4))
                            .unwrap_or(Decimal::ZERO)
                    ),
                    suggestion
                        .external_confirmation
                        .as_ref()
                        .map(|value| format!("external_confirmation={}", value))
                        .unwrap_or_else(|| "external_confirmation=".into()),
                    suggestion
                        .external_support_slug
                        .as_ref()
                        .map(|value| format!("external_support_slug={}", value))
                        .unwrap_or_else(|| "external_support_slug=".into()),
                    suggestion
                        .external_support_probability
                        .map(|value| format!("external_support_probability={}", value.round_dp(4)))
                        .unwrap_or_else(|| "external_support_probability=".into()),
                    suggestion
                        .external_conflict
                        .as_ref()
                        .map(|value| format!("external_conflict={}", value))
                        .unwrap_or_else(|| "external_conflict=".into()),
                    suggestion
                        .external_conflict_slug
                        .as_ref()
                        .map(|value| format!("external_conflict_slug={}", value))
                        .unwrap_or_else(|| "external_conflict_slug=".into()),
                    suggestion
                        .external_conflict_probability
                        .map(|value| format!("external_conflict_probability={}", value.round_dp(4)))
                        .unwrap_or_else(|| "external_conflict_probability=".into()),
                ],
            }
        })
        .collect::<Vec<_>>();

    let symbol_scope_setups: HashMap<ReasoningScope, String> = setups
        .iter()
        .map(|setup| (setup.scope.clone(), setup.setup_id.clone()))
        .collect();

    let mut hypotheses_by_scope: HashMap<ReasoningScope, Vec<&Hypothesis>> = HashMap::new();
    for hypothesis in hypotheses {
        hypotheses_by_scope
            .entry(hypothesis.scope.clone())
            .or_default()
            .push(hypothesis);
    }

    for (scope, mut ranked) in hypotheses_by_scope {
        ranked.sort_by(|a, b| {
            b.confidence
                .cmp(&a.confidence)
                .then_with(|| a.hypothesis_id.cmp(&b.hypothesis_id))
        });
        let top = ranked[0];
        let runner_up = ranked.get(1).copied();
        let gap = runner_up
            .map(|runner_up| top.confidence - runner_up.confidence)
            .unwrap_or(Decimal::ONE);

        if symbol_scope_setups.contains_key(&scope) {
            continue;
        }

        let action = if top.confidence >= Decimal::new(7, 1) && gap >= Decimal::new(15, 2) {
            "review"
        } else {
            "observe"
        };

        setups.push(TacticalSetup {
            setup_id: format!("setup:{}:{}", scope_id(&scope), action),
            hypothesis_id: top.hypothesis_id.clone(),
            runner_up_hypothesis_id: runner_up.map(|item| item.hypothesis_id.clone()),
            provenance: setup_provenance(
                decision.timestamp,
                &format!("setup:{}:{}", scope_id(&scope), action),
                Some(top.hypothesis_id.as_str()),
                runner_up.map(|item| item.hypothesis_id.as_str()),
                [format!("scope_case:{}", scope_id(&scope))],
            ),
            lineage: DecisionLineage::default(),
            scope: scope.clone(),
            title: format!("{} tactical case", scope_title(&scope)),
            action: action.into(),
            time_horizon: "intraday".into(),
            confidence: top.confidence,
            confidence_gap: gap,
            heuristic_edge: (top.confidence * gap).round_dp(4),
            workflow_id: None,
            entry_rationale: top.statement.clone(),
            risk_notes: vec![
                "scope-level case; requires operator judgement".into(),
                format!("local_support={}", top.local_support_weight.round_dp(4)),
            ],
        });
    }

    setups
}

fn derive_hypothesis_tracks(
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

fn apply_track_action_policy(
    setups: &[TacticalSetup],
    tracks: &[HypothesisTrack],
    previous_tracks: &[HypothesisTrack],
    market_regime: &MarketRegimeFilter,
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
            let track = track_map
                .get(track_id.as_str())
                .copied()
                .expect("track exists for current setup");
            let previous_track = previous_track_map.get(track_id.as_str()).copied();
            let decision = decide_track_action(setup, track, previous_track, market_regime);

            let mut updated = setup.clone();
            updated.action = decision.action.into();
            updated.setup_id = format!("setup:{}:{}", scope_id(&setup.scope), decision.action);
            updated.lineage = decision.lineage.clone();
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

fn derive_case_clusters(
    hypotheses: &[Hypothesis],
    propagation_paths: &[PropagationPath],
    setups: &[TacticalSetup],
    tracks: &[HypothesisTrack],
) -> Vec<CaseCluster> {
    #[derive(Default)]
    struct Bucket<'a> {
        setups: Vec<&'a TacticalSetup>,
        tracks: Vec<&'a HypothesisTrack>,
        hypotheses: Vec<&'a Hypothesis>,
        path_ids: Vec<String>,
    }

    let hypothesis_map = hypotheses
        .iter()
        .map(|hypothesis| (hypothesis.hypothesis_id.as_str(), hypothesis))
        .collect::<HashMap<_, _>>();
    let track_map = tracks
        .iter()
        .filter(|track| track.invalidated_at.is_none())
        .map(|track| (track.setup_id.as_str(), track))
        .collect::<HashMap<_, _>>();
    let path_map = propagation_paths
        .iter()
        .map(|path| (path.path_id.as_str(), path))
        .collect::<HashMap<_, _>>();
    let mut buckets: HashMap<(String, String), Bucket<'_>> = HashMap::new();

    for setup in setups {
        let Some(hypothesis) = hypothesis_map.get(setup.hypothesis_id.as_str()).copied() else {
            continue;
        };
        let Some(track) = track_map.get(setup.setup_id.as_str()).copied() else {
            continue;
        };
        let family_key = hypothesis.family_key.clone();
        let linkage_key = cluster_linkage_key(hypothesis, &path_map);
        let bucket = buckets.entry((family_key, linkage_key)).or_default();
        bucket.setups.push(setup);
        bucket.tracks.push(track);
        bucket.hypotheses.push(hypothesis);
        for path_id in &hypothesis.propagation_path_ids {
            if !bucket.path_ids.contains(path_id) {
                bucket.path_ids.push(path_id.clone());
            }
        }
    }

    let mut clusters = buckets
        .into_iter()
        .filter_map(|((family_key, linkage_key), bucket)| {
            let lead_idx = strongest_member_index(&bucket.setups)?;
            let weak_idx = weakest_member_index(&bucket.setups)?;
            let lead_setup = bucket.setups[lead_idx];
            let lead_hypothesis = bucket.hypotheses[lead_idx];
            let weakest_setup = bucket.setups[weak_idx];
            let trend = cluster_trend(&bucket.tracks);
            let member_count = bucket.setups.len();
            let divisor = Decimal::from(member_count as u64);
            let average_confidence = bucket
                .setups
                .iter()
                .map(|setup| setup.confidence)
                .sum::<Decimal>()
                / divisor;
            let average_gap = bucket
                .setups
                .iter()
                .map(|setup| setup.confidence_gap)
                .sum::<Decimal>()
                / divisor;
            let average_edge = bucket
                .setups
                .iter()
                .map(|setup| setup.heuristic_edge)
                .sum::<Decimal>()
                / divisor;
            let title = cluster_title(
                &family_key,
                &linkage_key,
                member_count,
                bucket
                    .path_ids
                    .first()
                    .and_then(|id| path_map.get(id.as_str()).copied()),
            );

            Some(CaseCluster {
                cluster_id: format!("cluster:{}:{}", family_key, linkage_key),
                family_key,
                linkage_key,
                title,
                lead_hypothesis_id: lead_hypothesis.hypothesis_id.clone(),
                lead_statement: lead_hypothesis.statement.clone(),
                trend,
                member_setup_ids: bucket
                    .setups
                    .iter()
                    .map(|setup| setup.setup_id.clone())
                    .collect(),
                member_track_ids: bucket
                    .tracks
                    .iter()
                    .map(|track| track.track_id.clone())
                    .collect(),
                member_scopes: bucket
                    .setups
                    .iter()
                    .map(|setup| setup.scope.clone())
                    .collect(),
                propagation_path_ids: bucket.path_ids,
                strongest_setup_id: lead_setup.setup_id.clone(),
                weakest_setup_id: weakest_setup.setup_id.clone(),
                strongest_title: lead_setup.title.clone(),
                weakest_title: weakest_setup.title.clone(),
                member_count,
                average_confidence: average_confidence.round_dp(4),
                average_gap: average_gap.round_dp(4),
                average_edge: average_edge.round_dp(4),
            })
        })
        .collect::<Vec<_>>();

    clusters.sort_by(|a, b| {
        cluster_trend_priority(a.trend)
            .cmp(&cluster_trend_priority(b.trend))
            .then_with(|| b.average_gap.cmp(&a.average_gap))
            .then_with(|| b.average_edge.cmp(&a.average_edge))
            .then_with(|| b.member_count.cmp(&a.member_count))
            .then_with(|| a.cluster_id.cmp(&b.cluster_id))
    });
    clusters
}

fn propagated_path_evidence(
    scope: &ReasoningScope,
    local_evidence: &[ReasoningEvidence],
    propagation_paths: &[PropagationPath],
) -> (Decimal, Vec<String>) {
    let local_support = local_evidence
        .iter()
        .filter(|item| item.polarity == EvidencePolarity::Supports)
        .map(|item| item.weight)
        .sum::<Decimal>()
        .min(Decimal::ONE);
    let local_contradict = local_evidence
        .iter()
        .filter(|item| item.polarity == EvidencePolarity::Contradicts)
        .map(|item| item.weight)
        .sum::<Decimal>()
        .min(Decimal::ONE);

    let local_bonus = if local_support > Decimal::ZERO {
        Decimal::ONE + local_support * Decimal::new(25, 2)
    } else {
        Decimal::new(35, 2)
    };
    let contradiction_penalty = Decimal::ONE - local_contradict * Decimal::new(40, 2);

    let relevant = propagation_paths
        .iter()
        .filter(|path| path_relevant_to_scope(path, scope))
        .collect::<Vec<_>>();
    if relevant.is_empty() {
        return (Decimal::ZERO, Vec::new());
    }

    let mut scored = relevant
        .into_iter()
        .map(|path| {
            let hop_penalty = hop_penalty(path.steps.len());
            let effective = (path.confidence * hop_penalty * local_bonus * contradiction_penalty)
                .round_dp(4)
                .clamp(Decimal::ZERO, Decimal::ONE);
            (effective, path.path_id.clone())
        })
        .collect::<Vec<_>>();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));

    let best_weight = scored.first().map(|item| item.0).unwrap_or(Decimal::ZERO);
    let path_ids = scored
        .into_iter()
        .take(3)
        .map(|item| item.1)
        .collect::<Vec<_>>();
    (best_weight, path_ids)
}

struct EvidenceWeightSummary {
    local_support: Decimal,
    local_contradict: Decimal,
    propagated_support: Decimal,
    propagated_contradict: Decimal,
}

fn summarize_evidence_weights(evidence: &[ReasoningEvidence]) -> EvidenceWeightSummary {
    let mut summary = EvidenceWeightSummary {
        local_support: Decimal::ZERO,
        local_contradict: Decimal::ZERO,
        propagated_support: Decimal::ZERO,
        propagated_contradict: Decimal::ZERO,
    };

    for item in evidence {
        match (item.kind, item.polarity) {
            (
                ReasoningEvidenceKind::LocalEvent | ReasoningEvidenceKind::LocalSignal,
                EvidencePolarity::Supports,
            ) => summary.local_support += item.weight,
            (
                ReasoningEvidenceKind::LocalEvent | ReasoningEvidenceKind::LocalSignal,
                EvidencePolarity::Contradicts,
            ) => summary.local_contradict += item.weight,
            (ReasoningEvidenceKind::PropagatedPath, EvidencePolarity::Supports) => {
                summary.propagated_support += item.weight
            }
            (ReasoningEvidenceKind::PropagatedPath, EvidencePolarity::Contradicts) => {
                summary.propagated_contradict += item.weight
            }
        }
    }

    summary
}

fn hypothesis_provenance(
    observed_at: OffsetDateTime,
    trace_id: &str,
    family_label: &str,
    evidence: &[ReasoningEvidence],
    path_ids: &[String],
) -> crate::ontology::ProvenanceMetadata {
    let mut inputs = evidence
        .iter()
        .flat_map(|item| item.provenance.inputs.clone())
        .collect::<Vec<_>>();
    inputs.extend(path_ids.iter().cloned());
    inputs.sort();
    inputs.dedup();

    crate::ontology::ProvenanceMetadata::new(
        crate::ontology::ProvenanceSource::Computed,
        observed_at,
    )
    .with_trace_id(trace_id)
    .with_confidence(competing_hypothesis_confidence(evidence))
    .with_inputs(inputs)
    .with_note(format!("family={}", family_label))
}

fn setup_provenance<I, S>(
    observed_at: OffsetDateTime,
    trace_id: &str,
    hypothesis_id: Option<&str>,
    runner_up_hypothesis_id: Option<&str>,
    inputs: I,
) -> crate::ontology::ProvenanceMetadata
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut lineage = inputs.into_iter().map(Into::into).collect::<Vec<_>>();
    if let Some(hypothesis_id) = hypothesis_id {
        lineage.push(hypothesis_id.to_string());
    }
    if let Some(runner_up_hypothesis_id) = runner_up_hypothesis_id {
        lineage.push(runner_up_hypothesis_id.to_string());
    }
    lineage.sort();
    lineage.dedup();

    crate::ontology::ProvenanceMetadata::new(
        crate::ontology::ProvenanceSource::Computed,
        observed_at,
    )
    .with_trace_id(trace_id)
    .with_inputs(lineage)
}

fn rotation_one_hop_paths(
    insights: &GraphInsights,
    observed_at: OffsetDateTime,
) -> Vec<PropagationPath> {
    insights
        .rotations
        .iter()
        .take(10)
        .map(|rotation| {
            let confidence = rotation.spread.abs().min(Decimal::ONE);
            PropagationPath {
                path_id: format!(
                    "path:rotation:{}:{}",
                    rotation.from_sector, rotation.to_sector
                ),
                summary: format!(
                    "rotation pressure may propagate from {} to {}",
                    rotation.from_sector, rotation.to_sector
                ),
                confidence,
                steps: vec![PropagationStep {
                    from: ReasoningScope::Sector(rotation.from_sector.to_string()),
                    to: ReasoningScope::Sector(rotation.to_sector.to_string()),
                    mechanism: if rotation.widening {
                        "capital rotation widening".into()
                    } else {
                        "capital rotation narrowing".into()
                    },
                    confidence,
                    references: vec![
                        format!("rotation:{}:{}", rotation.from_sector, rotation.to_sector),
                        format!("observed_at:{}", observed_at),
                    ],
                }],
            }
        })
        .collect()
}

fn shared_holder_one_hop_paths(insights: &GraphInsights) -> Vec<PropagationPath> {
    let mut paths = Vec::new();

    for shared in insights.shared_holders.iter().take(10) {
        let confidence = shared.jaccard.min(Decimal::ONE);
        for (from, to) in [
            (&shared.symbol_a, &shared.symbol_b),
            (&shared.symbol_b, &shared.symbol_a),
        ] {
            paths.push(PropagationPath {
                path_id: format!("path:shared_holder:{}:{}", from, to),
                summary: format!(
                    "shared-holder overlap may transmit repricing between {} and {}",
                    from, to
                ),
                confidence,
                steps: vec![PropagationStep {
                    from: ReasoningScope::Symbol(from.clone()),
                    to: ReasoningScope::Symbol(to.clone()),
                    mechanism: "shared holder overlap".into(),
                    confidence,
                    references: vec![
                        format!("shared_holder:{}", from),
                        format!("shared_holder:{}", to),
                    ],
                }],
            });
        }
    }

    paths
}

fn shared_holder_bridge_paths(insights: &GraphInsights) -> Vec<PropagationPath> {
    let mut paths = Vec::new();

    for shared in insights.shared_holders.iter().take(10) {
        let confidence = shared.jaccard.min(Decimal::ONE);
        let Some(sector_a) = shared.sector_a.as_ref() else {
            continue;
        };
        let Some(sector_b) = shared.sector_b.as_ref() else {
            continue;
        };

        let bridges = [
            (
                ReasoningScope::Sector(sector_a.to_string()),
                ReasoningScope::Symbol(shared.symbol_b.clone()),
                format!(
                    "shared-holder sector spillover from {} into {}",
                    sector_a, shared.symbol_b
                ),
                format!("path:bridge:sector_symbol:{}:{}", sector_a, shared.symbol_b),
                "shared-holder sector spillover",
            ),
            (
                ReasoningScope::Sector(sector_b.to_string()),
                ReasoningScope::Symbol(shared.symbol_a.clone()),
                format!(
                    "shared-holder sector spillover from {} into {}",
                    sector_b, shared.symbol_a
                ),
                format!("path:bridge:sector_symbol:{}:{}", sector_b, shared.symbol_a),
                "shared-holder sector spillover",
            ),
            (
                ReasoningScope::Symbol(shared.symbol_a.clone()),
                ReasoningScope::Sector(sector_b.to_string()),
                format!(
                    "peer stock {} may spill into sector {}",
                    shared.symbol_a, sector_b
                ),
                format!("path:bridge:symbol_sector:{}:{}", shared.symbol_a, sector_b),
                "peer sector spillover",
            ),
            (
                ReasoningScope::Symbol(shared.symbol_b.clone()),
                ReasoningScope::Sector(sector_a.to_string()),
                format!(
                    "peer stock {} may spill into sector {}",
                    shared.symbol_b, sector_a
                ),
                format!("path:bridge:symbol_sector:{}:{}", shared.symbol_b, sector_a),
                "peer sector spillover",
            ),
        ];

        for (from, to, summary, path_id, mechanism) in bridges {
            paths.push(PropagationPath {
                path_id,
                summary,
                confidence,
                steps: vec![PropagationStep {
                    from,
                    to,
                    mechanism: mechanism.into(),
                    confidence,
                    references: vec![
                        format!("shared_holder:{}", shared.symbol_a),
                        format!("shared_holder:{}", shared.symbol_b),
                    ],
                }],
            });
        }
    }

    paths
}

fn market_stress_sector_paths(insights: &GraphInsights) -> Vec<PropagationPath> {
    let stress = insights.stress.composite_stress.min(Decimal::ONE);
    if stress <= Decimal::ZERO {
        return Vec::new();
    }

    let mut seen = HashSet::new();
    let mut paths = Vec::new();
    for rotation in insights.rotations.iter().take(8) {
        for sector in [&rotation.from_sector, &rotation.to_sector] {
            if !seen.insert(sector.to_string()) {
                continue;
            }
            let confidence =
                ((stress + rotation.spread.abs().min(Decimal::ONE)) / Decimal::from(2)).round_dp(4);
            paths.push(PropagationPath {
                path_id: format!("path:market_stress:{}", sector),
                summary: format!("market stress may concentrate into sector {}", sector),
                confidence,
                steps: vec![PropagationStep {
                    from: ReasoningScope::Market,
                    to: ReasoningScope::Sector(sector.to_string()),
                    mechanism: "market stress concentration".into(),
                    confidence,
                    references: vec![
                        format!(
                            "market_stress:{}",
                            insights.stress.composite_stress.round_dp(4)
                        ),
                        format!("rotation_sector:{}", sector),
                    ],
                }],
            });
        }
    }
    paths
}

fn derive_two_hop_paths(
    one_hop_paths: &[PropagationPath],
    hop_decay: Decimal,
) -> Vec<PropagationPath> {
    derive_extended_paths(one_hop_paths, one_hop_paths, hop_decay, 2)
}

fn derive_three_hop_paths(
    two_hop_paths: &[PropagationPath],
    one_hop_paths: &[PropagationPath],
    hop_decay: Decimal,
) -> Vec<PropagationPath> {
    derive_extended_paths(two_hop_paths, one_hop_paths, hop_decay, 3)
}

fn derive_extended_paths(
    seed_paths: &[PropagationPath],
    extension_paths: &[PropagationPath],
    hop_decay: Decimal,
    total_hops: usize,
) -> Vec<PropagationPath> {
    let mut derived = Vec::new();

    for left in seed_paths {
        let Some(left_tail) = left.steps.last() else {
            continue;
        };
        for right in extension_paths {
            let Some(right_head) = right.steps.first() else {
                continue;
            };
            if left.path_id == right.path_id || left_tail.to != right_head.from {
                continue;
            }
            if path_contains_scope(left, &right_head.to) {
                continue;
            }

            let confidence = (left.confidence * right.confidence * hop_decay).round_dp(4);
            if confidence <= Decimal::ZERO {
                continue;
            }

            let mut steps = left.steps.clone();
            steps.extend(right.steps.clone());
            let summary = format!(
                "{} -> {} via {}",
                scope_title(&left.steps[0].from),
                scope_title(&steps.last().expect("extended path tail").to),
                steps
                    .iter()
                    .map(|step| step.mechanism.as_str())
                    .collect::<Vec<_>>()
                    .join(" -> "),
            );
            let path_id = format!("path:{}hop:{}=>{}", total_hops, left.path_id, right.path_id);
            let mut references = left
                .steps
                .iter()
                .flat_map(|step| step.references.clone())
                .collect::<Vec<_>>();
            references.extend(right_head.references.clone());
            if let Some(last_step) = steps.last_mut() {
                last_step.references.extend(references);
            }

            derived.push(PropagationPath {
                path_id,
                summary,
                confidence,
                steps,
            });
        }
    }

    derived.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.path_id.cmp(&b.path_id))
    });
    derived.dedup_by(|a, b| a.path_id == b.path_id);
    derived
}

fn hop_penalty(hops: usize) -> Decimal {
    match hops {
        0 | 1 => Decimal::ONE,
        2 => Decimal::new(80, 2),
        3 => Decimal::new(60, 2),
        _ => Decimal::new(50, 2),
    }
}

fn canonicalize_paths(paths: Vec<PropagationPath>) -> Vec<PropagationPath> {
    let mut ranked = paths;
    ranked.sort_by(|a, b| {
        b.confidence
            .cmp(&a.confidence)
            .then_with(|| a.steps.len().cmp(&b.steps.len()))
            .then_with(|| a.path_id.cmp(&b.path_id))
    });

    let mut seen = HashSet::new();
    let mut canonical = Vec::new();
    for path in ranked {
        let key = canonical_path_key(&path);
        if seen.insert(key) {
            canonical.push(path);
        }
    }
    canonical
}

fn canonical_path_key(path: &PropagationPath) -> String {
    let forward = path_directional_signature(path);
    if path
        .steps
        .iter()
        .all(|step| mechanism_is_symmetric(&step.mechanism))
    {
        let reverse = path_reverse_signature(path);
        if reverse < forward {
            reverse
        } else {
            forward
        }
    } else {
        forward
    }
}

fn path_directional_signature(path: &PropagationPath) -> String {
    path.steps
        .iter()
        .map(|step| {
            format!(
                "{}:{}:{}",
                mechanism_family(&step.mechanism),
                scope_id(&step.from),
                scope_id(&step.to)
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

fn path_reverse_signature(path: &PropagationPath) -> String {
    path.steps
        .iter()
        .rev()
        .map(|step| {
            format!(
                "{}:{}:{}",
                mechanism_family(&step.mechanism),
                scope_id(&step.to),
                scope_id(&step.from)
            )
        })
        .collect::<Vec<_>>()
        .join("|")
}

struct HypothesisTemplate {
    key: String,
    family_label: String,
    thesis: String,
}

fn hypothesis_templates(
    relevant_events: &[&crate::ontology::Event<crate::pipeline::signals::MarketEventRecord>],
    relevant_signals: &[&crate::ontology::DerivedSignal<
        crate::pipeline::signals::DerivedSignalRecord,
    >],
    relevant_paths: &[&PropagationPath],
) -> Vec<HypothesisTemplate> {
    let mut templates = vec![
        HypothesisTemplate {
            key: "flow".into(),
            family_label: "Directed Flow".into(),
            thesis: "directed flow repricing".into(),
        },
        HypothesisTemplate {
            key: "liquidity".into(),
            family_label: "Liquidity Dislocation".into(),
            thesis: "local liquidity dislocation".into(),
        },
        HypothesisTemplate {
            key: "propagation".into(),
            family_label: "Propagation Chain".into(),
            thesis: "cross-scope propagation".into(),
        },
        HypothesisTemplate {
            key: "risk".into(),
            family_label: "Risk Repricing".into(),
            thesis: "risk repricing".into(),
        },
    ];

    let has_family = |family: &str| {
        relevant_paths
            .iter()
            .any(|path| path_has_family(path, family))
    };
    let has_mixed = relevant_paths
        .iter()
        .any(|path| path_is_mixed_multi_hop(path));
    let has_event = |predicate: fn(&MarketEventKind) -> bool| {
        relevant_events
            .iter()
            .any(|event| predicate(&event.value.kind))
    };
    let has_signal = |predicate: fn(&crate::pipeline::signals::DerivedSignalKind) -> bool| {
        relevant_signals
            .iter()
            .any(|signal| predicate(&signal.value.kind))
    };

    if has_family("shared_holder") {
        templates.push(HypothesisTemplate {
            key: "shared_holder_spillover".into(),
            family_label: "Shared-Holder Spillover".into(),
            thesis: "shared-holder spillover".into(),
        });
    }
    if has_family("rotation") {
        templates.push(HypothesisTemplate {
            key: "sector_rotation_spillover".into(),
            family_label: "Sector Rotation Spillover".into(),
            thesis: "sector rotation spillover".into(),
        });
    }
    if has_family("market_stress")
        || has_signal(|kind| {
            matches!(
                kind,
                crate::pipeline::signals::DerivedSignalKind::MarketStress
            )
        })
    {
        templates.push(HypothesisTemplate {
            key: "stress_concentration".into(),
            family_label: "Stress Concentration".into(),
            thesis: "market stress concentration".into(),
        });
    }
    if has_family("sector_symbol_bridge") {
        templates.push(HypothesisTemplate {
            key: "sector_symbol_spillover".into(),
            family_label: "Sector-Symbol Spillover".into(),
            thesis: "sector-symbol spillover".into(),
        });
    }
    if has_mixed {
        templates.push(HypothesisTemplate {
            key: "cross_mechanism_chain".into(),
            family_label: "Cross-Mechanism Chain".into(),
            thesis: "cross-mechanism chain".into(),
        });
    }
    if has_event(|kind| matches!(kind, MarketEventKind::InstitutionalFlip)) {
        templates.push(HypothesisTemplate {
            key: "institution_reversal".into(),
            family_label: "Institution Reversal".into(),
            thesis: "institution reversal".into(),
        });
    }
    if has_event(|kind| matches!(kind, MarketEventKind::CandlestickBreakout))
        || has_signal(|kind| {
            matches!(
                kind,
                crate::pipeline::signals::DerivedSignalKind::CandlestickConviction
            )
        })
    {
        templates.push(HypothesisTemplate {
            key: "breakout_contagion".into(),
            family_label: "Breakout Contagion".into(),
            thesis: "breakout-led contagion".into(),
        });
    }

    let mut seen = HashSet::new();
    templates.retain(|template| seen.insert(template.key.clone()));
    templates
}

fn scope_matches_event(scope: &ReasoningScope, event_scope: &SignalScope) -> bool {
    let converted = convert_scope(event_scope);
    converted == *scope || matches!(event_scope, SignalScope::Market)
}

fn scope_matches_signal_or_market(scope: &ReasoningScope, signal_scope: &SignalScope) -> bool {
    scope_matches_signal(scope, signal_scope) || matches!(signal_scope, SignalScope::Market)
}

fn event_polarity(
    template: &HypothesisTemplate,
    kind: &MarketEventKind,
) -> Option<EvidencePolarity> {
    use EvidencePolarity::{Contradicts as C, Supports as S};
    use MarketEventKind as K;

    let polarity = match (template.key.as_str(), kind) {
        ("flow", K::SmartMoneyPressure | K::VolumeDislocation | K::CompositeAcceleration) => S,
        ("flow", K::ManualReviewRequired | K::InstitutionalFlip) => C,
        ("liquidity", K::OrderBookDislocation | K::CandlestickBreakout) => S,
        ("liquidity", K::SharedHolderAnomaly | K::StressRegimeShift) => C,
        ("propagation", K::SharedHolderAnomaly | K::StressRegimeShift) => S,
        ("propagation", K::OrderBookDislocation) => C,
        ("risk", K::MarketStressElevated | K::StressRegimeShift | K::InstitutionalFlip) => S,
        ("risk", K::CandlestickBreakout) => C,
        ("shared_holder_spillover", K::SharedHolderAnomaly) => S,
        ("shared_holder_spillover", K::InstitutionalFlip) => C,
        ("sector_rotation_spillover", K::StressRegimeShift | K::CompositeAcceleration) => S,
        ("sector_rotation_spillover", K::ManualReviewRequired) => C,
        ("stress_concentration", K::MarketStressElevated | K::StressRegimeShift) => S,
        ("stress_concentration", K::CandlestickBreakout) => C,
        ("sector_symbol_spillover", K::SharedHolderAnomaly | K::VolumeDislocation) => S,
        ("sector_symbol_spillover", K::ManualReviewRequired) => C,
        (
            "cross_mechanism_chain",
            K::SharedHolderAnomaly | K::StressRegimeShift | K::CompositeAcceleration,
        ) => S,
        ("cross_mechanism_chain", K::ManualReviewRequired) => C,
        ("institution_reversal", K::InstitutionalFlip | K::ManualReviewRequired) => S,
        ("institution_reversal", K::CandlestickBreakout) => C,
        ("breakout_contagion", K::CandlestickBreakout | K::SharedHolderAnomaly) => S,
        ("breakout_contagion", K::MarketStressElevated) => C,
        _ => return None,
    };
    Some(polarity)
}

fn signal_polarity(
    template: &HypothesisTemplate,
    kind: &crate::pipeline::signals::DerivedSignalKind,
) -> Option<EvidencePolarity> {
    use crate::pipeline::signals::DerivedSignalKind as K;
    use EvidencePolarity::{Contradicts as C, Supports as S};

    let polarity = match (template.key.as_str(), kind) {
        ("flow", K::Convergence | K::SmartMoneyPressure | K::ActivityMomentum) => S,
        ("flow", K::MarketStress) => C,
        ("liquidity", K::CandlestickConviction | K::StructuralComposite) => S,
        ("liquidity", K::MarketStress) => C,
        ("propagation", K::MarketStress | K::Convergence) => S,
        ("propagation", K::CandlestickConviction) => C,
        ("risk", K::MarketStress) => S,
        ("risk", K::ActivityMomentum) => C,
        ("shared_holder_spillover", K::Convergence | K::SmartMoneyPressure) => S,
        ("shared_holder_spillover", K::MarketStress) => C,
        ("sector_rotation_spillover", K::Convergence | K::StructuralComposite) => S,
        ("sector_rotation_spillover", K::MarketStress) => C,
        ("stress_concentration", K::MarketStress) => S,
        ("stress_concentration", K::ActivityMomentum) => C,
        ("sector_symbol_spillover", K::StructuralComposite | K::Convergence) => S,
        ("sector_symbol_spillover", K::MarketStress) => C,
        ("cross_mechanism_chain", K::Convergence | K::MarketStress) => S,
        ("cross_mechanism_chain", K::CandlestickConviction) => C,
        ("institution_reversal", K::SmartMoneyPressure | K::Convergence) => S,
        ("institution_reversal", K::MarketStress) => C,
        ("breakout_contagion", K::CandlestickConviction | K::ActivityMomentum) => S,
        ("breakout_contagion", K::MarketStress) => C,
        _ => return None,
    };
    Some(polarity)
}

fn path_polarity(template: &HypothesisTemplate) -> EvidencePolarity {
    match template.key.as_str() {
        "propagation"
        | "risk"
        | "shared_holder_spillover"
        | "sector_rotation_spillover"
        | "stress_concentration"
        | "sector_symbol_spillover"
        | "cross_mechanism_chain"
        | "breakout_contagion" => EvidencePolarity::Supports,
        _ => EvidencePolarity::Contradicts,
    }
}

fn template_statement(template: &HypothesisTemplate, scope: &ReasoningScope) -> String {
    match scope {
        ReasoningScope::Market => format!("the market may be governed by {}", template.thesis),
        ReasoningScope::Symbol(symbol) => {
            format!("{} may currently reflect {}", symbol, template.thesis)
        }
        ReasoningScope::Sector(sector) => {
            format!(
                "sector {} may currently reflect {}",
                sector, template.thesis
            )
        }
        ReasoningScope::Institution(institution) => {
            format!(
                "institution {} may currently reflect {}",
                institution, template.thesis
            )
        }
        ReasoningScope::Theme(theme) => {
            format!("theme {} may currently reflect {}", theme, template.thesis)
        }
        ReasoningScope::Region(region) => {
            format!(
                "region {} may currently reflect {}",
                region, template.thesis
            )
        }
        ReasoningScope::Custom(value) => {
            format!("{} may currently reflect {}", value, template.thesis)
        }
    }
}

fn template_invalidation(template: &HypothesisTemplate) -> Vec<InvalidationCondition> {
    let description = match template.key.as_str() {
        "flow" => "directional flow evidence reverses or weakens",
        "liquidity" => "depth asymmetry and candle stress normalize",
        "propagation" => "connected scopes stop co-moving or the path breaks",
        "risk" => "market stress and risk-sensitive events revert",
        "shared_holder_spillover" => "shared-holder crowding link weakens or peers decouple",
        "sector_rotation_spillover" => "sector rotation stalls or reverses",
        "stress_concentration" => "market stress diffuses and sectors decouple",
        "sector_symbol_spillover" => "sector-symbol spillover stops transmitting",
        "cross_mechanism_chain" => "one leg of the cross-mechanism chain breaks",
        "institution_reversal" => "institutional reversal no longer persists",
        "breakout_contagion" => "breakout loses follow-through or contagion stops",
        _ => "the core supporting evidence disappears",
    };

    vec![InvalidationCondition {
        description: description.into(),
        references: Vec::new(),
    }]
}

fn template_expected_observations(template: &HypothesisTemplate) -> Vec<String> {
    match template.key.as_str() {
        "flow" => vec!["directional participation should persist".into()],
        "liquidity" => vec!["local imbalance should remain visible in depth or candles".into()],
        "propagation" => vec!["linked scopes should start repricing in sequence".into()],
        "risk" => vec!["stress-sensitive assets should move coherently".into()],
        "shared_holder_spillover" => {
            vec!["peer names should move with shared-holder pressure".into()]
        }
        "sector_rotation_spillover" => {
            vec!["sector beneficiaries and victims should diverge further".into()]
        }
        "stress_concentration" => {
            vec!["market stress should cluster into the same vulnerable sectors".into()]
        }
        "sector_symbol_spillover" => vec!["sector move should leak into linked symbols".into()],
        "cross_mechanism_chain" => {
            vec!["multiple mechanisms should reinforce the same direction".into()]
        }
        "institution_reversal" => {
            vec!["institutional flow should continue flipping the same way".into()]
        }
        "breakout_contagion" => vec!["breakout leaders should drag peers along".into()],
        _ => vec!["supporting evidence should persist".into()],
    }
}

fn competing_hypothesis_confidence(evidence: &[ReasoningEvidence]) -> Decimal {
    let support = evidence
        .iter()
        .filter(|item| item.polarity == EvidencePolarity::Supports)
        .map(|item| item.weight)
        .sum::<Decimal>();
    let contradict = evidence
        .iter()
        .filter(|item| item.polarity == EvidencePolarity::Contradicts)
        .map(|item| item.weight)
        .sum::<Decimal>();

    let total = support + contradict;
    if total == Decimal::ZERO {
        Decimal::ZERO
    } else {
        ((support - contradict) / total + Decimal::ONE) / Decimal::TWO
    }
    .clamp(Decimal::ZERO, Decimal::ONE)
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
    transition_reason: Option<String>,
    lineage: DecisionLineage,
}

fn decide_track_action(
    setup: &TacticalSetup,
    track: &HypothesisTrack,
    previous_track: Option<&HypothesisTrack>,
    market_regime: &MarketRegimeFilter,
) -> TrackActionDecision {
    let min_enter_edge = Decimal::new(2, 2);
    let min_enter_gap = Decimal::new(12, 2);
    let min_enter_local_support = Decimal::new(25, 2);
    let min_hold_edge = Decimal::new(1, 2);
    let is_order_case = setup.workflow_id.is_some();
    let previous_action = previous_track.map(|item| item.action.as_str());
    let blocked_by_regime =
        setup_order_direction(setup).and_then(|direction| market_regime.gate_reason(direction));

    let (action, reason) = if !is_order_case {
        if track.status == HypothesisTrackStatus::Strengthening
            && track.status_streak >= 2
            && setup.confidence >= Decimal::new(7, 1)
            && setup.confidence_gap >= Decimal::new(15, 2)
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
                    "scope case remains non-executable; status={} gap={}",
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
            (
                "review",
                format!(
                    "confidence or gap weakened (d_conf={} d_gap={})",
                    track.confidence_change.round_dp(3),
                    track.confidence_gap_change.round_dp(3)
                ),
            )
        } else if let Some(ref reason) = blocked_by_regime {
            ("review", reason.clone())
        } else {
            match track.status {
                HypothesisTrackStatus::Strengthening
                    if track.status_streak >= 2
                        && setup.confidence >= Decimal::new(6, 1)
                        && setup.confidence_gap >= min_enter_gap
                        && track.confidence_change >= Decimal::ZERO
                        && track.confidence_gap_change >= Decimal::ZERO
                        && setup.heuristic_edge >= min_enter_edge
                        && local_support_from_reason(setup) >= min_enter_local_support =>
                {
                    (
                        "enter",
                        format!(
                            "promoted by strengthening streak={} with widening gap {} and local support {}",
                            track.status_streak,
                            setup.confidence_gap.round_dp(3),
                            local_support_from_reason(setup).round_dp(3),
                        ),
                    )
                }
                HypothesisTrackStatus::Stable
                    if previous_action == Some("enter")
                        && setup.confidence >= Decimal::new(58, 2)
                        && setup.confidence_gap >= Decimal::new(10, 2)
                        && setup.heuristic_edge >= min_hold_edge =>
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
                HypothesisTrackStatus::Stable | HypothesisTrackStatus::New => (
                    "review",
                    format!(
                        "waiting for stronger persistence before enter; status={} streak={}",
                        track.status, track.status_streak
                    ),
                ),
                HypothesisTrackStatus::Strengthening => (
                    "review",
                    format!(
                        "strengthening detected but streak={} is below enter threshold",
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

    TrackActionDecision {
        action,
        reason,
        transition_reason,
        lineage: decision_lineage(
            setup,
            track,
            previous_action,
            action,
            blocked_by_regime.as_deref(),
        ),
    }
}

fn decision_lineage(
    setup: &TacticalSetup,
    track: &HypothesisTrack,
    previous_action: Option<&str>,
    action: &str,
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

fn local_support_from_reason(setup: &TacticalSetup) -> Decimal {
    // The setup confidence is sourced from the linked hypothesis, so use the rationale payload
    // carried in risk notes as the fallback display-only proxy when a tighter local split is unavailable.
    setup
        .risk_notes
        .iter()
        .find_map(|note| note.strip_prefix("local_support="))
        .and_then(|value| value.parse::<Decimal>().ok())
        .unwrap_or(Decimal::ZERO)
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

fn action_priority(action: &str) -> i32 {
    match action {
        "enter" => 0,
        "review" => 1,
        "observe" => 2,
        _ => 3,
    }
}

pub fn mechanism_family(mechanism: &str) -> &'static str {
    if mechanism.contains("shared holder") || mechanism.contains("shared-holder") {
        "shared_holder"
    } else if mechanism.contains("rotation") {
        "rotation"
    } else if mechanism.contains("market stress") {
        "market_stress"
    } else if mechanism.contains("sector spillover") {
        "sector_symbol_bridge"
    } else {
        "other"
    }
}

fn mechanism_is_symmetric(mechanism: &str) -> bool {
    matches!(
        mechanism_family(mechanism),
        "shared_holder" | "sector_symbol_bridge"
    )
}

pub fn path_has_family(path: &PropagationPath, family: &str) -> bool {
    path.steps
        .iter()
        .any(|step| mechanism_family(&step.mechanism) == family)
}

pub fn path_is_mixed_multi_hop(path: &PropagationPath) -> bool {
    if path.steps.len() < 2 {
        return false;
    }
    let families = path
        .steps
        .iter()
        .map(|step| mechanism_family(&step.mechanism))
        .collect::<HashSet<_>>();
    families.len() > 1
}

fn cluster_linkage_key(
    hypothesis: &Hypothesis,
    path_map: &HashMap<&str, &PropagationPath>,
) -> String {
    if let Some(path_id) = hypothesis.propagation_path_ids.first() {
        if let Some(path) = path_map.get(path_id.as_str()) {
            if let Some(step) = path.steps.first() {
                return format!("path:{}->{}", scope_id(&step.from), scope_id(&step.to));
            }
        }
        return format!("path:{}", path_id);
    }

    match &hypothesis.scope {
        ReasoningScope::Market => "market".into(),
        ReasoningScope::Sector(sector) => format!("sector:{}", sector),
        ReasoningScope::Institution(institution) => format!("institution:{}", institution),
        ReasoningScope::Theme(theme) => format!("theme:{}", theme),
        ReasoningScope::Region(region) => format!("region:{}", region),
        ReasoningScope::Custom(value) => format!("custom:{}", value),
        ReasoningScope::Symbol(symbol) => format!("symbol:{}", symbol),
    }
}

fn strongest_member_index(setups: &[&TacticalSetup]) -> Option<usize> {
    setups
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            action_priority(&a.action)
                .cmp(&action_priority(&b.action))
                .reverse()
                .then_with(|| a.confidence_gap.cmp(&b.confidence_gap))
                .then_with(|| a.heuristic_edge.cmp(&b.heuristic_edge))
                .then_with(|| a.confidence.cmp(&b.confidence))
        })
        .map(|(idx, _)| idx)
}

fn weakest_member_index(setups: &[&TacticalSetup]) -> Option<usize> {
    setups
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            a.confidence_gap
                .cmp(&b.confidence_gap)
                .then_with(|| a.confidence.cmp(&b.confidence))
                .then_with(|| a.heuristic_edge.cmp(&b.heuristic_edge))
        })
        .map(|(idx, _)| idx)
}

fn cluster_trend(tracks: &[&HypothesisTrack]) -> HypothesisTrackStatus {
    let strengthening = tracks
        .iter()
        .filter(|track| track.status == HypothesisTrackStatus::Strengthening)
        .count();
    let weakening = tracks
        .iter()
        .filter(|track| {
            matches!(
                track.status,
                HypothesisTrackStatus::Weakening | HypothesisTrackStatus::Invalidated
            )
        })
        .count();
    let stable = tracks
        .iter()
        .filter(|track| track.status == HypothesisTrackStatus::Stable)
        .count();

    if strengthening > weakening && strengthening >= stable {
        HypothesisTrackStatus::Strengthening
    } else if weakening > strengthening && weakening >= stable {
        HypothesisTrackStatus::Weakening
    } else if tracks
        .iter()
        .all(|track| track.status == HypothesisTrackStatus::New)
    {
        HypothesisTrackStatus::New
    } else {
        HypothesisTrackStatus::Stable
    }
}

fn cluster_trend_priority(status: HypothesisTrackStatus) -> i32 {
    match status {
        HypothesisTrackStatus::Strengthening => 0,
        HypothesisTrackStatus::New => 1,
        HypothesisTrackStatus::Stable => 2,
        HypothesisTrackStatus::Weakening => 3,
        HypothesisTrackStatus::Invalidated => 4,
    }
}

fn family_label(family_key: &str) -> &'static str {
    match family_key {
        "flow" => "Flow",
        "liquidity" => "Liquidity",
        "propagation" => "Propagation",
        "risk" => "Risk",
        _ => "Narrative",
    }
}

fn cluster_title(
    family_key: &str,
    linkage_key: &str,
    member_count: usize,
    path: Option<&PropagationPath>,
) -> String {
    let family = family_label(family_key);
    if let Some(path) = path {
        if member_count <= 1 {
            format!("{} solo case via {}", family, path.summary)
        } else {
            format!("{} cluster x{} via {}", family, member_count, path.summary)
        }
    } else {
        if member_count <= 1 {
            format!("{} solo case around {}", family, linkage_key)
        } else {
            format!(
                "{} cluster x{} around {}",
                family, member_count, linkage_key
            )
        }
    }
}

fn path_contains_scope(path: &PropagationPath, scope: &ReasoningScope) -> bool {
    path.steps
        .iter()
        .any(|step| step.from == *scope || step.to == *scope)
}

fn derived_provenance(
    observed_at: OffsetDateTime,
    confidence: Decimal,
    inputs: &[String],
) -> crate::ontology::ProvenanceMetadata {
    crate::ontology::ProvenanceMetadata::new(
        crate::ontology::ProvenanceSource::Computed,
        observed_at,
    )
    .with_confidence(confidence)
    .with_inputs(inputs.iter().cloned())
}

fn convert_scope(scope: &SignalScope) -> ReasoningScope {
    match scope {
        SignalScope::Market => ReasoningScope::Market,
        SignalScope::Symbol(symbol) => ReasoningScope::Symbol(symbol.clone()),
        SignalScope::Institution(institution) => ReasoningScope::Institution(institution.clone()),
        SignalScope::Sector(sector) => ReasoningScope::Sector(sector.clone()),
    }
}

fn scope_id(scope: &ReasoningScope) -> String {
    match scope {
        ReasoningScope::Market => "market".into(),
        ReasoningScope::Symbol(symbol) => symbol.to_string(),
        ReasoningScope::Sector(sector) => sector.clone(),
        ReasoningScope::Institution(institution) => institution.clone(),
        ReasoningScope::Theme(theme) => theme.clone(),
        ReasoningScope::Region(region) => region.clone(),
        ReasoningScope::Custom(value) => value.clone(),
    }
}

fn track_id_for_scope(scope: &ReasoningScope) -> String {
    format!("track:{}", scope_id(scope))
}

fn scope_title(scope: &ReasoningScope) -> String {
    match scope {
        ReasoningScope::Market => "Market".into(),
        ReasoningScope::Symbol(symbol) => symbol.to_string(),
        ReasoningScope::Sector(sector) => format!("Sector {}", sector),
        ReasoningScope::Institution(institution) => format!("Institution {}", institution),
        ReasoningScope::Theme(theme) => format!("Theme {}", theme),
        ReasoningScope::Region(region) => format!("Region {}", region),
        ReasoningScope::Custom(value) => value.clone(),
    }
}

fn scope_matches_signal(scope: &ReasoningScope, signal_scope: &SignalScope) -> bool {
    matches!(
        (scope, signal_scope),
        (ReasoningScope::Market, SignalScope::Market)
            | (ReasoningScope::Symbol(_), SignalScope::Symbol(_))
            | (ReasoningScope::Institution(_), SignalScope::Institution(_))
            | (ReasoningScope::Sector(_), SignalScope::Sector(_))
    ) && scope_id(scope) == scope_id(&convert_scope(signal_scope))
}

fn path_relevant_to_scope(path: &PropagationPath, scope: &ReasoningScope) -> bool {
    path.steps
        .iter()
        .any(|step| step.from == *scope || step.to == *scope)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    use super::*;
    use crate::graph::decision::{
        ConvergenceScore, MarketRegimeBias, MarketRegimeFilter, OrderDirection, OrderSuggestion,
    };
    use crate::graph::insights::{
        GraphInsights, MarketStressIndex, RotationPair, SharedHolderAnomaly,
    };
    use crate::ontology::domain::{DerivedSignal, Event, ProvenanceMetadata, ProvenanceSource};
    use crate::ontology::objects::{SectorId, Symbol};
    use crate::pipeline::signals::{
        DerivedSignalKind, DerivedSignalRecord, EventSnapshot, MarketEventRecord,
    };

    fn sym(value: &str) -> Symbol {
        Symbol(value.into())
    }

    fn prov(trace_id: &str) -> ProvenanceMetadata {
        ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH)
            .with_trace_id(trace_id)
            .with_inputs([trace_id.to_string()])
    }

    #[test]
    fn reasoning_snapshot_builds_open_hypothesis_and_setup() {
        let event = Event::new(
            MarketEventRecord {
                scope: SignalScope::Symbol(sym("700.HK")),
                kind: MarketEventKind::InstitutionalFlip,
                magnitude: dec!(0.7),
                summary: "alignment flipped".into(),
            },
            ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH),
        );
        let events = EventSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            events: vec![event],
        };
        let signals = DerivedSignalSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals: vec![DerivedSignal::new(
                DerivedSignalRecord {
                    scope: SignalScope::Symbol(sym("700.HK")),
                    kind: DerivedSignalKind::Convergence,
                    strength: dec!(0.5),
                    summary: "convergence remains positive".into(),
                },
                ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH),
            )],
        };
        let insights = GraphInsights {
            pressures: vec![],
            rotations: vec![RotationPair {
                from_sector: SectorId("energy".into()),
                to_sector: SectorId("shipping".into()),
                spread: dec!(0.4),
                spread_delta: dec!(0.1),
                widening: true,
            }],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![SharedHolderAnomaly {
                symbol_a: sym("700.HK"),
                symbol_b: sym("9988.HK"),
                sector_a: None,
                sector_b: None,
                jaccard: dec!(0.5),
                shared_institutions: 2,
            }],
            stress: MarketStressIndex {
                sector_synchrony: dec!(0.2),
                pressure_consensus: dec!(0.2),
                conflict_intensity_mean: dec!(0.1),
                market_temperature_stress: dec!(0.3),
                composite_stress: dec!(0.2),
            },
            institution_stock_counts: HashMap::new(),
        };
        let decision = DecisionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            convergence_scores: HashMap::from([(
                sym("700.HK"),
                ConvergenceScore {
                    symbol: sym("700.HK"),
                    institutional_alignment: dec!(0.6),
                    sector_coherence: Some(dec!(0.2)),
                    cross_stock_correlation: dec!(0.1),
                    composite: dec!(0.4),
                },
            )]),
            market_regime: MarketRegimeFilter::neutral(),
            order_suggestions: vec![OrderSuggestion {
                symbol: sym("700.HK"),
                direction: OrderDirection::Buy,
                convergence: ConvergenceScore {
                    symbol: sym("700.HK"),
                    institutional_alignment: dec!(0.6),
                    sector_coherence: Some(dec!(0.2)),
                    cross_stock_correlation: dec!(0.1),
                    composite: dec!(0.4),
                },
                suggested_quantity: 100,
                price_low: Some(dec!(500)),
                price_high: Some(dec!(501)),
                estimated_cost: dec!(0.002),
                heuristic_edge: dec!(0.398),
                requires_confirmation: false,
                convergence_score: dec!(0.4),
                effective_confidence: dec!(0.4),
                external_confirmation: None,
                external_conflict: None,
                external_support_slug: None,
                external_support_probability: None,
                external_conflict_slug: None,
                external_conflict_probability: None,
            }],
            degradations: HashMap::new(),
        };

        let reasoning =
            ReasoningSnapshot::derive(&events, &signals, &insights, &decision, &[], &[]);
        assert!(reasoning.hypotheses.len() >= 3);
        assert!(!reasoning.tactical_setups.is_empty());
        assert!(!reasoning.propagation_paths.is_empty());
        assert!(!reasoning.hypothesis_tracks.is_empty());
        assert!(!reasoning.case_clusters.is_empty());
        assert!(reasoning
            .hypotheses
            .iter()
            .any(|hypothesis| hypothesis.statement.contains("directed flow repricing")));
        let mut ranked = reasoning
            .hypotheses
            .iter()
            .filter(|hypothesis| hypothesis.scope == ReasoningScope::Symbol(sym("700.HK")))
            .collect::<Vec<_>>();
        ranked.sort_by(|a, b| {
            b.confidence
                .cmp(&a.confidence)
                .then_with(|| a.hypothesis_id.cmp(&b.hypothesis_id))
        });
        assert_eq!(
            reasoning.tactical_setups[0].hypothesis_id,
            ranked[0].hypothesis_id
        );
        assert!(reasoning
            .hypotheses
            .iter()
            .any(|hypothesis| hypothesis.local_support_weight > Decimal::ZERO));
    }

    #[test]
    fn hypothesis_tracks_capture_strengthening_and_invalidation() {
        let previous_timestamp = OffsetDateTime::UNIX_EPOCH;
        let current_timestamp = previous_timestamp + time::Duration::seconds(2);
        let previous_setup = TacticalSetup {
            setup_id: "setup:700.HK:review".into(),
            hypothesis_id: "hyp:700.HK:flow".into(),
            runner_up_hypothesis_id: Some("hyp:700.HK:risk".into()),
            provenance: prov("setup:700.HK:review"),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            title: "Long 700.HK".into(),
            action: "review".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.58),
            confidence_gap: dec!(0.09),
            heuristic_edge: dec!(0.05),
            workflow_id: Some("order:700.HK:buy".into()),
            entry_rationale: "flow leads".into(),
            risk_notes: vec!["local_support=0.40".into()],
        };
        let previous_track = HypothesisTrack {
            track_id: "track:700.HK".into(),
            setup_id: previous_setup.setup_id.clone(),
            hypothesis_id: previous_setup.hypothesis_id.clone(),
            runner_up_hypothesis_id: previous_setup.runner_up_hypothesis_id.clone(),
            scope: previous_setup.scope.clone(),
            title: previous_setup.title.clone(),
            action: previous_setup.action.clone(),
            status: HypothesisTrackStatus::New,
            age_ticks: 1,
            status_streak: 1,
            confidence: previous_setup.confidence,
            previous_confidence: None,
            confidence_change: Decimal::ZERO,
            confidence_gap: previous_setup.confidence_gap,
            previous_confidence_gap: None,
            confidence_gap_change: Decimal::ZERO,
            heuristic_edge: previous_setup.heuristic_edge,
            policy_reason: "new case seeded".into(),
            transition_reason: None,
            first_seen_at: previous_timestamp,
            last_updated_at: previous_timestamp,
            invalidated_at: None,
        };
        let current_setup = TacticalSetup {
            setup_id: "setup:700.HK:enter".into(),
            confidence: dec!(0.66),
            confidence_gap: dec!(0.18),
            heuristic_edge: dec!(0.12),
            action: "enter".into(),
            ..previous_setup.clone()
        };

        let tracks = derive_hypothesis_tracks(
            current_timestamp,
            &[current_setup.clone()],
            &[previous_setup.clone()],
            &[previous_track.clone()],
        );
        let strengthening = tracks
            .iter()
            .find(|track| track.track_id == "track:700.HK")
            .expect("current track");
        assert_eq!(strengthening.status, HypothesisTrackStatus::Strengthening);
        assert_eq!(strengthening.age_ticks, 2);
        assert_eq!(strengthening.status_streak, 1);
        assert_eq!(strengthening.previous_confidence, Some(dec!(0.58)));

        let invalidated =
            derive_hypothesis_tracks(current_timestamp, &[], &[previous_setup], &[previous_track]);
        assert_eq!(invalidated.len(), 1);
        assert_eq!(invalidated[0].status, HypothesisTrackStatus::Invalidated);
        assert_eq!(invalidated[0].invalidated_at, Some(current_timestamp));
    }

    #[test]
    fn track_policy_promotes_after_strengthening_streak() {
        let setup = TacticalSetup {
            setup_id: "setup:700.HK:review".into(),
            hypothesis_id: "hyp:700.HK:flow".into(),
            runner_up_hypothesis_id: Some("hyp:700.HK:risk".into()),
            provenance: prov("setup:700.HK:review"),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            title: "Long 700.HK".into(),
            action: "review".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.64),
            confidence_gap: dec!(0.16),
            heuristic_edge: dec!(0.11),
            workflow_id: Some("order:700.HK:buy".into()),
            entry_rationale: "flow leads".into(),
            risk_notes: vec!["local_support=0.40".into()],
        };
        let track = HypothesisTrack {
            track_id: "track:700.HK".into(),
            setup_id: setup.setup_id.clone(),
            hypothesis_id: setup.hypothesis_id.clone(),
            runner_up_hypothesis_id: setup.runner_up_hypothesis_id.clone(),
            scope: setup.scope.clone(),
            title: setup.title.clone(),
            action: "review".into(),
            status: HypothesisTrackStatus::Strengthening,
            age_ticks: 3,
            status_streak: 2,
            confidence: setup.confidence,
            previous_confidence: Some(dec!(0.60)),
            confidence_change: dec!(0.04),
            confidence_gap: setup.confidence_gap,
            previous_confidence_gap: Some(dec!(0.11)),
            confidence_gap_change: dec!(0.05),
            heuristic_edge: setup.heuristic_edge,
            policy_reason: "case strengthened".into(),
            transition_reason: None,
            first_seen_at: OffsetDateTime::UNIX_EPOCH,
            last_updated_at: OffsetDateTime::UNIX_EPOCH,
            invalidated_at: None,
        };
        let previous_track = HypothesisTrack {
            action: "review".into(),
            ..track.clone()
        };

        let updated = apply_track_action_policy(
            &[setup],
            &[track],
            &[previous_track],
            &MarketRegimeFilter::neutral(),
        );
        assert_eq!(updated.len(), 1);
        assert_eq!(updated[0].action, "enter");
        assert!(updated[0]
            .entry_rationale
            .contains("promoted by strengthening streak"));
    }

    #[test]
    fn track_policy_blocks_low_edge_enter() {
        let setup = TacticalSetup {
            setup_id: "setup:700.HK:review".into(),
            hypothesis_id: "hyp:700.HK:flow".into(),
            runner_up_hypothesis_id: Some("hyp:700.HK:risk".into()),
            provenance: prov("setup:700.HK:review"),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            title: "Long 700.HK".into(),
            action: "review".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.64),
            confidence_gap: dec!(0.16),
            heuristic_edge: dec!(0.003),
            workflow_id: Some("order:700.HK:buy".into()),
            entry_rationale: "flow leads".into(),
            risk_notes: vec!["local_support=0.40".into()],
        };
        let track = HypothesisTrack {
            track_id: "track:700.HK".into(),
            setup_id: setup.setup_id.clone(),
            hypothesis_id: setup.hypothesis_id.clone(),
            runner_up_hypothesis_id: setup.runner_up_hypothesis_id.clone(),
            scope: setup.scope.clone(),
            title: setup.title.clone(),
            action: "review".into(),
            status: HypothesisTrackStatus::Strengthening,
            age_ticks: 3,
            status_streak: 2,
            confidence: setup.confidence,
            previous_confidence: Some(dec!(0.60)),
            confidence_change: dec!(0.04),
            confidence_gap: setup.confidence_gap,
            previous_confidence_gap: Some(dec!(0.11)),
            confidence_gap_change: dec!(0.05),
            heuristic_edge: setup.heuristic_edge,
            policy_reason: "case strengthened".into(),
            transition_reason: None,
            first_seen_at: OffsetDateTime::UNIX_EPOCH,
            last_updated_at: OffsetDateTime::UNIX_EPOCH,
            invalidated_at: None,
        };

        let updated =
            apply_track_action_policy(&[setup], &[track], &[], &MarketRegimeFilter::neutral());
        assert_eq!(updated[0].action, "review");
    }

    #[test]
    fn track_policy_blocks_long_enter_when_market_regime_is_risk_off() {
        let setup = TacticalSetup {
            setup_id: "setup:700.HK:review".into(),
            hypothesis_id: "hyp:700.HK:flow".into(),
            runner_up_hypothesis_id: Some("hyp:700.HK:risk".into()),
            provenance: prov("setup:700.HK:review"),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            title: "Long 700.HK".into(),
            action: "review".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.66),
            confidence_gap: dec!(0.18),
            heuristic_edge: dec!(0.12),
            workflow_id: Some("order:700.HK:buy".into()),
            entry_rationale: "flow leads".into(),
            risk_notes: vec!["local_support=0.40".into()],
        };
        let track = HypothesisTrack {
            track_id: "track:700.HK".into(),
            setup_id: setup.setup_id.clone(),
            hypothesis_id: setup.hypothesis_id.clone(),
            runner_up_hypothesis_id: setup.runner_up_hypothesis_id.clone(),
            scope: setup.scope.clone(),
            title: setup.title.clone(),
            action: "review".into(),
            status: HypothesisTrackStatus::Strengthening,
            age_ticks: 3,
            status_streak: 2,
            confidence: setup.confidence,
            previous_confidence: Some(dec!(0.60)),
            confidence_change: dec!(0.06),
            confidence_gap: setup.confidence_gap,
            previous_confidence_gap: Some(dec!(0.11)),
            confidence_gap_change: dec!(0.07),
            heuristic_edge: setup.heuristic_edge,
            policy_reason: "case strengthened".into(),
            transition_reason: None,
            first_seen_at: OffsetDateTime::UNIX_EPOCH,
            last_updated_at: OffsetDateTime::UNIX_EPOCH,
            invalidated_at: None,
        };
        let market_regime = MarketRegimeFilter {
            bias: MarketRegimeBias::RiskOff,
            confidence: dec!(0.82),
            breadth_up: dec!(0.12),
            breadth_down: dec!(0.78),
            average_return: dec!(-0.023),
            leader_return: Some(dec!(-0.041)),
            directional_consensus: dec!(-0.48),
            external_bias: None,
            external_confidence: None,
            external_driver: None,
        };

        let updated = apply_track_action_policy(&[setup], &[track], &[], &market_regime);
        assert_eq!(updated[0].action, "review");
        assert!(updated[0]
            .risk_notes
            .iter()
            .any(|note| note.contains("market regime risk_off blocks long entries")));
    }

    #[test]
    fn case_clusters_group_related_members() {
        let hypothesis_a = Hypothesis {
            hypothesis_id: "hyp:700.HK:flow".into(),
            family_key: "flow".into(),
            family_label: "Directed Flow".into(),
            provenance: prov("hyp:700.HK:flow"),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            statement: "700.HK may currently reflect directed flow repricing".into(),
            confidence: dec!(0.68),
            local_support_weight: Decimal::ZERO,
            local_contradict_weight: Decimal::ZERO,
            propagated_support_weight: Decimal::ZERO,
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![],
            invalidation_conditions: vec![],
            propagation_path_ids: vec!["path:shared_holder:700.HK:9988.HK".into()],
            expected_observations: vec![],
        };
        let hypothesis_b = Hypothesis {
            hypothesis_id: "hyp:9988.HK:flow".into(),
            family_key: "flow".into(),
            family_label: "Directed Flow".into(),
            provenance: prov("hyp:9988.HK:flow"),
            scope: ReasoningScope::Symbol(sym("9988.HK")),
            statement: "9988.HK may currently reflect directed flow repricing".into(),
            confidence: dec!(0.62),
            local_support_weight: Decimal::ZERO,
            local_contradict_weight: Decimal::ZERO,
            propagated_support_weight: Decimal::ZERO,
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![],
            invalidation_conditions: vec![],
            propagation_path_ids: vec!["path:shared_holder:700.HK:9988.HK".into()],
            expected_observations: vec![],
        };
        let path = PropagationPath {
            path_id: "path:shared_holder:700.HK:9988.HK".into(),
            summary: "shared-holder overlap may transmit repricing between 700.HK and 9988.HK"
                .into(),
            confidence: dec!(0.5),
            steps: vec![PropagationStep {
                from: ReasoningScope::Symbol(sym("700.HK")),
                to: ReasoningScope::Symbol(sym("9988.HK")),
                mechanism: "shared holder overlap".into(),
                confidence: dec!(0.5),
                references: vec![],
            }],
        };
        let setup_a = TacticalSetup {
            setup_id: "setup:700.HK:enter".into(),
            hypothesis_id: hypothesis_a.hypothesis_id.clone(),
            runner_up_hypothesis_id: Some("hyp:700.HK:risk".into()),
            provenance: prov("setup:700.HK:enter"),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(sym("700.HK")),
            title: "Long 700.HK".into(),
            action: "enter".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.68),
            confidence_gap: dec!(0.16),
            heuristic_edge: dec!(0.12),
            workflow_id: Some("order:700.HK:buy".into()),
            entry_rationale: "strong case".into(),
            risk_notes: vec![],
        };
        let setup_b = TacticalSetup {
            setup_id: "setup:9988.HK:review".into(),
            hypothesis_id: hypothesis_b.hypothesis_id.clone(),
            runner_up_hypothesis_id: Some("hyp:9988.HK:risk".into()),
            provenance: prov("setup:9988.HK:review"),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(sym("9988.HK")),
            title: "Long 9988.HK".into(),
            action: "review".into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.62),
            confidence_gap: dec!(0.12),
            heuristic_edge: dec!(0.07),
            workflow_id: Some("order:9988.HK:buy".into()),
            entry_rationale: "secondary case".into(),
            risk_notes: vec![],
        };
        let track_a = HypothesisTrack {
            track_id: "track:700.HK".into(),
            setup_id: setup_a.setup_id.clone(),
            hypothesis_id: setup_a.hypothesis_id.clone(),
            runner_up_hypothesis_id: setup_a.runner_up_hypothesis_id.clone(),
            scope: setup_a.scope.clone(),
            title: setup_a.title.clone(),
            action: setup_a.action.clone(),
            status: HypothesisTrackStatus::Strengthening,
            age_ticks: 3,
            status_streak: 2,
            confidence: setup_a.confidence,
            previous_confidence: Some(dec!(0.61)),
            confidence_change: dec!(0.07),
            confidence_gap: setup_a.confidence_gap,
            previous_confidence_gap: Some(dec!(0.11)),
            confidence_gap_change: dec!(0.05),
            heuristic_edge: setup_a.heuristic_edge,
            policy_reason: "strengthening".into(),
            transition_reason: None,
            first_seen_at: OffsetDateTime::UNIX_EPOCH,
            last_updated_at: OffsetDateTime::UNIX_EPOCH,
            invalidated_at: None,
        };
        let track_b = HypothesisTrack {
            track_id: "track:9988.HK".into(),
            setup_id: setup_b.setup_id.clone(),
            hypothesis_id: setup_b.hypothesis_id.clone(),
            runner_up_hypothesis_id: setup_b.runner_up_hypothesis_id.clone(),
            scope: setup_b.scope.clone(),
            title: setup_b.title.clone(),
            action: setup_b.action.clone(),
            status: HypothesisTrackStatus::Stable,
            age_ticks: 2,
            status_streak: 1,
            confidence: setup_b.confidence,
            previous_confidence: Some(dec!(0.60)),
            confidence_change: dec!(0.02),
            confidence_gap: setup_b.confidence_gap,
            previous_confidence_gap: Some(dec!(0.10)),
            confidence_gap_change: dec!(0.02),
            heuristic_edge: setup_b.heuristic_edge,
            policy_reason: "stable".into(),
            transition_reason: None,
            first_seen_at: OffsetDateTime::UNIX_EPOCH,
            last_updated_at: OffsetDateTime::UNIX_EPOCH,
            invalidated_at: None,
        };

        let clusters = derive_case_clusters(
            &[hypothesis_a, hypothesis_b],
            &[path],
            &[setup_a, setup_b],
            &[track_a, track_b],
        );

        assert_eq!(clusters.len(), 1);
        assert_eq!(clusters[0].member_count, 2);
        assert_eq!(clusters[0].trend, HypothesisTrackStatus::Strengthening);
        assert_eq!(clusters[0].strongest_title, "Long 700.HK");
    }

    #[test]
    fn derive_propagation_paths_builds_two_hop_chain() {
        let insights = GraphInsights {
            pressures: vec![],
            rotations: vec![
                RotationPair {
                    from_sector: SectorId("energy".into()),
                    to_sector: SectorId("shipping".into()),
                    spread: dec!(0.6),
                    spread_delta: dec!(0.2),
                    widening: true,
                },
                RotationPair {
                    from_sector: SectorId("shipping".into()),
                    to_sector: SectorId("ports".into()),
                    spread: dec!(0.5),
                    spread_delta: dec!(0.1),
                    widening: true,
                },
            ],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: MarketStressIndex {
                sector_synchrony: Decimal::ZERO,
                pressure_consensus: Decimal::ZERO,
                conflict_intensity_mean: Decimal::ZERO,
                market_temperature_stress: Decimal::ZERO,
                composite_stress: Decimal::ZERO,
            },
            institution_stock_counts: HashMap::new(),
        };

        let paths = derive_propagation_paths(&insights, OffsetDateTime::UNIX_EPOCH);
        let two_hop = paths
            .iter()
            .find(|path| path.steps.len() == 2)
            .expect("two-hop path");

        assert_eq!(
            two_hop.steps[0].from,
            ReasoningScope::Sector("energy".into())
        );
        assert_eq!(two_hop.steps[1].to, ReasoningScope::Sector("ports".into()));
        assert!(two_hop.path_id.contains("path:2hop:"));
        assert!(two_hop.summary.contains("via"));
    }

    #[test]
    fn derive_propagation_paths_builds_three_hop_chain() {
        let insights = GraphInsights {
            pressures: vec![],
            rotations: vec![
                RotationPair {
                    from_sector: SectorId("energy".into()),
                    to_sector: SectorId("shipping".into()),
                    spread: dec!(0.8),
                    spread_delta: dec!(0.2),
                    widening: true,
                },
                RotationPair {
                    from_sector: SectorId("shipping".into()),
                    to_sector: SectorId("ports".into()),
                    spread: dec!(0.7),
                    spread_delta: dec!(0.2),
                    widening: true,
                },
                RotationPair {
                    from_sector: SectorId("ports".into()),
                    to_sector: SectorId("logistics".into()),
                    spread: dec!(0.6),
                    spread_delta: dec!(0.1),
                    widening: true,
                },
            ],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: MarketStressIndex {
                sector_synchrony: Decimal::ZERO,
                pressure_consensus: Decimal::ZERO,
                conflict_intensity_mean: Decimal::ZERO,
                market_temperature_stress: Decimal::ZERO,
                composite_stress: Decimal::ZERO,
            },
            institution_stock_counts: HashMap::new(),
        };

        let paths = derive_propagation_paths(&insights, OffsetDateTime::UNIX_EPOCH);
        let three_hop = paths
            .iter()
            .find(|path| path.steps.len() == 3)
            .expect("three-hop path");

        assert_eq!(
            three_hop.steps[0].from,
            ReasoningScope::Sector("energy".into())
        );
        assert_eq!(
            three_hop.steps[2].to,
            ReasoningScope::Sector("logistics".into())
        );
        assert!(three_hop.path_id.contains("path:3hop:"));
    }

    #[test]
    fn canonicalize_paths_dedupes_symmetric_shared_holder_paths() {
        let path_ab = PropagationPath {
            path_id: "path:shared_holder:700.HK:9988.HK".into(),
            summary: "shared-holder overlap may transmit repricing between 700.HK and 9988.HK"
                .into(),
            confidence: dec!(0.8),
            steps: vec![PropagationStep {
                from: ReasoningScope::Symbol(sym("700.HK")),
                to: ReasoningScope::Symbol(sym("9988.HK")),
                mechanism: "shared holder overlap".into(),
                confidence: dec!(0.8),
                references: vec![],
            }],
        };
        let path_ba = PropagationPath {
            path_id: "path:shared_holder:9988.HK:700.HK".into(),
            summary: "shared-holder overlap may transmit repricing between 9988.HK and 700.HK"
                .into(),
            confidence: dec!(0.8),
            steps: vec![PropagationStep {
                from: ReasoningScope::Symbol(sym("9988.HK")),
                to: ReasoningScope::Symbol(sym("700.HK")),
                mechanism: "shared holder overlap".into(),
                confidence: dec!(0.8),
                references: vec![],
            }],
        };

        let canonical = canonicalize_paths(vec![path_ab, path_ba]);
        assert_eq!(canonical.len(), 1);
    }

    #[test]
    fn derive_propagation_paths_builds_mixed_mechanism_chain() {
        let insights = GraphInsights {
            pressures: vec![],
            rotations: vec![RotationPair {
                from_sector: SectorId("energy".into()),
                to_sector: SectorId("shipping".into()),
                spread: dec!(0.7),
                spread_delta: dec!(0.2),
                widening: true,
            }],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![SharedHolderAnomaly {
                symbol_a: sym("883.HK"),
                symbol_b: sym("1308.HK"),
                sector_a: Some(SectorId("energy".into())),
                sector_b: Some(SectorId("shipping".into())),
                jaccard: dec!(0.8),
                shared_institutions: 4,
            }],
            stress: MarketStressIndex {
                sector_synchrony: dec!(0.4),
                pressure_consensus: dec!(0.4),
                conflict_intensity_mean: dec!(0.2),
                market_temperature_stress: dec!(0.6),
                composite_stress: dec!(0.4),
            },
            institution_stock_counts: HashMap::new(),
        };

        let paths = derive_propagation_paths(&insights, OffsetDateTime::UNIX_EPOCH);
        let mixed = paths
            .iter()
            .find(|path| {
                path.steps.len() == 2
                    && path_is_mixed_multi_hop(path)
                    && path
                        .steps
                        .iter()
                        .any(|step| mechanism_family(&step.mechanism) == "rotation")
            })
            .expect("mixed 2-hop path");

        let families = mixed
            .steps
            .iter()
            .map(|step| mechanism_family(&step.mechanism))
            .collect::<HashSet<_>>();
        assert!(families.contains("rotation"));
        assert!(families.contains("sector_symbol_bridge") || families.contains("shared_holder"));
    }

    #[test]
    fn cluster_title_uses_solo_case_for_single_member() {
        let title = cluster_title("propagation", "symbol:1177.HK", 1, None);
        assert!(title.contains("solo case"));
        assert!(!title.contains("cluster x1"));
    }

    #[test]
    fn propagated_path_evidence_penalizes_longer_hops() {
        let scope = ReasoningScope::Sector("shipping".into());
        let two_hop = PropagationPath {
            path_id: "path:2hop:test".into(),
            summary: "two hop".into(),
            confidence: dec!(0.70),
            steps: vec![
                PropagationStep {
                    from: ReasoningScope::Market,
                    to: ReasoningScope::Sector("energy".into()),
                    mechanism: "market stress concentration".into(),
                    confidence: dec!(0.8),
                    references: vec![],
                },
                PropagationStep {
                    from: ReasoningScope::Sector("energy".into()),
                    to: scope.clone(),
                    mechanism: "capital rotation widening".into(),
                    confidence: dec!(0.7),
                    references: vec![],
                },
            ],
        };
        let three_hop = PropagationPath {
            path_id: "path:3hop:test".into(),
            summary: "three hop".into(),
            confidence: dec!(0.70),
            steps: vec![
                PropagationStep {
                    from: ReasoningScope::Market,
                    to: ReasoningScope::Sector("materials".into()),
                    mechanism: "market stress concentration".into(),
                    confidence: dec!(0.8),
                    references: vec![],
                },
                PropagationStep {
                    from: ReasoningScope::Sector("materials".into()),
                    to: ReasoningScope::Sector("energy".into()),
                    mechanism: "capital rotation widening".into(),
                    confidence: dec!(0.7),
                    references: vec![],
                },
                PropagationStep {
                    from: ReasoningScope::Sector("energy".into()),
                    to: scope.clone(),
                    mechanism: "capital rotation widening".into(),
                    confidence: dec!(0.6),
                    references: vec![],
                },
            ],
        };

        let evidence = vec![ReasoningEvidence {
            statement: "local support".into(),
            kind: ReasoningEvidenceKind::LocalEvent,
            polarity: EvidencePolarity::Supports,
            weight: dec!(0.4),
            references: vec![],
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            ),
        }];

        let (weight, ids) = propagated_path_evidence(&scope, &evidence, &[three_hop, two_hop]);
        assert_eq!(ids[0], "path:2hop:test");
        assert!(weight > Decimal::ZERO);
    }

    #[test]
    fn propagated_path_evidence_rewards_local_confirmation() {
        let scope = ReasoningScope::Sector("shipping".into());
        let path = PropagationPath {
            path_id: "path:2hop:test".into(),
            summary: "two hop".into(),
            confidence: dec!(0.50),
            steps: vec![
                PropagationStep {
                    from: ReasoningScope::Market,
                    to: ReasoningScope::Sector("energy".into()),
                    mechanism: "market stress concentration".into(),
                    confidence: dec!(0.6),
                    references: vec![],
                },
                PropagationStep {
                    from: ReasoningScope::Sector("energy".into()),
                    to: scope.clone(),
                    mechanism: "capital rotation widening".into(),
                    confidence: dec!(0.5),
                    references: vec![],
                },
            ],
        };
        let no_local = propagated_path_evidence(&scope, &[], std::slice::from_ref(&path)).0;
        let supporting_local = propagated_path_evidence(
            &scope,
            &[ReasoningEvidence {
                statement: "local support".into(),
                kind: ReasoningEvidenceKind::LocalSignal,
                polarity: EvidencePolarity::Supports,
                weight: dec!(0.6),
                references: vec![],
                provenance: ProvenanceMetadata::new(
                    ProvenanceSource::Computed,
                    OffsetDateTime::UNIX_EPOCH,
                ),
            }],
            &[path],
        )
        .0;

        assert!(supporting_local > no_local);
    }

    #[test]
    fn summarize_evidence_weights_splits_local_and_propagated() {
        let summary = summarize_evidence_weights(&[
            ReasoningEvidence {
                statement: "event support".into(),
                kind: ReasoningEvidenceKind::LocalEvent,
                polarity: EvidencePolarity::Supports,
                weight: dec!(0.4),
                references: vec![],
                provenance: ProvenanceMetadata::new(
                    ProvenanceSource::Computed,
                    OffsetDateTime::UNIX_EPOCH,
                ),
            },
            ReasoningEvidence {
                statement: "signal contradict".into(),
                kind: ReasoningEvidenceKind::LocalSignal,
                polarity: EvidencePolarity::Contradicts,
                weight: dec!(0.2),
                references: vec![],
                provenance: ProvenanceMetadata::new(
                    ProvenanceSource::Computed,
                    OffsetDateTime::UNIX_EPOCH,
                ),
            },
            ReasoningEvidence {
                statement: "path support".into(),
                kind: ReasoningEvidenceKind::PropagatedPath,
                polarity: EvidencePolarity::Supports,
                weight: dec!(0.3),
                references: vec![],
                provenance: ProvenanceMetadata::new(
                    ProvenanceSource::Computed,
                    OffsetDateTime::UNIX_EPOCH,
                ),
            },
        ]);

        assert_eq!(summary.local_support, dec!(0.4));
        assert_eq!(summary.local_contradict, dec!(0.2));
        assert_eq!(summary.propagated_support, dec!(0.3));
        assert_eq!(summary.propagated_contradict, Decimal::ZERO);
    }
}
