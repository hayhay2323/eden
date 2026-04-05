use super::*;

fn combined_knowledge_links(
    snapshot: &AgentSnapshot,
    recommendations: &AgentRecommendations,
) -> Vec<AgentKnowledgeLink> {
    let mut seen = std::collections::HashSet::new();
    snapshot
        .knowledge_links
        .iter()
        .cloned()
        .chain(recommendations.knowledge_links.iter().cloned())
        .filter(|item| seen.insert(item.link_id.to_ascii_lowercase()))
        .collect()
}

fn graph_node_endpoint(market: LiveMarket, node_id: &str) -> String {
    format!("/api/ontology/{}/graph/node/{node_id}", market_slug(market))
}

fn neighborhood_endpoint(market: LiveMarket, kind: OperationalObjectKind, id: &str) -> String {
    let kind = match kind {
        OperationalObjectKind::MarketSession => "market_session",
        OperationalObjectKind::SymbolState => "symbol_state",
        OperationalObjectKind::Case => "case",
        OperationalObjectKind::Recommendation => "recommendation",
        OperationalObjectKind::MacroEvent => "macro_event",
        OperationalObjectKind::Thread => "thread",
        OperationalObjectKind::Workflow => "workflow",
    };
    format!(
        "/api/ontology/{}/neighborhood/{kind}/{id}",
        market_slug(market)
    )
}

pub(crate) fn object_endpoint(market: LiveMarket, path: &str) -> String {
    format!("/api/ontology/{}/{}", market_slug(market), path)
}

pub(crate) fn symbol_contract_id(market: LiveMarket, source_tick: u64, symbol: &str) -> String {
    format!(
        "symbol_state:{}:{}:{}",
        market_slug(market),
        normalized_symbol_id(symbol),
        source_tick
    )
}

pub(crate) fn thread_contract_id(market: LiveMarket, symbol: &str) -> String {
    format!(
        "thread:{}:{}",
        market_slug(market),
        normalized_symbol_id(symbol)
    )
}

pub(crate) fn symbol_object_ref(
    market: LiveMarket,
    source_tick: u64,
    symbol: &str,
) -> OperationalObjectRef {
    OperationalObjectRef {
        id: symbol_contract_id(market, source_tick, symbol),
        kind: OperationalObjectKind::SymbolState,
        endpoint: object_endpoint(market, &format!("symbols/{symbol}")),
        label: Some(symbol.into()),
    }
}

pub(crate) fn case_object_ref(
    market: LiveMarket,
    case_id: &str,
    label: Option<String>,
) -> OperationalObjectRef {
    OperationalObjectRef {
        id: case_id.into(),
        kind: OperationalObjectKind::Case,
        endpoint: object_endpoint(market, &format!("cases/{case_id}")),
        label,
    }
}

pub(crate) fn recommendation_object_ref(
    market: LiveMarket,
    recommendation_id: &str,
    label: Option<String>,
) -> OperationalObjectRef {
    OperationalObjectRef {
        id: recommendation_id.into(),
        kind: OperationalObjectKind::Recommendation,
        endpoint: object_endpoint(market, &format!("recommendations/{recommendation_id}")),
        label,
    }
}

pub(crate) fn thread_object_ref(
    market: LiveMarket,
    symbol: &str,
    label: Option<String>,
) -> OperationalObjectRef {
    let thread_id = thread_contract_id(market, symbol);
    OperationalObjectRef {
        id: thread_id.clone(),
        kind: OperationalObjectKind::Thread,
        endpoint: object_endpoint(market, &format!("threads/{thread_id}")),
        label,
    }
}

pub(crate) fn workflow_object_ref(
    market: LiveMarket,
    workflow_id: &str,
    label: Option<String>,
) -> OperationalObjectRef {
    OperationalObjectRef {
        id: workflow_id.into(),
        kind: OperationalObjectKind::Workflow,
        endpoint: object_endpoint(market, &format!("workflows/{workflow_id}")),
        label,
    }
}

fn symbol_graph_ref(market: LiveMarket, symbol: &str) -> OperationalGraphRef {
    let node_id = crate::ontology::symbol_node_id(symbol);
    OperationalGraphRef {
        node_id: node_id.clone(),
        node_kind: KnowledgeNodeKind::Symbol,
        endpoint: graph_node_endpoint(market, &node_id),
    }
}

fn setup_graph_ref(market: LiveMarket, setup_id: &str) -> OperationalGraphRef {
    let node_id = crate::ontology::setup_node_id(setup_id);
    OperationalGraphRef {
        node_id: node_id.clone(),
        node_kind: KnowledgeNodeKind::Setup,
        endpoint: graph_node_endpoint(market, &node_id),
    }
}

fn decision_graph_ref(market: LiveMarket, recommendation_id: &str) -> OperationalGraphRef {
    let node_id = crate::ontology::decision_node_id(recommendation_id);
    OperationalGraphRef {
        node_id: node_id.clone(),
        node_kind: KnowledgeNodeKind::Decision,
        endpoint: graph_node_endpoint(market, &node_id),
    }
}

fn macro_event_graph_ref(market: LiveMarket, event_id: &str) -> OperationalGraphRef {
    let node_id = crate::ontology::macro_event_node_id(event_id);
    OperationalGraphRef {
        node_id: node_id.clone(),
        node_kind: KnowledgeNodeKind::MacroEvent,
        endpoint: graph_node_endpoint(market, &node_id),
    }
}

pub(crate) fn operational_history_ref(
    key: impl Into<String>,
    endpoint: impl Into<String>,
) -> OperationalHistoryRef {
    OperationalHistoryRef {
        key: key.into(),
        endpoint: endpoint.into(),
        count: None,
        latest_at: None,
    }
}

pub(crate) fn case_history_refs(
    market: LiveMarket,
    case_id: &str,
    setup_id: &str,
    workflow_id: Option<&str>,
) -> CaseHistoryRefs {
    let market = market_slug(market);
    CaseHistoryRefs {
        workflow: workflow_id.map(|workflow_id| {
            operational_history_ref(
                workflow_id,
                format!("/api/ontology/{market}/cases/{case_id}/history/workflow"),
            )
        }),
        reasoning: Some(operational_history_ref(
            setup_id,
            format!("/api/ontology/{market}/cases/{case_id}/history/reasoning"),
        )),
        outcomes: Some(operational_history_ref(
            setup_id,
            format!("/api/ontology/{market}/cases/{case_id}/history/outcomes"),
        )),
    }
}

pub(crate) fn recommendation_history_refs(
    market: LiveMarket,
    recommendation_id: &str,
    related_case_id: Option<&str>,
    related_workflow_id: Option<&str>,
) -> RecommendationHistoryRefs {
    let market = market_slug(market);
    RecommendationHistoryRefs {
        journal: Some(operational_history_ref(
            recommendation_id,
            format!("/api/ontology/{market}/recommendations/{recommendation_id}/history"),
        )),
        workflow: related_workflow_id.map(|workflow_id| {
            operational_history_ref(
                workflow_id,
                format!("/api/ontology/{market}/workflows/{workflow_id}/history"),
            )
        }),
        outcomes: related_case_id.map(|case_id| {
            operational_history_ref(
                case_id,
                format!("/api/ontology/{market}/cases/{case_id}/history/outcomes"),
            )
        }),
    }
}

pub(crate) fn workflow_history_refs(market: LiveMarket, workflow_id: &str) -> WorkflowHistoryRefs {
    let market = market_slug(market);
    WorkflowHistoryRefs {
        events: Some(operational_history_ref(
            workflow_id,
            format!("/api/ontology/{market}/workflows/{workflow_id}/history"),
        )),
    }
}

fn thread_workflow_focus_summary(thread: &AgentThread) -> Option<String> {
    match (
        thread.workflow_stage.as_deref(),
        thread.workflow_next_step.as_deref(),
    ) {
        (Some("monitoring"), _) => Some(format!(
            "{} is monitoring an active workflow",
            thread.symbol
        )),
        (Some("reviewed"), _) => Some(format!("{} has completed workflow review", thread.symbol)),
        (_, Some("review_gate")) => Some(format!("{} is in review_gate", thread.symbol)),
        (_, Some("review_desk")) => Some(format!("{} is queued for review_desk", thread.symbol)),
        (_, Some("collect_confirmation")) => {
            Some(format!("{} is collecting confirmation", thread.symbol))
        }
        (_, Some("execute")) => Some(format!("{} is execution_ready", thread.symbol)),
        (_, Some("complete")) => Some(format!("{} is complete", thread.symbol)),
        _ => None,
    }
}

fn market_session_object_ref(contract: &MarketSessionContract) -> OperationalObjectRef {
    OperationalObjectRef {
        id: contract.id.0.clone(),
        kind: OperationalObjectKind::MarketSession,
        endpoint: object_endpoint(contract.market, "market-session"),
        label: Some(format!(
            "{} Market",
            market_slug(contract.market).to_ascii_uppercase()
        )),
    }
}

fn parse_execution_policy_label(value: Option<&str>) -> Option<ActionExecutionPolicy> {
    match value.map(str::trim).filter(|value| !value.is_empty()) {
        Some("manual_only") => Some(ActionExecutionPolicy::ManualOnly),
        Some("review_required") => Some(ActionExecutionPolicy::ReviewRequired),
        Some("auto_eligible") => Some(ActionExecutionPolicy::AutoEligible),
        _ => None,
    }
}

fn judgment_lane(judgment: &AgentOperationalJudgment) -> &'static str {
    match judgment.kind {
        AgentJudgmentKind::Govern => "review_gate",
        AgentJudgmentKind::Escalate => "review_desk",
        AgentJudgmentKind::Investigate => "collect_confirmation",
        AgentJudgmentKind::Execute => "execute",
    }
}

fn case_operator_lane(case: &CaseContract) -> Option<&'static str> {
    if case
        .queue_pin
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
        || case.workflow_state.eq_ignore_ascii_case("review")
    {
        return Some("review_desk");
    }
    if case
        .multi_horizon_gate_reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
    {
        return Some("collect_confirmation");
    }
    if case
        .policy_reason
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
        || matches!(
            case.execution_policy,
            Some(ActionExecutionPolicy::ManualOnly)
        )
        || matches!(
            case.governance_reason_code,
            Some(
                ActionGovernanceReasonCode::OperatorActionRequired
                    | ActionGovernanceReasonCode::SeverityRequiresReview
                    | ActionGovernanceReasonCode::WorkflowTransitionWindow
                    | ActionGovernanceReasonCode::WorkflowNotCreated
                    | ActionGovernanceReasonCode::InvalidationRuleMissing
                    | ActionGovernanceReasonCode::NonPositiveExpectedAlpha
            )
        )
    {
        return Some("review_gate");
    }
    None
}

fn should_materialize_operator_surface(
    lane: Option<&str>,
    execution_policy: Option<ActionExecutionPolicy>,
    governance_reason_code: Option<ActionGovernanceReasonCode>,
    queue_pin: Option<&str>,
) -> bool {
    matches!(
        lane,
        Some("review_gate" | "review_desk" | "collect_confirmation" | "execute")
    ) || matches!(execution_policy, Some(ActionExecutionPolicy::ManualOnly))
        || matches!(
            governance_reason_code,
            Some(ActionGovernanceReasonCode::OperatorActionRequired)
        )
        || queue_pin
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .is_some()
}

fn preferred_case_for_symbol<'a>(
    cases: &'a [CaseContract],
    symbol: &str,
    best_action: Option<&str>,
) -> Option<&'a CaseContract> {
    cases
        .iter()
        .filter(|case| case.symbol.eq_ignore_ascii_case(symbol))
        .max_by(|left, right| {
            let left_matches_action = best_action
                .map(|action| left.action.eq_ignore_ascii_case(action))
                .unwrap_or(false);
            let right_matches_action = best_action
                .map(|action| right.action.eq_ignore_ascii_case(action))
                .unwrap_or(false);
            left_matches_action
                .cmp(&right_matches_action)
                .then_with(|| left.queue_pin.is_some().cmp(&right.queue_pin.is_some()))
                .then_with(|| {
                    left.workflow_state
                        .eq("review")
                        .cmp(&right.workflow_state.eq("review"))
                })
                .then_with(|| left.confidence.cmp(&right.confidence))
        })
}

fn judgment_symbol(judgment: &AgentOperationalJudgment) -> Option<&str> {
    judgment
        .reference_symbols
        .first()
        .map(|symbol| symbol.as_str())
        .or_else(|| {
            matches!(
                judgment.object_kind.as_str(),
                "symbol" | "cross_market_dependency"
            )
            .then_some(judgment.object_id.as_str())
        })
}

fn workflow_ref_from_case(case: &CaseContract) -> Option<OperationalObjectRef> {
    case.workflow_id.as_deref().map(|workflow_id| {
        workflow_object_ref(case.market, workflow_id, Some(case.workflow_state.clone()))
    })
}

fn object_ref_for_judgment(
    market_session: &MarketSessionContract,
    source_tick: u64,
    judgment: &AgentOperationalJudgment,
    symbol: Option<&str>,
) -> Option<OperationalObjectRef> {
    match judgment.object_kind.as_str() {
        "market" => Some(market_session_object_ref(market_session)),
        "symbol" | "cross_market_dependency" => {
            symbol.map(|value| symbol_object_ref(market_session.market, source_tick, value))
        }
        _ => None,
    }
}

fn operator_lane_rank(lane: &str) -> u8 {
    match lane {
        "review_gate" => 0,
        "review_desk" => 1,
        "collect_confirmation" => 2,
        "execute" => 3,
        _ => 4,
    }
}

fn thread_lane_from_text(thread: &AgentThread) -> Option<&'static str> {
    let unlock = thread
        .unlock_condition
        .as_deref()
        .map(|value| value.to_ascii_lowercase());
    let headline = thread
        .headline
        .as_deref()
        .map(|value| value.to_ascii_lowercase());
    let summary = thread
        .latest_summary
        .as_deref()
        .map(|value| value.to_ascii_lowercase());

    for value in [unlock.as_deref(), headline.as_deref(), summary.as_deref()]
        .into_iter()
        .flatten()
    {
        if value.contains("review_desk") || value.contains("operator review promotes") {
            return Some("review_desk");
        }
        if value.contains("review_gate") || value.contains("human review clears the gate") {
            return Some("review_gate");
        }
        if value.contains("collecting confirmation") || value.contains("confirming follow-through")
        {
            return Some("collect_confirmation");
        }
        if value.contains("execution_ready") || value.contains("execution workflow is opened") {
            return Some("execute");
        }
    }
    None
}

fn inferred_thread_lane(thread: &AgentThread, case: Option<&CaseContract>) -> String {
    thread
        .workflow_next_step
        .clone()
        .or_else(|| thread_lane_from_text(thread).map(str::to_string))
        .or_else(|| case.and_then(case_operator_lane).map(str::to_string))
        .unwrap_or_else(|| "review_gate".into())
}

fn operator_summary(
    object_id: &str,
    lane: &str,
    best_action: Option<&str>,
    fallback: Option<&str>,
) -> String {
    match lane {
        "review_gate" => format!(
            "{object_id} is in review_gate before {}",
            best_action.unwrap_or("progress")
        ),
        "review_desk" => format!("{object_id} is queued for review_desk"),
        "collect_confirmation" => format!("{object_id} is collecting confirmation"),
        "execute" => format!(
            "{object_id} is execution_ready for {}",
            best_action.unwrap_or("action")
        ),
        _ => fallback.unwrap_or(object_id).to_string(),
    }
}

fn operator_identity_key(object_kind: &str, object_id: &str, symbol: Option<&str>) -> String {
    symbol
        .map(|value| format!("symbol:{}", value.to_ascii_lowercase()))
        .unwrap_or_else(|| format!("{object_kind}:{}", object_id.to_ascii_lowercase()))
}

fn operator_work_item_grain(
    object_kind: &str,
    symbol: Option<&str>,
    sector: Option<&str>,
) -> WorkItemGrain {
    if symbol.is_some() || object_kind.eq_ignore_ascii_case("symbol") {
        WorkItemGrain::Symbol
    } else if sector.is_some() || object_kind.eq_ignore_ascii_case("sector") {
        WorkItemGrain::Sector
    } else {
        WorkItemGrain::Market
    }
}

fn operator_scope(
    case_ref: Option<&OperationalObjectRef>,
    workflow_ref: Option<&OperationalObjectRef>,
    object_ref: Option<&OperationalObjectRef>,
    fallback_kind: &str,
    fallback_id: &str,
) -> (String, String) {
    if let Some(case_ref) = case_ref {
        (case_ref.kind.slug().to_string(), case_ref.id.clone())
    } else if let Some(workflow_ref) = workflow_ref {
        (
            workflow_ref.kind.slug().to_string(),
            workflow_ref.id.clone(),
        )
    } else if let Some(object_ref) = object_ref {
        (object_ref.kind.slug().to_string(), object_ref.id.clone())
    } else {
        (fallback_kind.to_string(), fallback_id.to_string())
    }
}

fn collect_operator_source_refs(
    refs: impl IntoIterator<Item = Option<OperationalObjectRef>>,
) -> Vec<OperationalObjectRef> {
    let mut seen = std::collections::HashSet::new();
    refs.into_iter()
        .flatten()
        .filter(|item| {
            seen.insert(format!(
                "{}:{}",
                item.kind.slug(),
                item.id.to_ascii_lowercase()
            ))
        })
        .collect()
}

fn operator_primary_ref(
    item: &OperatorWorkItem,
    source_refs: &[OperationalObjectRef],
) -> Option<OperationalObjectRef> {
    item.case_ref
        .clone()
        .or_else(|| item.workflow_ref.clone())
        .or_else(|| item.object_ref.clone())
        .or_else(|| source_refs.first().cloned())
}

fn build_operator_work_item_navigation(
    snapshot: &OperationalSnapshot,
    item: &OperatorWorkItem,
    source_refs: &[OperationalObjectRef],
) -> OperationalNavigation {
    let primary_ref = operator_primary_ref(item, source_refs);
    let mut navigation = primary_ref
        .as_ref()
        .and_then(|reference| snapshot.navigation(reference.kind, &reference.id).cloned())
        .unwrap_or_default();

    if let Some(primary_ref) = primary_ref {
        navigation.self_ref = snapshot
            .resolve_object_ref(&primary_ref)
            .or(Some(primary_ref.clone()));
        if navigation.neighborhood_endpoint.is_none() {
            navigation.neighborhood_endpoint = Some(neighborhood_endpoint(
                snapshot.market,
                primary_ref.kind,
                &primary_ref.id,
            ));
        }
    }

    if !source_refs.is_empty() {
        let mut relationships = vec![OperationalRelationshipGroup {
            name: "sources".into(),
            refs: source_refs.to_vec(),
        }];
        relationships.extend(
            navigation
                .relationships
                .into_iter()
                .filter(|group| group.name != "sources"),
        );
        navigation.relationships = relationships;
    }

    navigation
}

fn materialize_operator_work_items(
    market_session: &MarketSessionContract,
    session: &AgentSession,
    cases: &[CaseContract],
) -> Vec<OperatorWorkItem> {
    let mut items = Vec::new();
    let mut seen_case_ids = std::collections::HashSet::new();
    let mut seen_item_ids = std::collections::HashSet::new();
    let mut seen_identity_keys = std::collections::HashSet::new();
    let source_tick = market_session.source_tick;
    let market = market_session.market;

    let judgments_by_symbol = session
        .current_judgments
        .iter()
        .filter_map(|judgment| {
            judgment_symbol(judgment).map(|symbol| (symbol.to_ascii_lowercase(), judgment))
        })
        .collect::<HashMap<_, _>>();

    for thread in &session.active_threads {
        let judgment = judgments_by_symbol
            .get(&thread.symbol.to_ascii_lowercase())
            .copied();
        let case = preferred_case_for_symbol(
            cases,
            &thread.symbol,
            judgment.and_then(|item| item.best_action.as_deref()),
        );
        let lane = inferred_thread_lane(thread, case);
        let execution_policy = judgment
            .and_then(|item| item.execution_policy)
            .or_else(|| case.and_then(|item| item.execution_policy))
            .or_else(|| parse_execution_policy_label(thread.execution_policy.as_deref()));
        let governance_reason_code = judgment
            .and_then(|item| item.governance_reason_code)
            .or_else(|| case.and_then(|item| item.governance_reason_code));
        let queue_pin = case.and_then(|item| item.queue_pin.clone());

        if !should_materialize_operator_surface(
            Some(lane.as_str()),
            execution_policy,
            governance_reason_code,
            queue_pin.as_deref(),
        ) {
            continue;
        }
        let identity_key = operator_identity_key("symbol", &thread.symbol, Some(&thread.symbol));
        if !seen_identity_keys.insert(identity_key) {
            continue;
        }

        let id = format!(
            "operator:{}:thread:{}",
            market_slug(market),
            normalized_symbol_id(&thread.symbol)
        );
        if !seen_item_ids.insert(id.clone()) {
            continue;
        }

        if let Some(case) = case {
            seen_case_ids.insert(case.id.0.clone());
        }

        let case_ref =
            case.map(|item| case_object_ref(market, &item.id.0, Some(item.title.clone())));
        let workflow_ref = case.and_then(workflow_ref_from_case);
        let object_ref = Some(symbol_object_ref(market, source_tick, &thread.symbol));
        let thread_ref = Some(thread_object_ref(
            market,
            &thread.symbol,
            thread.title.clone().or_else(|| Some(thread.symbol.clone())),
        ));
        let recommendation_ref = judgment.and_then(|item| {
            item.recommendation_id.as_deref().map(|recommendation_id| {
                recommendation_object_ref(
                    market,
                    recommendation_id,
                    Some(format!(
                        "{} {}",
                        thread.symbol,
                        item.best_action.as_deref().unwrap_or("review")
                    )),
                )
            })
        });
        let source_refs = collect_operator_source_refs([
            case_ref.clone(),
            workflow_ref.clone(),
            object_ref.clone(),
            thread_ref,
            recommendation_ref,
        ]);
        let origin = if case_ref.is_some() {
            WorkItemOrigin::Case
        } else if judgment.is_some() {
            WorkItemOrigin::Judgment
        } else {
            WorkItemOrigin::Thread
        };
        let (scope_kind, scope_id) = operator_scope(
            case_ref.as_ref(),
            workflow_ref.as_ref(),
            object_ref.as_ref(),
            "symbol",
            &thread.symbol,
        );

        items.push(OperatorWorkItem {
            id,
            origin,
            grain: operator_work_item_grain(
                "symbol",
                Some(thread.symbol.as_str()),
                thread.sector.as_deref(),
            ),
            lane: lane.clone(),
            status: thread.status.clone(),
            priority: thread.priority,
            scope_kind,
            scope_id,
            title: thread
                .title
                .clone()
                .unwrap_or_else(|| thread.symbol.clone()),
            summary: operator_summary(
                &thread.symbol,
                lane.as_str(),
                judgment.and_then(|item| item.best_action.as_deref()),
                thread
                    .headline
                    .as_deref()
                    .or(thread.latest_summary.as_deref())
                    .or(Some(thread.status.as_str())),
            ),
            symbol: Some(thread.symbol.clone()),
            sector: thread.sector.clone(),
            best_action: judgment.and_then(|item| item.best_action.clone()),
            execution_policy,
            governance_reason_code,
            blocker: thread
                .blocked_reason
                .clone()
                .or_else(|| thread.governance_reason.clone())
                .or_else(|| case.and_then(|item| item.governance_reason.clone()))
                .or_else(|| judgment.and_then(|item| item.governance_reason.clone())),
            queue_pin,
            owner: case.and_then(|item| item.owner.clone()),
            reviewer: case.and_then(|item| item.reviewer.clone()),
            object_ref,
            case_ref,
            workflow_ref,
            source_refs,
            navigation: OperationalNavigation::default(),
        });
    }

    for judgment in &session.current_judgments {
        let symbol = judgment_symbol(judgment);
        let case = symbol.and_then(|value| {
            preferred_case_for_symbol(cases, value, judgment.best_action.as_deref())
        });
        if let Some(case) = case {
            if seen_case_ids.contains(&case.id.0) {
                continue;
            }
        }
        let lane = case
            .and_then(case_operator_lane)
            .map(str::to_string)
            .unwrap_or_else(|| judgment_lane(judgment).to_string());
        let execution_policy = judgment
            .execution_policy
            .or_else(|| case.and_then(|item| item.execution_policy));
        let governance_reason_code = judgment
            .governance_reason_code
            .or_else(|| case.and_then(|item| item.governance_reason_code));
        let queue_pin = case.and_then(|item| item.queue_pin.clone());

        if !should_materialize_operator_surface(
            Some(lane.as_str()),
            execution_policy,
            governance_reason_code,
            queue_pin.as_deref(),
        ) {
            continue;
        }
        let identity_key = operator_identity_key(
            judgment.object_kind.as_str(),
            judgment.object_id.as_str(),
            symbol,
        );
        if !seen_identity_keys.insert(identity_key) {
            continue;
        }

        let id = format!(
            "operator:{}:judgment:{}:{}",
            market_slug(market),
            judgment.object_kind,
            judgment.object_id.to_ascii_lowercase()
        );
        if !seen_item_ids.insert(id.clone()) {
            continue;
        }

        if let Some(case) = case {
            seen_case_ids.insert(case.id.0.clone());
        }
        let sector = case.and_then(|item| item.sector.clone());
        let object_ref = object_ref_for_judgment(market_session, source_tick, judgment, symbol);
        let case_ref =
            case.map(|item| case_object_ref(market, &item.id.0, Some(item.title.clone())));
        let workflow_ref = case.and_then(workflow_ref_from_case);
        let recommendation_ref = judgment
            .recommendation_id
            .as_deref()
            .map(|recommendation_id| {
                recommendation_object_ref(
                    market,
                    recommendation_id,
                    Some(format!(
                        "{} {}",
                        judgment.object_id,
                        judgment.best_action.as_deref().unwrap_or("review")
                    )),
                )
            });
        let source_refs = collect_operator_source_refs([
            case_ref.clone(),
            workflow_ref.clone(),
            object_ref.clone(),
            recommendation_ref,
        ]);
        let origin = if case_ref.is_some() {
            WorkItemOrigin::Case
        } else {
            WorkItemOrigin::Judgment
        };
        let (scope_kind, scope_id) = operator_scope(
            case_ref.as_ref(),
            workflow_ref.as_ref(),
            object_ref.as_ref(),
            judgment.object_kind.as_str(),
            judgment.object_id.as_str(),
        );

        items.push(OperatorWorkItem {
            id,
            origin,
            grain: operator_work_item_grain(
                judgment.object_kind.as_str(),
                symbol,
                sector.as_deref(),
            ),
            lane: lane.clone(),
            status: format!("{:?}", judgment.kind).to_ascii_lowercase(),
            priority: judgment.priority,
            scope_kind,
            scope_id,
            title: judgment.title.clone(),
            summary: operator_summary(
                judgment.object_id.as_str(),
                lane.as_str(),
                judgment.best_action.as_deref(),
                Some(judgment.summary.as_str()),
            ),
            symbol: symbol.map(str::to_string),
            sector,
            best_action: judgment.best_action.clone(),
            execution_policy,
            governance_reason_code,
            blocker: case
                .and_then(|item| item.policy_reason.clone())
                .or_else(|| judgment.governance_reason.clone())
                .or_else(|| case.and_then(|item| item.governance_reason.clone())),
            queue_pin,
            owner: case.and_then(|item| item.owner.clone()),
            reviewer: case.and_then(|item| item.reviewer.clone()),
            object_ref,
            case_ref,
            workflow_ref,
            source_refs,
            navigation: OperationalNavigation::default(),
        });
    }

    for case in cases {
        if seen_case_ids.contains(&case.id.0) {
            continue;
        }
        let Some(lane) = case_operator_lane(case) else {
            continue;
        };
        if !should_materialize_operator_surface(
            Some(lane),
            case.execution_policy,
            case.governance_reason_code,
            case.queue_pin.as_deref(),
        ) {
            continue;
        }
        let has_symbol = !case.symbol.trim().is_empty();
        let primary_object_id = if has_symbol {
            case.symbol.as_str()
        } else {
            case.setup_id.as_str()
        };
        let identity_key = operator_identity_key(
            "case",
            primary_object_id,
            has_symbol.then_some(case.symbol.as_str()),
        );
        if !seen_identity_keys.insert(identity_key) {
            continue;
        }
        let object_ref = has_symbol.then(|| symbol_object_ref(market, source_tick, &case.symbol));
        let case_ref = Some(case_object_ref(
            market,
            &case.id.0,
            Some(case.title.clone()),
        ));
        let workflow_ref = workflow_ref_from_case(case);
        let recommendation_ref = case.recommendation_ids.first().map(|recommendation_id| {
            recommendation_object_ref(
                market,
                recommendation_id,
                Some(format!("{} {}", case.symbol, case.action)),
            )
        });
        let source_refs = collect_operator_source_refs([
            case_ref.clone(),
            workflow_ref.clone(),
            object_ref.clone(),
            recommendation_ref,
        ]);
        let (scope_kind, scope_id) = operator_scope(
            case_ref.as_ref(),
            workflow_ref.as_ref(),
            object_ref.as_ref(),
            if has_symbol { "symbol" } else { "case" },
            primary_object_id,
        );

        items.push(OperatorWorkItem {
            id: format!("operator:{}:case:{}", market_slug(market), case.setup_id),
            origin: WorkItemOrigin::Case,
            grain: operator_work_item_grain(
                if has_symbol { "symbol" } else { "case" },
                has_symbol.then_some(case.symbol.as_str()),
                case.sector.as_deref(),
            ),
            lane: lane.into(),
            status: case.workflow_state.clone(),
            priority: case.confidence,
            scope_kind,
            scope_id,
            title: case.title.clone(),
            summary: operator_summary(
                if has_symbol {
                    case.symbol.as_str()
                } else {
                    case.title.as_str()
                },
                lane,
                Some(case.action.as_str()),
                case.policy_reason
                    .as_deref()
                    .or(case.multi_horizon_gate_reason.as_deref())
                    .or(case.governance_reason.as_deref())
                    .or(Some(case.title.as_str())),
            ),
            symbol: has_symbol.then(|| case.symbol.clone()),
            sector: case.sector.clone(),
            best_action: Some(case.action.clone()),
            execution_policy: case.execution_policy,
            governance_reason_code: case.governance_reason_code,
            blocker: case
                .policy_reason
                .clone()
                .or_else(|| case.multi_horizon_gate_reason.clone())
                .or_else(|| case.governance_reason.clone()),
            queue_pin: case.queue_pin.clone(),
            owner: case.owner.clone(),
            reviewer: case.reviewer.clone(),
            object_ref,
            case_ref,
            workflow_ref,
            source_refs,
            navigation: OperationalNavigation::default(),
        });
    }

    items.sort_by(|left, right| {
        operator_lane_rank(left.lane.as_str())
            .cmp(&operator_lane_rank(right.lane.as_str()))
            .then_with(|| right.priority.cmp(&left.priority))
            .then_with(|| left.title.cmp(&right.title))
    });
    items
}

fn populate_operator_work_item_navigation(snapshot: &mut OperationalSnapshot) {
    let enriched = snapshot
        .sidecars
        .operator_work_items
        .iter()
        .map(|item| {
            let source_refs = if item.source_refs.is_empty() {
                collect_operator_source_refs([
                    item.case_ref.clone(),
                    item.workflow_ref.clone(),
                    item.object_ref.clone(),
                ])
            } else {
                item.source_refs.clone()
            };
            let navigation = build_operator_work_item_navigation(snapshot, item, &source_refs);
            (source_refs, navigation)
        })
        .collect::<Vec<_>>();

    debug_assert_eq!(
        enriched.len(),
        snapshot.sidecars.operator_work_items.len(),
        "operator work item enrichment must stay length-preserving"
    );

    for (item, (source_refs, navigation)) in snapshot
        .sidecars
        .operator_work_items
        .iter_mut()
        .zip(enriched)
    {
        item.source_refs = source_refs;
        item.navigation = navigation;
    }
}

pub fn build_market_session_contract(
    snapshot: &AgentSnapshot,
    session: Option<&AgentSession>,
    narration: Option<&AgentNarration>,
) -> Result<MarketSessionContract, String> {
    let observed_at = parse_timestamp(&snapshot.timestamp)?;
    let computed_at = observed_at;
    let preferred_judgments = session
        .map(|item| item.current_judgments.clone())
        .unwrap_or_default();
    let preferred_investigations = session
        .map(|item| item.current_investigations.clone())
        .unwrap_or_default();
    let preferred_thread_workflow_summaries = session
        .map(|item| {
            item.active_threads
                .iter()
                .filter_map(thread_workflow_focus_summary)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let preferred_local_judgments = preferred_judgments
        .iter()
        .filter(|item| item.object_kind == "symbol")
        .collect::<Vec<_>>();
    let preferred_local_investigations = preferred_investigations
        .iter()
        .filter(|item| item.object_kind == "symbol")
        .collect::<Vec<_>>();
    let preferred_judgment_summaries = if !preferred_local_judgments.is_empty() {
        preferred_local_judgments
            .iter()
            .map(|item| item.summary.clone())
            .collect::<Vec<_>>()
    } else {
        preferred_judgments
            .iter()
            .map(|item| item.summary.clone())
            .collect::<Vec<_>>()
    };
    let preferred_investigation_summaries = if !preferred_local_investigations.is_empty() {
        preferred_local_investigations
            .iter()
            .map(|item| item.summary.clone())
            .collect::<Vec<_>>()
    } else {
        preferred_investigations
            .iter()
            .map(|item| item.summary.clone())
            .collect::<Vec<_>>()
    };
    let wake_headline = preferred_judgment_summaries
        .first()
        .cloned()
        .or_else(|| preferred_thread_workflow_summaries.first().cloned())
        .or_else(|| preferred_investigation_summaries.first().cloned())
        .or_else(|| snapshot.wake.headline.clone());
    let mut wake_summary = snapshot.wake.summary.clone();
    for item in preferred_thread_workflow_summaries.iter().take(4).rev() {
        if !wake_summary.iter().any(|existing| existing == item) {
            wake_summary.insert(0, item.clone());
        }
    }
    for item in preferred_investigation_summaries.iter().take(4).rev() {
        if !wake_summary.iter().any(|existing| existing == item) {
            wake_summary.insert(0, item.clone());
        }
    }
    for item in preferred_judgment_summaries.iter().take(4).rev() {
        if !wake_summary.iter().any(|existing| existing == item) {
            wake_summary.insert(0, item.clone());
        }
    }
    wake_summary.truncate(6);
    let mut wake_reasons = snapshot.wake.reasons.clone();
    for item in preferred_thread_workflow_summaries.iter().take(5).rev() {
        if !wake_reasons.iter().any(|existing| existing == item) {
            wake_reasons.insert(0, item.clone());
        }
    }
    for item in preferred_investigation_summaries.iter().take(5).rev() {
        if !wake_reasons.iter().any(|existing| existing == item) {
            wake_reasons.insert(0, item.clone());
        }
    }
    for item in preferred_judgment_summaries.iter().take(5).rev() {
        if !wake_reasons.iter().any(|existing| existing == item) {
            wake_reasons.insert(0, item.clone());
        }
    }
    wake_reasons.truncate(6);
    Ok(MarketSessionContract {
        id: MarketSessionId(format!(
            "market_session:{}:{}",
            market_slug(snapshot.market),
            snapshot.tick
        )),
        market: snapshot.market,
        source_tick: snapshot.tick,
        observed_at,
        computed_at,
        market_regime: snapshot.market_regime.clone(),
        stress: snapshot.stress.clone(),
        focus_symbols: session
            .map(|item| item.focus_symbols.clone())
            .unwrap_or_else(|| snapshot.wake.focus_symbols.clone()),
        should_speak: session
            .map(|item| item.should_speak)
            .unwrap_or(snapshot.wake.should_speak),
        priority: snapshot.wake.priority,
        active_thread_count: session.map(|item| item.active_thread_count).unwrap_or(0),
        wake_headline,
        wake_summary,
        wake_reasons,
        suggested_tools: snapshot.wake.suggested_tools.clone(),
        market_summary: narration.and_then(|item| item.market_summary_5m.clone()),
        navigation: OperationalNavigation::default(),
        relationships: MarketSessionRelationships {
            focus_symbols: session
                .map(|item| {
                    item.focus_symbols
                        .iter()
                        .map(|symbol| symbol_object_ref(snapshot.market, snapshot.tick, symbol))
                        .collect()
                })
                .unwrap_or_else(|| {
                    snapshot
                        .wake
                        .focus_symbols
                        .iter()
                        .map(|symbol| symbol_object_ref(snapshot.market, snapshot.tick, symbol))
                        .collect()
                }),
        },
        focus_symbol_refs: session
            .map(|item| {
                item.focus_symbols
                    .iter()
                    .map(|symbol| symbol_object_ref(snapshot.market, snapshot.tick, symbol))
                    .collect()
            })
            .unwrap_or_else(|| {
                snapshot
                    .wake
                    .focus_symbols
                    .iter()
                    .map(|symbol| symbol_object_ref(snapshot.market, snapshot.tick, symbol))
                    .collect()
            }),
    })
}

pub fn build_symbol_state_contract(
    snapshot: &AgentSnapshot,
    state: &AgentSymbolState,
) -> Result<SymbolStateContract, String> {
    let observed_at = parse_timestamp(&snapshot.timestamp)?;
    Ok(SymbolStateContract {
        id: SymbolStateId(format!(
            "symbol_state:{}:{}:{}",
            market_slug(snapshot.market),
            normalized_symbol_id(&state.symbol),
            snapshot.tick
        )),
        market: snapshot.market,
        source_tick: snapshot.tick,
        observed_at,
        symbol: state.symbol.clone(),
        sector: state.sector.clone(),
        navigation: OperationalNavigation::default(),
        relationships: SymbolStateRelationships::default(),
        summary: SymbolStateSummary {
            symbol: state.symbol.clone(),
            sector: state.sector.clone(),
            structure_action: state.structure.as_ref().map(|item| item.action.clone()),
            structure_status: state
                .structure
                .as_ref()
                .and_then(|item| item.status.clone()),
            signal_composite: state.signal.as_ref().map(|item| item.composite),
            has_depth: state.depth.is_some(),
            has_brokers: state.brokers.is_some(),
            invalidated: state
                .invalidation
                .as_ref()
                .map(|item| item.invalidated)
                .unwrap_or(false),
            leading_falsifier: state
                .invalidation
                .as_ref()
                .and_then(|item| item.leading_falsifier.clone()),
            latest_event_count: state.latest_events.len(),
        },
        graph_ref: symbol_graph_ref(snapshot.market, &state.symbol),
        state: state.clone(),
    })
}

pub fn build_macro_event_contracts(
    snapshot: &AgentSnapshot,
) -> Result<Vec<MacroEventContract>, String> {
    let observed_at = parse_timestamp(&snapshot.timestamp)?;
    Ok(snapshot
        .macro_events
        .iter()
        .cloned()
        .map(|event| MacroEventContract {
            id: MacroEventContractId(event.event_id.clone()),
            market: snapshot.market,
            source_tick: snapshot.tick,
            observed_at,
            navigation: OperationalNavigation::default(),
            relationships: MacroEventRelationships::default(),
            summary: MacroEventSummary {
                headline: event.headline.clone(),
                event_type: event.event_type.clone(),
                authority_level: event.authority_level.clone(),
                confidence: event.confidence,
                confirmation_state: event.confirmation_state.clone(),
                primary_scope: event.impact.primary_scope.clone(),
                preferred_expression: event.impact.preferred_expression.clone(),
                affected_symbol_count: event.impact.affected_symbols.len(),
                affected_sector_count: event.impact.affected_sectors.len(),
            },
            graph_ref: macro_event_graph_ref(snapshot.market, &event.event_id),
            event,
        })
        .collect())
}

fn populate_operational_relationships(snapshot: &mut OperationalSnapshot) {
    for symbol in &mut snapshot.symbols {
        symbol.relationships.cases = snapshot
            .cases
            .iter()
            .filter(|item| item.symbol.eq_ignore_ascii_case(&symbol.symbol))
            .map(|item| case_self_ref(snapshot.market, item))
            .collect();
        symbol.relationships.recommendations = snapshot
            .recommendations
            .iter()
            .filter(|item| item.symbol.eq_ignore_ascii_case(&symbol.symbol))
            .map(|item| recommendation_self_ref(snapshot.market, item))
            .collect();
        symbol.relationships.macro_events = snapshot
            .macro_events
            .iter()
            .filter(|item| {
                item.event
                    .impact
                    .affected_symbols
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(&symbol.symbol))
            })
            .map(|item| macro_event_self_ref(snapshot.market, item))
            .collect();
    }

    let symbol_refs = snapshot
        .symbols
        .iter()
        .map(|item| {
            (
                item.symbol.to_ascii_lowercase(),
                OperationalObjectRef {
                    id: item.id.0.clone(),
                    kind: OperationalObjectKind::SymbolState,
                    endpoint: object_endpoint(snapshot.market, &format!("symbols/{}", item.symbol)),
                    label: Some(item.symbol.clone()),
                },
            )
        })
        .collect::<std::collections::HashMap<_, _>>();
    let case_refs = snapshot
        .cases
        .iter()
        .map(|item| {
            (
                item.symbol.to_ascii_lowercase(),
                case_self_ref(snapshot.market, item),
            )
        })
        .collect::<Vec<_>>();
    let recommendation_refs = snapshot
        .recommendations
        .iter()
        .map(|item| {
            (
                item.symbol.to_ascii_lowercase(),
                recommendation_self_ref(snapshot.market, item),
            )
        })
        .collect::<Vec<_>>();

    for event in &mut snapshot.macro_events {
        event.relationships.symbols = event
            .event
            .impact
            .affected_symbols
            .iter()
            .filter_map(|symbol| symbol_refs.get(&symbol.to_ascii_lowercase()).cloned())
            .collect();
        event.relationships.cases = case_refs
            .iter()
            .filter(|(symbol, _)| {
                event
                    .event
                    .impact
                    .affected_symbols
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(symbol))
            })
            .map(|(_, item)| item.clone())
            .collect();
        event.relationships.recommendations = recommendation_refs
            .iter()
            .filter(|(symbol, _)| {
                event
                    .event
                    .impact
                    .affected_symbols
                    .iter()
                    .any(|candidate| candidate.eq_ignore_ascii_case(symbol))
            })
            .map(|(_, item)| item.clone())
            .collect();
    }
}

fn populate_operational_navigation(snapshot: &mut OperationalSnapshot) {
    snapshot.market_session.navigation = OperationalNavigation {
        self_ref: Some(snapshot.market_session_ref()),
        graph: None,
        history: vec![],
        relationships: vec![OperationalRelationshipGroup {
            name: "focus_symbols".into(),
            refs: snapshot.market_session.relationships.focus_symbols.clone(),
        }],
        neighborhood_endpoint: Some(neighborhood_endpoint(
            snapshot.market,
            OperationalObjectKind::MarketSession,
            &snapshot.market_session.id.0,
        )),
    };

    for symbol in &mut snapshot.symbols {
        symbol.navigation = OperationalNavigation {
            self_ref: Some(OperationalObjectRef {
                id: symbol.id.0.clone(),
                kind: OperationalObjectKind::SymbolState,
                endpoint: object_endpoint(snapshot.market, &format!("symbols/{}", symbol.symbol)),
                label: Some(symbol.symbol.clone()),
            }),
            graph: Some(symbol.graph_ref.clone()),
            history: vec![],
            relationships: vec![
                OperationalRelationshipGroup {
                    name: "cases".into(),
                    refs: symbol.relationships.cases.clone(),
                },
                OperationalRelationshipGroup {
                    name: "recommendations".into(),
                    refs: symbol.relationships.recommendations.clone(),
                },
                OperationalRelationshipGroup {
                    name: "macro_events".into(),
                    refs: symbol.relationships.macro_events.clone(),
                },
            ],
            neighborhood_endpoint: Some(neighborhood_endpoint(
                snapshot.market,
                OperationalObjectKind::SymbolState,
                &symbol.id.0,
            )),
        };
    }

    for case in &mut snapshot.cases {
        case.navigation = OperationalNavigation {
            self_ref: Some(case_self_ref(snapshot.market, case)),
            graph: Some(case.graph_ref.clone()),
            history: collect_case_history_refs(case),
            relationships: vec![
                OperationalRelationshipGroup {
                    name: "symbol".into(),
                    refs: vec![case.relationships.symbol.clone()],
                },
                OperationalRelationshipGroup {
                    name: "workflow".into(),
                    refs: case.relationships.workflow.clone().into_iter().collect(),
                },
                OperationalRelationshipGroup {
                    name: "recommendations".into(),
                    refs: case.relationships.recommendations.clone(),
                },
            ],
            neighborhood_endpoint: Some(neighborhood_endpoint(
                snapshot.market,
                OperationalObjectKind::Case,
                &case.id.0,
            )),
        };
    }

    for recommendation in &mut snapshot.recommendations {
        recommendation.navigation = OperationalNavigation {
            self_ref: Some(recommendation_self_ref(snapshot.market, recommendation)),
            graph: Some(recommendation.graph_ref.clone()),
            history: collect_recommendation_history_refs(recommendation),
            relationships: vec![
                OperationalRelationshipGroup {
                    name: "symbol".into(),
                    refs: vec![recommendation.relationships.symbol.clone()],
                },
                OperationalRelationshipGroup {
                    name: "case".into(),
                    refs: recommendation
                        .relationships
                        .case
                        .clone()
                        .into_iter()
                        .collect(),
                },
                OperationalRelationshipGroup {
                    name: "workflow".into(),
                    refs: recommendation
                        .relationships
                        .workflow
                        .clone()
                        .into_iter()
                        .collect(),
                },
            ],
            neighborhood_endpoint: Some(neighborhood_endpoint(
                snapshot.market,
                OperationalObjectKind::Recommendation,
                &recommendation.id.0,
            )),
        };
    }

    for macro_event in &mut snapshot.macro_events {
        macro_event.navigation = OperationalNavigation {
            self_ref: Some(macro_event_self_ref(snapshot.market, macro_event)),
            graph: Some(macro_event.graph_ref.clone()),
            history: vec![],
            relationships: vec![
                OperationalRelationshipGroup {
                    name: "symbols".into(),
                    refs: macro_event.relationships.symbols.clone(),
                },
                OperationalRelationshipGroup {
                    name: "cases".into(),
                    refs: macro_event.relationships.cases.clone(),
                },
                OperationalRelationshipGroup {
                    name: "recommendations".into(),
                    refs: macro_event.relationships.recommendations.clone(),
                },
            ],
            neighborhood_endpoint: Some(neighborhood_endpoint(
                snapshot.market,
                OperationalObjectKind::MacroEvent,
                &macro_event.id.0,
            )),
        };
    }

    for thread in &mut snapshot.threads {
        thread.navigation = OperationalNavigation {
            self_ref: Some(OperationalObjectRef {
                id: thread.id.0.clone(),
                kind: OperationalObjectKind::Thread,
                endpoint: object_endpoint(snapshot.market, &format!("threads/{}", thread.id.0)),
                label: thread
                    .thread
                    .title
                    .clone()
                    .or_else(|| Some(thread.thread.symbol.clone())),
            }),
            graph: None,
            history: vec![],
            relationships: vec![],
            neighborhood_endpoint: Some(neighborhood_endpoint(
                snapshot.market,
                OperationalObjectKind::Thread,
                &thread.id.0,
            )),
        };
    }

    for workflow in &mut snapshot.workflows {
        workflow.navigation = OperationalNavigation {
            self_ref: Some(workflow_self_ref(snapshot.market, workflow)),
            graph: None,
            history: collect_workflow_history_refs(workflow),
            relationships: vec![
                OperationalRelationshipGroup {
                    name: "cases".into(),
                    refs: workflow.relationships.cases.clone(),
                },
                OperationalRelationshipGroup {
                    name: "recommendations".into(),
                    refs: workflow.relationships.recommendations.clone(),
                },
            ],
            neighborhood_endpoint: Some(neighborhood_endpoint(
                snapshot.market,
                OperationalObjectKind::Workflow,
                &workflow.id.0,
            )),
        };
    }
}

pub fn build_operational_snapshot(
    live_snapshot: &LiveSnapshot,
    snapshot: &AgentSnapshot,
    session: &AgentSession,
    recommendations: &AgentRecommendations,
    narration: Option<&AgentNarration>,
) -> Result<OperationalSnapshot, String> {
    let observed_at = parse_timestamp(&snapshot.timestamp)?;
    let computed_at = observed_at;

    let market_session = build_market_session_contract(snapshot, Some(session), narration)?;
    let live_cases_by_setup = live_snapshot
        .tactical_cases
        .iter()
        .map(|item| (item.setup_id.as_str(), item))
        .collect::<HashMap<_, _>>();

    let symbols = snapshot
        .symbols
        .iter()
        .map(|state| build_symbol_state_contract(snapshot, state))
        .collect::<Result<Vec<_>, _>>()?;

    let case_summaries = build_case_summaries(live_snapshot);
    let recommendation_links =
        link_recommendations_to_cases(&case_summaries, &recommendations.items);
    let cases = case_summaries
        .iter()
        .map(|item| {
            let live_case = live_cases_by_setup.get(item.setup_id.as_str()).copied();
            CaseContract {
                id: CaseContractId(item.case_id.clone()),
                setup_id: item.setup_id.clone(),
                market: item.market,
                source_tick: snapshot.tick,
                observed_at,
                symbol: item.symbol.clone(),
                sector: item.sector.clone(),
                title: item.title.clone(),
                action: item.recommended_action.clone(),
                workflow_state: item.workflow_state.clone(),
                workflow_id: Some(case_workflow_key(item)),
                execution_policy: item.execution_policy,
                governance_reason_code: item.governance_reason_code,
                governance_reason: item.governance_reason.clone(),
                owner: item.owner.clone(),
                reviewer: item.reviewer.clone(),
                queue_pin: item.queue_pin.clone(),
                confidence: item.confidence,
                confidence_gap: Some(item.confidence_gap),
                thesis_family: item.family_label.clone(),
                current_leader: item.current_leader.clone(),
                invalidation_rule: item.invalidation_rules.first().cloned(),
                expected_net_alpha: None,
                alpha_horizon: None,
                policy_primary: live_case.and_then(|entry| entry.policy_primary.clone()),
                policy_reason: live_case.and_then(|entry| entry.policy_reason.clone()),
                multi_horizon_gate_reason: live_case
                    .and_then(|entry| entry.multi_horizon_gate_reason.clone()),
                matched_success_pattern_signature: live_case
                    .and_then(|entry| entry.matched_success_pattern_signature.clone()),
                recommendation_ids: recommendation_links
                    .iter()
                    .filter(|(_, case_id, _, _)| case_id.as_deref() == Some(item.case_id.as_str()))
                    .map(|(rec_id, _, _, _)| rec_id.clone())
                    .collect(),
                navigation: OperationalNavigation::default(),
                relationships: CaseRelationships {
                    symbol: symbol_object_ref(item.market, snapshot.tick, &item.symbol),
                    workflow: item
                        .workflow_id
                        .as_deref()
                        .map(|workflow_id| workflow_object_ref(item.market, workflow_id, None)),
                    recommendations: recommendation_links
                        .iter()
                        .filter(|(_, case_id, _, _)| {
                            case_id.as_deref() == Some(item.case_id.as_str())
                        })
                        .map(|(rec_id, _, _, _)| {
                            recommendation_object_ref(
                                item.market,
                                rec_id,
                                Some(format!("{} {}", item.symbol, item.recommended_action)),
                            )
                        })
                        .collect(),
                },
                symbol_ref: symbol_object_ref(item.market, snapshot.tick, &item.symbol),
                workflow_ref: item
                    .workflow_id
                    .as_deref()
                    .map(|workflow_id| workflow_object_ref(item.market, workflow_id, None)),
                recommendation_refs: recommendation_links
                    .iter()
                    .filter(|(_, case_id, _, _)| case_id.as_deref() == Some(item.case_id.as_str()))
                    .map(|(rec_id, _, _, _)| {
                        recommendation_object_ref(
                            item.market,
                            rec_id,
                            Some(format!("{} {}", item.symbol, item.recommended_action)),
                        )
                    })
                    .collect(),
                graph_ref: setup_graph_ref(item.market, &item.setup_id),
                history_refs: case_history_refs(
                    item.market,
                    &item.case_id,
                    &item.setup_id,
                    item.workflow_id.as_deref(),
                ),
            }
        })
        .collect::<Vec<_>>();

    let recommendation_contracts = recommendations
        .items
        .iter()
        .cloned()
        .map(|item| {
            let linkage = recommendation_links
                .iter()
                .find(|(rec_id, _, _, _)| rec_id == &item.recommendation_id);
            RecommendationContract {
                id: RecommendationContractId(item.recommendation_id.clone()),
                market: snapshot.market,
                source_tick: snapshot.tick,
                observed_at,
                symbol: item.symbol.clone(),
                related_case_id: linkage.and_then(|(_, case_id, _, _)| case_id.clone()),
                related_setup_id: linkage.and_then(|(_, _, setup_id, _)| setup_id.clone()),
                related_workflow_id: linkage.and_then(|(_, _, _, workflow_id)| workflow_id.clone()),
                navigation: OperationalNavigation::default(),
                relationships: RecommendationRelationships {
                    symbol: symbol_object_ref(snapshot.market, snapshot.tick, &item.symbol),
                    case: linkage.and_then(|(_, case_id, _, _)| {
                        case_id.as_ref().map(|case_id| {
                            case_object_ref(snapshot.market, case_id, item.title.clone())
                        })
                    }),
                    workflow: linkage.and_then(|(_, _, _, workflow_id)| {
                        workflow_id.as_ref().map(|workflow_id| {
                            workflow_object_ref(snapshot.market, workflow_id, None)
                        })
                    }),
                },
                summary: RecommendationSummary {
                    action: item.action.clone(),
                    bias: item.bias.clone(),
                    severity: item.severity.clone(),
                    confidence: item.confidence,
                    best_action: item.best_action.clone(),
                    primary_lens: item.primary_lens.clone(),
                    matched_success_pattern_signature: item
                        .matched_success_pattern_signature
                        .clone(),
                    execution_policy: item.execution_policy,
                    governance_reason_code: item.governance_reason_code,
                },
                symbol_ref: symbol_object_ref(snapshot.market, snapshot.tick, &item.symbol),
                case_ref: linkage.and_then(|(_, case_id, _, _)| {
                    case_id.as_ref().map(|case_id| {
                        case_object_ref(snapshot.market, case_id, item.title.clone())
                    })
                }),
                workflow_ref: linkage.and_then(|(_, _, _, workflow_id)| {
                    workflow_id
                        .as_ref()
                        .map(|workflow_id| workflow_object_ref(snapshot.market, workflow_id, None))
                }),
                graph_ref: decision_graph_ref(snapshot.market, &item.recommendation_id),
                history_refs: recommendation_history_refs(
                    snapshot.market,
                    &item.recommendation_id,
                    linkage.and_then(|(_, case_id, _, _)| case_id.as_deref()),
                    linkage.and_then(|(_, _, _, workflow_id)| workflow_id.as_deref()),
                ),
                recommendation: item,
            }
        })
        .collect::<Vec<_>>();

    let macro_events = build_macro_event_contracts(snapshot)?;

    let threads = session
        .active_threads
        .iter()
        .cloned()
        .map(|thread| ThreadContract {
            id: ThreadContractId(thread_contract_id(snapshot.market, &thread.symbol)),
            market: snapshot.market,
            source_tick: snapshot.tick,
            observed_at,
            navigation: OperationalNavigation::default(),
            thread,
        })
        .collect::<Vec<_>>();

    let workflows = build_workflow_contracts(
        snapshot.market,
        snapshot.tick,
        observed_at,
        &cases,
        &recommendation_contracts,
    );
    let operator_work_items = materialize_operator_work_items(&market_session, session, &cases);
    let sidecars = OperationalSidecars {
        sector_flows: snapshot.sector_flows.clone(),
        backward_investigations: snapshot
            .backward_reasoning
            .as_ref()
            .map(|item| item.investigations.clone())
            .unwrap_or_default(),
        world_state: snapshot.world_state.clone(),
        macro_event_candidates: snapshot.macro_event_candidates.clone(),
        knowledge_links: combined_knowledge_links(snapshot, recommendations),
        operator_workflows: Vec::new(),
        operator_work_items,
    };

    let mut operational = OperationalSnapshot {
        version: 1,
        market: snapshot.market,
        source_tick: snapshot.tick,
        observed_at,
        computed_at,
        market_session,
        recent_turns: session.recent_turns.clone(),
        notices: snapshot.notices.clone(),
        recent_transitions: snapshot.recent_transitions.clone(),
        symbols,
        cases,
        market_recommendation: recommendations.market_recommendation.clone(),
        sector_recommendations: recommendations
            .decisions
            .iter()
            .filter_map(|item| {
                if let AgentDecision::Sector(item) = item {
                    Some(item.clone())
                } else {
                    None
                }
            })
            .collect(),
        recommendations: recommendation_contracts,
        macro_events,
        threads,
        workflows,
        sidecars,
        events: snapshot.events.clone(),
        temporal_bars: live_snapshot.temporal_bars.clone(),
        lineage: live_snapshot.lineage.clone(),
        success_patterns: live_snapshot.success_patterns.clone(),
    };
    populate_operational_relationships(&mut operational);
    populate_operational_navigation(&mut operational);
    populate_operator_work_item_navigation(&mut operational);
    Ok(operational)
}

pub fn rebuild_operational_case_graph(snapshot: &mut OperationalSnapshot, cases: &[CaseSummary]) {
    let previous_cases = snapshot
        .cases
        .iter()
        .map(|item| (item.setup_id.clone(), item.clone()))
        .collect::<HashMap<_, _>>();
    let recommendation_links = link_recommendations_to_cases(
        cases,
        &snapshot
            .recommendations
            .iter()
            .map(|item| item.recommendation.clone())
            .collect::<Vec<_>>(),
    );

    snapshot.cases = cases
        .iter()
        .map(|item| {
            let previous = previous_cases.get(&item.setup_id);
            CaseContract {
                id: CaseContractId(item.case_id.clone()),
                setup_id: item.setup_id.clone(),
                market: item.market,
                source_tick: snapshot.source_tick,
                observed_at: snapshot.observed_at,
                symbol: item.symbol.clone(),
                sector: item.sector.clone(),
                title: item.title.clone(),
                action: item.recommended_action.clone(),
                workflow_state: item.workflow_state.clone(),
                workflow_id: Some(case_workflow_key(item)),
                execution_policy: item.execution_policy,
                governance_reason_code: item.governance_reason_code,
                governance_reason: item.governance_reason.clone(),
                owner: item.owner.clone(),
                reviewer: item.reviewer.clone(),
                queue_pin: item.queue_pin.clone(),
                confidence: item.confidence,
                confidence_gap: Some(item.confidence_gap),
                thesis_family: item.family_label.clone(),
                current_leader: item.current_leader.clone(),
                invalidation_rule: item.invalidation_rules.first().cloned(),
                expected_net_alpha: None,
                alpha_horizon: None,
                policy_primary: previous.and_then(|entry| entry.policy_primary.clone()),
                policy_reason: previous.and_then(|entry| entry.policy_reason.clone()),
                multi_horizon_gate_reason: previous
                    .and_then(|entry| entry.multi_horizon_gate_reason.clone()),
                matched_success_pattern_signature: previous
                    .and_then(|entry| entry.matched_success_pattern_signature.clone()),
                recommendation_ids: recommendation_links
                    .iter()
                    .filter(|(_, case_id, _, _)| case_id.as_deref() == Some(item.case_id.as_str()))
                    .map(|(rec_id, _, _, _)| rec_id.clone())
                    .collect(),
                navigation: OperationalNavigation::default(),
                relationships: CaseRelationships {
                    symbol: symbol_object_ref(item.market, snapshot.source_tick, &item.symbol),
                    workflow: item
                        .workflow_id
                        .as_deref()
                        .map(|workflow_id| workflow_object_ref(item.market, workflow_id, None)),
                    recommendations: recommendation_links
                        .iter()
                        .filter(|(_, case_id, _, _)| {
                            case_id.as_deref() == Some(item.case_id.as_str())
                        })
                        .map(|(rec_id, _, _, _)| {
                            recommendation_object_ref(
                                item.market,
                                rec_id,
                                Some(format!("{} {}", item.symbol, item.recommended_action)),
                            )
                        })
                        .collect(),
                },
                symbol_ref: symbol_object_ref(item.market, snapshot.source_tick, &item.symbol),
                workflow_ref: item
                    .workflow_id
                    .as_deref()
                    .map(|workflow_id| workflow_object_ref(item.market, workflow_id, None)),
                recommendation_refs: recommendation_links
                    .iter()
                    .filter(|(_, case_id, _, _)| case_id.as_deref() == Some(item.case_id.as_str()))
                    .map(|(rec_id, _, _, _)| {
                        recommendation_object_ref(
                            item.market,
                            rec_id,
                            Some(format!("{} {}", item.symbol, item.recommended_action)),
                        )
                    })
                    .collect(),
                graph_ref: setup_graph_ref(item.market, &item.setup_id),
                history_refs: case_history_refs(
                    item.market,
                    &item.case_id,
                    &item.setup_id,
                    item.workflow_id.as_deref(),
                ),
            }
        })
        .collect();

    for recommendation in &mut snapshot.recommendations {
        if let Some((_, case_id, setup_id, workflow_id)) = recommendation_links
            .iter()
            .find(|(rec_id, _, _, _)| rec_id == &recommendation.id.0)
        {
            recommendation.related_case_id = case_id.clone();
            recommendation.related_setup_id = setup_id.clone();
            recommendation.related_workflow_id = workflow_id.clone();
            recommendation.relationships.case = case_id.as_ref().map(|case_id| {
                case_object_ref(
                    snapshot.market,
                    case_id,
                    recommendation.recommendation.title.clone(),
                )
            });
            recommendation.relationships.workflow = workflow_id
                .as_ref()
                .map(|workflow_id| workflow_object_ref(snapshot.market, workflow_id, None));
            recommendation.case_ref = case_id.as_ref().map(|case_id| {
                case_object_ref(
                    snapshot.market,
                    case_id,
                    recommendation.recommendation.title.clone(),
                )
            });
            recommendation.workflow_ref = workflow_id
                .as_ref()
                .map(|workflow_id| workflow_object_ref(snapshot.market, workflow_id, None));
            recommendation.history_refs = recommendation_history_refs(
                snapshot.market,
                &recommendation.id.0,
                case_id.as_deref(),
                workflow_id.as_deref(),
            );
        }
    }

    snapshot.workflows = build_workflow_contracts(
        snapshot.market,
        snapshot.source_tick,
        snapshot.observed_at,
        &snapshot.cases,
        &snapshot.recommendations,
    );
    populate_operational_relationships(snapshot);
    populate_operational_navigation(snapshot);
    populate_operator_work_item_navigation(snapshot);
}
