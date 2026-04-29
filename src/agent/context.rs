use super::*;

pub(crate) fn hk_context_action_expectancies(
    item: &FamilyContextLineageOutcome,
) -> AgentActionExpectancies {
    if item.follow_expectancy != Decimal::ZERO
        || item.fade_expectancy != Decimal::ZERO
        || item.wait_expectancy != Decimal::ZERO
    {
        return lineage_action_expectancies(
            item.follow_expectancy,
            item.fade_expectancy,
            item.wait_expectancy,
        );
    }
    historical_action_expectancies(
        item.mean_net_return,
        item.resolved,
        decimal_mean(
            item.hit_rate
                + item.follow_through_rate
                + item.structure_retention_rate
                + (Decimal::ONE - item.invalidation_rate),
            4,
        ),
        decimal_mean(
            (Decimal::ONE - item.hit_rate)
                + item.invalidation_rate
                + (Decimal::ONE - item.follow_through_rate)
                + (Decimal::ONE - item.structure_retention_rate),
            4,
        ),
    )
}

pub(crate) fn us_context_action_expectancies(
    item: &crate::us::temporal::lineage::UsLineageContextStats,
) -> AgentActionExpectancies {
    if item.follow_expectancy != Decimal::ZERO
        || item.fade_expectancy != Decimal::ZERO
        || item.wait_expectancy != Decimal::ZERO
    {
        return lineage_action_expectancies(
            item.follow_expectancy,
            item.fade_expectancy,
            item.wait_expectancy,
        );
    }
    historical_action_expectancies(
        item.mean_return,
        item.resolved,
        item.hit_rate,
        Decimal::ONE - item.hit_rate,
    )
}

fn lineage_action_expectancies(
    follow_expectancy: Decimal,
    fade_expectancy: Decimal,
    wait_expectancy: Decimal,
) -> AgentActionExpectancies {
    AgentActionExpectancies {
        follow_expectancy: Some(follow_expectancy.round_dp(4)),
        fade_expectancy: Some(fade_expectancy.round_dp(4)),
        wait_expectancy: Some(wait_expectancy.round_dp(4)),
    }
}

fn historical_action_expectancies(
    expected_net_alpha: Decimal,
    resolved: usize,
    follow_support: Decimal,
    fade_support: Decimal,
) -> AgentActionExpectancies {
    let mut expectancies = AgentActionExpectancies {
        wait_expectancy: Some(Decimal::ZERO),
        ..AgentActionExpectancies::default()
    };
    if resolved < 3 {
        return expectancies;
    }

    let confidence_scale = prior_confidence_scale(resolved);
    let follow_multiplier =
        Decimal::new(5, 1) + clamp_unit_interval(follow_support) * confidence_scale;
    let fade_multiplier = Decimal::new(5, 1) + clamp_unit_interval(fade_support) * confidence_scale;
    expectancies.follow_expectancy = Some((expected_net_alpha * follow_multiplier).round_dp(4));
    expectancies.fade_expectancy = Some((-expected_net_alpha * fade_multiplier).round_dp(4));
    expectancies
}

fn prior_confidence_scale(resolved: usize) -> Decimal {
    if resolved == 0 {
        Decimal::ZERO
    } else {
        Decimal::from(resolved.min(8) as i64) / Decimal::from(8)
    }
}

pub(crate) use crate::math::clamp_unit_interval;

pub(crate) fn world_state_regime(world_state: &WorldStateSnapshot) -> &str {
    world_state
        .entities
        .iter()
        .find(|entity| matches!(entity.scope, ReasoningScope::Market(_)))
        .map(|entity| entity.regime.as_str())
        .unwrap_or("unknown")
}

fn hk_session_label(timestamp: OffsetDateTime) -> &'static str {
    use crate::ontology::horizon::SessionPhase;
    let hk = timestamp.to_offset(UtcOffset::from_hms(8, 0, 0).expect("valid hk offset"));
    let minutes = u16::from(hk.hour()) * 60 + u16::from(hk.minute());
    let phase = match minutes {
        570..=630 => SessionPhase::Opening,
        631..=870 => SessionPhase::Midday,
        871..=970 => SessionPhase::Closing,
        _ => SessionPhase::AfterHours,
    };
    phase.as_label()
}

pub(crate) fn best_hk_context_prior(
    family: &str,
    timestamp: OffsetDateTime,
    market_regime: &str,
    priors: &[FamilyContextLineageOutcome],
) -> Option<AgentContextPrior> {
    let session = hk_session_label(timestamp);
    priors
        .iter()
        .find(|item| {
            item.family == family && item.session == session && item.market_regime == market_regime
        })
        .map(|item| AgentContextPrior {
            family: item.family.clone(),
            session: item.session.clone(),
            market_regime: item.market_regime.clone(),
            resolved: item.resolved,
            hit_rate: item.hit_rate,
            expected_net_alpha: item.mean_net_return,
            action_expectancies: hk_context_action_expectancies(item),
            follow_through_rate: Some(item.follow_through_rate),
            invalidation_rate: Some(item.invalidation_rate),
            structure_retention_rate: Some(item.structure_retention_rate),
        })
}

pub(crate) fn current_hk_context_priors(
    priors: &[FamilyContextLineageOutcome],
    timestamp: OffsetDateTime,
    market_regime: &str,
) -> Vec<AgentContextPrior> {
    let session = hk_session_label(timestamp);
    let mut items = priors
        .iter()
        .filter(|item| item.session == session && item.market_regime == market_regime)
        .map(|item| AgentContextPrior {
            family: item.family.clone(),
            session: item.session.clone(),
            market_regime: item.market_regime.clone(),
            resolved: item.resolved,
            hit_rate: item.hit_rate,
            expected_net_alpha: item.mean_net_return,
            action_expectancies: hk_context_action_expectancies(item),
            follow_through_rate: Some(item.follow_through_rate),
            invalidation_rate: Some(item.invalidation_rate),
            structure_retention_rate: Some(item.structure_retention_rate),
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        b.expected_net_alpha
            .cmp(&a.expected_net_alpha)
            .then_with(|| b.hit_rate.cmp(&a.hit_rate))
            .then_with(|| a.family.cmp(&b.family))
    });
    items.truncate(24);
    items
}

pub(crate) fn current_us_context_priors(
    lineage_stats: &UsLineageStats,
    timestamp: OffsetDateTime,
    market_regime: &str,
) -> Vec<AgentContextPrior> {
    let session = crate::us::temporal::lineage::classify_us_session(timestamp).as_str();
    let mut items = lineage_stats
        .by_context
        .iter()
        .filter(|item| item.session == session && item.market_regime == market_regime)
        .map(|item| AgentContextPrior {
            family: item.template.clone(),
            session: item.session.clone(),
            market_regime: item.market_regime.clone(),
            resolved: item.resolved,
            hit_rate: item.hit_rate,
            expected_net_alpha: item.mean_return,
            action_expectancies: us_context_action_expectancies(item),
            follow_through_rate: None,
            invalidation_rate: None,
            structure_retention_rate: None,
        })
        .collect::<Vec<_>>();
    for item in &lineage_stats.by_template {
        if items
            .iter()
            .any(|existing| existing.family == item.template)
        {
            continue;
        }
        items.push(AgentContextPrior {
            family: item.template.clone(),
            session: "any".into(),
            market_regime: "any".into(),
            resolved: item.resolved,
            hit_rate: item.hit_rate,
            expected_net_alpha: item.mean_return,
            action_expectancies: us_context_action_expectancies(item),
            follow_through_rate: None,
            invalidation_rate: None,
            structure_retention_rate: None,
        });
    }
    items.sort_by(|a, b| {
        b.expected_net_alpha
            .cmp(&a.expected_net_alpha)
            .then_with(|| b.hit_rate.cmp(&a.hit_rate))
            .then_with(|| a.family.cmp(&b.family))
    });
    items.truncate(24);
    items
}

pub(crate) fn best_us_context_prior(
    family: &str,
    timestamp: OffsetDateTime,
    market_regime: &str,
    lineage_stats: &UsLineageStats,
) -> Option<AgentContextPrior> {
    let session = crate::us::temporal::lineage::classify_us_session(timestamp).as_str();
    lineage_stats
        .by_context
        .iter()
        .find(|item| {
            item.template == family
                && item.session == session
                && item.market_regime == market_regime
        })
        .map(|item| AgentContextPrior {
            family: item.template.clone(),
            session: item.session.clone(),
            market_regime: item.market_regime.clone(),
            resolved: item.resolved,
            hit_rate: item.hit_rate,
            expected_net_alpha: item.mean_return,
            action_expectancies: us_context_action_expectancies(item),
            follow_through_rate: None,
            invalidation_rate: None,
            structure_retention_rate: None,
        })
        .or_else(|| {
            lineage_stats
                .by_template
                .iter()
                .find(|item| item.template == family)
                .map(|item| AgentContextPrior {
                    family: item.template.clone(),
                    session: "any".into(),
                    market_regime: "any".into(),
                    resolved: item.resolved,
                    hit_rate: item.hit_rate,
                    expected_net_alpha: item.mean_return,
                    action_expectancies: us_context_action_expectancies(item),
                    follow_through_rate: None,
                    invalidation_rate: None,
                    structure_retention_rate: None,
                })
        })
}
