use std::collections::HashMap;

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::{DecisionLineage, Hypothesis, InvestigationSelection, ReasoningScope, TacticalSetup};
use crate::us::graph::decision::UsMarketRegimeBias;
use crate::us::temporal::lineage::{classify_us_session, UsLineageStats};

use super::{
    scope_id, scope_label, UsStructuralRankMetrics, TEMPLATE_CROSS_MARKET_ARBITRAGE,
    TEMPLATE_MOMENTUM_CONTINUATION, TEMPLATE_PRE_MARKET_POSITIONING,
    TEMPLATE_SECTOR_ROTATION, TEMPLATE_STRUCTURAL_DIFFUSION,
};

pub(super) fn derive_investigation_selections(
    hypotheses: &[Hypothesis],
    previous_setups: &[TacticalSetup],
    timestamp: OffsetDateTime,
    market_regime: Option<UsMarketRegimeBias>,
    lineage_stats: Option<&UsLineageStats>,
    structural_metrics: Option<&HashMap<Symbol, UsStructuralRankMetrics>>,
) -> Vec<InvestigationSelection> {
    let previous_lookup: HashMap<&str, &TacticalSetup> = previous_setups
        .iter()
        .map(|s| (s.hypothesis_id.as_str(), s))
        .collect();
    let previous_scope_lookup: HashMap<String, &TacticalSetup> = previous_setups
        .iter()
        .map(|setup| (scope_id(&setup.scope), setup))
        .collect();

    let mut scope_ranked: HashMap<String, Vec<&Hypothesis>> = HashMap::new();
    for hyp in hypotheses {
        scope_ranked
            .entry(scope_id(&hyp.scope))
            .or_default()
            .push(hyp);
    }

    let mut selections = Vec::new();
    for (_, ranked) in &scope_ranked {
        if ranked.is_empty() {
            continue;
        }
        let mut ranked = ranked.iter().copied().collect::<Vec<_>>();
        ranked.sort_by(|left, right| {
            effective_us_hypothesis_score(
                right,
                timestamp,
                market_regime,
                lineage_stats,
                structural_metrics,
            )
            .cmp(&effective_us_hypothesis_score(
                left,
                timestamp,
                market_regime,
                lineage_stats,
                structural_metrics,
            ))
            .then_with(|| right.confidence.cmp(&left.confidence))
            .then_with(|| left.hypothesis_id.cmp(&right.hypothesis_id))
        });

        let winner = ranked[0];
        let runner_up = ranked.get(1).copied();
        let winner_confidence = effective_us_hypothesis_score(
            winner,
            timestamp,
            market_regime,
            lineage_stats,
            structural_metrics,
        );
        let runner_up_confidence = runner_up
            .map(|item| {
                effective_us_hypothesis_score(
                    item,
                    timestamp,
                    market_regime,
                    lineage_stats,
                    structural_metrics,
                )
            })
            .unwrap_or(Decimal::ZERO);
        let gap = if runner_up.is_some() {
            (winner_confidence - runner_up_confidence).max(Decimal::ZERO)
        } else {
            winner_confidence
        };
        let previous_same_hypothesis = previous_lookup.get(winner.hypothesis_id.as_str()).copied();
        let previous_same_scope = previous_scope_lookup.get(&scope_id(&winner.scope)).copied();
        let previous_action = previous_same_scope.map(|setup| setup.action.as_str());
        let confidence_change = previous_same_hypothesis
            .map(|prev| winner_confidence - prev.confidence)
            .unwrap_or(Decimal::ZERO);
        let gap_change = previous_same_scope
            .map(|prev| gap - prev.confidence_gap)
            .unwrap_or(Decimal::ZERO);
        let prior = best_us_lineage_prior(
            winner.family_key.as_str(),
            timestamp,
            market_regime,
            lineage_stats,
        );
        let prior_signal = prior
            .map(classify_us_lineage_prior)
            .unwrap_or(UsPriorSignal::Neutral);
        let prior_note = prior
            .map(describe_us_lineage_prior)
            .unwrap_or_else(|| "lineage prior unavailable".into());

        let attention_hint = if previous_action == Some("enter")
            && previous_same_hypothesis.is_some()
            && winner_confidence >= Decimal::new(60, 2)
            && gap >= Decimal::new(12, 2)
            && prior_signal != UsPriorSignal::Negative
        {
            "enter"
        } else if prior_signal == UsPriorSignal::Negative && previous_action != Some("enter") {
            "observe"
        } else if previous_same_hypothesis.is_some()
            && winner_confidence >= Decimal::new(72, 2)
            && gap >= Decimal::new(20, 2)
            && confidence_change >= Decimal::new(2, 2)
            && gap_change >= Decimal::ZERO
            && prior_signal != UsPriorSignal::Negative
        {
            "enter"
        } else if previous_same_scope.is_some()
            && winner_confidence >= Decimal::new(66, 2)
            && gap >= Decimal::new(15, 2)
            && confidence_change >= Decimal::ZERO
            && prior_signal != UsPriorSignal::Negative
        {
            "review"
        } else if prior_signal == UsPriorSignal::Positive
            && winner_confidence >= Decimal::new(74, 2)
            && gap >= Decimal::new(18, 2)
        {
            "review"
        } else {
            "observe"
        };

        let lineage_adjustment = prior
            .map(lineage_confidence_adjustment)
            .unwrap_or(Decimal::ZERO);
        let propagation_bonus = propagation_rank_adjustment(winner);
        let structural_bonus = structural_rank_adjustment(winner, structural_metrics);
        let priority_score = (gap * winner_confidence
            + propagation_bonus.max(Decimal::ZERO)
            + structural_bonus.max(Decimal::ZERO))
        .clamp(Decimal::ZERO, Decimal::ONE);

        let mut notes = winner
            .invalidation_conditions
            .iter()
            .map(|ic| ic.description.clone())
            .collect::<Vec<_>>();
        notes.insert(0, format!("lineage_prior={}", prior_note));
        notes.insert(0, format!("family={}", winner.family_key));
        notes.insert(
            0,
            format!("lineage_adjustment={}", lineage_adjustment.round_dp(4)),
        );
        notes.insert(
            0,
            format!("propagation_bonus={}", propagation_bonus.round_dp(4)),
        );
        notes.insert(
            0,
            format!("structural_bonus={}", structural_bonus.round_dp(4)),
        );

        selections.push(InvestigationSelection {
            investigation_id: format!("investigation:{}", scope_id(&winner.scope)),
            hypothesis_id: winner.hypothesis_id.clone(),
            runner_up_hypothesis_id: runner_up.map(|h| h.hypothesis_id.clone()),
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                winner.provenance.observed_at,
            )
            .with_trace_id(format!("investigation:{}", scope_id(&winner.scope)))
            .with_inputs([
                winner.hypothesis_id.clone(),
                format!("confidence:{}", winner_confidence.round_dp(4)),
            ])
            .with_note("investigation selection"),
            scope: winner.scope.clone(),
            title: format!("{} — {}", scope_label(&winner.scope), winner.family_label),
            family_key: winner.family_key.clone(),
            family_label: winner.family_label.clone(),
            confidence: winner_confidence,
            confidence_gap: gap,
            priority_score,
            attention_hint: attention_hint.into(),
            rationale: winner.statement.clone(),
            notes,
        });
    }

    selections.sort_by(|a, b| {
        b.priority_score
            .cmp(&a.priority_score)
            .then_with(|| b.confidence.cmp(&a.confidence))
            .then_with(|| b.confidence_gap.cmp(&a.confidence_gap))
            .then_with(|| a.investigation_id.cmp(&b.investigation_id))
    });
    selections
}

pub(super) fn derive_tactical_setups(
    hypotheses: &[Hypothesis],
    investigation_selections: &[InvestigationSelection],
    previous_setups: &[TacticalSetup],
    lineage_stats: Option<&UsLineageStats>,
) -> Vec<TacticalSetup> {
    let hypothesis_map = hypotheses
        .iter()
        .map(|hypothesis| (hypothesis.hypothesis_id.as_str(), hypothesis))
        .collect::<HashMap<_, _>>();
    let previous_scope_lookup = previous_setups
        .iter()
        .map(|setup| (scope_id(&setup.scope), setup))
        .collect::<HashMap<_, _>>();
    let setups = investigation_selections
        .iter()
        .filter_map(|selection| {
            let hypothesis = hypothesis_map
                .get(selection.hypothesis_id.as_str())
                .copied()?;
            Some(TacticalSetup {
                setup_id: format!(
                    "setup:{}:{}",
                    scope_id(&selection.scope),
                    selection.attention_hint
                ),
                hypothesis_id: selection.hypothesis_id.clone(),
                runner_up_hypothesis_id: selection.runner_up_hypothesis_id.clone(),
                provenance: selection.provenance.clone().with_trace_id(format!(
                    "setup:{}:{}",
                    scope_id(&selection.scope),
                    selection.attention_hint
                )),
                lineage: DecisionLineage {
                    based_on: vec![selection.hypothesis_id.clone()],
                    blocked_by: vec![],
                    promoted_by: selection.notes.clone(),
                    falsified_by: hypothesis
                        .invalidation_conditions
                        .iter()
                        .map(|ic| ic.description.clone())
                        .collect(),
                },
                scope: selection.scope.clone(),
                title: selection.title.clone(),
                action: selection.attention_hint.clone(),
                time_horizon: "intraday".into(),
                confidence: selection.confidence,
                confidence_gap: selection.confidence_gap,
                heuristic_edge: selection.priority_score.clamp(Decimal::ZERO, Decimal::ONE),
                convergence_score: None,
                workflow_id: None,
                entry_rationale: selection.rationale.clone(),
                risk_notes: selection.notes.clone(),
                policy_verdict: None,
            })
        })
        .collect::<Vec<_>>();
    let mut setups = apply_us_case_budget(setups, &previous_scope_lookup, lineage_stats);
    setups.sort_by(|a, b| {
        b.heuristic_edge
            .cmp(&a.heuristic_edge)
            .then_with(|| b.confidence.cmp(&a.confidence))
            .then_with(|| b.confidence_gap.cmp(&a.confidence_gap))
            .then_with(|| a.setup_id.cmp(&b.setup_id))
    });
    setups
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum UsPriorSignal {
    Positive,
    Negative,
    Neutral,
}

fn best_us_lineage_prior<'a>(
    family_key: &str,
    timestamp: OffsetDateTime,
    market_regime: Option<UsMarketRegimeBias>,
    lineage_stats: Option<&'a UsLineageStats>,
) -> Option<&'a crate::us::temporal::lineage::UsLineageContextStats> {
    let lineage_stats = lineage_stats?;
    let session = classify_us_session(timestamp).as_str();
    let regime = market_regime
        .unwrap_or(UsMarketRegimeBias::Neutral)
        .as_str();

    let best = |items: Vec<&'a crate::us::temporal::lineage::UsLineageContextStats>| {
        items.into_iter().max_by(|left, right| {
            lineage_prior_rank(left)
                .cmp(&lineage_prior_rank(right))
                .then_with(|| left.hit_rate.cmp(&right.hit_rate))
                .then_with(|| left.mean_return.cmp(&right.mean_return))
                .then_with(|| left.resolved.cmp(&right.resolved))
        })
    };

    best(
        lineage_stats
            .by_context
            .iter()
            .filter(|item| {
                item.template == family_key
                    && item.session == session
                    && item.market_regime == regime
            })
            .collect(),
    )
    .or_else(|| {
        best(
            lineage_stats
                .by_context
                .iter()
                .filter(|item| item.template == family_key && item.session == session)
                .collect(),
        )
    })
    .or_else(|| {
        best(
            lineage_stats
                .by_template
                .iter()
                .filter(|item| item.template == family_key)
                .collect(),
        )
    })
}

fn effective_us_hypothesis_confidence(
    hypothesis: &Hypothesis,
    timestamp: OffsetDateTime,
    market_regime: Option<UsMarketRegimeBias>,
    lineage_stats: Option<&UsLineageStats>,
) -> Decimal {
    let adjustment = best_us_lineage_prior(
        hypothesis.family_key.as_str(),
        timestamp,
        market_regime,
        lineage_stats,
    )
    .map(lineage_confidence_adjustment)
    .unwrap_or(Decimal::ZERO);
    (hypothesis.confidence + adjustment).clamp(Decimal::ZERO, Decimal::ONE)
}

fn effective_us_hypothesis_score(
    hypothesis: &Hypothesis,
    timestamp: OffsetDateTime,
    market_regime: Option<UsMarketRegimeBias>,
    lineage_stats: Option<&UsLineageStats>,
    structural_metrics: Option<&HashMap<Symbol, UsStructuralRankMetrics>>,
) -> Decimal {
    let base =
        effective_us_hypothesis_confidence(hypothesis, timestamp, market_regime, lineage_stats);
    let propagation_bonus = propagation_rank_adjustment(hypothesis);
    let structural_bonus = structural_rank_adjustment(hypothesis, structural_metrics);
    (base + propagation_bonus + structural_bonus).clamp(Decimal::ZERO, Decimal::ONE)
}

fn propagation_rank_adjustment(hypothesis: &Hypothesis) -> Decimal {
    let path_count_bonus = (Decimal::from(hypothesis.propagation_path_ids.len().min(3) as i64)
        / Decimal::from(3))
        * Decimal::new(8, 2);
    let support_bonus =
        hypothesis.propagated_support_weight.min(Decimal::ONE) * Decimal::new(18, 2);
    let contradict_penalty =
        hypothesis.propagated_contradict_weight.min(Decimal::ONE) * Decimal::new(12, 2);
    if hypothesis
        .family_key
        .starts_with(TEMPLATE_PRE_MARKET_POSITIONING)
    {
        (Decimal::ZERO - path_count_bonus - contradict_penalty)
            .clamp(Decimal::new(-28, 2), Decimal::ZERO)
    } else {
        (path_count_bonus + support_bonus - contradict_penalty)
            .clamp(Decimal::new(-10, 2), Decimal::new(28, 2))
    }
}

fn structural_rank_adjustment(
    hypothesis: &Hypothesis,
    structural_metrics: Option<&HashMap<Symbol, UsStructuralRankMetrics>>,
) -> Decimal {
    let Some(structural_metrics) = structural_metrics else {
        return Decimal::ZERO;
    };
    let ReasoningScope::Symbol(symbol) = &hypothesis.scope else {
        return Decimal::ZERO;
    };
    let Some(metrics) = structural_metrics.get(symbol) else {
        return Decimal::ZERO;
    };

    let delta_bonus = metrics.composite_delta.abs().min(Decimal::ONE) * Decimal::new(18, 2);
    let accel_bonus = metrics.composite_acceleration.abs().min(Decimal::ONE) * Decimal::new(12, 2);
    let flow_bonus = metrics.capital_flow_delta.abs().min(Decimal::ONE) * Decimal::new(10, 2);
    let persistence_bonus = (Decimal::from(metrics.flow_persistence.min(6) as i64)
        / Decimal::from(6))
        * Decimal::new(8, 2);
    let reversal_penalty = if metrics.flow_reversal {
        Decimal::new(6, 2)
    } else {
        Decimal::ZERO
    };

    let intensity = (delta_bonus + accel_bonus + flow_bonus + persistence_bonus - reversal_penalty)
        .clamp(Decimal::new(-10, 2), Decimal::new(30, 2));

    if hypothesis
        .family_key
        .starts_with(TEMPLATE_PRE_MARKET_POSITIONING)
    {
        (Decimal::ZERO - intensity.max(Decimal::ZERO) * Decimal::new(75, 2))
            .clamp(Decimal::new(-24, 2), Decimal::ZERO)
    } else if hypothesis.family_key == TEMPLATE_STRUCTURAL_DIFFUSION {
        intensity
    } else if hypothesis.family_key == TEMPLATE_CROSS_MARKET_ARBITRAGE {
        (intensity * Decimal::new(55, 2)).clamp(Decimal::new(-10, 2), Decimal::new(22, 2))
    } else if hypothesis.family_key == TEMPLATE_SECTOR_ROTATION {
        (intensity * Decimal::new(65, 2)).clamp(Decimal::new(-10, 2), Decimal::new(24, 2))
    } else if hypothesis
        .family_key
        .starts_with(TEMPLATE_MOMENTUM_CONTINUATION)
    {
        (intensity * Decimal::new(85, 2)).clamp(Decimal::new(-10, 2), Decimal::new(28, 2))
    } else {
        (intensity * Decimal::new(25, 2)).clamp(Decimal::new(-10, 2), Decimal::new(10, 2))
    }
}

fn lineage_prior_rank(prior: &crate::us::temporal::lineage::UsLineageContextStats) -> Decimal {
    let sample_weight = lineage_sample_weight(prior.resolved);
    let hit_component = (prior.hit_rate - Decimal::new(50, 2)) * Decimal::new(30, 2);
    let return_component = (prior.mean_return / Decimal::new(3, 2))
        .clamp(-Decimal::ONE, Decimal::ONE)
        * Decimal::new(10, 2);
    hit_component + return_component + sample_weight * Decimal::new(5, 2)
}

fn lineage_sample_weight(resolved: usize) -> Decimal {
    (Decimal::from(resolved.min(24) as i64) / Decimal::from(24)).clamp(Decimal::ZERO, Decimal::ONE)
}

fn lineage_confidence_adjustment(
    prior: &crate::us::temporal::lineage::UsLineageContextStats,
) -> Decimal {
    let sample_weight = lineage_sample_weight(prior.resolved);
    let hit_component = (prior.hit_rate - Decimal::new(50, 2)) * Decimal::new(30, 2);
    let return_component = (prior.mean_return / Decimal::new(3, 2))
        .clamp(-Decimal::ONE, Decimal::ONE)
        * Decimal::new(12, 2);
    let exploration_component = if prior.resolved < 12
        && prior.hit_rate >= Decimal::new(50, 2)
        && prior.mean_return >= Decimal::ZERO
    {
        (Decimal::from((12 - prior.resolved) as i64) / Decimal::from(12)) * Decimal::new(3, 2)
    } else {
        Decimal::ZERO
    };

    (hit_component + return_component) * sample_weight + exploration_component
}

fn classify_us_lineage_prior(
    prior: &crate::us::temporal::lineage::UsLineageContextStats,
) -> UsPriorSignal {
    if prior.resolved < 3 {
        return UsPriorSignal::Neutral;
    }
    if prior.mean_return <= Decimal::ZERO || prior.hit_rate < Decimal::new(45, 2) {
        UsPriorSignal::Negative
    } else if prior.mean_return > Decimal::ZERO && prior.hit_rate >= Decimal::new(55, 2) {
        UsPriorSignal::Positive
    } else {
        UsPriorSignal::Neutral
    }
}

fn describe_us_lineage_prior(
    prior: &crate::us::temporal::lineage::UsLineageContextStats,
) -> String {
    format!(
        "template={} session={} regime={} resolved={} hit_rate={} mean_return={}",
        prior.template,
        prior.session,
        prior.market_regime,
        prior.resolved,
        prior.hit_rate.round_dp(3),
        prior.mean_return.round_dp(4),
    )
}

pub(crate) fn apply_us_case_budget(
    mut setups: Vec<TacticalSetup>,
    previous_scope_lookup: &HashMap<String, &TacticalSetup>,
    lineage_stats: Option<&UsLineageStats>,
) -> Vec<TacticalSetup> {
    const MAX_NEW_ENTERS_PER_TICK: usize = 1;
    const MAX_TOTAL_ATTENTION_CASES: usize = 5;

    let mut new_enter_indices = setups
        .iter()
        .enumerate()
        .filter_map(|(index, setup)| {
            let previous_action = previous_scope_lookup
                .get(&scope_id(&setup.scope))
                .map(|prev| prev.action.as_str());
            (setup.action == "enter" && previous_action != Some("enter")).then_some(index)
        })
        .collect::<Vec<_>>();
    new_enter_indices.sort_by(|left, right| {
        compare_us_attention_priority(&setups[*left], &setups[*right], previous_scope_lookup)
    });
    for index in new_enter_indices.iter().skip(MAX_NEW_ENTERS_PER_TICK) {
        demote_us_setup_for_budget(
            &mut setups[*index],
            "review",
            "new-enter budget reached; only the strongest US promotion advances this tick",
        );
    }

    let preserved_enter_count = setups
        .iter()
        .filter(|setup| {
            if setup.action != "enter" {
                return false;
            }
            previous_scope_lookup
                .get(&scope_id(&setup.scope))
                .map(|prev| prev.action.as_str() == "enter")
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
            let previous_action = previous_scope_lookup
                .get(&scope_id(&setup.scope))
                .map(|prev| prev.action.as_str());
            (previous_action != Some("enter")).then_some(index)
        })
        .collect::<Vec<_>>();
    attention_indices.sort_by(|left, right| {
        compare_us_attention_priority(&setups[*left], &setups[*right], previous_scope_lookup)
    });
    let mut preserved_attention = 0usize;
    let mut family_attention_counts: HashMap<String, usize> = HashMap::new();
    for index in attention_indices {
        let family = setup_family_key(&setups[index])
            .unwrap_or("unknown")
            .to_string();
        let family_cap = family_attention_cap(family.as_str(), lineage_stats);
        let current_family_count = family_attention_counts.get(&family).copied().unwrap_or(0);
        if preserved_attention < remaining_attention_slots && current_family_count < family_cap {
            preserved_attention += 1;
            family_attention_counts.insert(family, current_family_count + 1);
            continue;
        }
        demote_us_setup_for_budget(
            &mut setups[index],
            "observe",
            "attention budget reached; lower-priority US cases remain backgrounded this tick",
        );
    }

    diversify_us_case_surface(setups, lineage_stats)
}

pub(crate) fn setup_family_key<'a>(setup: &'a TacticalSetup) -> Option<&'a str> {
    setup
        .risk_notes
        .iter()
        .find_map(|note| note.strip_prefix("family="))
}

fn family_attention_cap(family_key: &str, lineage_stats: Option<&UsLineageStats>) -> usize {
    let Some(stats) = lineage_stats else {
        return 2;
    };
    let Some(template) = stats
        .by_template
        .iter()
        .find(|item| item.template == family_key)
    else {
        return 2;
    };

    if template.resolved >= 24
        && (template.hit_rate < Decimal::new(45, 2) || template.follow_expectancy <= Decimal::ZERO)
    {
        1
    } else {
        2
    }
}

fn family_surface_cap(family_key: &str, lineage_stats: Option<&UsLineageStats>) -> usize {
    if family_key.starts_with(TEMPLATE_PRE_MARKET_POSITIONING) {
        2
    } else if family_key == TEMPLATE_CROSS_MARKET_ARBITRAGE
        || family_key == TEMPLATE_STRUCTURAL_DIFFUSION
    {
        3
    } else {
        family_attention_cap(family_key, lineage_stats).max(2)
    }
}

pub(crate) fn diversify_us_case_surface(
    setups: Vec<TacticalSetup>,
    lineage_stats: Option<&UsLineageStats>,
) -> Vec<TacticalSetup> {
    const SURFACE_WINDOW: usize = 10;
    let mut selected = Vec::new();
    let mut deferred = Vec::new();
    let mut family_counts: HashMap<String, usize> = HashMap::new();

    for setup in setups {
        if selected.len() >= SURFACE_WINDOW {
            deferred.push(setup);
            continue;
        }
        let family = setup_family_key(&setup).unwrap_or("unknown").to_string();
        let cap = family_surface_cap(&family, lineage_stats);
        let current = family_counts.get(&family).copied().unwrap_or(0);
        if current < cap {
            family_counts.insert(family, current + 1);
            selected.push(setup);
        } else {
            deferred.push(setup);
        }
    }

    selected.extend(deferred);
    selected
}

fn compare_us_attention_priority(
    left: &TacticalSetup,
    right: &TacticalSetup,
    previous_scope_lookup: &HashMap<String, &TacticalSetup>,
) -> std::cmp::Ordering {
    let left_previous = previous_scope_lookup.get(&scope_id(&left.scope)).copied();
    let right_previous = previous_scope_lookup.get(&scope_id(&right.scope)).copied();
    previous_us_enter_priority(left_previous)
        .cmp(&previous_us_enter_priority(right_previous))
        .then_with(|| {
            us_action_budget_priority(left.action.as_str())
                .cmp(&us_action_budget_priority(right.action.as_str()))
        })
        .then_with(|| left.heuristic_edge.cmp(&right.heuristic_edge))
        .then_with(|| left.confidence_gap.cmp(&right.confidence_gap))
        .then_with(|| left.confidence.cmp(&right.confidence))
        .reverse()
}

fn previous_us_enter_priority(previous_setup: Option<&TacticalSetup>) -> i32 {
    if previous_setup.map(|setup| setup.action.as_str()) == Some("enter") {
        1
    } else {
        0
    }
}

fn us_action_budget_priority(action: &str) -> i32 {
    match action {
        "enter" => 2,
        "review" => 1,
        _ => 0,
    }
}

fn demote_us_setup_for_budget(setup: &mut TacticalSetup, target_action: &str, reason: &str) {
    if setup.action == target_action {
        return;
    }
    let previous_action = setup.action.clone();
    setup.action = target_action.into();
    setup.setup_id = format!("setup:{}:{}", scope_id(&setup.scope), target_action);
    setup.provenance = setup
        .provenance
        .clone()
        .with_trace_id(setup.setup_id.clone());
    setup.lineage.blocked_by.push(format!(
        "case_budget {} -> {} because {}",
        previous_action, target_action, reason
    ));
    setup
        .risk_notes
        .insert(0, format!("policy_gate: {}", reason));
}

// ── Helpers ──
