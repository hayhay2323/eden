use std::collections::HashMap;

use rust_decimal::Decimal;

use super::super::{EvaluatedOutcome, SetupOutcomeContext};

#[derive(Default)]
pub(in super::super::super) struct OutcomeAccumulator {
    pub(super) total: usize,
    pub(super) resolved: usize,
    pub(super) hits: usize,
    pub(super) sum_return: Decimal,
    pub(super) sum_net_return: Decimal,
    pub(super) sum_fade_return: Decimal,
    pub(super) sum_mfe: Decimal,
    pub(super) sum_mae: Decimal,
    pub(super) follow_throughs: usize,
    pub(super) invalidations: usize,
    pub(super) structure_retained: usize,
    pub(super) sum_convergence_score: Decimal,
    pub(super) sum_external_delta: Decimal,
    pub(super) external_follow_throughs: usize,
}

#[derive(Default)]
pub(in super::super::super) struct ContextualOutcomeAccumulator {
    pub(super) label: String,
    pub(super) family: String,
    pub(super) session: String,
    pub(super) market_regime: String,
    pub(super) total: usize,
    pub(super) resolved: usize,
    pub(super) hits: usize,
    pub(super) sum_return: Decimal,
    pub(super) sum_net_return: Decimal,
    pub(super) sum_fade_return: Decimal,
    pub(super) sum_mfe: Decimal,
    pub(super) sum_mae: Decimal,
    pub(super) follow_throughs: usize,
    pub(super) invalidations: usize,
    pub(super) structure_retained: usize,
    pub(super) sum_convergence_score: Decimal,
    pub(super) sum_external_delta: Decimal,
    pub(super) external_follow_throughs: usize,
}

#[derive(Default)]
pub(in super::super::super) struct FamilyContextAccumulator {
    pub(super) family: String,
    pub(super) session: String,
    pub(super) market_regime: String,
    pub(super) total: usize,
    pub(super) resolved: usize,
    pub(super) hits: usize,
    pub(super) sum_return: Decimal,
    pub(super) sum_net_return: Decimal,
    pub(super) sum_fade_return: Decimal,
    pub(super) sum_mfe: Decimal,
    pub(super) sum_mae: Decimal,
    pub(super) follow_throughs: usize,
    pub(super) invalidations: usize,
    pub(super) structure_retained: usize,
    pub(super) sum_convergence_score: Decimal,
    pub(super) sum_external_delta: Decimal,
    pub(super) external_follow_throughs: usize,
}

pub(in super::super::super) fn update_outcome(
    map: &mut HashMap<String, OutcomeAccumulator>,
    label: &str,
    outcome: Option<&EvaluatedOutcome>,
) {
    let item = map.entry(label.to_string()).or_default();
    item.total += 1;
    if let Some(outcome) = outcome {
        item.resolved += 1;
        if outcome.net_return > Decimal::ZERO {
            item.hits += 1;
        }
        if outcome.followed_through {
            item.follow_throughs += 1;
        }
        if outcome.invalidated {
            item.invalidations += 1;
        }
        if outcome.structure_retained {
            item.structure_retained += 1;
        }
        if outcome.external_follow_through {
            item.external_follow_throughs += 1;
        }
        item.sum_convergence_score += outcome.convergence_score;
        item.sum_return += outcome.return_pct;
        item.sum_net_return += outcome.net_return;
        item.sum_fade_return += outcome.fade_return;
        item.sum_mfe += outcome.max_favorable_excursion;
        item.sum_mae += outcome.max_adverse_excursion;
        item.sum_external_delta += outcome.external_delta;
    }
}

pub(in super::super::super) fn update_context_outcome(
    map: &mut HashMap<String, ContextualOutcomeAccumulator>,
    label: &str,
    context: &SetupOutcomeContext,
    outcome: Option<&EvaluatedOutcome>,
) {
    let key = format!(
        "{}|{}|{}|{}",
        label, context.family, context.session, context.market_regime
    );
    let item = map
        .entry(key)
        .or_insert_with(|| ContextualOutcomeAccumulator {
            label: label.to_string(),
            family: context.family.clone(),
            session: context.session.clone(),
            market_regime: context.market_regime.clone(),
            ..Default::default()
        });
    item.total += 1;
    if let Some(outcome) = outcome {
        item.resolved += 1;
        if outcome.net_return > Decimal::ZERO {
            item.hits += 1;
        }
        if outcome.followed_through {
            item.follow_throughs += 1;
        }
        if outcome.invalidated {
            item.invalidations += 1;
        }
        if outcome.structure_retained {
            item.structure_retained += 1;
        }
        if outcome.external_follow_through {
            item.external_follow_throughs += 1;
        }
        item.sum_convergence_score += outcome.convergence_score;
        item.sum_return += outcome.return_pct;
        item.sum_net_return += outcome.net_return;
        item.sum_fade_return += outcome.fade_return;
        item.sum_mfe += outcome.max_favorable_excursion;
        item.sum_mae += outcome.max_adverse_excursion;
        item.sum_external_delta += outcome.external_delta;
    }
}

pub(in super::super::super) fn update_family_context_outcome(
    map: &mut HashMap<String, FamilyContextAccumulator>,
    context: &SetupOutcomeContext,
    outcome: Option<&EvaluatedOutcome>,
) {
    let key = format!(
        "{}|{}|{}",
        context.family, context.session, context.market_regime
    );
    let item = map.entry(key).or_insert_with(|| FamilyContextAccumulator {
        family: context.family.clone(),
        session: context.session.clone(),
        market_regime: context.market_regime.clone(),
        ..Default::default()
    });
    item.total += 1;
    if let Some(outcome) = outcome {
        item.resolved += 1;
        if outcome.net_return > Decimal::ZERO {
            item.hits += 1;
        }
        if outcome.followed_through {
            item.follow_throughs += 1;
        }
        if outcome.invalidated {
            item.invalidations += 1;
        }
        if outcome.structure_retained {
            item.structure_retained += 1;
        }
        if outcome.external_follow_through {
            item.external_follow_throughs += 1;
        }
        item.sum_convergence_score += outcome.convergence_score;
        item.sum_return += outcome.return_pct;
        item.sum_net_return += outcome.net_return;
        item.sum_fade_return += outcome.fade_return;
        item.sum_mfe += outcome.max_favorable_excursion;
        item.sum_mae += outcome.max_adverse_excursion;
        item.sum_external_delta += outcome.external_delta;
    }
}
