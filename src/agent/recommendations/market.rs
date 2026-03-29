use super::*;
use crate::action::workflow::{ActionExecutionPolicy, ActionGovernanceContract};
use crate::agent::{governance_reason_code_for_signal_action, governance_reason_for_signal_action};

pub(super) fn build_market_recommendation(
    snapshot: &AgentSnapshot,
    items: &[AgentRecommendation],
) -> Option<AgentMarketRecommendation> {
    let breadth_up = snapshot.market_regime.breadth_up;
    let breadth_down = snapshot.market_regime.breadth_down;
    let average_return = snapshot.market_regime.average_return;
    let synchrony = snapshot.stress.sector_synchrony.unwrap_or(Decimal::ZERO);
    let pressure_consensus = snapshot
        .stress
        .pressure_consensus
        .unwrap_or(Decimal::ZERO)
        .abs();
    let transition_burst = Decimal::from(
        snapshot
            .recent_transitions
            .iter()
            .filter(|item| item.to_tick == snapshot.tick)
            .count()
            .min(8) as i64,
    ) / Decimal::from(8);
    let aligned_sector_ratio = sector_alignment_ratio(snapshot, average_return);
    let breadth_extreme = breadth_up.max(breadth_down);
    let return_impulse = clamp_unit_interval(average_return.abs() / Decimal::new(3, 2));
    let market_impulse_score = decimal_mean(
        breadth_extreme + return_impulse + aligned_sector_ratio + synchrony,
        4,
    )
    .round_dp(4);
    let macro_regime_discontinuity = decimal_mean(
        breadth_extreme + synchrony + pressure_consensus + transition_burst,
        4,
    )
    .round_dp(4);

    let bias = if breadth_up >= Decimal::new(75, 2) && average_return > Decimal::ZERO {
        "long"
    } else if breadth_down >= Decimal::new(75, 2) && average_return < Decimal::ZERO {
        "short"
    } else {
        "neutral"
    };

    let best_action = if bias == "neutral"
        || market_impulse_score < Decimal::new(62, 2)
        || macro_regime_discontinuity < Decimal::new(55, 2)
    {
        "wait"
    } else {
        "follow"
    };

    let focus_sectors = snapshot
        .sector_flows
        .iter()
        .filter(|flow| {
            let sign = decimal_sign(flow.average_composite);
            (bias == "long" && sign > 0) || (bias == "short" && sign < 0)
        })
        .take(3)
        .map(|flow| flow.sector.clone())
        .collect::<Vec<_>>();

    let preferred_expression = preferred_market_expression(
        snapshot,
        bias,
        market_impulse_score,
        aligned_sector_ratio,
        &focus_sectors,
    );
    let why_not_single_name = market_why_not_single_name(snapshot, items, best_action);
    let decisive_factors = market_decisive_factors(
        snapshot,
        bias,
        market_impulse_score,
        macro_regime_discontinuity,
        aligned_sector_ratio,
    );
    let expected_net_alpha = if best_action == "follow" {
        Some(
            (average_return.abs() * Decimal::new(35, 2))
                .max(Decimal::new(20, 4))
                .round_dp(4),
        )
    } else {
        None
    };
    let summary = if best_action == "follow" {
        format!(
            "market-level {} impulse detected; use {} instead of forcing single names",
            bias, preferred_expression
        )
    } else {
        format!(
            "market tape is active, but single-name dispersion still dominates; stay selective on {}",
            preferred_expression
        )
    };

    let governance = ActionGovernanceContract::for_recommendation(ActionExecutionPolicy::ManualOnly);
    let governance_reason = governance_reason_for_signal_action(
        best_action,
        "high",
        None,
        expected_net_alpha,
        governance.execution_policy,
    );
    let governance_reason_code = governance_reason_code_for_signal_action(
        best_action,
        "high",
        None,
        expected_net_alpha,
        governance.execution_policy,
    );
    Some(AgentMarketRecommendation {
        recommendation_id: format!("market:{}:{}", snapshot.tick, preferred_expression),
        tick: snapshot.tick,
        market: snapshot.market,
        regime_bias: snapshot.market_regime.bias.clone(),
        edge_layer: "market".into(),
        bias: bias.into(),
        best_action: best_action.into(),
        preferred_expression,
        market_impulse_score,
        macro_regime_discontinuity,
        expected_net_alpha,
        horizon_ticks: 20,
        alpha_horizon: alpha_horizon_label("intraday", 20),
        summary,
        why_not_single_name,
        focus_sectors: focus_sectors.clone(),
        decisive_factors,
        reference_symbols: market_reference_symbols(snapshot, &focus_sectors),
        average_return_at_decision: snapshot.market_regime.average_return,
        resolution: None,
        execution_policy: governance.execution_policy,
        governance,
        governance_reason_code,
        governance_reason,
    })
}

pub(super) fn build_sector_recommendations(
    snapshot: &AgentSnapshot,
) -> Vec<AgentSectorRecommendation> {
    let mut items = snapshot
        .sector_flows
        .iter()
        .filter_map(|flow| {
            let impulse = flow.average_composite.abs();
            if impulse < Decimal::new(12, 2) {
                return None;
            }
            let bias = match decimal_sign(flow.average_composite) {
                1 => "long",
                -1 => "short",
                _ => "neutral",
            };
            if bias == "neutral" {
                return None;
            }
            let best_action = if impulse >= Decimal::new(22, 2)
                && flow.exceptions.len() <= 1
                && !flow.leaders.is_empty()
            {
                "follow"
            } else {
                "wait"
            };
            let expected_net_alpha =
                (best_action == "follow").then_some((impulse * Decimal::new(3, 2)).round_dp(4));
            let governance =
                ActionGovernanceContract::for_recommendation(ActionExecutionPolicy::ReviewRequired);
            let governance_reason = governance_reason_for_signal_action(
                best_action,
                "high",
                None,
                expected_net_alpha,
                governance.execution_policy,
            );
            let governance_reason_code = governance_reason_code_for_signal_action(
                best_action,
                "high",
                None,
                expected_net_alpha,
                governance.execution_policy,
            );
            Some(AgentSectorRecommendation {
                recommendation_id: format!("sector:{}:{}", snapshot.tick, flow.sector),
                tick: snapshot.tick,
                market: snapshot.market,
                sector: flow.sector.clone(),
                regime_bias: snapshot.market_regime.bias.clone(),
                edge_layer: "sector".into(),
                bias: bias.into(),
                best_action: best_action.into(),
                preferred_expression: "sector_basket".into(),
                sector_impulse_score: impulse.round_dp(4),
                expected_net_alpha,
                horizon_ticks: 15,
                alpha_horizon: alpha_horizon_label("intraday", 15),
                summary: format!(
                    "{} sector impulse {:+} with leaders {}",
                    flow.sector,
                    flow.average_composite.round_dp(3),
                    flow.leaders.join(", ")
                ),
                leaders: flow.leaders.clone(),
                decisive_factors: vec![
                    format!(
                        "sector composite={:+} capital_flow={:+}",
                        flow.average_composite.round_dp(3),
                        flow.average_capital_flow.round_dp(3)
                    ),
                    format!("exceptions={}", flow.exceptions.len()),
                ],
                reference_symbols: flow.leaders.clone(),
                average_return_at_decision: flow.average_composite,
                resolution: None,
                execution_policy: governance.execution_policy,
                governance,
                governance_reason_code,
                governance_reason,
            })
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        b.sector_impulse_score
            .cmp(&a.sector_impulse_score)
            .then_with(|| a.sector.cmp(&b.sector))
    });
    items.truncate(3);
    items
}

fn market_reference_symbols(snapshot: &AgentSnapshot, focus_sectors: &[String]) -> Vec<String> {
    let mut symbols = Vec::new();
    for sector in focus_sectors {
        if let Some(flow) = snapshot
            .sector_flows
            .iter()
            .find(|flow| &flow.sector == sector)
        {
            for leader in &flow.leaders {
                push_unique(&mut symbols, leader.clone());
            }
        }
    }
    if symbols.is_empty() {
        for symbol in snapshot.wake.focus_symbols.iter().take(4) {
            push_unique(&mut symbols, symbol.clone());
        }
    }
    symbols
}

fn sector_alignment_ratio(snapshot: &AgentSnapshot, average_return: Decimal) -> Decimal {
    if snapshot.sector_flows.is_empty() {
        return Decimal::ZERO;
    }
    let target_sign = decimal_sign(average_return);
    if target_sign == 0 {
        return Decimal::ZERO;
    }
    let top = snapshot.sector_flows.iter().take(6).collect::<Vec<_>>();
    if top.is_empty() {
        return Decimal::ZERO;
    }
    let aligned = top
        .iter()
        .filter(|flow| decimal_sign(flow.average_composite) == target_sign)
        .count();
    Decimal::from(aligned as i64) / Decimal::from(top.len() as i64)
}

fn preferred_market_expression(
    snapshot: &AgentSnapshot,
    bias: &str,
    market_impulse_score: Decimal,
    aligned_sector_ratio: Decimal,
    focus_sectors: &[String],
) -> String {
    if bias == "neutral" {
        return "no_trade".into();
    }
    if market_impulse_score >= Decimal::new(75, 2)
        && snapshot.stress.sector_synchrony.unwrap_or(Decimal::ZERO) >= Decimal::new(85, 2)
    {
        return "index".into();
    }
    if aligned_sector_ratio >= Decimal::new(70, 2) && !focus_sectors.is_empty() {
        return "sector_basket".into();
    }
    "leaders_basket".into()
}

fn market_why_not_single_name(
    snapshot: &AgentSnapshot,
    items: &[AgentRecommendation],
    best_action: &str,
) -> Option<String> {
    let single_name_wait_ratio = if items.is_empty() {
        Decimal::ONE
    } else {
        let wait_count = items
            .iter()
            .filter(|item| item.best_action == "wait")
            .count();
        Decimal::from(wait_count as i64) / Decimal::from(items.len() as i64)
    };
    if best_action == "follow" && single_name_wait_ratio >= Decimal::new(75, 2) {
        Some("index lift dominates idiosyncratic edge; keep single names selective".into())
    } else if best_action == "wait" && single_name_wait_ratio >= Decimal::new(75, 2) {
        Some("market tape is active, but single-name dispersion is still too noisy".into())
    } else if snapshot.sector_flows.is_empty() {
        Some("broad tape signal is present, but sector confirmation is still thin".into())
    } else {
        None
    }
}

fn market_decisive_factors(
    snapshot: &AgentSnapshot,
    bias: &str,
    market_impulse_score: Decimal,
    macro_regime_discontinuity: Decimal,
    aligned_sector_ratio: Decimal,
) -> Vec<String> {
    let mut factors = vec![
        format!(
            "breadth up={:.0}% down={:.0}% avg_return={:+.2}%",
            snapshot.market_regime.breadth_up * Decimal::from(100),
            snapshot.market_regime.breadth_down * Decimal::from(100),
            (snapshot.market_regime.average_return * Decimal::from(100)).round_dp(2)
        ),
        format!(
            "market_impulse={:.0}% macro_discontinuity={:.0}% sector_alignment={:.0}%",
            (market_impulse_score * Decimal::from(100)).round_dp(0),
            (macro_regime_discontinuity * Decimal::from(100)).round_dp(0),
            (aligned_sector_ratio * Decimal::from(100)).round_dp(0)
        ),
    ];
    if let Some(sync) = snapshot.stress.sector_synchrony {
        factors.push(format!(
            "sector_synchrony={:.0}% pressure_consensus={:.0}%",
            (sync * Decimal::from(100)).round_dp(0),
            (snapshot.stress.pressure_consensus.unwrap_or(Decimal::ZERO) * Decimal::from(100))
                .round_dp(0)
        ));
    }
    if bias != "neutral" {
        factors.push(format!(
            "dominant edge layer is market, not single-name ({bias})"
        ));
    }
    factors
}
