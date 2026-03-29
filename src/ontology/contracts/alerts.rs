use super::*;

pub fn derive_agent_scoreboard(
    snapshot: &OperationalSnapshot,
    previous: Option<&AgentAlertScoreboard>,
) -> AgentAlertScoreboard {
    let recommendations = derive_agent_recommendations(snapshot);
    let mut alerts = previous
        .map(|item| item.alerts.clone())
        .unwrap_or_default();

    for alert in alerts.iter_mut().filter(|item| item.resolution.is_none()) {
        alert.resolution = resolve_alert_resolution_contract(snapshot, alert);
        alert.outcome_after_n_ticks = alert_outcome_from_resolution_contract(alert);
    }

    for decision in &recommendations.decisions {
        if let Some(alert) = decision_alert_record_contract(snapshot, decision, &alerts) {
            alerts.push(alert);
        }
    }

    alerts.sort_by(|a, b| b.tick.cmp(&a.tick).then_with(|| a.alert_id.cmp(&b.alert_id)));

    let unresolved = alerts.iter().filter(|item| item.resolution.is_none()).count();
    if alerts.len() > 240 {
        let keep = alerts
            .iter()
            .take_while(|item| item.resolution.is_none())
            .count()
            .max(unresolved.min(40));
        alerts.truncate(keep.saturating_add(200).min(alerts.len()));
    }

    let stats = compute_alert_stats_contract(&alerts);
    let by_kind = compute_alert_slice_stats_contract(&alerts, |item| item.kind.clone());
    let by_action = compute_alert_slice_stats_contract(&alerts, |item| item.suggested_action.clone());
    let by_scope = compute_alert_slice_stats_contract(&alerts, |item| item.scope_kind.clone());
    let by_regime = compute_alert_slice_stats_contract(&alerts, |item| item.regime_bias.clone());
    let by_sector = compute_alert_slice_stats_contract(&alerts, |item| {
        item.sector.clone().unwrap_or_else(|| "unknown".into())
    });

    AgentAlertScoreboard {
        tick: snapshot.source_tick,
        timestamp: format_timestamp(snapshot.observed_at),
        market: snapshot.market,
        regime_bias: snapshot.market_session.market_regime.bias.clone(),
        total: alerts.len(),
        alerts,
        stats,
        by_kind,
        by_action,
        by_scope,
        by_regime,
        by_sector,
    }
}

pub fn derive_agent_eod_review(
    snapshot: &OperationalSnapshot,
    scoreboard: &AgentAlertScoreboard,
) -> AgentEodReview {
    let effective_kinds = top_positive_slices_contract(&scoreboard.by_kind, 3);
    let noisy_kinds = top_noisy_slices_contract(&scoreboard.by_kind, 3);
    let effective_actions = top_positive_slices_contract(&scoreboard.by_action, 3);
    let effective_sectors = top_positive_slices_contract(&scoreboard.by_sector, 3);
    let effective_regimes = top_positive_slices_contract(&scoreboard.by_regime, 3);
    let top_hits = top_resolved_alerts_contract(&scoreboard.alerts, "hit", 3);
    let top_misses = top_resolved_alerts_contract(&scoreboard.alerts, "miss", 3);

    let mut conclusions = Vec::new();
    conclusions.push(format!(
        "resolved {} / {} alerts, hit_rate {:.0}%, false_positive_rate {:.0}%, mean_oriented_return {:+.2}%",
        scoreboard.stats.resolved_alerts,
        scoreboard.stats.total_alerts,
        (scoreboard.stats.hit_rate * Decimal::from(100)).round_dp(0),
        (scoreboard.stats.false_positive_rate * Decimal::from(100)).round_dp(0),
        (scoreboard.stats.mean_oriented_return * Decimal::from(100)).round_dp(2),
    ));
    if let Some(slice) = effective_kinds.first() {
        conclusions.push(format!(
            "best alert kind so far: {} (hit_rate {:.0}% on {} resolved)",
            slice.key,
            (slice.hit_rate * Decimal::from(100)).round_dp(0),
            slice.resolved_alerts
        ));
    }
    if let Some(slice) = noisy_kinds.first() {
        conclusions.push(format!(
            "noisiest alert kind so far: {} (false_positive_rate {:.0}% on {} resolved)",
            slice.key,
            (slice.false_positive_rate * Decimal::from(100)).round_dp(0),
            slice.resolved_alerts
        ));
    }
    if let Some(slice) = effective_sectors.first() {
        conclusions.push(format!(
            "sector with best follow-through: {} (hit_rate {:.0}%)",
            slice.key,
            (slice.hit_rate * Decimal::from(100)).round_dp(0)
        ));
    }

    AgentEodReview {
        tick: snapshot.source_tick,
        timestamp: format_timestamp(snapshot.observed_at),
        market: snapshot.market,
        regime_bias: snapshot.market_session.market_regime.bias.clone(),
        total_alerts: scoreboard.stats.total_alerts,
        resolved_alerts: scoreboard.stats.resolved_alerts,
        hits: scoreboard.stats.hits,
        misses: scoreboard.stats.misses,
        flats: scoreboard.stats.flats,
        hit_rate: scoreboard.stats.hit_rate,
        mean_oriented_return: scoreboard.stats.mean_oriented_return,
        false_positive_rate: scoreboard.stats.false_positive_rate,
        effective_kinds,
        noisy_kinds,
        effective_actions,
        effective_sectors,
        effective_regimes,
        top_hits,
        top_misses,
        conclusions,
        analyst_lift: None,
    }
}

pub(crate) fn parse_timestamp(raw: &str) -> Result<OffsetDateTime, String> {
    OffsetDateTime::parse(raw, &time::format_description::well_known::Rfc3339)
        .map_err(|error| format!("invalid operational snapshot timestamp `{raw}`: {error}"))
}

pub(crate) fn market_slug(market: LiveMarket) -> &'static str {
    match market {
        LiveMarket::Hk => "hk",
        LiveMarket::Us => "us",
    }
}

pub(crate) fn normalized_symbol_id(symbol: &str) -> String {
    Symbol(symbol.into()).0.to_ascii_lowercase()
}

pub(crate) fn case_workflow_key(case: &CaseSummary) -> String {
    case.workflow_id
        .clone()
        .unwrap_or_else(|| format!("workflow:{}", case.setup_id))
}

pub(crate) fn link_recommendations_to_cases(
    cases: &[CaseSummary],
    recommendations: &[AgentRecommendation],
) -> Vec<(String, Option<String>, Option<String>, Option<String>)> {
    let mut by_symbol = HashMap::<String, Vec<&CaseSummary>>::new();
    for case in cases {
        by_symbol
            .entry(case.symbol.to_ascii_lowercase())
            .or_default()
            .push(case);
    }

    recommendations
        .iter()
        .map(|item| {
            let linked_case = by_symbol
                .get(&item.symbol.to_ascii_lowercase())
                .and_then(|cases| {
                    cases.iter()
                        .find(|case| case.recommended_action.eq_ignore_ascii_case(&item.action))
                        .copied()
                        .or_else(|| cases.first().copied())
                });
            (
                item.recommendation_id.clone(),
                linked_case.map(|case| case.case_id.clone()),
                linked_case.map(|case| case.setup_id.clone()),
                linked_case.map(case_workflow_key),
            )
        })
        .collect()
}

pub(crate) fn build_workflow_contracts(
    market: LiveMarket,
    source_tick: u64,
    observed_at: OffsetDateTime,
    cases: &[CaseContract],
    recommendations: &[RecommendationContract],
) -> Vec<WorkflowContract> {
    let mut grouped = HashMap::<String, WorkflowContract>::new();

    for case in cases {
        let workflow_key = case
            .workflow_id
            .clone()
            .unwrap_or_else(|| format!("workflow:{}", case.setup_id));
        let entry = grouped.entry(workflow_key.clone()).or_insert_with(|| WorkflowContract {
            id: WorkflowContractId(workflow_key.clone()),
            market,
            source_tick,
            observed_at,
            stage: case.workflow_state.clone(),
            execution_policy: case.execution_policy,
            governance_reason_code: case.governance_reason_code,
            owner: case.owner.clone(),
            reviewer: case.reviewer.clone(),
            queue_pin: case.queue_pin.clone(),
            synthetic: case.workflow_id.is_none(),
            case_ids: Vec::new(),
            recommendation_ids: Vec::new(),
            navigation: OperationalNavigation::default(),
            relationships: WorkflowRelationships::default(),
            case_refs: Vec::new(),
            recommendation_refs: Vec::new(),
            history_refs: workflow_history_refs(market, &workflow_key),
        });
        entry.case_ids.push(case.id.0.clone());
        entry.relationships.cases.push(case_object_ref(
            market,
            &case.id.0,
            Some(case.title.clone()),
        ));
        entry.case_refs.push(case_object_ref(
            market,
            &case.id.0,
            Some(case.title.clone()),
        ));
    }

    for recommendation in recommendations {
        if let Some(workflow_id) = recommendation.related_workflow_id.as_ref() {
            let entry = grouped
                .entry(workflow_id.clone())
                .or_insert_with(|| WorkflowContract {
                    id: WorkflowContractId(workflow_id.clone()),
                    market,
                    source_tick,
                    observed_at,
                    stage: "suggest".into(),
                    execution_policy: Some(recommendation.recommendation.execution_policy),
                    governance_reason_code: Some(
                        recommendation.recommendation.governance_reason_code,
                    ),
                    owner: None,
                    reviewer: None,
                    queue_pin: None,
                    synthetic: true,
                    case_ids: Vec::new(),
                    recommendation_ids: Vec::new(),
                    navigation: OperationalNavigation::default(),
                    relationships: WorkflowRelationships::default(),
                    case_refs: Vec::new(),
                    recommendation_refs: Vec::new(),
                    history_refs: workflow_history_refs(market, workflow_id),
                });
            entry.recommendation_ids.push(recommendation.id.0.clone());
            entry.relationships.recommendations.push(recommendation_object_ref(
                market,
                &recommendation.id.0,
                recommendation.recommendation.title.clone(),
            ));
            entry.recommendation_refs.push(recommendation_object_ref(
                market,
                &recommendation.id.0,
                recommendation.recommendation.title.clone(),
            ));
        }
    }

    let mut workflows = grouped.into_values().collect::<Vec<_>>();
    workflows.sort_by(|left, right| left.id.0.cmp(&right.id.0));
    workflows
}

pub(crate) fn format_timestamp(timestamp: OffsetDateTime) -> String {
    timestamp
        .format(&Rfc3339)
        .unwrap_or_else(|_| timestamp.to_string())
}

pub(crate) fn decision_watchlist_visible(decision: &AgentDecision) -> bool {
    match decision {
        AgentDecision::Market(item) => item.best_action != "wait",
        AgentDecision::Sector(item) => item.best_action != "wait",
        AgentDecision::Symbol(item) => item.action != "ignore",
    }
}

pub(crate) fn decision_watchlist_entry(
    snapshot: &OperationalSnapshot,
    decision: &AgentDecision,
    rank: usize,
) -> AgentWatchlistEntry {
    match decision {
        AgentDecision::Market(item) => AgentWatchlistEntry {
            rank,
            scope_kind: "market".into(),
            symbol: match snapshot.market {
                LiveMarket::Hk => "HK Market".into(),
                LiveMarket::Us => "US Market".into(),
            },
            sector: None,
            edge_layer: Some(item.edge_layer.clone()),
            title: Some(format!(
                "{} macro / market setup",
                match snapshot.market {
                    LiveMarket::Hk => "HK Market",
                    LiveMarket::Us => "US Market",
                }
            )),
            action: item.best_action.clone(),
            action_label: Some(item.preferred_expression.clone()),
            bias: item.bias.clone(),
            severity: if item.best_action == "wait" { "normal".into() } else { "high".into() },
            score: item.market_impulse_score,
            status: Some(snapshot.market_session.market_regime.bias.clone()),
            why: item.summary.clone(),
            why_components: vec![],
            primary_lens: None,
            supporting_lenses: vec![],
            review_lens: None,
            transition: Some(item.summary.clone()),
            watch_next: item.decisive_factors.iter().take(2).cloned().collect(),
            do_not: item
                .why_not_single_name
                .clone()
                .into_iter()
                .collect::<Vec<_>>(),
            recommendation_id: item.recommendation_id.clone(),
            thesis_family: None,
            state_transition: None,
            best_action: item.best_action.clone(),
            action_expectancies: AgentActionExpectancies {
                follow_expectancy: (item.best_action == "follow").then_some(
                    item.expected_net_alpha.unwrap_or(Decimal::ZERO),
                ),
                fade_expectancy: (item.best_action == "fade").then_some(
                    item.expected_net_alpha.unwrap_or(Decimal::ZERO),
                ),
                wait_expectancy: Some(Decimal::ZERO),
            },
            decision_attribution: AgentDecisionAttribution {
                historical_expectancies: AgentActionExpectancies {
                    wait_expectancy: Some(Decimal::ZERO),
                    ..AgentActionExpectancies::default()
                },
                live_expectancy_shift: Decimal::ZERO,
                decisive_factors: item.decisive_factors.clone(),
            },
            expected_net_alpha: item.expected_net_alpha,
            alpha_horizon: item.alpha_horizon.clone(),
            preferred_expression: Some(item.preferred_expression.clone()),
            reference_symbols: item.reference_symbols.clone(),
            invalidation_rule: None,
            invalidation_components: vec![],
            execution_policy: Some(item.execution_policy),
            governance: Some(item.governance.clone()),
            governance_reason_code: Some(item.governance_reason_code),
            governance_reason: Some(item.governance_reason.clone()),
        },
        AgentDecision::Sector(item) => AgentWatchlistEntry {
            rank,
            scope_kind: "sector".into(),
            symbol: item.sector.clone(),
            sector: Some(item.sector.clone()),
            edge_layer: Some(item.edge_layer.clone()),
            title: Some(format!("{} sector setup", item.sector)),
            action: item.best_action.clone(),
            action_label: Some(item.preferred_expression.clone()),
            bias: item.bias.clone(),
            severity: if item.best_action == "wait" { "normal".into() } else { "high".into() },
            score: item.sector_impulse_score,
            status: None,
            why: item.summary.clone(),
            why_components: vec![],
            primary_lens: None,
            supporting_lenses: vec![],
            review_lens: None,
            transition: Some(item.summary.clone()),
            watch_next: item.decisive_factors.iter().take(2).cloned().collect(),
            do_not: vec![],
            recommendation_id: item.recommendation_id.clone(),
            thesis_family: None,
            state_transition: None,
            best_action: item.best_action.clone(),
            action_expectancies: AgentActionExpectancies {
                follow_expectancy: (item.best_action == "follow").then_some(
                    item.expected_net_alpha.unwrap_or(Decimal::ZERO),
                ),
                fade_expectancy: (item.best_action == "fade").then_some(
                    item.expected_net_alpha.unwrap_or(Decimal::ZERO),
                ),
                wait_expectancy: Some(Decimal::ZERO),
            },
            decision_attribution: AgentDecisionAttribution {
                historical_expectancies: AgentActionExpectancies {
                    wait_expectancy: Some(Decimal::ZERO),
                    ..AgentActionExpectancies::default()
                },
                live_expectancy_shift: Decimal::ZERO,
                decisive_factors: item.decisive_factors.clone(),
            },
            expected_net_alpha: item.expected_net_alpha,
            alpha_horizon: item.alpha_horizon.clone(),
            preferred_expression: Some(item.preferred_expression.clone()),
            reference_symbols: item.reference_symbols.clone(),
            invalidation_rule: None,
            invalidation_components: vec![],
            execution_policy: Some(item.execution_policy),
            governance: Some(item.governance.clone()),
            governance_reason_code: Some(item.governance_reason_code),
            governance_reason: Some(item.governance_reason.clone()),
        },
        AgentDecision::Symbol(item) => AgentWatchlistEntry {
            rank,
            scope_kind: "symbol".into(),
            symbol: item.symbol.clone(),
            sector: item.sector.clone(),
            edge_layer: None,
            title: item.title.clone(),
            action: item.action.clone(),
            action_label: item.action_label.clone(),
            bias: item.bias.clone(),
            severity: item.severity.clone(),
            score: item.score,
            status: item.status.clone(),
            why: item.why.clone(),
            why_components: item.why_components.clone(),
            primary_lens: item.primary_lens.clone(),
            supporting_lenses: item.supporting_lenses.clone(),
            review_lens: item.review_lens.clone(),
            transition: item.transition.clone(),
            watch_next: item.watch_next.clone(),
            do_not: item.do_not.clone(),
            recommendation_id: item.recommendation_id.clone(),
            thesis_family: item.thesis_family.clone(),
            state_transition: item.state_transition.clone(),
            best_action: item.best_action.clone(),
            action_expectancies: item.action_expectancies.clone(),
            decision_attribution: item.decision_attribution.clone(),
            expected_net_alpha: item.expected_net_alpha,
            alpha_horizon: item.alpha_horizon.clone(),
            preferred_expression: None,
            reference_symbols: vec![item.symbol.clone()],
            invalidation_rule: item.invalidation_rule.clone(),
            invalidation_components: item.invalidation_components.clone(),
            execution_policy: Some(item.execution_policy),
            governance: Some(item.governance.clone()),
            governance_reason_code: Some(item.governance_reason_code),
            governance_reason: Some(item.governance_reason.clone()),
        },
    }
}

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

fn decision_alert_record_contract(
    snapshot: &OperationalSnapshot,
    decision: &AgentDecision,
    existing: &[AgentAlertRecord],
) -> Option<AgentAlertRecord> {
    match decision {
        AgentDecision::Symbol(recommendation) => {
            let fresh_transition = snapshot
                .recent_transitions
                .iter()
                .find(|item| item.to_tick == snapshot.source_tick && item.symbol.eq_ignore_ascii_case(&recommendation.symbol));
            let fresh_notice = snapshot
                .notices
                .iter()
                .find(|item| item.tick == snapshot.source_tick && item.symbol.as_deref() == Some(recommendation.symbol.as_str()));
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
                snapshot.source_tick, recommendation.symbol, kind, recommendation.best_action
            );
            if existing.iter().any(|item| item.alert_id == alert_id) {
                return None;
            }
            Some(AgentAlertRecord {
                alert_id,
                tick: snapshot.source_tick,
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
                price_at_alert: symbol_mark_price_contract(snapshot, &recommendation.symbol),
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
                snapshot.source_tick, recommendation.preferred_expression, recommendation.best_action
            );
            if existing.iter().any(|item| item.alert_id == alert_id) {
                return None;
            }
            Some(AgentAlertRecord {
                alert_id,
                tick: snapshot.source_tick,
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
                snapshot.source_tick, recommendation.sector, recommendation.best_action
            );
            if existing.iter().any(|item| item.alert_id == alert_id) {
                return None;
            }
            Some(AgentAlertRecord {
                alert_id,
                tick: snapshot.source_tick,
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

fn resolve_alert_resolution_contract(
    snapshot: &OperationalSnapshot,
    alert: &AgentAlertRecord,
) -> Option<crate::agent::AgentRecommendationResolution> {
    if snapshot.source_tick < alert.tick.saturating_add(alert.horizon_ticks) {
        return None;
    }

    let reference_at_alert = alert.reference_value_at_alert.or(alert.price_at_alert)?;
    if reference_at_alert <= Decimal::ZERO {
        return None;
    }

    let current_reference = current_alert_reference_value_contract(snapshot, alert)?;
    let price_return = (current_reference - reference_at_alert) / reference_at_alert;
    let expected_direction = expected_alert_direction_contract(&alert.suggested_action, &alert.action_bias);
    let follow_realized_return = if expected_direction == 0 {
        Decimal::ZERO
    } else {
        price_return * Decimal::from(expected_direction as i64)
    };
    let fade_realized_return = -follow_realized_return;
    let wait_realized_return = Decimal::ZERO;
    let threshold = Decimal::new(20, 4);
    let best_action = alert_resolution_action_contract(&alert.suggested_action);
    let counterfactual_best_action = best_counterfactual_action_contract(
        follow_realized_return,
        fade_realized_return,
        threshold,
    );
    let wait_regret = counterfactual_regret_contract(
        follow_realized_return,
        fade_realized_return,
        wait_realized_return,
    );
    let best_action_return = realized_return_for_action_contract(
        best_action,
        follow_realized_return,
        fade_realized_return,
    );
    let status = recommendation_resolution_status_contract(
        best_action,
        best_action_return,
        wait_regret,
        threshold,
    );

    Some(crate::agent::AgentRecommendationResolution {
        resolved_tick: snapshot.source_tick,
        ticks_elapsed: snapshot.source_tick.saturating_sub(alert.tick),
        status: status.into(),
        price_return: price_return.round_dp(4),
        follow_realized_return: follow_realized_return.round_dp(4),
        fade_realized_return: fade_realized_return.round_dp(4),
        wait_regret: wait_regret.round_dp(4),
        counterfactual_best_action: counterfactual_best_action.into(),
        best_action_was_correct: best_action == counterfactual_best_action,
    })
}

fn alert_outcome_from_resolution_contract(
    alert: &AgentAlertRecord,
) -> Option<AgentAlertOutcome> {
    let resolution = alert.resolution.as_ref()?;
    Some(AgentAlertOutcome {
        resolved_tick: resolution.resolved_tick,
        ticks_elapsed: resolution.ticks_elapsed,
        status: resolution.status.clone(),
        price_return: Some(resolution.price_return),
        oriented_return: alert_oriented_return_contract(alert, resolution),
        follow_through: summarize_follow_through_contract(&resolution.status, resolution.price_return),
    })
}

fn compute_alert_stats_contract(alerts: &[AgentAlertRecord]) -> AgentAlertStats {
    let mut accumulator = AgentAlertSliceAccumulator::default();
    for alert in alerts {
        update_alert_accumulator_contract(&mut accumulator, alert);
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

fn compute_alert_slice_stats_contract<F>(
    alerts: &[AgentAlertRecord],
    mut key_fn: F,
) -> Vec<AgentAlertSliceStat>
where
    F: FnMut(&AgentAlertRecord) -> String,
{
    let mut slices = HashMap::<String, AgentAlertSliceAccumulator>::new();
    for alert in alerts {
        let key = key_fn(alert);
        update_alert_accumulator_contract(slices.entry(key).or_default(), alert);
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

    items.sort_by(|a, b| b.total_alerts.cmp(&a.total_alerts).then_with(|| a.key.cmp(&b.key)));
    items
}

fn top_positive_slices_contract(
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

fn top_noisy_slices_contract(
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

fn top_resolved_alerts_contract(
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
                follow_through: summarize_follow_through_contract(
                    &resolution.status,
                    resolution.price_return,
                ),
                oriented_return: alert_oriented_return_contract(alert, resolution),
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

fn update_alert_accumulator_contract(
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
    if let Some(value) = alert_oriented_return_contract(alert, resolution) {
        accumulator.oriented_return_sum += value;
        accumulator.oriented_return_count += 1;
    }
}

fn current_alert_reference_value_contract(
    snapshot: &OperationalSnapshot,
    alert: &AgentAlertRecord,
) -> Option<Decimal> {
    if !alert.reference_symbols.is_empty() {
        return sector_reference_value_contract(snapshot, &alert.reference_symbols);
    }
    let symbol = alert.symbol.as_deref()?;
    symbol_mark_price_contract(snapshot, symbol)
}

fn symbol_mark_price_contract(snapshot: &OperationalSnapshot, symbol: &str) -> Option<Decimal> {
    snapshot
        .symbol(symbol)
        .and_then(|item| item.state.signal.as_ref())
        .and_then(|signal| signal.mark_price)
}

fn sector_reference_value_contract(
    snapshot: &OperationalSnapshot,
    symbols: &[String],
) -> Option<Decimal> {
    if symbols.is_empty() {
        return None;
    }
    let prices = symbols
        .iter()
        .filter_map(|symbol| symbol_mark_price_contract(snapshot, symbol))
        .collect::<Vec<_>>();
    if prices.is_empty() {
        return None;
    }
    Some(prices.iter().copied().sum::<Decimal>() / Decimal::from(prices.len() as i64))
}

fn alert_resolution_action_contract(action: &str) -> &'static str {
    match action {
        "follow" | "enter" | "add" | "watch" | "review" => "follow",
        "fade" | "trim" | "hedge" => "fade",
        "wait" => "wait",
        _ => "wait",
    }
}

fn alert_oriented_return_contract(
    alert: &AgentAlertRecord,
    resolution: &crate::agent::AgentRecommendationResolution,
) -> Option<Decimal> {
    match alert_resolution_action_contract(&alert.suggested_action) {
        "follow" => Some(resolution.follow_realized_return),
        "fade" => Some(resolution.fade_realized_return),
        "wait" => Some(Decimal::ZERO),
        _ => None,
    }
}

fn expected_alert_direction_contract(action: &str, bias: &str) -> i8 {
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

fn summarize_follow_through_contract(status: &str, price_return: Decimal) -> String {
    let pct = (price_return * Decimal::from(100)).round_dp(2);
    match status {
        "hit" => format!("follow-through {pct:+}%"),
        "miss" => format!("reversed {pct:+}%"),
        "flat" => format!("flat {pct:+}%"),
        _ => format!("unscored {pct:+}%"),
    }
}

fn decimal_ratio(numerator: usize, denominator: usize) -> Decimal {
    if denominator == 0 {
        Decimal::ZERO
    } else {
        Decimal::from(numerator as i64) / Decimal::from(denominator as i64)
    }
}

fn decimal_mean(sum: Decimal, count: usize) -> Decimal {
    if count == 0 {
        Decimal::ZERO
    } else {
        sum / Decimal::from(count as i64)
    }
}

fn best_counterfactual_action_contract(
    follow_realized_return: Decimal,
    fade_realized_return: Decimal,
    threshold: Decimal,
) -> &'static str {
    if follow_realized_return >= fade_realized_return && follow_realized_return > threshold {
        "follow"
    } else if fade_realized_return > follow_realized_return && fade_realized_return > threshold {
        "fade"
    } else {
        "wait"
    }
}

fn counterfactual_regret_contract(
    follow_realized_return: Decimal,
    fade_realized_return: Decimal,
    wait_realized_return: Decimal,
) -> Decimal {
    follow_realized_return
        .max(fade_realized_return)
        .max(wait_realized_return)
        - wait_realized_return
}

fn realized_return_for_action_contract(
    action: &str,
    follow_realized_return: Decimal,
    fade_realized_return: Decimal,
) -> Decimal {
    match action {
        "follow" | "enter" | "add" | "watch" | "review" => follow_realized_return,
        "fade" | "trim" | "hedge" => fade_realized_return,
        _ => Decimal::ZERO,
    }
}

fn recommendation_resolution_status_contract(
    best_action: &str,
    best_action_return: Decimal,
    wait_regret: Decimal,
    threshold: Decimal,
) -> &'static str {
    if best_action == "wait" {
        if wait_regret > threshold { "miss" } else { "flat" }
    } else if best_action_return > threshold {
        "hit"
    } else if wait_regret <= threshold {
        "flat"
    } else {
        "miss"
    }
}

pub(crate) fn narration_action_card(
    decision: &AgentDecision,
    market: LiveMarket,
) -> AgentNarrationActionCard {
    match decision {
        AgentDecision::Market(item) => AgentNarrationActionCard {
            card_id: format!("card:{}:market", item.tick),
            scope_kind: "market".into(),
            symbol: market_label(market).into(),
            sector: None,
            edge_layer: Some(item.edge_layer.clone()),
            setup_id: Some(item.recommendation_id.clone()),
            action: item.best_action.clone(),
            action_label: Some(item.preferred_expression.clone()),
            severity: if item.best_action == "wait" { "normal".into() } else { "high".into() },
            title: Some(format!("{} macro / market setup", market_label(market))),
            summary: item.summary.clone(),
            why_now: item.summary.clone(),
            why_components: vec![],
            primary_lens: None,
            supporting_lenses: vec![],
            review_lens: None,
            confidence_band: Some(confidence_band_for_score(item.market_impulse_score)),
            watch_next: item.decisive_factors.iter().take(3).cloned().collect(),
            do_not: item
                .why_not_single_name
                .clone()
                .into_iter()
                .collect::<Vec<_>>(),
            thesis_family: None,
            state_transition: None,
            best_action: item.best_action.clone(),
            action_expectancies: AgentActionExpectancies {
                follow_expectancy: (item.best_action == "follow").then_some(
                    item.expected_net_alpha.unwrap_or(Decimal::ZERO),
                ),
                fade_expectancy: (item.best_action == "fade").then_some(
                    item.expected_net_alpha.unwrap_or(Decimal::ZERO),
                ),
                wait_expectancy: Some(Decimal::ZERO),
            },
            decision_attribution: AgentDecisionAttribution {
                historical_expectancies: AgentActionExpectancies {
                    wait_expectancy: Some(Decimal::ZERO),
                    ..AgentActionExpectancies::default()
                },
                live_expectancy_shift: Decimal::ZERO,
                decisive_factors: item.decisive_factors.clone(),
            },
            expected_net_alpha: item.expected_net_alpha,
            alpha_horizon: item.alpha_horizon.clone(),
            preferred_expression: Some(item.preferred_expression.clone()),
            reference_symbols: item.reference_symbols.clone(),
            invalidation_rule: None,
            invalidation_components: vec![],
            execution_policy: item.execution_policy,
            governance_reason_code: item.governance_reason_code,
            governance_reason: item.governance_reason.clone(),
        },
        AgentDecision::Sector(item) => AgentNarrationActionCard {
            card_id: format!("card:{}:sector:{}", item.tick, item.sector),
            scope_kind: "sector".into(),
            symbol: item.sector.clone(),
            sector: Some(item.sector.clone()),
            edge_layer: Some(item.edge_layer.clone()),
            setup_id: Some(item.recommendation_id.clone()),
            action: item.best_action.clone(),
            action_label: Some(item.preferred_expression.clone()),
            severity: if item.best_action == "wait" { "normal".into() } else { "high".into() },
            title: Some(format!("{} sector setup", item.sector)),
            summary: item.summary.clone(),
            why_now: item.summary.clone(),
            why_components: vec![],
            primary_lens: None,
            supporting_lenses: vec![],
            review_lens: None,
            confidence_band: Some(confidence_band_for_score(item.sector_impulse_score)),
            watch_next: item.decisive_factors.iter().take(3).cloned().collect(),
            do_not: vec![],
            thesis_family: None,
            state_transition: None,
            best_action: item.best_action.clone(),
            action_expectancies: AgentActionExpectancies {
                follow_expectancy: (item.best_action == "follow").then_some(
                    item.expected_net_alpha.unwrap_or(Decimal::ZERO),
                ),
                fade_expectancy: (item.best_action == "fade").then_some(
                    item.expected_net_alpha.unwrap_or(Decimal::ZERO),
                ),
                wait_expectancy: Some(Decimal::ZERO),
            },
            decision_attribution: AgentDecisionAttribution {
                historical_expectancies: AgentActionExpectancies {
                    wait_expectancy: Some(Decimal::ZERO),
                    ..AgentActionExpectancies::default()
                },
                live_expectancy_shift: Decimal::ZERO,
                decisive_factors: item.decisive_factors.clone(),
            },
            expected_net_alpha: item.expected_net_alpha,
            alpha_horizon: item.alpha_horizon.clone(),
            preferred_expression: Some(item.preferred_expression.clone()),
            reference_symbols: item.reference_symbols.clone(),
            invalidation_rule: None,
            invalidation_components: vec![],
            execution_policy: item.execution_policy,
            governance_reason_code: item.governance_reason_code,
            governance_reason: item.governance_reason.clone(),
        },
        AgentDecision::Symbol(item) => AgentNarrationActionCard {
            card_id: format!("card:{}:symbol:{}", item.tick, item.symbol),
            scope_kind: "symbol".into(),
            symbol: item.symbol.clone(),
            sector: item.sector.clone(),
            edge_layer: None,
            setup_id: Some(item.recommendation_id.clone()),
            action: item.action.clone(),
            action_label: item.action_label.clone(),
            severity: item.severity.clone(),
            title: item.title.clone(),
            summary: item.why.clone(),
            why_now: item.why.clone(),
            why_components: item.why_components.clone(),
            primary_lens: item.primary_lens.clone(),
            supporting_lenses: item.supporting_lenses.clone(),
            review_lens: item.review_lens.clone(),
            confidence_band: Some(confidence_band_for_score(item.confidence)),
            watch_next: item.watch_next.clone(),
            do_not: item.do_not.clone(),
            thesis_family: item.thesis_family.clone(),
            state_transition: item.state_transition.clone(),
            best_action: item.best_action.clone(),
            action_expectancies: item.action_expectancies.clone(),
            decision_attribution: item.decision_attribution.clone(),
            expected_net_alpha: item.expected_net_alpha,
            alpha_horizon: item.alpha_horizon.clone(),
            preferred_expression: None,
            reference_symbols: vec![item.symbol.clone()],
            invalidation_rule: item.invalidation_rule.clone(),
            invalidation_components: item.invalidation_components.clone(),
            execution_policy: item.execution_policy,
            governance_reason_code: item.governance_reason_code,
            governance_reason: item.governance_reason.clone(),
        },
    }
}

pub(crate) fn aggregate_dominant_lenses(cards: &[AgentNarrationActionCard]) -> Vec<AgentDominantLens> {
    #[derive(Default)]
    struct LensAccumulator {
        card_count: usize,
        total_confidence: Decimal,
        max_confidence: Decimal,
    }

    let mut aggregates: HashMap<String, LensAccumulator> = HashMap::new();
    for card in cards {
        let mut per_card: HashMap<String, Decimal> = HashMap::new();
        for component in card
            .why_components
            .iter()
            .chain(card.invalidation_components.iter())
        {
            let lens_name = component.lens_name.trim();
            if lens_name.is_empty() {
                continue;
            }
            let confidence = component.confidence.abs();
            per_card
                .entry(lens_name.to_string())
                .and_modify(|value| {
                    if confidence > *value {
                        *value = confidence;
                    }
                })
                .or_insert(confidence);
        }

        for (lens_name, confidence) in per_card {
            let entry = aggregates.entry(lens_name).or_default();
            entry.card_count += 1;
            entry.total_confidence += confidence;
            if confidence > entry.max_confidence {
                entry.max_confidence = confidence;
            }
        }
    }

    let mut lenses = aggregates
        .into_iter()
        .map(|(lens_name, item)| AgentDominantLens {
            lens_name,
            card_count: item.card_count,
            max_confidence: item.max_confidence.round_dp(4),
            mean_confidence: if item.card_count == 0 {
                Decimal::ZERO
            } else {
                (item.total_confidence / Decimal::from(item.card_count as i64)).round_dp(4)
            },
        })
        .collect::<Vec<_>>();
    lenses.sort_by(|left, right| {
        right
            .card_count
            .cmp(&left.card_count)
            .then_with(|| right.max_confidence.cmp(&left.max_confidence))
            .then_with(|| left.lens_name.cmp(&right.lens_name))
    });
    lenses.truncate(6);
    lenses
}

pub(crate) fn dominant_lens_summary(lenses: &[AgentDominantLens]) -> Option<String> {
    if lenses.is_empty() {
        return None;
    }
    let summary = lenses
        .iter()
        .take(3)
        .map(|item| {
            format!(
                "{} {}",
                render_lens_label(&item.lens_name),
                (item.max_confidence * Decimal::from(100)).round_dp(0)
            )
        })
        .collect::<Vec<_>>()
        .join(" • ");
    Some(format!("Dominant lenses: {summary}"))
}

fn render_lens_label(name: &str) -> String {
    name.split('_')
        .filter(|part| !part.is_empty())
        .map(|part| {
            let mut chars = part.chars();
            match chars.next() {
                Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn confidence_band_for_score(score: Decimal) -> String {
    if score >= Decimal::new(8, 2) {
        "high".into()
    } else if score >= Decimal::new(4, 2) {
        "medium".into()
    } else {
        "low".into()
    }
}

pub(crate) fn market_label(market: LiveMarket) -> &'static str {
    match market {
        LiveMarket::Hk => "HK Market",
        LiveMarket::Us => "US Market",
    }
}
