use super::*;

fn judgment_kind_rank(kind: AgentJudgmentKind) -> u8 {
    match kind {
        AgentJudgmentKind::Execute => 4,
        AgentJudgmentKind::Govern => 3,
        AgentJudgmentKind::Escalate => 2,
        AgentJudgmentKind::Investigate => 1,
    }
}

fn judgment_object_rank(object_kind: &str) -> u8 {
    match object_kind {
        "symbol" => 4,
        "workflow" => 3,
        "sector" => 2,
        "market" => 1,
        "cross_market_dependency" => 0,
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

fn governance_summary(object_id: &str, best_action: Option<&str>) -> String {
    if let Some(best_action) = best_action {
        format!("{object_id} is in review_gate before {best_action}")
    } else {
        format!("{object_id} is in review_gate before progress")
    }
}

fn execution_summary(object_id: &str, best_action: Option<&str>) -> String {
    if let Some(best_action) = best_action {
        format!("{object_id} is execution_ready for {best_action}")
    } else {
        format!("{object_id} is execution_ready")
    }
}

fn escalation_summary(object_id: &str) -> String {
    format!("{object_id} is queued for review_desk")
}

fn build_symbol_judgment(
    snapshot: &AgentSnapshot,
    investigation: &AgentInvestigation,
    recommendation: Option<&AgentRecommendation>,
) -> AgentOperationalJudgment {
    let symbol = investigation
        .reference_symbols
        .first()
        .cloned()
        .unwrap_or_else(|| investigation.object_id.clone());
    let symbol_state = snapshot.symbol(&symbol);
    let gate_reason = symbol_state.and_then(shared::multi_horizon_gate_reason);
    let policy_reason = symbol_state.and_then(shared::policy_reason);
    let best_action = recommendation.map(|item| item.best_action.as_str());
    let execution_policy = recommendation.map(|item| item.execution_policy);

    let kind = if matches!(
        execution_policy,
        Some(crate::action::workflow::ActionExecutionPolicy::AutoEligible)
    ) && !matches!(best_action, Some("wait" | "ignore") | None)
    {
        AgentJudgmentKind::Execute
    } else if gate_reason.is_some()
        || policy_reason.is_some()
        || recommendation
            .map(|item| !matches!(item.best_action.as_str(), "wait" | "ignore"))
            .unwrap_or(false)
    {
        AgentJudgmentKind::Govern
    } else if investigation.attention_hint == "review" {
        AgentJudgmentKind::Escalate
    } else {
        AgentJudgmentKind::Investigate
    };

    let mut reasons = investigation.reasons.clone();
    push_reason(
        &mut reasons,
        gate_reason.map(|item| format!("multi-horizon gate: {item}")),
    );
    push_reason(
        &mut reasons,
        policy_reason.map(|item| format!("policy gate: {item}")),
    );
    push_reason(
        &mut reasons,
        recommendation.map(|item| item.governance_reason.clone()),
    );
    push_reason(&mut reasons, recommendation.map(|item| item.why.clone()));
    reasons.truncate(4);

    let summary = match kind {
        AgentJudgmentKind::Execute => {
            execution_summary(investigation.object_id.as_str(), best_action)
        }
        AgentJudgmentKind::Govern => {
            governance_summary(investigation.object_id.as_str(), best_action)
        }
        AgentJudgmentKind::Escalate => escalation_summary(investigation.object_id.as_str()),
        AgentJudgmentKind::Investigate => investigation.summary.clone(),
    };

    AgentOperationalJudgment {
        rank: 0,
        kind,
        object_kind: investigation.object_kind.clone(),
        object_id: investigation.object_id.clone(),
        title: recommendation
            .and_then(|item| item.title.clone())
            .unwrap_or_else(|| investigation.title.clone()),
        summary,
        priority: recommendation
            .map(|item| item.score.max(item.confidence))
            .unwrap_or(investigation.priority)
            .max(investigation.priority),
        reasons,
        best_action: recommendation.map(|item| item.best_action.clone()),
        execution_policy,
        governance_reason_code: recommendation.map(|item| item.governance_reason_code),
        governance_reason: recommendation.map(|item| item.governance_reason.clone()),
        recommendation_id: recommendation
            .map(|item| item.recommendation_id.clone())
            .or_else(|| investigation.recommendation_id.clone()),
        reference_symbols: investigation.reference_symbols.clone(),
    }
}

fn build_sector_or_market_judgment(
    investigation: &AgentInvestigation,
    best_action: Option<&str>,
    execution_policy: Option<crate::action::workflow::ActionExecutionPolicy>,
    governance_reason_code: Option<crate::action::workflow::ActionGovernanceReasonCode>,
    governance_reason: Option<String>,
    recommendation_id: Option<String>,
) -> AgentOperationalJudgment {
    let kind = if matches!(
        execution_policy,
        Some(crate::action::workflow::ActionExecutionPolicy::AutoEligible)
    ) && !matches!(best_action, Some("wait" | "ignore") | None)
    {
        AgentJudgmentKind::Execute
    } else if !matches!(best_action, Some("wait" | "ignore") | None) {
        AgentJudgmentKind::Govern
    } else if investigation.attention_hint == "review" {
        AgentJudgmentKind::Escalate
    } else {
        AgentJudgmentKind::Investigate
    };

    let mut reasons = investigation.reasons.clone();
    push_reason(&mut reasons, governance_reason.clone());
    reasons.truncate(4);

    let summary = match kind {
        AgentJudgmentKind::Execute => {
            execution_summary(investigation.object_id.as_str(), best_action)
        }
        AgentJudgmentKind::Govern => {
            governance_summary(investigation.object_id.as_str(), best_action)
        }
        AgentJudgmentKind::Escalate => escalation_summary(investigation.object_id.as_str()),
        AgentJudgmentKind::Investigate => investigation.summary.clone(),
    };

    AgentOperationalJudgment {
        rank: 0,
        kind,
        object_kind: investigation.object_kind.clone(),
        object_id: investigation.object_id.clone(),
        title: investigation.title.clone(),
        summary,
        priority: investigation.priority,
        reasons,
        best_action: best_action.map(str::to_string),
        execution_policy,
        governance_reason_code,
        governance_reason,
        recommendation_id,
        reference_symbols: investigation.reference_symbols.clone(),
    }
}

fn build_generic_judgment(investigation: &AgentInvestigation) -> AgentOperationalJudgment {
    let kind = if investigation.attention_hint == "review" {
        AgentJudgmentKind::Escalate
    } else {
        AgentJudgmentKind::Investigate
    };
    let summary = if kind == AgentJudgmentKind::Escalate {
        escalation_summary(investigation.object_id.as_str())
    } else {
        investigation.summary.clone()
    };
    AgentOperationalJudgment {
        rank: 0,
        kind,
        object_kind: investigation.object_kind.clone(),
        object_id: investigation.object_id.clone(),
        title: investigation.title.clone(),
        summary,
        priority: investigation.priority,
        reasons: investigation.reasons.clone(),
        best_action: None,
        execution_policy: None,
        governance_reason_code: None,
        governance_reason: None,
        recommendation_id: investigation.recommendation_id.clone(),
        reference_symbols: investigation.reference_symbols.clone(),
    }
}

pub fn build_judgments(
    snapshot: &AgentSnapshot,
    session: Option<&AgentSession>,
    recommendations: Option<&AgentRecommendations>,
    limit: usize,
) -> AgentJudgments {
    // FP2: when no caller-provided recommendations are passed, fall
    // back to an empty shell instead of running the deprecated
    // heuristic builder. Downstream judgment helpers handle
    // `Option<&...>` recommendation context as None gracefully.
    let recommendations = recommendations
        .cloned()
        .unwrap_or_else(|| AgentRecommendations::empty(snapshot));
    let investigations =
        build_investigations(snapshot, session, Some(&recommendations), usize::MAX);
    let symbol_recommendations = recommendations
        .items
        .iter()
        .map(|item| (item.symbol.to_ascii_lowercase(), item))
        .collect::<std::collections::HashMap<_, _>>();
    let sector_recommendations = recommendations
        .decisions
        .iter()
        .filter_map(|decision| match decision {
            AgentDecision::Sector(item) => Some((item.sector.to_ascii_lowercase(), item)),
            _ => None,
        })
        .collect::<std::collections::HashMap<_, _>>();
    let market_recommendation =
        recommendations
            .decisions
            .iter()
            .find_map(|decision| match decision {
                AgentDecision::Market(item) => Some(item),
                _ => None,
            });

    let mut items = investigations
        .items
        .iter()
        .map(|investigation| match investigation.object_kind.as_str() {
            "symbol" | "cross_market_dependency" => build_symbol_judgment(
                snapshot,
                investigation,
                symbol_recommendations
                    .get(&investigation.object_id.to_ascii_lowercase())
                    .copied(),
            ),
            "sector" => {
                let recommendation = sector_recommendations
                    .get(&investigation.object_id.to_ascii_lowercase())
                    .copied();
                build_sector_or_market_judgment(
                    investigation,
                    recommendation.map(|item| item.best_action.as_str()),
                    recommendation.map(|item| item.execution_policy),
                    recommendation.map(|item| item.governance_reason_code),
                    recommendation.map(|item| item.governance_reason.clone()),
                    recommendation.map(|item| item.recommendation_id.clone()),
                )
            }
            "market" => build_sector_or_market_judgment(
                investigation,
                market_recommendation.map(|item| item.best_action.as_str()),
                market_recommendation.map(|item| item.execution_policy),
                market_recommendation.map(|item| item.governance_reason_code),
                market_recommendation.map(|item| item.governance_reason.clone()),
                market_recommendation.map(|item| item.recommendation_id.clone()),
            ),
            _ => build_generic_judgment(investigation),
        })
        .collect::<Vec<_>>();

    items.sort_by(|left, right| {
        judgment_kind_rank(right.kind)
            .cmp(&judgment_kind_rank(left.kind))
            .then_with(|| {
                judgment_object_rank(&right.object_kind)
                    .cmp(&judgment_object_rank(&left.object_kind))
            })
            .then_with(|| right.priority.cmp(&left.priority))
            .then_with(|| left.object_id.cmp(&right.object_id))
    });
    if items.len() > limit {
        let mut selected = Vec::new();
        let mut dependency_count = 0usize;
        let mut deferred = Vec::new();
        for item in items {
            if item.object_kind == "cross_market_dependency" && dependency_count >= 2 {
                deferred.push(item);
                continue;
            }
            if item.object_kind == "cross_market_dependency" {
                dependency_count += 1;
            }
            selected.push(item);
            if selected.len() == limit {
                break;
            }
        }
        if selected.len() < limit {
            selected.extend(deferred.into_iter().take(limit - selected.len()));
        }
        items = selected;
    }
    for (index, item) in items.iter_mut().enumerate() {
        item.rank = index + 1;
    }

    AgentJudgments {
        tick: snapshot.tick,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        total: items.len(),
        items,
    }
}
