use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::temporal::lineage::{
    ContextualLineageOutcome, FamilyContextLineageOutcome, LineageOutcome,
};

use super::{EvaluatedOutcome, SetupOutcomeContext};

#[derive(Debug, Clone, Default)]
pub(crate) struct OutcomeAccumulator {
    pub total: usize,
    pub resolved: usize,
    pub hits: usize,
    pub sum_return: Decimal,
    pub sum_net_return: Decimal,
    pub sum_mfe: Decimal,
    pub sum_mae: Decimal,
    pub follow_through_count: usize,
    pub invalidation_count: usize,
    pub structure_retention_count: usize,
    pub sum_convergence_score: Decimal,
    pub sum_external_delta: Decimal,
    pub external_follow_through_count: usize,
    pub sum_follow_expectancy: Decimal,
    pub sum_fade_expectancy: Decimal,
    pub sum_wait_expectancy: Decimal,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct ContextualOutcomeAccumulator {
    pub label: String,
    pub family: String,
    pub session: String,
    pub market_regime: String,
    pub stats: OutcomeAccumulator,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct FamilyContextAccumulator {
    pub family: String,
    pub session: String,
    pub market_regime: String,
    pub stats: OutcomeAccumulator,
}

pub(crate) fn update_outcome(
    acc: &mut HashMap<String, OutcomeAccumulator>,
    label: &str,
    outcome: Option<&EvaluatedOutcome>,
) {
    let entry = acc.entry(label.to_string()).or_default();
    update_stats(entry, outcome);
}

pub(crate) fn update_context_outcome(
    acc: &mut HashMap<String, ContextualOutcomeAccumulator>,
    label: &str,
    context: &SetupOutcomeContext,
    outcome: Option<&EvaluatedOutcome>,
) {
    let key = format!(
        "{}|{}|{}|{}",
        label, context.family, context.session, context.market_regime
    );
    let entry = acc
        .entry(key)
        .or_insert_with(|| ContextualOutcomeAccumulator {
            label: label.to_string(),
            family: context.family.clone(),
            session: context.session.clone(),
            market_regime: context.market_regime.clone(),
            stats: OutcomeAccumulator::default(),
        });
    update_stats(&mut entry.stats, outcome);
}

pub(crate) fn update_family_context_outcome(
    acc: &mut HashMap<String, FamilyContextAccumulator>,
    context: &SetupOutcomeContext,
    outcome: Option<&EvaluatedOutcome>,
) {
    let key = format!(
        "{}|{}|{}",
        context.family, context.session, context.market_regime
    );
    let entry = acc.entry(key).or_insert_with(|| FamilyContextAccumulator {
        family: context.family.clone(),
        session: context.session.clone(),
        market_regime: context.market_regime.clone(),
        stats: OutcomeAccumulator::default(),
    });
    update_stats(&mut entry.stats, outcome);
}

pub(crate) fn top_counts(acc: HashMap<String, usize>) -> Vec<(String, usize)> {
    let mut items = acc.into_iter().collect::<Vec<_>>();
    items.sort_by(|left, right| right.1.cmp(&left.1).then_with(|| left.0.cmp(&right.0)));
    items
}

pub(crate) fn top_outcomes(acc: HashMap<String, OutcomeAccumulator>) -> Vec<LineageOutcome> {
    let mut items = acc
        .into_iter()
        .map(|(label, stats)| finalize_outcome(label, stats))
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .mean_net_return
            .cmp(&left.mean_net_return)
            .then_with(|| right.hit_rate.cmp(&left.hit_rate))
            .then_with(|| left.label.cmp(&right.label))
    });
    items
}

pub(crate) fn top_context_outcomes(
    acc: HashMap<String, ContextualOutcomeAccumulator>,
) -> Vec<ContextualLineageOutcome> {
    let mut items = acc
        .into_values()
        .map(|item| finalize_context_outcome(item))
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .mean_net_return
            .cmp(&left.mean_net_return)
            .then_with(|| right.hit_rate.cmp(&left.hit_rate))
            .then_with(|| left.label.cmp(&right.label))
            .then_with(|| left.family.cmp(&right.family))
    });
    items
}

pub(crate) fn top_family_context_outcomes(
    acc: HashMap<String, FamilyContextAccumulator>,
) -> Vec<FamilyContextLineageOutcome> {
    let mut items = acc
        .into_values()
        .map(|item| finalize_family_context_outcome(item))
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .mean_net_return
            .cmp(&left.mean_net_return)
            .then_with(|| right.hit_rate.cmp(&left.hit_rate))
            .then_with(|| left.family.cmp(&right.family))
            .then_with(|| left.session.cmp(&right.session))
    });
    items
}

fn update_stats(acc: &mut OutcomeAccumulator, outcome: Option<&EvaluatedOutcome>) {
    acc.total += 1;
    let Some(outcome) = outcome else {
        return;
    };

    acc.resolved += 1;
    if outcome.net_return > Decimal::ZERO {
        acc.hits += 1;
    }
    acc.sum_return += outcome.return_pct;
    acc.sum_net_return += outcome.net_return;
    acc.sum_mfe += outcome.max_favorable_excursion;
    acc.sum_mae += outcome.max_adverse_excursion;
    acc.sum_convergence_score += outcome.convergence_score;
    acc.sum_external_delta += outcome.external_delta;
    acc.sum_follow_expectancy += outcome.follow_expectancy;
    acc.sum_fade_expectancy += outcome.fade_expectancy;
    acc.sum_wait_expectancy += outcome.wait_expectancy;
    if outcome.followed_through {
        acc.follow_through_count += 1;
    }
    if outcome.invalidated {
        acc.invalidation_count += 1;
    }
    if outcome.structure_retained {
        acc.structure_retention_count += 1;
    }
    if outcome.external_follow_through {
        acc.external_follow_through_count += 1;
    }
}

fn finalize_outcome(label: String, stats: OutcomeAccumulator) -> LineageOutcome {
    LineageOutcome {
        label,
        total: stats.total,
        resolved: stats.resolved,
        hits: stats.hits,
        hit_rate: ratio(stats.hits, stats.resolved),
        mean_return: mean(stats.sum_return, stats.resolved),
        mean_net_return: mean(stats.sum_net_return, stats.resolved),
        mean_mfe: mean(stats.sum_mfe, stats.resolved),
        mean_mae: mean(stats.sum_mae, stats.resolved),
        follow_through_rate: ratio(stats.follow_through_count, stats.resolved),
        invalidation_rate: ratio(stats.invalidation_count, stats.resolved),
        structure_retention_rate: ratio(stats.structure_retention_count, stats.resolved),
        mean_convergence_score: mean(stats.sum_convergence_score, stats.resolved),
        mean_external_delta: mean(stats.sum_external_delta, stats.resolved),
        external_follow_through_rate: ratio(stats.external_follow_through_count, stats.resolved),
        follow_expectancy: mean(stats.sum_follow_expectancy, stats.resolved),
        fade_expectancy: mean(stats.sum_fade_expectancy, stats.resolved),
        wait_expectancy: mean(stats.sum_wait_expectancy, stats.resolved),
    }
}

fn finalize_context_outcome(item: ContextualOutcomeAccumulator) -> ContextualLineageOutcome {
    let stats = finalize_outcome(item.label, item.stats);
    ContextualLineageOutcome {
        label: stats.label,
        family: item.family,
        session: item.session,
        market_regime: item.market_regime,
        total: stats.total,
        resolved: stats.resolved,
        hits: stats.hits,
        hit_rate: stats.hit_rate,
        mean_return: stats.mean_return,
        mean_net_return: stats.mean_net_return,
        mean_mfe: stats.mean_mfe,
        mean_mae: stats.mean_mae,
        follow_through_rate: stats.follow_through_rate,
        invalidation_rate: stats.invalidation_rate,
        structure_retention_rate: stats.structure_retention_rate,
        mean_convergence_score: stats.mean_convergence_score,
        mean_external_delta: stats.mean_external_delta,
        external_follow_through_rate: stats.external_follow_through_rate,
        follow_expectancy: stats.follow_expectancy,
        fade_expectancy: stats.fade_expectancy,
        wait_expectancy: stats.wait_expectancy,
    }
}

fn finalize_family_context_outcome(item: FamilyContextAccumulator) -> FamilyContextLineageOutcome {
    let stats = finalize_outcome(String::new(), item.stats);
    FamilyContextLineageOutcome {
        family: item.family,
        session: item.session,
        market_regime: item.market_regime,
        total: stats.total,
        resolved: stats.resolved,
        hits: stats.hits,
        hit_rate: stats.hit_rate,
        mean_return: stats.mean_return,
        mean_net_return: stats.mean_net_return,
        mean_mfe: stats.mean_mfe,
        mean_mae: stats.mean_mae,
        follow_through_rate: stats.follow_through_rate,
        invalidation_rate: stats.invalidation_rate,
        structure_retention_rate: stats.structure_retention_rate,
        mean_convergence_score: stats.mean_convergence_score,
        mean_external_delta: stats.mean_external_delta,
        external_follow_through_rate: stats.external_follow_through_rate,
        follow_expectancy: stats.follow_expectancy,
        fade_expectancy: stats.fade_expectancy,
        wait_expectancy: stats.wait_expectancy,
    }
}

fn ratio(numerator: usize, denominator: usize) -> Decimal {
    if denominator == 0 {
        Decimal::ZERO
    } else {
        Decimal::from(numerator as i64) / Decimal::from(denominator as i64)
    }
}

fn mean(total: Decimal, count: usize) -> Decimal {
    if count == 0 {
        Decimal::ZERO
    } else {
        total / Decimal::from(count as i64)
    }
}
