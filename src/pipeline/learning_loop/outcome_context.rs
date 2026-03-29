use super::*;

pub fn derive_outcome_learning_context_from_hk_rows(
    rows: &[LineageMetricRowRecord],
) -> OutcomeLearningContext {
    let promoted = average_bucket_metrics(rows, "promoted_outcomes");
    let falsified = average_bucket_metrics(rows, "falsified_outcomes");
    let blocked = average_bucket_metrics(rows, "blocked_outcomes");

    let reward_strength = mean(&[
        promoted.follow_through_rate,
        promoted.structure_retention_rate,
        normalize_positive_return(promoted.mean_net_return),
    ]);
    let penalty_strength = mean(&[
        falsified.invalidation_rate.max(blocked.invalidation_rate),
        falsified
            .follow_through_rate
            .max(blocked.follow_through_rate),
        normalize_negative_return(falsified.mean_net_return.min(blocked.mean_net_return)),
    ]);

    OutcomeLearningContext {
        source: "hk_lineage".into(),
        reward_multiplier: clamp_multiplier(reward_strength * dec!(0.5)),
        penalty_multiplier: clamp_multiplier(penalty_strength * dec!(0.5)),
        promoted_follow_through: promoted.follow_through_rate,
        promoted_retention: promoted.structure_retention_rate,
        promoted_mean_net_return: promoted.mean_net_return,
        falsified_invalidation: falsified.invalidation_rate.max(blocked.invalidation_rate),
        falsified_follow_through: falsified
            .follow_through_rate
            .max(blocked.follow_through_rate),
        us_hit_rate: Decimal::ZERO,
        us_mean_return: Decimal::ZERO,
    }
}

pub fn derive_outcome_learning_context_from_case_outcomes(
    outcomes: &[CaseRealizedOutcomeRecord],
    market: &str,
) -> OutcomeLearningContext {
    if outcomes.is_empty() {
        return OutcomeLearningContext::default();
    }

    let reward_strength = mean(&[
        rate(
            outcomes.iter().filter(|item| item.followed_through).count(),
            outcomes.len(),
        ),
        rate(
            outcomes
                .iter()
                .filter(|item| item.structure_retained)
                .count(),
            outcomes.len(),
        ),
        normalize_positive_return(mean(
            &outcomes
                .iter()
                .map(|item| item.net_return)
                .collect::<Vec<_>>(),
        )),
    ]);
    let penalty_strength = mean(&[
        rate(
            outcomes.iter().filter(|item| item.invalidated).count(),
            outcomes.len(),
        ),
        rate(
            outcomes
                .iter()
                .filter(|item| item.net_return < Decimal::ZERO)
                .count(),
            outcomes.len(),
        ),
        normalize_negative_return(mean(
            &outcomes
                .iter()
                .map(|item| item.net_return)
                .collect::<Vec<_>>(),
        )),
    ]);

    OutcomeLearningContext {
        source: format!("{market}_case_outcomes"),
        reward_multiplier: clamp_multiplier(reward_strength * dec!(0.5)),
        penalty_multiplier: clamp_multiplier(penalty_strength * dec!(0.5)),
        promoted_follow_through: rate(
            outcomes.iter().filter(|item| item.followed_through).count(),
            outcomes.len(),
        ),
        promoted_retention: rate(
            outcomes
                .iter()
                .filter(|item| item.structure_retained)
                .count(),
            outcomes.len(),
        ),
        promoted_mean_net_return: mean(
            &outcomes
                .iter()
                .map(|item| item.net_return)
                .collect::<Vec<_>>(),
        ),
        falsified_invalidation: rate(
            outcomes.iter().filter(|item| item.invalidated).count(),
            outcomes.len(),
        ),
        falsified_follow_through: rate(
            outcomes.iter().filter(|item| item.followed_through).count(),
            outcomes.len(),
        ),
        us_hit_rate: Decimal::ZERO,
        us_mean_return: Decimal::ZERO,
    }
}

pub fn derive_outcome_learning_context_from_us_rows(
    rows: &[UsLineageMetricRowRecord],
) -> OutcomeLearningContext {
    if rows.is_empty() {
        return OutcomeLearningContext::default();
    }

    let hit_rate = mean(
        &rows
            .iter()
            .map(|row| row.hit_rate)
            .collect::<Vec<_>>(),
    );
    let mean_return = mean(
        &rows
            .iter()
            .map(|row| row.mean_return)
            .collect::<Vec<_>>(),
    );

    OutcomeLearningContext {
        source: "us_lineage".into(),
        reward_multiplier: clamp_multiplier(
            mean(&[hit_rate, normalize_positive_return(mean_return)]) * dec!(0.5),
        ),
        penalty_multiplier: clamp_multiplier(
            mean(&[
                Decimal::ONE - hit_rate,
                normalize_negative_return(mean_return),
            ]) * dec!(0.5),
        ),
        promoted_follow_through: Decimal::ZERO,
        promoted_retention: Decimal::ZERO,
        promoted_mean_net_return: Decimal::ZERO,
        falsified_invalidation: Decimal::ZERO,
        falsified_follow_through: Decimal::ZERO,
        us_hit_rate: hit_rate,
        us_mean_return: mean_return,
    }
}

#[derive(Default)]
struct HkBucketMetrics {
    mean_net_return: Decimal,
    follow_through_rate: Decimal,
    invalidation_rate: Decimal,
    structure_retention_rate: Decimal,
}

fn average_bucket_metrics(rows: &[LineageMetricRowRecord], bucket: &str) -> HkBucketMetrics {
    let matched = rows
        .iter()
        .filter(|row| row.bucket == bucket)
        .collect::<Vec<_>>();
    if matched.is_empty() {
        return HkBucketMetrics::default();
    }

    HkBucketMetrics {
        mean_net_return: mean(
            &matched
                .iter()
                .map(|row| row.mean_net_return)
                .collect::<Vec<_>>(),
        ),
        follow_through_rate: mean(
            &matched
                .iter()
                .map(|row| row.follow_through_rate)
                .collect::<Vec<_>>(),
        ),
        invalidation_rate: mean(
            &matched
                .iter()
                .map(|row| row.invalidation_rate)
                .collect::<Vec<_>>(),
        ),
        structure_retention_rate: mean(
            &matched
                .iter()
                .map(|row| row.structure_retention_rate)
                .collect::<Vec<_>>(),
        ),
    }
}

fn mean(values: &[Decimal]) -> Decimal {
    if values.is_empty() {
        return Decimal::ZERO;
    }
    values.iter().copied().sum::<Decimal>() / Decimal::from(values.len() as i64)
}

fn rate(count: usize, total: usize) -> Decimal {
    if total == 0 {
        Decimal::ZERO
    } else {
        Decimal::from(count as i64) / Decimal::from(total as i64)
    }
}

fn normalize_positive_return(value: Decimal) -> Decimal {
    if value <= Decimal::ZERO {
        Decimal::ZERO
    } else {
        clamp_unit_interval(value * dec!(4))
    }
}

pub(super) fn normalize_negative_return(value: Decimal) -> Decimal {
    if value >= Decimal::ZERO {
        Decimal::ZERO
    } else {
        clamp_unit_interval((-value) * dec!(8))
    }
}

fn clamp_multiplier(value: Decimal) -> Decimal {
    if value < Decimal::ZERO {
        Decimal::ZERO
    } else if value > dec!(0.5) {
        dec!(0.5)
    } else {
        value
    }
}
