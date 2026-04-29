use super::*;

#[derive(Clone)]
pub(super) struct RecommendationDecisionModel {
    pub(super) historical_expectancies: AgentActionExpectancies,
    pub(super) final_expectancies: AgentActionExpectancies,
    pub(super) live_expectancy_shift: Decimal,
    pub(super) decisive_factors: Vec<String>,
}

struct ExpectancyContribution {
    label: &'static str,
    delta: Decimal,
}

pub(super) fn recommendation_best_action(action_expectancies: &AgentActionExpectancies) -> String {
    let wait = action_expectancies.wait_expectancy.unwrap_or(Decimal::ZERO);
    let mut candidates = vec![("wait", wait)];
    if let Some(value) = action_expectancies.follow_expectancy {
        candidates.push(("follow", value));
    }
    if let Some(value) = action_expectancies.fade_expectancy {
        candidates.push(("fade", value));
    }
    candidates.sort_by(|a, b| {
        b.1.cmp(&a.1).then_with(|| {
            if a.0 == "wait" {
                std::cmp::Ordering::Less
            } else if b.0 == "wait" {
                std::cmp::Ordering::Greater
            } else {
                a.0.cmp(b.0)
            }
        })
    });

    let (best_action, best_value) = candidates[0];
    let runner_up_value = candidates
        .get(1)
        .map(|item| item.1)
        .unwrap_or(Decimal::ZERO);
    if best_action == "wait"
        || best_value <= wait
        || best_value < Decimal::new(2, 4)
        || best_value - runner_up_value <= Decimal::new(1, 4)
    {
        return "wait".into();
    }

    best_action.into()
}

pub(super) fn expectancy_for_action(
    action_expectancies: &AgentActionExpectancies,
    action: &str,
) -> Option<Decimal> {
    let expectancy = match action {
        "follow" => action_expectancies.follow_expectancy,
        "fade" => action_expectancies.fade_expectancy,
        "wait" => None,
        _ => None,
    }?;
    (expectancy > Decimal::ZERO).then_some(expectancy)
}

pub(super) fn recommendation_decision_model(
    snapshot: &AgentSnapshot,
    state: &AgentSymbolState,
    bias: &str,
    invalidated: bool,
    status: Option<&str>,
    confidence: Decimal,
    enough_confirmation: bool,
    depth_confirms: bool,
    broker_confirms: bool,
    current_transition: Option<&AgentTransition>,
    actionable_notice: Option<&AgentNotice>,
    state_transition: Option<&str>,
) -> RecommendationDecisionModel {
    let historical_expectancies = state
        .structure
        .as_ref()
        .map(|item| item.action_expectancies.clone())
        .unwrap_or_else(|| AgentActionExpectancies {
            wait_expectancy: Some(Decimal::ZERO),
            ..AgentActionExpectancies::default()
        });
    let shift_components = live_expectancy_components(
        snapshot,
        bias,
        invalidated,
        status,
        confidence,
        enough_confirmation,
        depth_confirms,
        broker_confirms,
        current_transition,
        actionable_notice,
        state_transition,
    );
    let shift = shift_components
        .iter()
        .fold(Decimal::ZERO, |acc, item| acc + item.delta)
        .round_dp(4);
    let final_expectancies = apply_live_shift(&historical_expectancies, shift);
    let decisive_factors = decision_factors(&historical_expectancies, shift, &shift_components);

    RecommendationDecisionModel {
        historical_expectancies,
        final_expectancies,
        live_expectancy_shift: shift,
        decisive_factors,
    }
}

fn apply_live_shift(
    historical_expectancies: &AgentActionExpectancies,
    shift: Decimal,
) -> AgentActionExpectancies {
    let mut expectancies = historical_expectancies.clone();
    if expectancies.wait_expectancy.is_none() {
        expectancies.wait_expectancy = Some(Decimal::ZERO);
    }
    if let Some(value) = expectancies.follow_expectancy.as_mut() {
        *value = (*value + shift).round_dp(4);
    }
    if let Some(value) = expectancies.fade_expectancy.as_mut() {
        *value = (*value - shift).round_dp(4);
    }
    expectancies
}

fn live_expectancy_components(
    snapshot: &AgentSnapshot,
    bias: &str,
    invalidated: bool,
    status: Option<&str>,
    confidence: Decimal,
    enough_confirmation: bool,
    depth_confirms: bool,
    broker_confirms: bool,
    current_transition: Option<&AgentTransition>,
    actionable_notice: Option<&AgentNotice>,
    state_transition: Option<&str>,
) -> Vec<ExpectancyContribution> {
    if bias == "neutral" {
        return vec![];
    }

    let mut components = Vec::new();
    if matches!(status, Some("strengthening")) {
        components.push(ExpectancyContribution {
            label: "status strengthening",
            delta: Decimal::new(6, 4),
        });
    }
    if matches!(status, Some("weakening")) {
        components.push(ExpectancyContribution {
            label: "status weakening",
            delta: Decimal::new(-6, 4),
        });
    }
    if invalidated {
        components.push(ExpectancyContribution {
            label: "structure invalidated",
            delta: Decimal::new(-10, 4),
        });
    }
    if enough_confirmation {
        components.push(ExpectancyContribution {
            label: "confirmation stack aligned",
            delta: Decimal::new(5, 4),
        });
    } else {
        components.push(ExpectancyContribution {
            label: "confirmation still thin",
            delta: Decimal::new(-3, 4),
        });
    }
    if depth_confirms {
        components.push(ExpectancyContribution {
            label: "depth confirms bias",
            delta: Decimal::new(2, 4),
        });
    }
    if broker_confirms {
        components.push(ExpectancyContribution {
            label: "broker flow confirms bias",
            delta: Decimal::new(2, 4),
        });
    }
    if confidence >= Decimal::new(8, 2) {
        components.push(ExpectancyContribution {
            label: "confidence already high",
            delta: Decimal::new(2, 4),
        });
    } else if confidence < Decimal::new(4, 2) {
        components.push(ExpectancyContribution {
            label: "confidence still low",
            delta: Decimal::new(-2, 4),
        });
    }
    if bias_regime_conflict(snapshot, bias) {
        components.push(ExpectancyContribution {
            label: "bias conflicts with regime",
            delta: Decimal::new(-7, 4),
        });
    }
    if breadth_extreme_against_bias(snapshot, bias) {
        components.push(ExpectancyContribution {
            label: "breadth extreme against bias",
            delta: Decimal::new(-5, 4),
        });
    }
    if let Some(transition) = current_transition {
        if transition_looks_constructive(transition.summary.as_str()) {
            components.push(ExpectancyContribution {
                label: "transition looks constructive",
                delta: Decimal::new(4, 4),
            });
        }
        if transition_looks_fragile(transition.summary.as_str()) {
            components.push(ExpectancyContribution {
                label: "transition looks fragile",
                delta: Decimal::new(-4, 4),
            });
        }
    } else if let Some(transition) = state_transition {
        if transition_looks_constructive(transition) {
            components.push(ExpectancyContribution {
                label: "state transition constructive",
                delta: Decimal::new(3, 4),
            });
        }
        if transition_looks_fragile(transition) {
            components.push(ExpectancyContribution {
                label: "state transition fragile",
                delta: Decimal::new(-3, 4),
            });
        }
    }
    if let Some(notice) = actionable_notice {
        match notice.kind.as_str() {
            "invalidation" => components.push(ExpectancyContribution {
                label: "fresh invalidation notice",
                delta: Decimal::new(-6, 4),
            }),
            "cross_market_signal" => components.push(ExpectancyContribution {
                label: "cross-market notice confirms move",
                delta: Decimal::new(1, 4),
            }),
            "transition" => components.push(ExpectancyContribution {
                label: "fresh transition keeps setup live",
                delta: Decimal::new(1, 4),
            }),
            _ => {}
        }
    }

    components
}

fn decision_factors(
    historical_expectancies: &AgentActionExpectancies,
    live_expectancy_shift: Decimal,
    shift_components: &[ExpectancyContribution],
) -> Vec<String> {
    let mut factors = Vec::new();
    if let (Some(follow), Some(fade)) = (
        historical_expectancies.follow_expectancy,
        historical_expectancies.fade_expectancy,
    ) {
        factors.push(format!(
            "historical prior follow={:+.2}% fade={:+.2}% wait={:+.2}%",
            expectancy_pct(follow),
            expectancy_pct(fade),
            expectancy_pct(
                historical_expectancies
                    .wait_expectancy
                    .unwrap_or(Decimal::ZERO)
            ),
        ));
    }
    if live_expectancy_shift != Decimal::ZERO {
        factors.push(format!(
            "live shift {:+.2}% on follow, {:+.2}% on fade",
            expectancy_pct(live_expectancy_shift),
            expectancy_pct(-live_expectancy_shift),
        ));
    }

    let mut sorted = shift_components.iter().collect::<Vec<_>>();
    sorted.sort_by(|a, b| {
        b.delta
            .abs()
            .cmp(&a.delta.abs())
            .then_with(|| a.label.cmp(b.label))
    });
    for item in sorted
        .into_iter()
        .filter(|item| item.delta != Decimal::ZERO)
        .take(3)
    {
        factors.push(format!(
            "{:+.2}% {}",
            expectancy_pct(item.delta),
            item.label,
        ));
    }
    factors
}

fn expectancy_pct(value: Decimal) -> Decimal {
    (value * Decimal::from(100)).round_dp(2)
}

fn bias_regime_conflict(snapshot: &AgentSnapshot, bias: &str) -> bool {
    (bias == "long" && snapshot.market_regime.bias.eq_ignore_ascii_case("risk_off"))
        || (bias == "short" && snapshot.market_regime.bias.eq_ignore_ascii_case("risk_on"))
}

fn breadth_extreme_against_bias(snapshot: &AgentSnapshot, bias: &str) -> bool {
    (bias == "long" && snapshot.market_regime.breadth_down >= Decimal::new(90, 2))
        || (bias == "short" && snapshot.market_regime.breadth_up >= Decimal::new(90, 2))
}

fn transition_looks_constructive(summary: &str) -> bool {
    let normalized = summary.to_ascii_lowercase();
    normalized.contains("strengthening")
        || normalized.contains("entered the active structure set")
        || normalized.contains("follow-through")
        || normalized.contains("confirms")
}

fn transition_looks_fragile(summary: &str) -> bool {
    let normalized = summary.to_ascii_lowercase();
    normalized.contains("contested")
        || normalized.contains("weakening")
        || normalized.contains("left the active structure set")
        || normalized.contains("invalidated")
        || normalized.contains("loses")
        || normalized.contains("stops")
}
