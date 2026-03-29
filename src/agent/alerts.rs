use super::*;

#[derive(Default)]
struct AgentAlertSliceAccumulator {
    total_alerts: usize,
    resolved_alerts: usize,
    hits: usize,
    misses: usize,
    flats: usize,
    oriented_return_sum: Decimal,
    oriented_return_count: usize,
}

pub(crate) fn alert_resolution_action(action: &str) -> &'static str {
    match action {
        "follow" | "enter" | "add" | "watch" | "review" => "follow",
        "fade" | "trim" | "hedge" => "fade",
        "wait" => "wait",
        _ => "wait",
    }
}

pub(crate) fn resolve_alert_resolution(
    snapshot: &AgentSnapshot,
    alert: &AgentAlertRecord,
) -> Option<AgentRecommendationResolution> {
    if snapshot.tick < alert.tick.saturating_add(alert.horizon_ticks) {
        return None;
    }

    let reference_at_alert = alert.reference_value_at_alert.or(alert.price_at_alert)?;
    if reference_at_alert <= Decimal::ZERO {
        return None;
    }

    let current_reference = current_alert_reference_value(snapshot, alert)?;
    let price_return = (current_reference - reference_at_alert) / reference_at_alert;
    let expected_direction = expected_alert_direction(&alert.suggested_action, &alert.action_bias);
    let follow_realized_return = if expected_direction == 0 {
        Decimal::ZERO
    } else {
        price_return * Decimal::from(expected_direction as i64)
    };
    let fade_realized_return = -follow_realized_return;
    let wait_realized_return = Decimal::ZERO;
    let threshold = Decimal::new(20, 4);
    let best_action = alert_resolution_action(&alert.suggested_action);
    let counterfactual_best_action =
        best_counterfactual_action(follow_realized_return, fade_realized_return, threshold);
    let wait_regret = counterfactual_regret(
        follow_realized_return,
        fade_realized_return,
        wait_realized_return,
    );
    let best_action_return =
        realized_return_for_action(best_action, follow_realized_return, fade_realized_return);
    let status =
        recommendation_resolution_status(best_action, best_action_return, wait_regret, threshold);

    Some(AgentRecommendationResolution {
        resolved_tick: snapshot.tick,
        ticks_elapsed: snapshot.tick.saturating_sub(alert.tick),
        status: status.into(),
        price_return: price_return.round_dp(4),
        follow_realized_return: follow_realized_return.round_dp(4),
        fade_realized_return: fade_realized_return.round_dp(4),
        wait_regret: wait_regret.round_dp(4),
        counterfactual_best_action: counterfactual_best_action.into(),
        best_action_was_correct: best_action == counterfactual_best_action,
    })
}

pub(crate) fn alert_outcome_from_resolution(
    alert: &AgentAlertRecord,
    resolution: Option<&AgentRecommendationResolution>,
) -> Option<AgentAlertOutcome> {
    let resolution = resolution?;
    Some(AgentAlertOutcome {
        resolved_tick: resolution.resolved_tick,
        ticks_elapsed: resolution.ticks_elapsed,
        status: resolution.status.clone(),
        price_return: Some(resolution.price_return),
        oriented_return: alert_oriented_return(alert, resolution),
        follow_through: summarize_follow_through(&resolution.status, resolution.price_return),
    })
}

pub(crate) fn compute_alert_stats(alerts: &[AgentAlertRecord]) -> AgentAlertStats {
    let mut accumulator = AgentAlertSliceAccumulator::default();
    for alert in alerts {
        update_alert_accumulator(&mut accumulator, alert);
    }

    AgentAlertStats {
        total_alerts: accumulator.total_alerts,
        resolved_alerts: accumulator.resolved_alerts,
        hits: accumulator.hits,
        misses: accumulator.misses,
        flats: accumulator.flats,
        hit_rate: decimal_ratio(accumulator.hits, accumulator.resolved_alerts),
        mean_oriented_return: decimal_mean(
            accumulator.oriented_return_sum,
            accumulator.oriented_return_count,
        ),
        false_positive_rate: decimal_ratio(accumulator.misses, accumulator.resolved_alerts),
    }
}

pub(crate) fn compute_alert_slice_stats<F>(
    alerts: &[AgentAlertRecord],
    mut key_fn: F,
) -> Vec<AgentAlertSliceStat>
where
    F: FnMut(&AgentAlertRecord) -> String,
{
    let mut slices = HashMap::<String, AgentAlertSliceAccumulator>::new();
    for alert in alerts {
        let key = key_fn(alert);
        update_alert_accumulator(slices.entry(key).or_default(), alert);
    }

    let mut items = slices
        .into_iter()
        .map(|(key, accumulator)| AgentAlertSliceStat {
            key,
            total_alerts: accumulator.total_alerts,
            resolved_alerts: accumulator.resolved_alerts,
            hits: accumulator.hits,
            misses: accumulator.misses,
            flats: accumulator.flats,
            hit_rate: decimal_ratio(accumulator.hits, accumulator.resolved_alerts),
            mean_oriented_return: decimal_mean(
                accumulator.oriented_return_sum,
                accumulator.oriented_return_count,
            ),
            false_positive_rate: decimal_ratio(accumulator.misses, accumulator.resolved_alerts),
        })
        .collect::<Vec<_>>();

    items.sort_by(|a, b| {
        b.total_alerts
            .cmp(&a.total_alerts)
            .then_with(|| a.key.cmp(&b.key))
    });
    items
}

pub(crate) fn top_positive_slices(
    items: &[AgentAlertSliceStat],
    limit: usize,
) -> Vec<AgentAlertSliceStat> {
    let mut items = items
        .iter()
        .filter(|item| item.resolved_alerts > 0)
        .cloned()
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        b.hit_rate
            .cmp(&a.hit_rate)
            .then_with(|| b.mean_oriented_return.cmp(&a.mean_oriented_return))
            .then_with(|| b.resolved_alerts.cmp(&a.resolved_alerts))
            .then_with(|| a.key.cmp(&b.key))
    });
    items.truncate(limit);
    items
}

pub(crate) fn top_noisy_slices(
    items: &[AgentAlertSliceStat],
    limit: usize,
) -> Vec<AgentAlertSliceStat> {
    let mut items = items
        .iter()
        .filter(|item| item.resolved_alerts > 0)
        .cloned()
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        b.false_positive_rate
            .cmp(&a.false_positive_rate)
            .then_with(|| a.hit_rate.cmp(&b.hit_rate))
            .then_with(|| b.resolved_alerts.cmp(&a.resolved_alerts))
            .then_with(|| a.key.cmp(&b.key))
    });
    items.truncate(limit);
    items
}

pub(crate) fn top_resolved_alerts(
    alerts: &[AgentAlertRecord],
    status: &str,
    limit: usize,
) -> Vec<AgentResolvedAlertDigest> {
    let mut items = alerts
        .iter()
        .filter_map(|alert| {
            let resolution = alert.resolution.as_ref()?;
            (resolution.status == status).then(|| AgentResolvedAlertDigest {
                alert_id: alert.alert_id.clone(),
                symbol: alert.symbol.clone(),
                kind: alert.kind.clone(),
                suggested_action: alert.suggested_action.clone(),
                sector: alert.sector.clone(),
                follow_through: summarize_follow_through(
                    &resolution.status,
                    resolution.price_return,
                ),
                oriented_return: alert_oriented_return(alert, resolution),
                why: alert.why.clone(),
            })
        })
        .collect::<Vec<_>>();

    items.sort_by(|a, b| {
        b.oriented_return
            .unwrap_or(Decimal::ZERO)
            .cmp(&a.oriented_return.unwrap_or(Decimal::ZERO))
            .then_with(|| a.alert_id.cmp(&b.alert_id))
    });
    if status == "miss" {
        items.reverse();
    }
    items.truncate(limit);
    items
}

fn alert_oriented_return(
    alert: &AgentAlertRecord,
    resolution: &AgentRecommendationResolution,
) -> Option<Decimal> {
    let normalized_action = alert_resolution_action(&alert.suggested_action);
    match normalized_action {
        "follow" => Some(resolution.follow_realized_return),
        "fade" => Some(resolution.fade_realized_return),
        "wait" => Some(Decimal::ZERO),
        _ => None,
    }
}

fn current_alert_reference_value(
    snapshot: &AgentSnapshot,
    alert: &AgentAlertRecord,
) -> Option<Decimal> {
    if !alert.reference_symbols.is_empty() {
        return sector_reference_value(snapshot, &alert.reference_symbols);
    }
    let symbol = alert.symbol.as_deref()?;
    symbol_mark_price(snapshot, symbol)
}

fn expected_alert_direction(action: &str, bias: &str) -> i8 {
    match (action, bias) {
        ("follow", "long") => 1,
        ("follow", "short") => -1,
        ("fade", "long") => -1,
        ("fade", "short") => 1,
        ("wait", _) => 0,
        ("enter" | "add" | "watch" | "review", "long") => 1,
        ("enter" | "add" | "watch" | "review", "short") => -1,
        ("trim" | "hedge", "long") => -1,
        ("trim" | "hedge", "short") => 1,
        _ => 0,
    }
}

fn summarize_follow_through(status: &str, price_return: Decimal) -> String {
    let pct = (price_return * Decimal::from(100)).round_dp(2);
    match status {
        "hit" => format!("follow-through {pct:+}%"),
        "miss" => format!("reversed {pct:+}%"),
        "flat" => format!("flat {pct:+}%"),
        _ => format!("unscored {pct:+}%"),
    }
}

fn update_alert_accumulator(
    accumulator: &mut AgentAlertSliceAccumulator,
    alert: &AgentAlertRecord,
) {
    accumulator.total_alerts += 1;
    let Some(resolution) = &alert.resolution else {
        return;
    };

    accumulator.resolved_alerts += 1;
    match resolution.status.as_str() {
        "hit" => accumulator.hits += 1,
        "miss" => accumulator.misses += 1,
        "flat" => accumulator.flats += 1,
        _ => {}
    }
    if let Some(value) = alert_oriented_return(alert, resolution) {
        accumulator.oriented_return_sum += value;
        accumulator.oriented_return_count += 1;
    }
}

fn decimal_ratio(numerator: usize, denominator: usize) -> Decimal {
    if denominator == 0 {
        Decimal::ZERO
    } else {
        Decimal::from(numerator as i64) / Decimal::from(denominator as i64)
    }
}
