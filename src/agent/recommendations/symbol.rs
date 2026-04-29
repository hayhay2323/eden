use super::decision_model::{
    expectancy_for_action, recommendation_best_action, recommendation_decision_model,
};
use super::*;
use crate::agent::governance_decision_for_signal_action;
use crate::ontology::{action_direction_from_title_prefix, action_direction_position_label};

pub(super) fn build_symbol_recommendation(
    snapshot: &AgentSnapshot,
    state: &AgentSymbolState,
) -> Option<AgentRecommendation> {
    let bias = agent_bias_for_symbol(state).unwrap_or("neutral");
    let status = symbol_status(state).map(str::to_string);
    let confidence = symbol_priority(state).unwrap_or(Decimal::ZERO);
    let current_transition = current_transition_for_symbol(snapshot, &state.symbol);
    let current_notice = current_notice_for_symbol(snapshot, &state.symbol);
    let actionable_notice = current_notice.filter(|notice| {
        matches!(
            notice.kind.as_str(),
            "transition" | "invalidation" | "cross_market_signal"
        )
    });
    let structure_action = state.structure.as_ref().map(|item| item.action.as_str());
    let invalidated = state
        .invalidation
        .as_ref()
        .map(|item| item.invalidated)
        .unwrap_or(false);
    let streak = state
        .structure
        .as_ref()
        .and_then(|item| item.status_streak)
        .unwrap_or(0);
    let has_active_position = state.active_position.is_some();
    let depth_confirms = depth_confirms_bias(state, bias);
    let broker_confirms = broker_confirms_bias(state, bias);
    let risk_off = snapshot.market_regime.bias.eq_ignore_ascii_case("risk_off");
    let risk_on = snapshot.market_regime.bias.eq_ignore_ascii_case("risk_on");
    let breadth_down_extreme = snapshot.market_regime.breadth_down >= Decimal::new(90, 2);
    let breadth_up_extreme = snapshot.market_regime.breadth_up >= Decimal::new(90, 2);
    let confidence_high = confidence >= Decimal::new(8, 2);
    let confidence_medium = confidence >= Decimal::new(4, 2);
    let confirmation_count = usize::from(current_transition.is_some())
        + usize::from(depth_confirms)
        + usize::from(broker_confirms);
    let enough_confirmation = confirmation_count >= 2;
    let thesis_family = state
        .structure
        .as_ref()
        .and_then(|item| item.thesis_family.clone());
    let matched_success_pattern_signature = super::shared::matched_success_pattern_signature(state);
    let state_transition = current_transition
        .map(|item| item.summary.clone())
        .or_else(|| {
            state.structure.as_ref().and_then(|item| {
                item.transition_reason
                    .clone()
                    .or_else(|| item.leader_transition_summary.clone())
            })
        });
    let mut invalidation_rule = state
        .structure
        .as_ref()
        .and_then(|item| item.invalidation_rule.clone())
        .or_else(|| {
            state
                .invalidation
                .as_ref()
                .and_then(|item| item.rules.first().cloned())
        });
    let gate_reason = super::shared::multi_horizon_gate_reason(state);
    let policy_primary = super::shared::policy_primary(state);
    let policy_reason = super::shared::policy_reason(state);
    let review_reason_code = super::shared::review_reason_code(state);

    if matches!(structure_action, Some("observe"))
        && current_transition.is_none()
        && actionable_notice.is_none()
        && !invalidated
        && !has_active_position
    {
        return None;
    }

    if bias == "neutral"
        && current_transition.is_none()
        && actionable_notice.is_none()
        && confidence < Decimal::new(2, 2)
    {
        return None;
    }

    let (action, severity) = if invalidated {
        (
            if has_active_position {
                "hedge"
            } else {
                "review"
            },
            "critical",
        )
    } else if bias == "long" && risk_off {
        if has_active_position
            && (matches!(status.as_deref(), Some("weakening")) || breadth_down_extreme)
        {
            ("trim", "high")
        } else if matches!(status.as_deref(), Some("strengthening"))
            && enough_confirmation
            && streak >= 2
            && confidence_high
        {
            ("review", "high")
        } else if current_transition.is_some() || actionable_notice.is_some() || confidence_medium {
            ("review", "high")
        } else {
            ("watch", "normal")
        }
    } else if bias == "short" && risk_on {
        if has_active_position
            && (matches!(status.as_deref(), Some("weakening")) || breadth_up_extreme)
        {
            ("trim", "high")
        } else if matches!(status.as_deref(), Some("strengthening"))
            && enough_confirmation
            && streak >= 2
            && confidence_high
        {
            ("review", "high")
        } else if current_transition.is_some() || actionable_notice.is_some() || confidence_medium {
            ("review", "high")
        } else {
            ("watch", "normal")
        }
    } else if matches!(status.as_deref(), Some("strengthening"))
        && enough_confirmation
        && streak >= 2
        && confidence_high
    {
        (if has_active_position { "add" } else { "enter" }, "high")
    } else if matches!(status.as_deref(), Some("weakening")) {
        (if has_active_position { "trim" } else { "watch" }, "high")
    } else if current_transition.is_some() || actionable_notice.is_some() || confidence_medium {
        ("review", "high")
    } else if confidence > Decimal::ZERO {
        ("watch", "normal")
    } else {
        ("ignore", "normal")
    };

    let action_label = recommendation_action_label(
        action,
        bias,
        snapshot,
        state,
        breadth_down_extreme,
        breadth_up_extreme,
    );

    let mut why_parts = Vec::new();
    if let Some(transition) = current_transition {
        why_parts.push(transition.summary.clone());
    } else if let Some(notice) = actionable_notice {
        why_parts.push(notice.summary.clone());
    } else if let Some(structure) = &state.structure {
        why_parts.push(format!(
            "{} {} conf={:+}",
            structure.title,
            structure.action,
            structure.confidence.round_dp(3)
        ));
    }
    why_parts.push(format!(
        "regime={} breadth_up={:.0}% breadth_down={:.0}%",
        snapshot.market_regime.bias,
        (snapshot.market_regime.breadth_up * Decimal::from(100)).round_dp(0),
        (snapshot.market_regime.breadth_down * Decimal::from(100)).round_dp(0)
    ));
    if depth_confirms {
        why_parts.push("depth confirms".into());
    }
    if broker_confirms {
        why_parts.push("broker flow confirms".into());
    }
    if let Some(primary) = &policy_primary {
        why_parts.push(format!("policy={primary}"));
    }
    if let Some(reason) = &gate_reason {
        why_parts.push(format!("gate={reason}"));
    }
    if let Some(signature) = &matched_success_pattern_signature {
        why_parts.push(format!("pattern={signature}"));
    }

    let mut watch_next = Vec::new();
    if matches!(status.as_deref(), Some("strengthening")) && streak < 2 {
        watch_next.push("等 status_streak 連續到 2 以上".into());
    }
    if !depth_confirms {
        watch_next.push("等深度站到同向".into());
    }
    if !broker_confirms {
        watch_next.push("等 broker 轉向更明確".into());
    }
    if current_transition.is_some() {
        watch_next.push("看下一個 tick 是否有 follow-through".into());
    }
    if let Some(reason) = &gate_reason {
        watch_next.insert(0, format!("等 multi-horizon gate 解鎖: {reason}"));
    }
    dedupe_strings(&mut watch_next);
    watch_next.truncate(3);

    let mut do_not = Vec::new();
    if risk_off && bias == "long" {
        do_not.push("不准在 risk_off tape 追多".into());
    }
    if risk_on && bias == "short" {
        do_not.push("不要在 risk_on tape 追空".into());
    }
    if breadth_down_extreme && bias == "long" {
        do_not.push("breadth_down 超過 90% 時不准追價".into());
    }
    if breadth_up_extreme && bias == "short" {
        do_not.push("breadth_up 過熱時不要加空".into());
    }
    if invalidated {
        do_not.push("不要把失效結構當成新進場".into());
    }
    if let Some(reason) = &policy_reason {
        do_not.insert(0, reason.clone());
    }
    dedupe_strings(&mut do_not);
    do_not.truncate(3);

    let mut fragility = state
        .invalidation
        .as_ref()
        .map(|item| item.rules.iter().take(2).cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    if !enough_confirmation {
        fragility.push("confirmation 不足".into());
    }
    if confidence < Decimal::new(4, 2) {
        fragility.push("confidence 偏低".into());
    }
    if let Some(code) = &review_reason_code {
        fragility.insert(0, format!("review_reason_code={code}"));
    }
    if let Some(reason) = &policy_reason {
        fragility.insert(0, format!("policy_gate: {reason}"));
    }
    if let Some(reason) = &gate_reason {
        fragility.insert(0, format!("multi_horizon_gate: {reason}"));
    }
    dedupe_strings(&mut fragility);
    fragility.truncate(3);

    let mut score = confidence;
    if current_transition.is_some() {
        score += Decimal::new(2, 2);
    }
    if enough_confirmation {
        score += Decimal::new(2, 2);
    }
    if matches!(action, "enter" | "add") {
        score += Decimal::new(3, 2);
    }
    if matches!(severity, "critical") {
        score += Decimal::new(4, 2);
    }
    if matches!(action, "watch" | "ignore") {
        score -= Decimal::new(1, 2);
    }
    if score < Decimal::ZERO {
        score = Decimal::ZERO;
    }

    let decision_model = recommendation_decision_model(
        snapshot,
        state,
        bias,
        invalidated,
        status.as_deref(),
        confidence,
        enough_confirmation,
        depth_confirms,
        broker_confirms,
        current_transition,
        actionable_notice,
        state_transition.as_deref(),
    );
    let best_action = recommendation_best_action(&decision_model.final_expectancies);
    let expected_net_alpha =
        expectancy_for_action(&decision_model.final_expectancies, &best_action);
    let alpha_horizon = state
        .structure
        .as_ref()
        .and_then(|item| item.alpha_horizon.clone())
        .unwrap_or_else(|| alpha_horizon_label("intraday", recommendation_horizon_ticks(action)));

    let governance_decision = governance_decision_for_signal_action(
        &best_action,
        severity,
        invalidation_rule.as_deref(),
        expected_net_alpha,
    );
    let governance = governance_decision.governance;
    let governance_reason = governance_decision.governance_reason;
    let governance_reason_code = governance_decision.governance_reason_code;

    let lens_context = LensContext {
        snapshot,
        symbol: state,
        current_transition,
        current_notice: actionable_notice.or(current_notice),
        backward: snapshot.backward_investigation(&state.symbol),
        bias,
        confidence,
        best_action: &best_action,
        severity,
        expected_net_alpha,
    };
    let lens_bundle = default_lens_engine().observe(&lens_context);
    let why_components = lens_why_components(&lens_bundle);
    let invalidation_components = lens_invalidation_components(&lens_bundle);
    let (primary_lens, supporting_lenses) = lens_hierarchy(&lens_bundle);
    let review_lens = (governance.execution_policy
        == crate::action::workflow::ActionExecutionPolicy::ReviewRequired)
        .then(|| primary_lens.clone())
        .flatten();
    if !lens_bundle.why_fragments.is_empty() {
        let mut combined_why_parts = lens_bundle.why_fragments.clone();
        combined_why_parts.extend(why_parts);
        dedupe_strings(&mut combined_why_parts);
        why_parts = combined_why_parts;
    }
    if let Some(lens_invalidation_rule) = lens_bundle.invalidation_fragments.first().cloned() {
        invalidation_rule = Some(lens_invalidation_rule);
    }

    Some(AgentRecommendation {
        recommendation_id: format!("rec:{}:{}:{}", snapshot.tick, state.symbol, action),
        tick: snapshot.tick,
        symbol: state.symbol.clone(),
        sector: state.sector.clone(),
        title: state
            .structure
            .as_ref()
            .map(|structure| structure.title.clone()),
        action: action.into(),
        action_label,
        bias: bias.into(),
        severity: severity.into(),
        confidence,
        score,
        horizon_ticks: recommendation_horizon_ticks(action),
        regime_bias: snapshot.market_regime.bias.clone(),
        status,
        why: why_parts.join(" | "),
        why_components,
        primary_lens,
        supporting_lenses,
        review_lens,
        watch_next,
        do_not,
        fragility,
        transition: current_transition.map(|item| item.summary.clone()),
        thesis_family,
        matched_success_pattern_signature,
        state_transition,
        best_action,
        action_expectancies: decision_model.final_expectancies.clone(),
        decision_attribution: AgentDecisionAttribution {
            historical_expectancies: decision_model.historical_expectancies,
            live_expectancy_shift: decision_model.live_expectancy_shift,
            decisive_factors: decision_model.decisive_factors,
        },
        expected_net_alpha,
        alpha_horizon,
        price_at_decision: symbol_mark_price(snapshot, &state.symbol),
        resolution: None,
        invalidation_rule,
        invalidation_components,
        execution_policy: governance.execution_policy,
        governance,
        governance_reason_code,
        governance_reason,
    })
}

fn lens_why_components(bundle: &LensBundle) -> Vec<AgentLensComponent> {
    let mut components = bundle
        .observations
        .iter()
        .filter_map(|item| {
            let content = item.why_fragment.trim();
            (!content.is_empty()).then(|| AgentLensComponent {
                lens_name: item.lens_name.into(),
                confidence: item.confidence,
                content: content.into(),
                tags: item.tags.clone(),
            })
        })
        .collect::<Vec<_>>();
    dedupe_lens_components(&mut components);
    components.truncate(4);
    components
}

fn lens_invalidation_components(bundle: &LensBundle) -> Vec<AgentLensComponent> {
    let mut components = bundle
        .observations
        .iter()
        .flat_map(|item| {
            item.invalidation_fragments
                .iter()
                .filter_map(|fragment| {
                    let content = fragment.trim();
                    (!content.is_empty()).then(|| AgentLensComponent {
                        lens_name: item.lens_name.into(),
                        confidence: item.confidence,
                        content: content.into(),
                        tags: item.tags.clone(),
                    })
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    dedupe_lens_components(&mut components);
    components.truncate(4);
    components
}

fn dedupe_lens_components(components: &mut Vec<AgentLensComponent>) {
    let mut seen = HashSet::new();
    components.retain(|item| seen.insert(item.content.clone()));
}

fn lens_hierarchy(bundle: &LensBundle) -> (Option<String>, Vec<String>) {
    let mut seen = HashSet::new();
    let mut lenses = bundle
        .observations
        .iter()
        .filter_map(|item| {
            let lens_name = item.lens_name.trim();
            (!lens_name.is_empty() && seen.insert(lens_name.to_string()))
                .then(|| lens_name.to_string())
        })
        .collect::<Vec<_>>();
    let primary = lenses.first().cloned();
    if !lenses.is_empty() {
        lenses.remove(0);
    }
    lenses.truncate(3);
    (primary, lenses)
}

pub(crate) fn agent_bias_for_symbol(state: &AgentSymbolState) -> Option<&'static str> {
    if let Some(structure) = &state.structure {
        if let Some(direction) = action_direction_from_title_prefix(&structure.title) {
            return Some(action_direction_position_label(direction));
        }
    }

    state
        .signal
        .as_ref()
        .and_then(|signal| match decimal_sign(signal.composite) {
            1 => Some("long"),
            -1 => Some("short"),
            _ => None,
        })
}

pub(crate) fn symbol_status(state: &AgentSymbolState) -> Option<&str> {
    state
        .invalidation
        .as_ref()
        .map(|item| item.status.as_str())
        .or_else(|| {
            state
                .structure
                .as_ref()
                .and_then(|item| item.status.as_deref())
        })
}

fn current_transition_for_symbol<'a>(
    snapshot: &'a AgentSnapshot,
    symbol: &str,
) -> Option<&'a AgentTransition> {
    snapshot
        .recent_transitions
        .iter()
        .find(|item| item.to_tick == snapshot.tick && item.symbol.eq_ignore_ascii_case(symbol))
}

fn current_notice_for_symbol<'a>(
    snapshot: &'a AgentSnapshot,
    symbol: &str,
) -> Option<&'a AgentNotice> {
    snapshot
        .notices
        .iter()
        .find(|item| item.tick == snapshot.tick && item.symbol.as_deref() == Some(symbol))
}

pub(crate) fn decision_alert_record(
    snapshot: &AgentSnapshot,
    decision: &AgentDecision,
    existing: &[AgentAlertRecord],
) -> Option<AgentAlertRecord> {
    match decision {
        AgentDecision::Symbol(recommendation) => {
            let fresh_transition = current_transition_for_symbol(snapshot, &recommendation.symbol);
            let fresh_notice = current_notice_for_symbol(snapshot, &recommendation.symbol);
            let should_record = recommendation.action != "ignore"
                && (fresh_transition.is_some()
                    || fresh_notice.is_some()
                    || matches!(recommendation.severity.as_str(), "high" | "critical"));
            if !should_record {
                return None;
            }
            let kind = fresh_transition
                .map(|_| "transition".to_string())
                .or_else(|| fresh_notice.map(|item| item.kind.clone()))
                .unwrap_or_else(|| "recommendation".into());
            let alert_id = format!(
                "alert:{}:symbol:{}:{}:{}",
                snapshot.tick, recommendation.symbol, kind, recommendation.best_action
            );
            if existing.iter().any(|item| item.alert_id == alert_id) {
                return None;
            }
            Some(AgentAlertRecord {
                alert_id,
                tick: snapshot.tick,
                scope_kind: "symbol".into(),
                symbol: Some(recommendation.symbol.clone()),
                sector: recommendation.sector.clone(),
                kind,
                severity: recommendation.severity.clone(),
                why: recommendation.why.clone(),
                suggested_action: recommendation.best_action.clone(),
                action_label: recommendation.action_label.clone(),
                horizon_ticks: recommendation.horizon_ticks,
                regime_bias: recommendation.regime_bias.clone(),
                price_at_alert: symbol_mark_price(snapshot, &recommendation.symbol),
                reference_value_at_alert: recommendation.price_at_decision,
                reference_symbols: vec![recommendation.symbol.clone()],
                action_bias: recommendation.bias.clone(),
                recommendation_id: Some(recommendation.recommendation_id.clone()),
                resolution: None,
                outcome_after_n_ticks: None,
            })
        }
        AgentDecision::Market(recommendation) => {
            if recommendation.best_action == "wait" {
                return None;
            }
            let alert_id = format!(
                "alert:{}:market:{}:{}",
                snapshot.tick, recommendation.preferred_expression, recommendation.best_action
            );
            if existing.iter().any(|item| item.alert_id == alert_id) {
                return None;
            }
            Some(AgentAlertRecord {
                alert_id,
                tick: snapshot.tick,
                scope_kind: "market".into(),
                symbol: None,
                sector: None,
                kind: "market_recommendation".into(),
                severity: "high".into(),
                why: recommendation.summary.clone(),
                suggested_action: recommendation.best_action.clone(),
                action_label: Some(recommendation.preferred_expression.clone()),
                horizon_ticks: recommendation.horizon_ticks,
                regime_bias: recommendation.regime_bias.clone(),
                price_at_alert: None,
                reference_value_at_alert: Some(recommendation.average_return_at_decision),
                reference_symbols: recommendation.reference_symbols.clone(),
                action_bias: recommendation.bias.clone(),
                recommendation_id: Some(recommendation.recommendation_id.clone()),
                resolution: None,
                outcome_after_n_ticks: None,
            })
        }
        AgentDecision::Sector(recommendation) => {
            if recommendation.best_action == "wait" {
                return None;
            }
            let alert_id = format!(
                "alert:{}:sector:{}:{}",
                snapshot.tick, recommendation.sector, recommendation.best_action
            );
            if existing.iter().any(|item| item.alert_id == alert_id) {
                return None;
            }
            Some(AgentAlertRecord {
                alert_id,
                tick: snapshot.tick,
                scope_kind: "sector".into(),
                symbol: None,
                sector: Some(recommendation.sector.clone()),
                kind: "sector_recommendation".into(),
                severity: "high".into(),
                why: recommendation.summary.clone(),
                suggested_action: recommendation.best_action.clone(),
                action_label: Some(recommendation.preferred_expression.clone()),
                horizon_ticks: recommendation.horizon_ticks,
                regime_bias: recommendation.regime_bias.clone(),
                price_at_alert: None,
                reference_value_at_alert: Some(recommendation.average_return_at_decision),
                reference_symbols: recommendation.reference_symbols.clone(),
                action_bias: recommendation.bias.clone(),
                recommendation_id: Some(recommendation.recommendation_id.clone()),
                resolution: None,
                outcome_after_n_ticks: None,
            })
        }
    }
}

fn recommendation_action_label(
    action: &str,
    bias: &str,
    snapshot: &AgentSnapshot,
    state: &AgentSymbolState,
    breadth_down_extreme: bool,
    breadth_up_extreme: bool,
) -> Option<String> {
    if action == "review"
        && bias == "short"
        && snapshot.market_regime.bias.eq_ignore_ascii_case("risk_off")
    {
        return Some("review_short".into());
    }
    if action == "review"
        && bias == "long"
        && snapshot.market_regime.bias.eq_ignore_ascii_case("risk_on")
    {
        return Some("review_long".into());
    }
    if action == "trim"
        && bias == "long"
        && (breadth_down_extreme || matches!(symbol_status(state), Some("weakening")))
    {
        return Some("trim_or_hold".into());
    }
    if action == "trim"
        && bias == "short"
        && (breadth_up_extreme || matches!(symbol_status(state), Some("weakening")))
    {
        return Some("cover_or_hold".into());
    }
    None
}

fn recommendation_horizon_ticks(action: &str) -> u64 {
    match action {
        "enter" | "add" => 15,
        "trim" | "hedge" => 12,
        "review" => 10,
        "watch" => 8,
        _ => 6,
    }
}

fn depth_confirms_bias(state: &AgentSymbolState, bias: &str) -> bool {
    let Some(depth) = state.depth.as_ref() else {
        return false;
    };

    match bias {
        "long" => {
            depth.imbalance > Decimal::ZERO
                || depth.bid_top3_ratio > depth.ask_top3_ratio
                || depth.bid_best_ratio > depth.ask_best_ratio
        }
        "short" => {
            depth.imbalance < Decimal::ZERO
                || depth.ask_top3_ratio > depth.bid_top3_ratio
                || depth.ask_best_ratio > depth.bid_best_ratio
        }
        _ => false,
    }
}

fn broker_confirms_bias(state: &AgentSymbolState, bias: &str) -> bool {
    let Some(brokers) = state.brokers.as_ref() else {
        return false;
    };

    let entered_supports = |supports: fn(&AgentBrokerInstitution) -> bool| {
        brokers
            .current
            .iter()
            .any(|institution| brokers.entered.contains(&institution.name) && supports(institution))
    };

    match bias {
        "long" => {
            !brokers.switched_to_bid.is_empty()
                || entered_supports(|institution| !institution.bid_positions.is_empty())
        }
        "short" => {
            !brokers.switched_to_ask.is_empty()
                || entered_supports(|institution| !institution.ask_positions.is_empty())
        }
        _ => false,
    }
}
