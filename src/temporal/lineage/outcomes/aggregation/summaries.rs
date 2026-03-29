use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::temporal::lineage::{
    ContextualLineageOutcome, FamilyContextLineageOutcome, LineageOutcome,
};

use super::accumulators::{
    ContextualOutcomeAccumulator, FamilyContextAccumulator, OutcomeAccumulator,
};

pub(in super::super::super) fn top_counts(map: HashMap<String, usize>) -> Vec<(String, usize)> {
    let mut items = map.into_iter().collect::<Vec<_>>();
    items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    items
}

pub(in super::super::super) fn top_outcomes(
    map: HashMap<String, OutcomeAccumulator>,
) -> Vec<LineageOutcome> {
    let mut items = map
        .into_iter()
        .map(|(label, acc)| LineageOutcome {
            label,
            total: acc.total,
            resolved: acc.resolved,
            hits: acc.hits,
            hit_rate: rate(acc.hits, acc.resolved),
            mean_return: mean(acc.sum_return, acc.resolved),
            mean_net_return: mean(acc.sum_net_return, acc.resolved),
            mean_mfe: mean(acc.sum_mfe, acc.resolved),
            mean_mae: mean(acc.sum_mae, acc.resolved),
            follow_through_rate: rate(acc.follow_throughs, acc.resolved),
            invalidation_rate: rate(acc.invalidations, acc.resolved),
            structure_retention_rate: rate(acc.structure_retained, acc.resolved),
            mean_convergence_score: mean(acc.sum_convergence_score, acc.resolved),
            mean_external_delta: mean(acc.sum_external_delta, acc.resolved),
            external_follow_through_rate: rate(acc.external_follow_throughs, acc.resolved),
            follow_expectancy: mean(acc.sum_net_return, acc.resolved),
            fade_expectancy: mean(acc.sum_fade_return, acc.resolved),
            wait_expectancy: {
                let ftr = rate(acc.follow_throughs, acc.resolved);
                let mfe = mean(acc.sum_mfe, acc.resolved);
                crate::math::clamp_signed_unit_interval(mfe * ftr)
            },
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        b.mean_net_return
            .cmp(&a.mean_net_return)
            .then_with(|| b.follow_through_rate.cmp(&a.follow_through_rate))
            .then_with(|| b.structure_retention_rate.cmp(&a.structure_retention_rate))
            .then_with(|| b.hit_rate.cmp(&a.hit_rate))
            .then_with(|| a.label.cmp(&b.label))
    });
    items
}

pub(in super::super::super) fn top_context_outcomes(
    map: HashMap<String, ContextualOutcomeAccumulator>,
) -> Vec<ContextualLineageOutcome> {
    let mut items = map
        .into_values()
        .map(|acc| ContextualLineageOutcome {
            label: acc.label,
            family: acc.family,
            session: acc.session,
            market_regime: acc.market_regime,
            total: acc.total,
            resolved: acc.resolved,
            hits: acc.hits,
            hit_rate: rate(acc.hits, acc.resolved),
            mean_return: mean(acc.sum_return, acc.resolved),
            mean_net_return: mean(acc.sum_net_return, acc.resolved),
            mean_mfe: mean(acc.sum_mfe, acc.resolved),
            mean_mae: mean(acc.sum_mae, acc.resolved),
            follow_through_rate: rate(acc.follow_throughs, acc.resolved),
            invalidation_rate: rate(acc.invalidations, acc.resolved),
            structure_retention_rate: rate(acc.structure_retained, acc.resolved),
            mean_convergence_score: mean(acc.sum_convergence_score, acc.resolved),
            mean_external_delta: mean(acc.sum_external_delta, acc.resolved),
            external_follow_through_rate: rate(acc.external_follow_throughs, acc.resolved),
            follow_expectancy: mean(acc.sum_net_return, acc.resolved),
            fade_expectancy: mean(acc.sum_fade_return, acc.resolved),
            wait_expectancy: {
                let ftr = rate(acc.follow_throughs, acc.resolved);
                let mfe = mean(acc.sum_mfe, acc.resolved);
                crate::math::clamp_signed_unit_interval(mfe * ftr)
            },
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        b.mean_net_return
            .cmp(&a.mean_net_return)
            .then_with(|| b.follow_through_rate.cmp(&a.follow_through_rate))
            .then_with(|| b.structure_retention_rate.cmp(&a.structure_retention_rate))
            .then_with(|| b.hit_rate.cmp(&a.hit_rate))
            .then_with(|| a.label.cmp(&b.label))
    });
    items
}

pub(in super::super::super) fn top_family_context_outcomes(
    map: HashMap<String, FamilyContextAccumulator>,
) -> Vec<FamilyContextLineageOutcome> {
    let mut items = map
        .into_values()
        .map(|acc| FamilyContextLineageOutcome {
            family: acc.family,
            session: acc.session,
            market_regime: acc.market_regime,
            total: acc.total,
            resolved: acc.resolved,
            hits: acc.hits,
            hit_rate: rate(acc.hits, acc.resolved),
            mean_return: mean(acc.sum_return, acc.resolved),
            mean_net_return: mean(acc.sum_net_return, acc.resolved),
            mean_mfe: mean(acc.sum_mfe, acc.resolved),
            mean_mae: mean(acc.sum_mae, acc.resolved),
            follow_through_rate: rate(acc.follow_throughs, acc.resolved),
            invalidation_rate: rate(acc.invalidations, acc.resolved),
            structure_retention_rate: rate(acc.structure_retained, acc.resolved),
            mean_convergence_score: mean(acc.sum_convergence_score, acc.resolved),
            mean_external_delta: mean(acc.sum_external_delta, acc.resolved),
            external_follow_through_rate: rate(acc.external_follow_throughs, acc.resolved),
            follow_expectancy: mean(acc.sum_net_return, acc.resolved),
            fade_expectancy: mean(acc.sum_fade_return, acc.resolved),
            wait_expectancy: {
                let ftr = rate(acc.follow_throughs, acc.resolved);
                let mfe = mean(acc.sum_mfe, acc.resolved);
                crate::math::clamp_signed_unit_interval(mfe * ftr)
            },
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        b.mean_net_return
            .cmp(&a.mean_net_return)
            .then_with(|| b.follow_through_rate.cmp(&a.follow_through_rate))
            .then_with(|| b.structure_retention_rate.cmp(&a.structure_retention_rate))
            .then_with(|| b.hit_rate.cmp(&a.hit_rate))
            .then_with(|| a.family.cmp(&b.family))
            .then_with(|| a.session.cmp(&b.session))
            .then_with(|| a.market_regime.cmp(&b.market_regime))
    });
    items
}

fn rate(count: usize, total: usize) -> Decimal {
    if total == 0 {
        Decimal::ZERO
    } else {
        Decimal::from(count as i64) / Decimal::from(total as i64)
    }
}

fn mean(sum: Decimal, count: usize) -> Decimal {
    if count == 0 {
        Decimal::ZERO
    } else {
        sum / Decimal::from(count as i64)
    }
}
