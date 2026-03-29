use super::*;

pub(crate) fn symbol_mark_price(snapshot: &AgentSnapshot, symbol: &str) -> Option<Decimal> {
    snapshot
        .symbol(symbol)
        .and_then(|item| item.signal.as_ref())
        .and_then(|signal| signal.mark_price)
}

pub(crate) fn resolve_recommendation_outcome(
    snapshot: &AgentSnapshot,
    recommendation: &AgentRecommendation,
) -> Option<AgentRecommendationResolution> {
    if recommendation.resolution.is_some()
        || snapshot.tick < recommendation.tick.saturating_add(recommendation.horizon_ticks)
    {
        return None;
    }

    let entry_price = recommendation.price_at_decision?;
    if entry_price <= Decimal::ZERO {
        return None;
    }
    let current_price = symbol_mark_price(snapshot, &recommendation.symbol)?;
    let price_return = (current_price - entry_price) / entry_price;
    let follow_realized_return =
        price_return * Decimal::from(recommendation_bias_direction(&recommendation.bias) as i64);
    let fade_realized_return = -follow_realized_return;
    let wait_realized_return = Decimal::ZERO;
    let threshold = Decimal::new(20, 4);

    let counterfactual_best_action =
        best_counterfactual_action(follow_realized_return, fade_realized_return, threshold);
    let wait_regret = counterfactual_regret(
        follow_realized_return,
        fade_realized_return,
        wait_realized_return,
    );
    let best_action_return = realized_return_for_action(
        &recommendation.best_action,
        follow_realized_return,
        fade_realized_return,
    );
    let status = recommendation_resolution_status(
        &recommendation.best_action,
        best_action_return,
        wait_regret,
        threshold,
    );

    Some(AgentRecommendationResolution {
        resolved_tick: snapshot.tick,
        ticks_elapsed: snapshot.tick.saturating_sub(recommendation.tick),
        status: status.into(),
        price_return: price_return.round_dp(4),
        follow_realized_return: follow_realized_return.round_dp(4),
        fade_realized_return: fade_realized_return.round_dp(4),
        wait_regret: wait_regret.round_dp(4),
        counterfactual_best_action: counterfactual_best_action.into(),
        best_action_was_correct: recommendation.best_action == counterfactual_best_action,
    })
}

pub(crate) fn resolve_market_recommendation_outcome(
    snapshot: &AgentSnapshot,
    recommendation: &AgentMarketRecommendation,
) -> Option<AgentRecommendationResolution> {
    if recommendation.resolution.is_some()
        || snapshot.tick < recommendation.tick.saturating_add(recommendation.horizon_ticks)
    {
        return None;
    }

    let basis_return =
        snapshot.market_regime.average_return - recommendation.average_return_at_decision;
    let follow_realized_return =
        basis_return * Decimal::from(recommendation_bias_direction(&recommendation.bias) as i64);
    let fade_realized_return = -follow_realized_return;
    let wait_realized_return = Decimal::ZERO;
    let threshold = Decimal::new(25, 4);

    let counterfactual_best_action =
        best_counterfactual_action(follow_realized_return, fade_realized_return, threshold);
    let wait_regret = counterfactual_regret(
        follow_realized_return,
        fade_realized_return,
        wait_realized_return,
    );
    let best_action_return = realized_return_for_action(
        &recommendation.best_action,
        follow_realized_return,
        fade_realized_return,
    );
    let status = recommendation_resolution_status(
        &recommendation.best_action,
        best_action_return,
        wait_regret,
        threshold,
    );

    Some(AgentRecommendationResolution {
        resolved_tick: snapshot.tick,
        ticks_elapsed: snapshot.tick.saturating_sub(recommendation.tick),
        status: status.into(),
        price_return: basis_return.round_dp(4),
        follow_realized_return: follow_realized_return.round_dp(4),
        fade_realized_return: fade_realized_return.round_dp(4),
        wait_regret: wait_regret.round_dp(4),
        counterfactual_best_action: counterfactual_best_action.into(),
        best_action_was_correct: recommendation.best_action == counterfactual_best_action,
    })
}

pub(crate) fn resolve_sector_recommendation_outcome(
    snapshot: &AgentSnapshot,
    recommendation: &AgentSectorRecommendation,
) -> Option<AgentRecommendationResolution> {
    if recommendation.resolution.is_some()
        || snapshot.tick < recommendation.tick.saturating_add(recommendation.horizon_ticks)
    {
        return None;
    }

    let current = sector_reference_value(snapshot, &recommendation.reference_symbols)?;
    let basis_return = current - recommendation.average_return_at_decision;
    let follow_realized_return =
        basis_return * Decimal::from(recommendation_bias_direction(&recommendation.bias) as i64);
    let fade_realized_return = -follow_realized_return;
    let threshold = Decimal::new(25, 4);
    let counterfactual_best_action =
        best_counterfactual_action(follow_realized_return, fade_realized_return, threshold);
    let wait_regret =
        counterfactual_regret(follow_realized_return, fade_realized_return, Decimal::ZERO);
    let best_action_return = realized_return_for_action(
        &recommendation.best_action,
        follow_realized_return,
        fade_realized_return,
    );
    let status = recommendation_resolution_status(
        &recommendation.best_action,
        best_action_return,
        wait_regret,
        threshold,
    );

    Some(AgentRecommendationResolution {
        resolved_tick: snapshot.tick,
        ticks_elapsed: snapshot.tick.saturating_sub(recommendation.tick),
        status: status.into(),
        price_return: basis_return.round_dp(4),
        follow_realized_return: follow_realized_return.round_dp(4),
        fade_realized_return: fade_realized_return.round_dp(4),
        wait_regret: wait_regret.round_dp(4),
        counterfactual_best_action: counterfactual_best_action.into(),
        best_action_was_correct: recommendation.best_action == counterfactual_best_action,
    })
}

pub(crate) fn sector_reference_value(
    snapshot: &AgentSnapshot,
    symbols: &[String],
) -> Option<Decimal> {
    if symbols.is_empty() {
        return None;
    }
    let prices = symbols
        .iter()
        .filter_map(|symbol| symbol_mark_price(snapshot, symbol))
        .collect::<Vec<_>>();
    if prices.is_empty() {
        return None;
    }
    Some(prices.iter().copied().sum::<Decimal>() / Decimal::from(prices.len() as i64))
}

fn recommendation_bias_direction(bias: &str) -> i8 {
    match bias {
        "long" => 1,
        "short" => -1,
        _ => 0,
    }
}

pub(crate) fn best_counterfactual_action(
    follow_realized_return: Decimal,
    fade_realized_return: Decimal,
    threshold: Decimal,
) -> &'static str {
    let mut best_action = "wait";
    let mut best_return = Decimal::ZERO;
    if follow_realized_return > best_return {
        best_action = "follow";
        best_return = follow_realized_return;
    }
    if fade_realized_return > best_return {
        best_action = "fade";
        best_return = fade_realized_return;
    }
    if best_return <= threshold {
        "wait"
    } else {
        best_action
    }
}

pub(crate) fn counterfactual_regret(
    follow_realized_return: Decimal,
    fade_realized_return: Decimal,
    wait_realized_return: Decimal,
) -> Decimal {
    follow_realized_return
        .max(fade_realized_return)
        .max(wait_realized_return)
        - wait_realized_return
}

pub(crate) fn realized_return_for_action(
    action: &str,
    follow_realized_return: Decimal,
    fade_realized_return: Decimal,
) -> Decimal {
    match action {
        "follow" => follow_realized_return,
        "fade" => fade_realized_return,
        _ => Decimal::ZERO,
    }
}

pub(crate) fn recommendation_resolution_status(
    best_action: &str,
    best_action_return: Decimal,
    wait_regret: Decimal,
    threshold: Decimal,
) -> &'static str {
    match best_action {
        "wait" => {
            if wait_regret > threshold {
                "miss"
            } else {
                "hit"
            }
        }
        _ => {
            if best_action_return > threshold {
                "hit"
            } else if best_action_return < -threshold {
                "miss"
            } else {
                "flat"
            }
        }
    }
}
