use super::*;
use crate::ontology::CausalContestState;

fn investigation_object_rank(object_kind: &str) -> u8 {
    match object_kind {
        "symbol" => 6,
        "workflow" => 5,
        "sector" => 4,
        "market" => 3,
        "institution" => 2,
        "theme" | "region" | "custom" => 1,
        "cross_market_dependency" => 0,
        _ => 0,
    }
}

fn investigation_attention_rank(attention_hint: &str) -> u8 {
    match attention_hint {
        "enter" => 3,
        "review" => 2,
        "observe" => 1,
        _ => 0,
    }
}

fn push_reason(reasons: &mut Vec<String>, value: Option<String>) {
    if let Some(value) = value {
        let trimmed = value.trim();
        if !trimmed.is_empty() && !reasons.iter().any(|item| item == trimmed) {
            reasons.push(trimmed.to_string());
        }
    }
}

fn investigation_object_kind(market: LiveMarket, scope: &ReasoningScope) -> &'static str {
    match scope {
        ReasoningScope::Market(_) => "market",
        ReasoningScope::Symbol(symbol) => {
            if matches!(market, LiveMarket::Us) && symbol.0.ends_with(".HK") {
                "cross_market_dependency"
            } else {
                "symbol"
            }
        }
        ReasoningScope::Sector(_) => "sector",
        ReasoningScope::Institution(_) => "institution",
        ReasoningScope::Theme(_) => "theme",
        ReasoningScope::Region(_) => "region",
        ReasoningScope::Custom(_) => "custom",
    }
}

fn scope_reference_symbols(scope: &ReasoningScope) -> Vec<String> {
    match scope {
        ReasoningScope::Symbol(symbol) => vec![symbol.0.clone()],
        _ => vec![],
    }
}

fn humanize_investigation_note(note: &str) -> Option<String> {
    let trimmed = note.trim();
    if trimmed.is_empty() {
        return None;
    }
    if trimmed.starts_with("structural_bonus=")
        || trimmed.starts_with("propagation_bonus=")
        || trimmed.starts_with("lineage_adjustment=")
        || trimmed.starts_with("opening_bootstrap_penalty=")
    {
        return None;
    }
    if let Some(value) = trimmed.strip_prefix("family=") {
        return Some(format!("family: {value}"));
    }
    if let Some(value) = trimmed.strip_prefix("lineage_prior=") {
        return Some(format!("lineage prior: {value}"));
    }
    if let Some(value) = trimmed.strip_prefix("multi_horizon_gate=") {
        return Some(format!("multi-horizon gate: {value}"));
    }
    if let Some(value) = trimmed.strip_prefix("review_reason_code=") {
        return Some(format!("review reason: {value}"));
    }
    Some(trimmed.to_string())
}

fn investigation_summary(object_id: &str, family_label: &str, attention_hint: &str) -> String {
    match attention_hint {
        "enter" | "review" => {
            format!("{object_id} is queued for review_desk in {family_label}")
        }
        _ => format!("{object_id} is collecting confirmation in {family_label}"),
    }
}

fn build_selection_investigation(
    snapshot: &AgentSnapshot,
    selection: &InvestigationSelection,
) -> AgentInvestigation {
    let object_kind = investigation_object_kind(snapshot.market, &selection.scope).to_string();
    let object_id = selection.scope.label();
    let mut reasons = Vec::new();
    push_reason(&mut reasons, Some(selection.rationale.clone()));
    if let Some(symbol) = scope_symbol(&selection.scope) {
        if let Some(backward) = snapshot.backward_investigation(&symbol.0) {
            push_reason(&mut reasons, backward.leader_transition_summary.clone());
            push_reason(
                &mut reasons,
                backward
                    .leading_cause
                    .as_ref()
                    .map(|item| item.explanation.clone()),
            );
            push_reason(&mut reasons, backward.leading_falsifier.clone());
        }
    }
    for note in &selection.notes {
        push_reason(&mut reasons, humanize_investigation_note(note));
        if reasons.len() >= 4 {
            break;
        }
    }

    AgentInvestigation {
        rank: 0,
        object_kind,
        object_id: object_id.clone(),
        title: selection.title.clone(),
        summary: investigation_summary(
            object_id.as_str(),
            selection.family_label.as_str(),
            selection.attention_hint.as_str(),
        ),
        priority: selection.priority_score,
        attention_hint: selection.attention_hint.clone(),
        // V2 Pass 2: InvestigationSelection.family_key removed. The
        // AgentInvestigation.family_key field still exists for API
        // back-compat; populate from family_label (operator-readable).
        family_key: Some(selection.family_label.clone()),
        family_label: Some(selection.family_label.clone()),
        rationale: Some(selection.rationale.clone()),
        review_reason_code: selection.review_reason_code,
        hypothesis_id: Some(selection.hypothesis_id.clone()),
        runner_up_hypothesis_id: selection.runner_up_hypothesis_id.clone(),
        backward_investigation_id: scope_symbol(&selection.scope)
            .and_then(|symbol| snapshot.backward_investigation(&symbol.0))
            .map(|item| item.investigation_id.clone()),
        recommendation_id: None,
        reasons,
        reference_symbols: scope_reference_symbols(&selection.scope),
    }
}

fn build_backward_only_investigation(
    snapshot: &AgentSnapshot,
    backward: &BackwardInvestigation,
) -> AgentInvestigation {
    let object_kind = investigation_object_kind(snapshot.market, &backward.leaf_scope).to_string();
    let object_id = backward.leaf_scope.label();
    let mut reasons = Vec::new();
    push_reason(
        &mut reasons,
        backward
            .leading_cause
            .as_ref()
            .map(|item| item.explanation.clone()),
    );
    push_reason(&mut reasons, backward.leader_transition_summary.clone());
    push_reason(&mut reasons, backward.leading_falsifier.clone());
    push_reason(
        &mut reasons,
        Some(format!("causal contest: {}", backward.contest_state)),
    );
    let family_label = backward
        .leading_cause
        .as_ref()
        .map(|item| format!("{} causal driver", item.layer))
        .unwrap_or_else(|| "causal driver".into());
    AgentInvestigation {
        rank: 0,
        object_kind,
        object_id: object_id.clone(),
        title: backward.leaf_label.clone(),
        summary: format!(
            "{object_id} is under causal investigation ({})",
            backward.contest_state
        ),
        priority: backward
            .leading_cause
            .as_ref()
            .map(|item| item.competitive_score)
            .or(backward.cause_gap)
            .unwrap_or(Decimal::ZERO),
        attention_hint: if matches!(
            backward.contest_state,
            CausalContestState::Contested | CausalContestState::Flipped
        ) {
            "review".into()
        } else {
            "observe".into()
        },
        family_key: None,
        family_label: Some(family_label),
        rationale: backward
            .leading_cause
            .as_ref()
            .map(|item| item.explanation.clone()),
        review_reason_code: None,
        hypothesis_id: None,
        runner_up_hypothesis_id: None,
        backward_investigation_id: Some(backward.investigation_id.clone()),
        recommendation_id: None,
        reasons,
        reference_symbols: scope_reference_symbols(&backward.leaf_scope),
    }
}

fn build_sector_investigation(recommendation: &AgentSectorRecommendation) -> AgentInvestigation {
    AgentInvestigation {
        rank: 0,
        object_kind: "sector".into(),
        object_id: recommendation.sector.clone(),
        title: format!("{} sector", recommendation.sector),
        summary: format!(
            "{} sector is under investigation for active impulse",
            recommendation.sector
        ),
        priority: recommendation.sector_impulse_score,
        attention_hint: if recommendation.best_action == "wait" {
            "observe".into()
        } else {
            "review".into()
        },
        family_key: None,
        family_label: Some("sector_impulse".into()),
        rationale: Some(recommendation.summary.clone()),
        review_reason_code: None,
        hypothesis_id: None,
        runner_up_hypothesis_id: None,
        backward_investigation_id: None,
        recommendation_id: Some(recommendation.recommendation_id.clone()),
        reasons: recommendation
            .decisive_factors
            .iter()
            .take(4)
            .cloned()
            .collect(),
        reference_symbols: recommendation.reference_symbols.clone(),
    }
}

fn build_market_investigation(recommendation: &AgentMarketRecommendation) -> AgentInvestigation {
    AgentInvestigation {
        rank: 0,
        object_kind: "market".into(),
        object_id: market_scope_symbol(recommendation.market),
        title: format!("{} market", market_scope_symbol(recommendation.market)),
        summary: format!(
            "{} market is under investigation for regime-level impulse",
            market_scope_symbol(recommendation.market)
        ),
        priority: recommendation.market_impulse_score,
        attention_hint: if recommendation.best_action == "wait" {
            "observe".into()
        } else {
            "review".into()
        },
        family_key: None,
        family_label: Some("market_impulse".into()),
        rationale: Some(recommendation.summary.clone()),
        review_reason_code: None,
        hypothesis_id: None,
        runner_up_hypothesis_id: None,
        backward_investigation_id: None,
        recommendation_id: Some(recommendation.recommendation_id.clone()),
        reasons: recommendation
            .decisive_factors
            .iter()
            .take(4)
            .cloned()
            .collect(),
        reference_symbols: recommendation.reference_symbols.clone(),
    }
}

fn build_notice_investigation(
    market: LiveMarket,
    notice: &AgentNotice,
) -> Option<AgentInvestigation> {
    let object_kind = if let Some(symbol) = &notice.symbol {
        if matches!(market, LiveMarket::Us) && symbol.ends_with(".HK") {
            "cross_market_dependency"
        } else {
            "symbol"
        }
    } else if notice.sector.is_some() {
        "sector"
    } else {
        "market"
    };
    let object_id = notice
        .symbol
        .clone()
        .or_else(|| notice.sector.clone())
        .unwrap_or_else(|| "market".into());
    let attention_hint = match notice.kind.as_str() {
        "policy_gate" | "lineage_gate" | "transition" | "invalidation" => "review",
        "cross_market_signal"
        | "market_event"
        | "sector_divergence"
        | "depth_shift"
        | "broker_movement" => "observe",
        _ => return None,
    };
    Some(AgentInvestigation {
        rank: 0,
        object_kind: object_kind.into(),
        object_id,
        title: notice.title.clone(),
        summary: notice.summary.clone(),
        priority: notice.significance,
        attention_hint: attention_hint.into(),
        family_key: None,
        family_label: None,
        rationale: Some(notice.summary.clone()),
        review_reason_code: None,
        hypothesis_id: None,
        runner_up_hypothesis_id: None,
        backward_investigation_id: None,
        recommendation_id: None,
        reasons: vec![notice.summary.clone()],
        reference_symbols: notice.symbol.clone().into_iter().collect(),
    })
}

pub fn build_investigations(
    snapshot: &AgentSnapshot,
    session: Option<&AgentSession>,
    recommendations: Option<&AgentRecommendations>,
    limit: usize,
) -> AgentInvestigations {
    let recommendations = recommendations
        .cloned()
        .unwrap_or_else(|| build_recommendations(snapshot, session));
    let mut by_object = std::collections::HashMap::<String, AgentInvestigation>::new();

    for selection in &snapshot.investigation_selections {
        let investigation = build_selection_investigation(snapshot, selection);
        let key = format!("{}:{}", investigation.object_kind, investigation.object_id);
        by_object.insert(key, investigation);
    }

    if let Some(backward_reasoning) = &snapshot.backward_reasoning {
        for backward in &backward_reasoning.investigations {
            let candidate = build_backward_only_investigation(snapshot, backward);
            let key = format!("{}:{}", candidate.object_kind, candidate.object_id);
            by_object.entry(key).or_insert(candidate);
        }
    }

    for decision in &recommendations.decisions {
        let investigation = match decision {
            AgentDecision::Market(item) => build_market_investigation(item),
            AgentDecision::Sector(item) => build_sector_investigation(item),
            AgentDecision::Symbol(_) => continue,
        };
        let key = format!("{}:{}", investigation.object_kind, investigation.object_id);
        by_object.entry(key).or_insert(investigation);
    }

    for notice in &snapshot.notices {
        let Some(investigation) = build_notice_investigation(snapshot.market, notice) else {
            continue;
        };
        let key = format!("{}:{}", investigation.object_kind, investigation.object_id);
        by_object.entry(key).or_insert(investigation);
    }

    let mut items = by_object.into_values().collect::<Vec<_>>();
    items.sort_by(|left, right| {
        investigation_object_rank(&right.object_kind)
            .cmp(&investigation_object_rank(&left.object_kind))
            .then_with(|| {
                investigation_attention_rank(right.attention_hint.as_str())
                    .cmp(&investigation_attention_rank(left.attention_hint.as_str()))
            })
            .then_with(|| right.priority.cmp(&left.priority))
            .then_with(|| left.object_id.cmp(&right.object_id))
    });
    if items.len() > limit {
        items.truncate(limit);
    }
    for (index, item) in items.iter_mut().enumerate() {
        item.rank = index + 1;
    }

    AgentInvestigations {
        tick: snapshot.tick,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        total: items.len(),
        items,
    }
}
