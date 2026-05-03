use super::*;
use crate::pipeline::perception::build_world_state_snapshot;

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

fn effective_recommendations_for_contracts(
    snapshot: &AgentSnapshot,
    recommendations: &AgentRecommendations,
    cases: &[CaseSummary],
) -> AgentRecommendations {
    let mut effective = recommendations.clone();
    effective.tick = snapshot.tick;
    effective.timestamp = snapshot.timestamp.clone();
    effective.market = snapshot.market;
    effective.regime_bias = snapshot.market_regime.bias.clone();

    if effective.items.is_empty() && !cases.is_empty() {
        effective.items = cases
            .iter()
            .map(|case| agent_recommendation_from_case(snapshot, case))
            .collect();
        effective
            .decisions
            .retain(|decision| !matches!(decision, AgentDecision::Symbol(_)));
        effective
            .decisions
            .extend(effective.items.iter().cloned().map(AgentDecision::Symbol));
    } else if effective.decisions.is_empty() && !effective.items.is_empty() {
        effective.decisions = effective
            .items
            .iter()
            .cloned()
            .map(AgentDecision::Symbol)
            .collect();
    }

    effective.total = effective.decisions.len();
    effective
}

fn agent_recommendation_from_case(
    snapshot: &AgentSnapshot,
    case: &CaseSummary,
) -> AgentRecommendation {
    let action = case.recommended_action.clone();
    let best_action = case_best_action(&action);
    let horizon_ticks = case_horizon_ticks(&action);
    let expected_net_alpha = case_expected_net_alpha(case, &best_action);
    let mut action_expectancies = AgentActionExpectancies {
        wait_expectancy: Some(Decimal::ZERO),
        ..AgentActionExpectancies::default()
    };
    match best_action.as_str() {
        "follow" => action_expectancies.follow_expectancy = expected_net_alpha,
        "fade" => action_expectancies.fade_expectancy = expected_net_alpha,
        _ => {}
    }

    let policy = case
        .execution_policy
        .unwrap_or_else(|| case_execution_policy(&action));
    let governance = case
        .governance
        .clone()
        .unwrap_or_else(|| ActionGovernanceContract::for_recommendation(policy));
    let severity = case_severity(case, policy);
    let governance_reason_code = case
        .governance_reason_code
        .unwrap_or_else(|| default_case_governance_reason_code(policy, &best_action, &severity));
    let governance_reason = case.governance_reason.clone().unwrap_or_else(|| {
        format!(
            "policy={} governs operational case {}",
            policy, case.setup_id
        )
    });
    let decisive_factors = case_decisive_factors(case);
    let invalidation_rule = case.invalidation_rules.first().cloned();

    AgentRecommendation {
        recommendation_id: format!(
            "rec:{}:{}:{}",
            snapshot.tick,
            normalized_symbol_id(&case.symbol),
            recommendation_id_fragment(&case.setup_id)
        ),
        tick: snapshot.tick,
        symbol: case.symbol.clone(),
        sector: case.sector.clone(),
        title: Some(case.title.clone()),
        action,
        action_label: None,
        bias: case_bias(case, &best_action),
        severity,
        confidence: case.confidence,
        score: case_score(case),
        horizon_ticks,
        regime_bias: case.market_regime_bias.clone(),
        status: case_status(case),
        why: if case.why_now.trim().is_empty() {
            case.title.clone()
        } else {
            case.why_now.clone()
        },
        why_components: case_why_components(case),
        primary_lens: case.primary_lens.clone(),
        supporting_lenses: case_supporting_lenses(case),
        review_lens: matches!(policy, ActionExecutionPolicy::ReviewRequired)
            .then(|| case.primary_lens.clone())
            .flatten(),
        watch_next: case_watch_next(case),
        do_not: case_do_not(case),
        fragility: case_fragility(case),
        transition: case_transition(case),
        thesis_family: case.family_label.clone(),
        matched_success_pattern_signature: None,
        state_transition: case.state_reason_codes.first().cloned(),
        best_action,
        action_expectancies: action_expectancies.clone(),
        decision_attribution: AgentDecisionAttribution {
            historical_expectancies: AgentActionExpectancies {
                wait_expectancy: Some(Decimal::ZERO),
                ..AgentActionExpectancies::default()
            },
            live_expectancy_shift: expected_net_alpha.unwrap_or(Decimal::ZERO),
            decisive_factors,
        },
        expected_net_alpha,
        alpha_horizon: format!("intraday:{}t", horizon_ticks),
        price_at_decision: None,
        resolution: None,
        invalidation_rule,
        invalidation_components: case_invalidation_components(case),
        execution_policy: governance.execution_policy,
        governance,
        governance_reason_code,
        governance_reason,
    }
}

fn recommendation_id_fragment(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else if !out.ends_with('_') {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}

fn case_best_action(action: &str) -> String {
    match action {
        "enter" | "add" => "follow",
        "trim" | "hedge" | "exit" | "reduce" => "fade",
        _ => "wait",
    }
    .into()
}

fn case_horizon_ticks(action: &str) -> u64 {
    match action {
        "enter" | "add" => 15,
        "trim" | "hedge" | "exit" | "reduce" => 12,
        "review" => 10,
        "watch" => 8,
        _ => 6,
    }
}

fn case_execution_policy(action: &str) -> ActionExecutionPolicy {
    match action {
        "enter" | "add" => ActionExecutionPolicy::ReviewRequired,
        _ => ActionExecutionPolicy::ManualOnly,
    }
}

fn case_expected_net_alpha(case: &CaseSummary, best_action: &str) -> Option<Decimal> {
    if best_action == "wait" {
        return None;
    }
    let mut alpha = case.heuristic_edge.abs();
    if alpha <= Decimal::ZERO {
        alpha = case.confidence_gap.abs();
    }
    if alpha <= Decimal::ZERO {
        alpha = case.actionability_score.unwrap_or(Decimal::ZERO).abs();
    }
    (alpha > Decimal::ZERO).then_some(alpha.round_dp(4))
}

fn case_severity(case: &CaseSummary, policy: ActionExecutionPolicy) -> String {
    if case.priority_rank.is_some_and(|rank| rank <= 2) || case.confidence >= Decimal::new(8, 1) {
        "critical"
    } else if matches!(policy, ActionExecutionPolicy::ReviewRequired)
        || case.confidence >= Decimal::new(65, 2)
    {
        "high"
    } else if case.confidence >= Decimal::new(4, 1) {
        "medium"
    } else {
        "normal"
    }
    .into()
}

fn case_bias(case: &CaseSummary, best_action: &str) -> String {
    match (best_action, case.heuristic_edge.cmp(&Decimal::ZERO)) {
        ("follow", std::cmp::Ordering::Less) => "short",
        ("follow", _) => "long",
        ("fade", std::cmp::Ordering::Less) => "long",
        ("fade", _) => "short",
        _ => "neutral",
    }
    .into()
}

fn case_score(case: &CaseSummary) -> Decimal {
    let actionability = case.actionability_score.unwrap_or(Decimal::ZERO);
    (case.confidence + case.heuristic_edge.abs() + actionability).round_dp(4)
}

fn case_status(case: &CaseSummary) -> Option<String> {
    case.actionability_state
        .clone()
        .or_else(|| case.freshness_state.clone())
        .or_else(|| case.local_state.clone())
        .or_else(|| Some(case.workflow_state.clone()))
}

fn push_unique_line(items: &mut Vec<String>, value: impl Into<String>) {
    let value = value.into();
    if value.trim().is_empty() || items.iter().any(|item| item == &value) {
        return;
    }
    items.push(value);
}

fn case_decisive_factors(case: &CaseSummary) -> Vec<String> {
    let mut factors = Vec::new();
    for evidence in case.key_evidence.iter().take(3) {
        push_unique_line(&mut factors, evidence.description.clone());
    }
    for reason in case.state_reason_codes.iter().take(3) {
        push_unique_line(&mut factors, reason.clone());
    }
    if let Some(driver) = case.primary_driver.as_ref() {
        push_unique_line(&mut factors, format!("primary_driver={driver}"));
    }
    if let Some(state) = case.timing_state.as_ref() {
        push_unique_line(&mut factors, format!("timing_state={state}"));
    }
    factors.truncate(5);
    factors
}

fn case_why_components(case: &CaseSummary) -> Vec<AgentLensComponent> {
    case.key_evidence
        .iter()
        .take(4)
        .map(|evidence| AgentLensComponent {
            lens_name: case
                .primary_lens
                .clone()
                .unwrap_or_else(|| "case_evidence".into()),
            confidence: evidence.weight.abs().min(Decimal::ONE),
            content: evidence.description.clone(),
            tags: vec![format!("direction={}", evidence.direction.round_dp(2))],
        })
        .collect()
}

fn case_supporting_lenses(case: &CaseSummary) -> Vec<String> {
    let mut lenses = Vec::new();
    if let Some(driver) = case.primary_driver.as_ref() {
        push_unique_line(&mut lenses, driver.clone());
    }
    if let Some(family) = case.family_label.as_ref() {
        push_unique_line(&mut lenses, family.clone());
    }
    if let Some(leader) = case.current_leader.as_ref() {
        push_unique_line(&mut lenses, format!("leader={leader}"));
    }
    lenses.truncate(3);
    lenses
}

fn case_watch_next(case: &CaseSummary) -> Vec<String> {
    let mut items = Vec::new();
    if let Some(summary) = case.competition_summary.as_ref() {
        push_unique_line(&mut items, summary.clone());
    }
    if let Some(summary) = case.absence_summary.as_ref() {
        push_unique_line(&mut items, summary.clone());
    }
    for reason in case.state_reason_codes.iter().take(3) {
        push_unique_line(&mut items, reason.clone());
    }
    items.truncate(4);
    items
}

fn case_do_not(case: &CaseSummary) -> Vec<String> {
    let mut items = case
        .invalidation_rules
        .iter()
        .take(3)
        .cloned()
        .collect::<Vec<_>>();
    if let Some(reason) = case.governance_reason.as_ref() {
        push_unique_line(&mut items, reason.clone());
    }
    items.truncate(4);
    items
}

fn case_fragility(case: &CaseSummary) -> Vec<String> {
    let mut items = case.review_reason_subreasons.clone();
    if let Some(reason) = case.review_reason_family.as_ref() {
        push_unique_line(&mut items, reason.clone());
    }
    if let Some(summary) = case.competition_summary.as_ref() {
        push_unique_line(&mut items, summary.clone());
    }
    if let Some(summary) = case.absence_summary.as_ref() {
        push_unique_line(&mut items, summary.clone());
    }
    items.truncate(4);
    items
}

fn case_transition(case: &CaseSummary) -> Option<String> {
    case.hypothesis_status
        .as_ref()
        .map(|status| format!("hypothesis_status={status}"))
        .or_else(|| {
            case.local_state
                .as_ref()
                .map(|state| format!("local_state={state}"))
        })
}

fn case_invalidation_components(case: &CaseSummary) -> Vec<AgentLensComponent> {
    case.invalidation_rules
        .iter()
        .take(3)
        .map(|rule| AgentLensComponent {
            lens_name: "invalidation".into(),
            confidence: case.confidence,
            content: rule.clone(),
            tags: vec!["operational_case".into()],
        })
        .collect()
}

fn default_case_governance_reason_code(
    policy: ActionExecutionPolicy,
    best_action: &str,
    severity: &str,
) -> ActionGovernanceReasonCode {
    match policy {
        ActionExecutionPolicy::ManualOnly => {
            if matches!(best_action, "wait" | "ignore" | "review" | "observe") {
                ActionGovernanceReasonCode::AdvisoryAction
            } else {
                ActionGovernanceReasonCode::OperatorActionRequired
            }
        }
        ActionExecutionPolicy::ReviewRequired => {
            if matches!(severity, "high" | "critical") {
                ActionGovernanceReasonCode::SeverityRequiresReview
            } else {
                ActionGovernanceReasonCode::OperatorActionRequired
            }
        }
        ActionExecutionPolicy::AutoEligible => ActionGovernanceReasonCode::AutoExecutionEligible,
    }
}

fn graph_node_endpoint(market: LiveMarket, node_id: &str) -> String {
    format!("/api/ontology/{}/graph/node/{node_id}", market_slug(market))
}

fn neighborhood_endpoint(market: LiveMarket, kind: OperationalObjectKind, id: &str) -> String {
    let kind = match kind {
        OperationalObjectKind::MarketSession => "market_session",
        OperationalObjectKind::SymbolState => "symbol_state",
        OperationalObjectKind::PerceptualState => "perceptual_state",
        OperationalObjectKind::PerceptualEvidence => "perceptual_evidence",
        OperationalObjectKind::PerceptualExpectation => "perceptual_expectation",
        OperationalObjectKind::AttentionAllocation => "attention_allocation",
        OperationalObjectKind::PerceptualUncertainty => "perceptual_uncertainty",
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

fn perceptual_state_object_ref(
    market: LiveMarket,
    symbol: &str,
    state: &crate::ontology::world::PerceptualState,
) -> OperationalObjectRef {
    OperationalObjectRef {
        id: state.state_id.clone(),
        kind: OperationalObjectKind::PerceptualState,
        endpoint: object_endpoint(market, &format!("symbols/{symbol}/perceptual-state")),
        label: Some(format!("{} {}", symbol, state.state_kind)),
    }
}

fn perceptual_evidence_object_ref(
    market: LiveMarket,
    symbol: &str,
    evidence: &crate::ontology::world::PerceptualEvidence,
) -> OperationalObjectRef {
    OperationalObjectRef {
        id: evidence.evidence_id.clone(),
        kind: OperationalObjectKind::PerceptualEvidence,
        endpoint: object_endpoint(
            market,
            &format!(
                "symbols/{symbol}/perceptual-evidence/{}",
                evidence.evidence_id
            ),
        ),
        label: Some(evidence.rationale.clone()),
    }
}

fn perceptual_expectation_object_ref(
    market: LiveMarket,
    symbol: &str,
    expectation: &crate::ontology::world::PerceptualExpectation,
) -> OperationalObjectRef {
    OperationalObjectRef {
        id: expectation.expectation_id.clone(),
        kind: OperationalObjectKind::PerceptualExpectation,
        endpoint: object_endpoint(
            market,
            &format!(
                "symbols/{symbol}/perceptual-expectations/{}",
                expectation.expectation_id
            ),
        ),
        label: Some(expectation.rationale.clone()),
    }
}

fn attention_allocation_object_ref(
    market: LiveMarket,
    symbol: &str,
    allocation: &crate::ontology::world::AttentionAllocation,
) -> OperationalObjectRef {
    OperationalObjectRef {
        id: allocation.allocation_id.clone(),
        kind: OperationalObjectKind::AttentionAllocation,
        endpoint: object_endpoint(
            market,
            &format!(
                "symbols/{symbol}/attention-allocations/{}",
                allocation.allocation_id
            ),
        ),
        label: Some(format!("{} {}", symbol, allocation.channel)),
    }
}

fn perceptual_uncertainty_object_ref(
    market: LiveMarket,
    symbol: &str,
    uncertainty: &crate::ontology::world::PerceptualUncertainty,
) -> OperationalObjectRef {
    OperationalObjectRef {
        id: uncertainty.uncertainty_id.clone(),
        kind: OperationalObjectKind::PerceptualUncertainty,
        endpoint: object_endpoint(
            market,
            &format!(
                "symbols/{symbol}/perceptual-uncertainties/{}",
                uncertainty.uncertainty_id
            ),
        ),
        label: Some(uncertainty.rationale.clone()),
    }
}

fn perceptual_state_graph_ref(market: LiveMarket, symbol: &str) -> OperationalGraphRef {
    symbol_graph_ref(market, symbol)
}

fn build_perceptual_contracts(
    snapshot: &AgentSnapshot,
    symbols: &[SymbolStateContract],
    observed_at: OffsetDateTime,
) -> (
    Vec<PerceptualStateContract>,
    Vec<PerceptualEvidenceContract>,
    Vec<PerceptualExpectationContract>,
    Vec<AttentionAllocationContract>,
    Vec<PerceptualUncertaintyContract>,
) {
    let perceptual_states = symbols
        .iter()
        .filter_map(|symbol| {
            symbol
                .perceptual_state
                .clone()
                .map(|state| PerceptualStateContract {
                    id: state.state_id.clone(),
                    market: snapshot.market,
                    source_tick: snapshot.tick,
                    observed_at,
                    symbol: symbol.symbol.clone(),
                    sector: symbol.sector.clone(),
                    navigation: OperationalNavigation::default(),
                    graph_ref: perceptual_state_graph_ref(snapshot.market, &symbol.symbol),
                    state,
                })
        })
        .collect::<Vec<_>>();

    let perceptual_evidence = perceptual_states
        .iter()
        .flat_map(|state| {
            state
                .state
                .supporting_evidence
                .iter()
                .chain(state.state.opposing_evidence.iter())
                .chain(state.state.missing_evidence.iter())
                .cloned()
                .map(|evidence| PerceptualEvidenceContract {
                    id: evidence.evidence_id.clone(),
                    market: snapshot.market,
                    source_tick: snapshot.tick,
                    observed_at,
                    symbol: state.symbol.clone(),
                    navigation: OperationalNavigation::default(),
                    graph_ref: state.graph_ref.clone(),
                    evidence,
                })
        })
        .collect::<Vec<_>>();

    let perceptual_expectations = perceptual_states
        .iter()
        .flat_map(|state| {
            state.state.expectations.iter().cloned().map(|expectation| {
                PerceptualExpectationContract {
                    id: expectation.expectation_id.clone(),
                    market: snapshot.market,
                    source_tick: snapshot.tick,
                    observed_at,
                    symbol: state.symbol.clone(),
                    navigation: OperationalNavigation::default(),
                    graph_ref: state.graph_ref.clone(),
                    expectation,
                }
            })
        })
        .collect::<Vec<_>>();

    let attention_allocations = perceptual_states
        .iter()
        .flat_map(|state| {
            state
                .state
                .attention_allocations
                .iter()
                .cloned()
                .map(|allocation| AttentionAllocationContract {
                    id: allocation.allocation_id.clone(),
                    market: snapshot.market,
                    source_tick: snapshot.tick,
                    observed_at,
                    symbol: state.symbol.clone(),
                    navigation: OperationalNavigation::default(),
                    graph_ref: state.graph_ref.clone(),
                    allocation,
                })
        })
        .collect::<Vec<_>>();

    let perceptual_uncertainties = perceptual_states
        .iter()
        .flat_map(|state| {
            state
                .state
                .uncertainties
                .iter()
                .cloned()
                .map(|uncertainty| PerceptualUncertaintyContract {
                    id: uncertainty.uncertainty_id.clone(),
                    market: snapshot.market,
                    source_tick: snapshot.tick,
                    observed_at,
                    symbol: state.symbol.clone(),
                    navigation: OperationalNavigation::default(),
                    graph_ref: state.graph_ref.clone(),
                    uncertainty,
                })
        })
        .collect::<Vec<_>>();

    (
        perceptual_states,
        perceptual_evidence,
        perceptual_expectations,
        attention_allocations,
        perceptual_uncertainties,
    )
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

fn live_case_signature(case: Option<&LiveTacticalCase>) -> Option<CaseSignature> {
    let case = case?;
    if let Some(signature) = case.case_signature.clone() {
        return Some(signature);
    }
    let mut active_channels = Vec::new();
    if case.entry_rationale.to_ascii_lowercase().contains("volume") {
        active_channels.push(CaseChannel::Volume);
    }
    if case.entry_rationale.to_ascii_lowercase().contains("flow") {
        active_channels.push(CaseChannel::CapitalFlow);
    }
    if case.entry_rationale.to_ascii_lowercase().contains("cross") {
        active_channels.push(CaseChannel::CrossMarket);
    }
    if case.tension_driver.is_some() {
        active_channels.push(CaseChannel::Propagation);
    }
    if case
        .matched_success_pattern_signature
        .as_deref()
        .is_some_and(|value| !value.is_empty())
    {
        active_channels.push(CaseChannel::MacroEvent);
    }
    active_channels.sort_by_key(|channel| *channel as u8);
    active_channels.dedup();

    let topology = if case.is_isolated == Some(true) {
        CaseTopology::Isolated
    } else if case
        .family_label
        .as_deref()
        .is_some_and(|label| label.to_ascii_lowercase().contains("cross"))
    {
        CaseTopology::CrossMarket
    } else {
        CaseTopology::SectorLinked
    };

    let temporal_shape = match case.lifecycle_phase.as_deref() {
        Some("Growing") => CaseTemporalShape::Persistent,
        Some("Peaking") => CaseTemporalShape::Burst,
        Some("Fading") => CaseTemporalShape::Reversal,
        Some("New") => CaseTemporalShape::Drift,
        _ => CaseTemporalShape::Unknown,
    };

    let conflict_shape = if case.is_isolated == Some(true) {
        ConflictShape::Contradictory
    } else if active_channels.len() >= 2 {
        ConflictShape::Aligned
    } else {
        ConflictShape::Unknown
    };

    Some(CaseSignature {
        active_channels,
        topology,
        temporal_shape,
        conflict_shape,
        expectation_support: usize::from(case.tension_driver.is_some()),
        expectation_violations: usize::from(case.is_isolated == Some(true)),
        novelty_score: if case.is_isolated == Some(true) {
            Decimal::new(7, 1)
        } else if case.tension_driver.is_some() {
            Decimal::new(5, 1)
        } else {
            Decimal::ZERO
        },
        notes: [
            case.lifecycle_phase
                .as_ref()
                .map(|value| format!("phase={value}")),
            case.tension_driver
                .as_ref()
                .map(|value| format!("driver={value}")),
        ]
        .into_iter()
        .flatten()
        .collect(),
    })
}

fn live_case_archetype_projections(case: Option<&LiveTacticalCase>) -> Vec<ArchetypeProjection> {
    let Some(case) = case else {
        return Vec::new();
    };
    if !case.archetype_projections.is_empty() {
        return case.archetype_projections.clone();
    }
    let mut projections = Vec::new();
    if let Some(label) = case.family_label.as_ref() {
        projections.push(ArchetypeProjection {
            archetype_key: label.to_ascii_lowercase().replace(' ', "_"),
            label: label.clone(),
            affinity: Decimal::new(6, 1),
            rationale: "projected from live tactical case family label".into(),
        });
    }
    if case.is_isolated == Some(true) {
        projections.push(ArchetypeProjection {
            archetype_key: "emergent".into(),
            label: "emergent pattern".into(),
            affinity: Decimal::new(7, 1),
            rationale: "isolated case behavior suggests an emergent, non-sector-linked pattern"
                .into(),
        });
    }
    projections
}

fn live_case_expectation_bindings(case: Option<&LiveTacticalCase>) -> Vec<ExpectationBinding> {
    case.map(|case| case.expectation_bindings.clone())
        .unwrap_or_default()
}

fn live_case_expectation_violations(case: Option<&LiveTacticalCase>) -> Vec<ExpectationViolation> {
    case.map(|case| case.expectation_violations.clone())
        .unwrap_or_default()
}

fn live_case_inferred_intent(case: Option<&LiveTacticalCase>) -> Option<IntentHypothesis> {
    case.and_then(|case| case.inferred_intent.clone())
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

fn driver_priority(driver_class: Option<&str>) -> Decimal {
    match driver_class.unwrap_or_default() {
        "sector_wave" => Decimal::new(18, 2),
        "company_specific" => Decimal::new(16, 2),
        "liquidity_dislocation" => Decimal::new(14, 2),
        "institutional" => Decimal::new(12, 2),
        "capital_flow" => Decimal::new(11, 2),
        "trade_flow" => Decimal::new(10, 2),
        "microstructure" => Decimal::new(9, 2),
        "mixed_structural" => Decimal::new(5, 2),
        _ => Decimal::ZERO,
    }
}

fn reasoning_priority_bonus(case: &CaseContract) -> Decimal {
    let lifecycle_bonus = match case.lifecycle_phase.as_deref().unwrap_or_default() {
        "growing" | "Growing" => Decimal::new(8, 2),
        "peaking" | "Peaking" => Decimal::new(2, 2),
        "new" | "New" => Decimal::new(1, 2),
        "fading" | "Fading" => Decimal::new(-5, 2),
        _ => Decimal::ZERO,
    };
    driver_priority(case.driver_class.as_deref())
        + case
            .peer_confirmation_ratio
            .unwrap_or(Decimal::ZERO)
            .min(Decimal::ONE)
            * Decimal::new(20, 2)
        + case
            .competition_margin
            .unwrap_or(Decimal::ZERO)
            .min(Decimal::ONE)
            * Decimal::new(15, 2)
        + lifecycle_bonus
}

fn enriched_case_priority(case: &CaseContract) -> Decimal {
    (case.confidence + reasoning_priority_bonus(case))
        .max(Decimal::ZERO)
        .min(Decimal::ONE)
}

fn operator_reasoning_summary(case: &CaseContract) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(local_state) = case.local_state.as_deref() {
        parts.push(local_state.to_string());
    }
    if let Some(actionability) = case.actionability_state.as_deref() {
        parts.push(actionability.to_string());
    }
    if let Some(driver) = case
        .driver_class
        .as_deref()
        .or(case.tension_driver.as_deref())
    {
        parts.push(driver.to_string());
    }
    if let Some(phase) = case.lifecycle_phase.as_deref() {
        parts.push(phase.to_string());
    }
    if let Some(timing) = case.timing_state.as_deref() {
        parts.push(format!("timing {timing}"));
    }
    if let Some(freshness) = case.freshness_state.as_deref() {
        parts.push(format!("freshness {freshness}"));
    }
    if let Some(peer) = case.peer_confirmation_ratio {
        parts.push(format!("peer {:.0}%", peer * Decimal::from(100)));
    }
    if let Some(margin) = case.competition_margin {
        parts.push(format!("margin {:.0}%", margin * Decimal::from(100)));
    }
    if parts.is_empty() {
        None
    } else {
        Some(parts.join(" · "))
    }
}

fn operator_case_best_action(case: &CaseContract) -> String {
    match case.actionability_state.as_deref() {
        Some("actionable") => case.action.clone(),
        Some("observe_only") | Some("do_not_trade") => "observe".into(),
        _ => case.action.clone(),
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
    reasoning: Option<&str>,
) -> String {
    let base = match lane {
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
    };
    match reasoning {
        Some(reasoning) if !reasoning.is_empty() => format!("{base} | {reasoning}"),
        _ => base,
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

fn build_cohort_signals(cases: &[CaseContract]) -> Vec<CohortSignal> {
    use std::collections::BTreeMap;

    let mut grouped = BTreeMap::<(Option<String>, String, String), Vec<&CaseContract>>::new();
    for case in cases {
        let Some(driver_class) = case.driver_class.as_ref().filter(|value| !value.is_empty())
        else {
            continue;
        };
        grouped
            .entry((
                case.sector.clone(),
                driver_class.clone(),
                case.action.clone(),
            ))
            .or_default()
            .push(case);
    }

    let mut items = grouped
        .into_iter()
        .filter_map(|((sector, driver_class, action), members)| {
            if members.len() < 2 {
                return None;
            }
            let member_count = members.len();
            let mean_confidence = members.iter().map(|case| case.confidence).sum::<Decimal>()
                / Decimal::from(member_count as i64);
            let mean_peer_confirmation_ratio = members
                .iter()
                .map(|case| case.peer_confirmation_ratio.unwrap_or(Decimal::ZERO))
                .sum::<Decimal>()
                / Decimal::from(member_count as i64);
            let mean_competition_margin = members
                .iter()
                .map(|case| case.competition_margin.unwrap_or(Decimal::ZERO))
                .sum::<Decimal>()
                / Decimal::from(member_count as i64);
            let symbols = members
                .iter()
                .map(|case| case.symbol.clone())
                .collect::<Vec<_>>();
            let scope_label = sector.clone().unwrap_or_else(|| "market".into());
            Some(CohortSignal {
                id: format!(
                    "cohort:{}:{}:{}",
                    scope_label.to_ascii_lowercase(),
                    driver_class,
                    action.to_ascii_lowercase()
                ),
                market: members[0].market,
                sector,
                driver_class: driver_class.clone(),
                action: action.clone(),
                member_count,
                mean_confidence,
                mean_peer_confirmation_ratio,
                mean_competition_margin,
                symbols: symbols.clone(),
                summary: format!(
                    "{} {} cohort with {} members ({})",
                    driver_class,
                    action,
                    member_count,
                    symbols.join(", ")
                ),
            })
        })
        .collect::<Vec<_>>();

    items.sort_by(|left, right| {
        right
            .member_count
            .cmp(&left.member_count)
            .then_with(|| {
                right
                    .mean_peer_confirmation_ratio
                    .cmp(&left.mean_peer_confirmation_ratio)
            })
            .then_with(|| {
                right
                    .mean_competition_margin
                    .cmp(&left.mean_competition_margin)
            })
            .then_with(|| left.id.cmp(&right.id))
    });
    items
}

fn materialize_operator_work_items(
    market_session: &MarketSessionContract,
    session: &AgentSession,
    cases: &[CaseContract],
    cohort_signals: &[CohortSignal],
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
            priority: judgment
                .map(|item| item.priority)
                .unwrap_or(thread.priority)
                .max(case.map(enriched_case_priority).unwrap_or(Decimal::ZERO)),
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
                case.and_then(operator_reasoning_summary).as_deref(),
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
            driver_class: case.and_then(|item| item.driver_class.clone()),
            peer_confirmation_ratio: case.and_then(|item| item.peer_confirmation_ratio),
            competition_margin: case.and_then(|item| item.competition_margin),
            cohort_id: None,
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
            priority: judgment
                .priority
                .max(case.map(enriched_case_priority).unwrap_or(Decimal::ZERO)),
            scope_kind,
            scope_id,
            title: judgment.title.clone(),
            summary: operator_summary(
                judgment.object_id.as_str(),
                lane.as_str(),
                judgment.best_action.as_deref(),
                Some(judgment.summary.as_str()),
                case.and_then(operator_reasoning_summary).as_deref(),
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
            driver_class: case.and_then(|item| item.driver_class.clone()),
            peer_confirmation_ratio: case.and_then(|item| item.peer_confirmation_ratio),
            competition_margin: case.and_then(|item| item.competition_margin),
            cohort_id: None,
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
            priority: enriched_case_priority(case),
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
                Some(operator_case_best_action(case).as_str()),
                case.policy_reason
                    .as_deref()
                    .or(case.multi_horizon_gate_reason.as_deref())
                    .or(case.governance_reason.as_deref())
                    .or(Some(case.title.as_str())),
                operator_reasoning_summary(case).as_deref(),
            ),
            symbol: has_symbol.then(|| case.symbol.clone()),
            sector: case.sector.clone(),
            best_action: Some(operator_case_best_action(case)),
            execution_policy: case.execution_policy,
            governance_reason_code: case.governance_reason_code,
            blocker: case
                .policy_reason
                .clone()
                .or_else(|| case.multi_horizon_gate_reason.clone())
                .or_else(|| case.governance_reason.clone()),
            queue_pin: case.queue_pin.clone(),
            driver_class: case.driver_class.clone(),
            peer_confirmation_ratio: case.peer_confirmation_ratio,
            competition_margin: case.competition_margin,
            cohort_id: None,
            owner: case.owner.clone(),
            reviewer: case.reviewer.clone(),
            object_ref,
            case_ref,
            workflow_ref,
            source_refs,
            navigation: OperationalNavigation::default(),
        });
    }

    for cohort in cohort_signals {
        let id = format!("operator:{}:cohort:{}", market_slug(market), cohort.id);
        if !seen_item_ids.insert(id.clone()) {
            continue;
        }

        items.push(OperatorWorkItem {
            id,
            origin: WorkItemOrigin::Judgment,
            grain: WorkItemGrain::Sector,
            lane: if cohort.mean_peer_confirmation_ratio >= Decimal::new(6, 1) {
                "review_desk".into()
            } else {
                "collect_confirmation".into()
            },
            status: "cohort_signal".into(),
            priority: (cohort.mean_confidence
                + cohort.mean_peer_confirmation_ratio * Decimal::new(2, 1)
                + cohort.mean_competition_margin * Decimal::new(15, 2))
            .min(Decimal::ONE),
            scope_kind: "sector".into(),
            scope_id: cohort
                .sector
                .clone()
                .unwrap_or_else(|| cohort.driver_class.clone()),
            title: cohort
                .sector
                .clone()
                .unwrap_or_else(|| cohort.driver_class.clone()),
            summary: cohort.summary.clone(),
            symbol: None,
            sector: cohort.sector.clone(),
            best_action: Some(cohort.action.clone()),
            execution_policy: None,
            governance_reason_code: None,
            blocker: None,
            owner: None,
            reviewer: None,
            queue_pin: None,
            driver_class: Some(cohort.driver_class.clone()),
            peer_confirmation_ratio: Some(cohort.mean_peer_confirmation_ratio),
            competition_margin: Some(cohort.mean_competition_margin),
            cohort_id: Some(cohort.id.clone()),
            object_ref: None,
            case_ref: None,
            workflow_ref: None,
            source_refs: cohort
                .symbols
                .iter()
                .map(|symbol| symbol_object_ref(market, source_tick, symbol))
                .collect(),
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
    let perceptual_state = snapshot
        .world_state
        .as_ref()
        .and_then(|world| {
            world
                .perceptual_states
                .iter()
                .find(|item| match &item.scope {
                    crate::ontology::ReasoningScope::Symbol(symbol) => {
                        symbol.0.eq_ignore_ascii_case(&state.symbol)
                    }
                    _ => false,
                })
        })
        .cloned();
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
        relationships: {
            let mut relationships = SymbolStateRelationships::default();
            if let Some(perceptual_state) = perceptual_state.as_ref() {
                relationships.perceptual_state = Some(perceptual_state_object_ref(
                    snapshot.market,
                    &state.symbol,
                    perceptual_state,
                ));
                relationships.supporting_evidence = perceptual_state
                    .supporting_evidence
                    .iter()
                    .map(|evidence| {
                        perceptual_evidence_object_ref(snapshot.market, &state.symbol, evidence)
                    })
                    .collect();
                relationships.opposing_evidence = perceptual_state
                    .opposing_evidence
                    .iter()
                    .map(|evidence| {
                        perceptual_evidence_object_ref(snapshot.market, &state.symbol, evidence)
                    })
                    .collect();
                relationships.missing_evidence = perceptual_state
                    .missing_evidence
                    .iter()
                    .map(|evidence| {
                        perceptual_evidence_object_ref(snapshot.market, &state.symbol, evidence)
                    })
                    .collect();
                relationships.expectations = perceptual_state
                    .expectations
                    .iter()
                    .map(|expectation| {
                        perceptual_expectation_object_ref(
                            snapshot.market,
                            &state.symbol,
                            expectation,
                        )
                    })
                    .collect();
                relationships.attention_allocations = perceptual_state
                    .attention_allocations
                    .iter()
                    .map(|allocation| {
                        attention_allocation_object_ref(snapshot.market, &state.symbol, allocation)
                    })
                    .collect();
                relationships.uncertainties = perceptual_state
                    .uncertainties
                    .iter()
                    .map(|uncertainty| {
                        perceptual_uncertainty_object_ref(
                            snapshot.market,
                            &state.symbol,
                            uncertainty,
                        )
                    })
                    .collect();
            }
            relationships
        },
        summary: SymbolStateSummary {
            symbol: state.symbol.clone(),
            sector: state.sector.clone(),
            perceptual_state_kind: perceptual_state
                .as_ref()
                .map(|item| item.state_kind.clone()),
            perceptual_trend: perceptual_state.as_ref().map(|item| item.trend.clone()),
            weighted_support_fraction: perceptual_state
                .as_ref()
                .map(|item| item.weighted_support_fraction),
            count_support_fraction: perceptual_state
                .as_ref()
                .map(|item| item.count_support_fraction),
            top_supporting_evidence: perceptual_state
                .as_ref()
                .and_then(|item| item.supporting_evidence.first())
                .map(|item| item.rationale.clone()),
            top_opposing_evidence: perceptual_state
                .as_ref()
                .and_then(|item| item.opposing_evidence.first())
                .map(|item| item.rationale.clone()),
            top_missing_evidence: perceptual_state
                .as_ref()
                .and_then(|item| item.missing_evidence.first())
                .map(|item| item.rationale.clone()),
            expectation_statuses: perceptual_state
                .as_ref()
                .map(|item| {
                    item.expectations
                        .iter()
                        .map(|expectation| format!("{}:{}", expectation.kind, expectation.status))
                        .collect()
                })
                .unwrap_or_default(),
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
        perceptual_state,
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
                    name: "perceptual_state".into(),
                    refs: symbol
                        .relationships
                        .perceptual_state
                        .clone()
                        .into_iter()
                        .collect(),
                },
                OperationalRelationshipGroup {
                    name: "supporting_evidence".into(),
                    refs: symbol.relationships.supporting_evidence.clone(),
                },
                OperationalRelationshipGroup {
                    name: "opposing_evidence".into(),
                    refs: symbol.relationships.opposing_evidence.clone(),
                },
                OperationalRelationshipGroup {
                    name: "missing_evidence".into(),
                    refs: symbol.relationships.missing_evidence.clone(),
                },
                OperationalRelationshipGroup {
                    name: "expectations".into(),
                    refs: symbol.relationships.expectations.clone(),
                },
                OperationalRelationshipGroup {
                    name: "attention_allocations".into(),
                    refs: symbol.relationships.attention_allocations.clone(),
                },
                OperationalRelationshipGroup {
                    name: "uncertainties".into(),
                    refs: symbol.relationships.uncertainties.clone(),
                },
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

    for perceptual_state in &mut snapshot.perceptual_states {
        perceptual_state.navigation = OperationalNavigation {
            self_ref: Some(OperationalObjectRef {
                id: perceptual_state.id.clone(),
                kind: OperationalObjectKind::PerceptualState,
                endpoint: object_endpoint(
                    snapshot.market,
                    &format!("perceptual-states/{}", perceptual_state.id),
                ),
                label: Some(format!(
                    "{} {}",
                    perceptual_state.symbol, perceptual_state.state.state_kind
                )),
            }),
            graph: Some(perceptual_state.graph_ref.clone()),
            history: vec![],
            relationships: vec![
                OperationalRelationshipGroup {
                    name: "supporting_evidence".into(),
                    refs: snapshot
                        .perceptual_evidence
                        .iter()
                        .filter(|item| {
                            item.symbol.eq_ignore_ascii_case(&perceptual_state.symbol)
                                && matches!(
                                    item.evidence.polarity,
                                    crate::ontology::world::PerceptualEvidencePolarity::Supports
                                )
                        })
                        .map(|item| OperationalObjectRef {
                            id: item.id.clone(),
                            kind: OperationalObjectKind::PerceptualEvidence,
                            endpoint: object_endpoint(
                                snapshot.market,
                                &format!("perceptual-evidence/{}", item.id),
                            ),
                            label: Some(item.evidence.rationale.clone()),
                        })
                        .collect(),
                },
                OperationalRelationshipGroup {
                    name: "opposing_evidence".into(),
                    refs: snapshot
                        .perceptual_evidence
                        .iter()
                        .filter(|item| {
                            item.symbol.eq_ignore_ascii_case(&perceptual_state.symbol)
                                && matches!(
                                    item.evidence.polarity,
                                    crate::ontology::world::PerceptualEvidencePolarity::Contradicts
                                )
                        })
                        .map(|item| OperationalObjectRef {
                            id: item.id.clone(),
                            kind: OperationalObjectKind::PerceptualEvidence,
                            endpoint: object_endpoint(
                                snapshot.market,
                                &format!("perceptual-evidence/{}", item.id),
                            ),
                            label: Some(item.evidence.rationale.clone()),
                        })
                        .collect(),
                },
                OperationalRelationshipGroup {
                    name: "missing_evidence".into(),
                    refs: snapshot
                        .perceptual_evidence
                        .iter()
                        .filter(|item| {
                            item.symbol.eq_ignore_ascii_case(&perceptual_state.symbol)
                                && matches!(
                                    item.evidence.polarity,
                                    crate::ontology::world::PerceptualEvidencePolarity::Missing
                                )
                        })
                        .map(|item| OperationalObjectRef {
                            id: item.id.clone(),
                            kind: OperationalObjectKind::PerceptualEvidence,
                            endpoint: object_endpoint(
                                snapshot.market,
                                &format!("perceptual-evidence/{}", item.id),
                            ),
                            label: Some(item.evidence.rationale.clone()),
                        })
                        .collect(),
                },
                OperationalRelationshipGroup {
                    name: "expectations".into(),
                    refs: snapshot
                        .perceptual_expectations
                        .iter()
                        .filter(|item| item.symbol.eq_ignore_ascii_case(&perceptual_state.symbol))
                        .map(|item| OperationalObjectRef {
                            id: item.id.clone(),
                            kind: OperationalObjectKind::PerceptualExpectation,
                            endpoint: object_endpoint(
                                snapshot.market,
                                &format!("perceptual-expectations/{}", item.id),
                            ),
                            label: Some(item.expectation.rationale.clone()),
                        })
                        .collect(),
                },
                OperationalRelationshipGroup {
                    name: "attention_allocations".into(),
                    refs: snapshot
                        .attention_allocations
                        .iter()
                        .filter(|item| item.symbol.eq_ignore_ascii_case(&perceptual_state.symbol))
                        .map(|item| OperationalObjectRef {
                            id: item.id.clone(),
                            kind: OperationalObjectKind::AttentionAllocation,
                            endpoint: object_endpoint(
                                snapshot.market,
                                &format!("attention-allocations/{}", item.id),
                            ),
                            label: Some(item.allocation.rationale.clone()),
                        })
                        .collect(),
                },
                OperationalRelationshipGroup {
                    name: "uncertainties".into(),
                    refs: snapshot
                        .perceptual_uncertainties
                        .iter()
                        .filter(|item| item.symbol.eq_ignore_ascii_case(&perceptual_state.symbol))
                        .map(|item| OperationalObjectRef {
                            id: item.id.clone(),
                            kind: OperationalObjectKind::PerceptualUncertainty,
                            endpoint: object_endpoint(
                                snapshot.market,
                                &format!("perceptual-uncertainties/{}", item.id),
                            ),
                            label: Some(item.uncertainty.rationale.clone()),
                        })
                        .collect(),
                },
            ],
            neighborhood_endpoint: Some(neighborhood_endpoint(
                snapshot.market,
                OperationalObjectKind::PerceptualState,
                &perceptual_state.id,
            )),
        };
    }

    for evidence in &mut snapshot.perceptual_evidence {
        evidence.navigation = OperationalNavigation {
            self_ref: Some(OperationalObjectRef {
                id: evidence.id.clone(),
                kind: OperationalObjectKind::PerceptualEvidence,
                endpoint: object_endpoint(
                    snapshot.market,
                    &format!("perceptual-evidence/{}", evidence.id),
                ),
                label: Some(evidence.evidence.rationale.clone()),
            }),
            graph: Some(evidence.graph_ref.clone()),
            history: vec![],
            relationships: vec![],
            neighborhood_endpoint: Some(neighborhood_endpoint(
                snapshot.market,
                OperationalObjectKind::PerceptualEvidence,
                &evidence.id,
            )),
        };
    }

    for expectation in &mut snapshot.perceptual_expectations {
        expectation.navigation = OperationalNavigation {
            self_ref: Some(OperationalObjectRef {
                id: expectation.id.clone(),
                kind: OperationalObjectKind::PerceptualExpectation,
                endpoint: object_endpoint(
                    snapshot.market,
                    &format!("perceptual-expectations/{}", expectation.id),
                ),
                label: Some(expectation.expectation.rationale.clone()),
            }),
            graph: Some(expectation.graph_ref.clone()),
            history: vec![],
            relationships: vec![],
            neighborhood_endpoint: Some(neighborhood_endpoint(
                snapshot.market,
                OperationalObjectKind::PerceptualExpectation,
                &expectation.id,
            )),
        };
    }

    for allocation in &mut snapshot.attention_allocations {
        allocation.navigation = OperationalNavigation {
            self_ref: Some(OperationalObjectRef {
                id: allocation.id.clone(),
                kind: OperationalObjectKind::AttentionAllocation,
                endpoint: object_endpoint(
                    snapshot.market,
                    &format!("attention-allocations/{}", allocation.id),
                ),
                label: Some(allocation.allocation.rationale.clone()),
            }),
            graph: Some(allocation.graph_ref.clone()),
            history: vec![],
            relationships: vec![],
            neighborhood_endpoint: Some(neighborhood_endpoint(
                snapshot.market,
                OperationalObjectKind::AttentionAllocation,
                &allocation.id,
            )),
        };
    }

    for uncertainty in &mut snapshot.perceptual_uncertainties {
        uncertainty.navigation = OperationalNavigation {
            self_ref: Some(OperationalObjectRef {
                id: uncertainty.id.clone(),
                kind: OperationalObjectKind::PerceptualUncertainty,
                endpoint: object_endpoint(
                    snapshot.market,
                    &format!("perceptual-uncertainties/{}", uncertainty.id),
                ),
                label: Some(uncertainty.uncertainty.rationale.clone()),
            }),
            graph: Some(uncertainty.graph_ref.clone()),
            history: vec![],
            relationships: vec![],
            neighborhood_endpoint: Some(neighborhood_endpoint(
                snapshot.market,
                OperationalObjectKind::PerceptualUncertainty,
                &uncertainty.id,
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
    let (
        perceptual_states,
        perceptual_evidence,
        perceptual_expectations,
        attention_allocations,
        perceptual_uncertainties,
    ) = build_perceptual_contracts(snapshot, &symbols, observed_at);

    let case_summaries = build_case_summaries(live_snapshot);
    let case_summaries_by_setup = case_summaries
        .iter()
        .map(|item| (item.setup_id.as_str(), item))
        .collect::<HashMap<_, _>>();
    let effective_recommendations =
        effective_recommendations_for_contracts(snapshot, recommendations, &case_summaries);
    let recommendation_links =
        link_recommendations_to_cases(&case_summaries, &effective_recommendations.items);
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
                causal_narrative: live_case.and_then(|entry| entry.causal_narrative.clone()),
                lifecycle_phase: live_case.and_then(|entry| entry.lifecycle_phase.clone()),
                tension_driver: live_case.and_then(|entry| entry.tension_driver.clone()),
                driver_class: live_case.and_then(|entry| entry.driver_class.clone()),
                is_isolated: live_case.and_then(|entry| entry.is_isolated),
                peer_active_count: live_case.and_then(|entry| entry.peer_active_count),
                peer_silent_count: live_case.and_then(|entry| entry.peer_silent_count),
                peer_confirmation_ratio: live_case.and_then(|entry| entry.peer_confirmation_ratio),
                isolation_score: live_case.and_then(|entry| entry.isolation_score),
                competition_margin: live_case.and_then(|entry| entry.competition_margin),
                lifecycle_velocity: live_case.and_then(|entry| entry.lifecycle_velocity),
                lifecycle_acceleration: live_case.and_then(|entry| entry.lifecycle_acceleration),
                freshness_state: live_case.and_then(|entry| entry.freshness_state.clone()),
                review_reason_family: live_case
                    .and_then(|entry| entry.review_reason_family.clone()),
                review_reason_subreasons: live_case
                    .map(|entry| entry.review_reason_subreasons.clone())
                    .unwrap_or_default(),
                ticks_since_first_seen: live_case.and_then(|entry| entry.ticks_since_first_seen),
                timing_state: live_case.and_then(|entry| entry.timing_state.clone()),
                timing_position_in_range: live_case
                    .and_then(|entry| entry.timing_position_in_range),
                local_state: live_case.and_then(|entry| entry.local_state.clone()),
                local_state_confidence: live_case.and_then(|entry| entry.local_state_confidence),
                actionability_score: live_case.and_then(|entry| entry.actionability_score),
                actionability_state: live_case.and_then(|entry| entry.actionability_state.clone()),
                confidence_velocity_5t: live_case.and_then(|entry| entry.confidence_velocity_5t),
                support_fraction_velocity_5t: live_case
                    .and_then(|entry| entry.support_fraction_velocity_5t),
                driver_confidence: live_case.and_then(|entry| entry.driver_confidence),
                absence_summary: live_case.and_then(|entry| entry.absence_summary.clone()),
                competition_summary: live_case.and_then(|entry| entry.competition_summary.clone()),
                competition_winner: live_case.and_then(|entry| entry.competition_winner.clone()),
                competition_runner_up: live_case
                    .and_then(|entry| entry.competition_runner_up.clone()),
                priority_rank: live_case.and_then(|entry| entry.priority_rank),
                state_persistence_ticks: live_case.and_then(|entry| entry.state_persistence_ticks),
                direction_stability_rounds: live_case
                    .and_then(|entry| entry.direction_stability_rounds),
                state_reason_codes: live_case
                    .map(|entry| entry.state_reason_codes.clone())
                    .unwrap_or_default(),
                matched_success_pattern_signature: live_case
                    .and_then(|entry| entry.matched_success_pattern_signature.clone()),
                case_signature: live_case_signature(live_case),
                archetype_projections: live_case_archetype_projections(live_case),
                inferred_intent: item.inferred_intent.clone(),
                expectation_bindings: live_case_expectation_bindings(live_case),
                expectation_violations: live_case_expectation_violations(live_case),
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

    let recommendation_contracts = effective_recommendations
        .items
        .iter()
        .cloned()
        .map(|item| {
            let linkage = recommendation_links
                .iter()
                .find(|(rec_id, _, _, _)| rec_id == &item.recommendation_id);
            let related_live_case = linkage
                .and_then(|(_, _, setup_id, _)| setup_id.as_ref())
                .and_then(|setup_id| live_cases_by_setup.get(setup_id.as_str()).copied());
            let related_case_summary = linkage
                .and_then(|(_, _, setup_id, _)| setup_id.as_ref())
                .and_then(|setup_id| case_summaries_by_setup.get(setup_id.as_str()).copied());
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
                    driver_class: related_live_case.and_then(|case| case.driver_class.clone()),
                    lifecycle_phase: related_live_case
                        .and_then(|case| case.lifecycle_phase.clone()),
                    peer_confirmation_ratio: related_live_case
                        .and_then(|case| case.peer_confirmation_ratio),
                    competition_margin: related_live_case.and_then(|case| case.competition_margin),
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
                case_signature: live_case_signature(related_live_case),
                archetype_projections: live_case_archetype_projections(related_live_case),
                inferred_intent: related_case_summary
                    .and_then(|summary| summary.inferred_intent.clone())
                    .or_else(|| live_case_inferred_intent(related_live_case)),
                expectation_bindings: live_case_expectation_bindings(related_live_case),
                expectation_violations: live_case_expectation_violations(related_live_case),
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
    let cohort_signals = build_cohort_signals(&cases);
    let operator_work_items =
        materialize_operator_work_items(&market_session, session, &cases, &cohort_signals);
    let sidecars = OperationalSidecars {
        sector_flows: snapshot.sector_flows.clone(),
        raw_sources: live_snapshot.raw_sources.clone(),
        signal_translation_gaps: live_snapshot.signal_translation_gaps.clone(),
        cluster_states: live_snapshot.cluster_states.clone(),
        world_summary: live_snapshot.world_summary.clone(),
        backward_investigations: snapshot
            .backward_reasoning
            .as_ref()
            .map(|item| item.investigations.clone())
            .unwrap_or_default(),
        world_state: Some(build_world_state_snapshot(
            snapshot.market,
            &live_snapshot.timestamp,
            &live_snapshot.symbol_states,
            &live_snapshot.cluster_states,
            live_snapshot.world_summary.as_ref(),
        )),
        macro_event_candidates: snapshot.macro_event_candidates.clone(),
        knowledge_links: combined_knowledge_links(snapshot, &effective_recommendations),
        operator_workflows: Vec::new(),
        operator_work_items,
        cohort_signals,
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
        perceptual_states,
        perceptual_evidence,
        perceptual_expectations,
        attention_allocations,
        perceptual_uncertainties,
        cases,
        market_recommendation: effective_recommendations.market_recommendation.clone(),
        sector_recommendations: effective_recommendations
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
                causal_narrative: previous.and_then(|entry| entry.causal_narrative.clone()),
                lifecycle_phase: previous.and_then(|entry| entry.lifecycle_phase.clone()),
                tension_driver: previous.and_then(|entry| entry.tension_driver.clone()),
                driver_class: previous.and_then(|entry| entry.driver_class.clone()),
                is_isolated: previous.and_then(|entry| entry.is_isolated),
                peer_active_count: previous.and_then(|entry| entry.peer_active_count),
                peer_silent_count: previous.and_then(|entry| entry.peer_silent_count),
                peer_confirmation_ratio: previous.and_then(|entry| entry.peer_confirmation_ratio),
                isolation_score: previous.and_then(|entry| entry.isolation_score),
                competition_margin: previous.and_then(|entry| entry.competition_margin),
                lifecycle_velocity: previous.and_then(|entry| entry.lifecycle_velocity),
                lifecycle_acceleration: previous.and_then(|entry| entry.lifecycle_acceleration),
                freshness_state: previous.and_then(|entry| entry.freshness_state.clone()),
                review_reason_family: previous.and_then(|entry| entry.review_reason_family.clone()),
                review_reason_subreasons: previous
                    .map(|entry| entry.review_reason_subreasons.clone())
                    .unwrap_or_default(),
                ticks_since_first_seen: previous.and_then(|entry| entry.ticks_since_first_seen),
                timing_state: previous.and_then(|entry| entry.timing_state.clone()),
                timing_position_in_range: previous.and_then(|entry| entry.timing_position_in_range),
                local_state: previous.and_then(|entry| entry.local_state.clone()),
                local_state_confidence: previous.and_then(|entry| entry.local_state_confidence),
                actionability_score: previous.and_then(|entry| entry.actionability_score),
                actionability_state: previous.and_then(|entry| entry.actionability_state.clone()),
                confidence_velocity_5t: previous.and_then(|entry| entry.confidence_velocity_5t),
                support_fraction_velocity_5t: previous
                    .and_then(|entry| entry.support_fraction_velocity_5t),
                driver_confidence: previous.and_then(|entry| entry.driver_confidence),
                absence_summary: previous.and_then(|entry| entry.absence_summary.clone()),
                competition_summary: previous.and_then(|entry| entry.competition_summary.clone()),
                competition_winner: previous.and_then(|entry| entry.competition_winner.clone()),
                competition_runner_up: previous
                    .and_then(|entry| entry.competition_runner_up.clone()),
                priority_rank: previous.and_then(|entry| entry.priority_rank),
                state_persistence_ticks: previous.and_then(|entry| entry.state_persistence_ticks),
                direction_stability_rounds: previous
                    .and_then(|entry| entry.direction_stability_rounds),
                state_reason_codes: previous
                    .map(|entry| entry.state_reason_codes.clone())
                    .unwrap_or_default(),
                matched_success_pattern_signature: previous
                    .and_then(|entry| entry.matched_success_pattern_signature.clone()),
                case_signature: previous.and_then(|entry| entry.case_signature.clone()),
                archetype_projections: previous
                    .map(|entry| entry.archetype_projections.clone())
                    .unwrap_or_default(),
                inferred_intent: previous.and_then(|entry| entry.inferred_intent.clone()),
                expectation_bindings: previous
                    .map(|entry| entry.expectation_bindings.clone())
                    .unwrap_or_default(),
                expectation_violations: previous
                    .map(|entry| entry.expectation_violations.clone())
                    .unwrap_or_default(),
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
