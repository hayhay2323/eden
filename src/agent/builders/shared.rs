use super::*;

pub(super) fn setup_family(setup: &TacticalSetup) -> Option<String> {
    setup
        .risk_notes
        .iter()
        .find_map(|note| note.strip_prefix("family=").map(str::to_string))
}

pub(super) fn primary_invalidation_rule(
    hypothesis: Option<&Hypothesis>,
    fallback: Option<&str>,
    setup: Option<&TacticalSetup>,
) -> Option<String> {
    hypothesis
        .and_then(|item| item.invalidation_conditions.first())
        .map(|item| item.description.clone())
        .or_else(|| {
            setup.and_then(|item| {
                item.risk_notes
                    .iter()
                    .find(|note| {
                        !note.starts_with("family=")
                            && !note.starts_with("local_support=")
                            && !note.starts_with("policy_gate:")
                            && !note.starts_with("policy_transition:")
                            && !note.starts_with("estimated execution cost=")
                            && !note.starts_with("convergence_score=")
                            && !note.starts_with("effective_confidence=")
                            && !note.starts_with("hypothesis_margin=")
                            && !note.starts_with("external_")
                            && !note.starts_with("lineage_prior=")
                    })
                    .cloned()
            })
        })
        .or_else(|| fallback.map(str::to_string))
}

pub(crate) fn alpha_horizon_label(time_horizon: &str, ticks: u64) -> String {
    format!("{}:{}t", time_horizon, ticks)
}

pub(crate) fn build_broker_state(
    current: &[AgentBrokerInstitution],
    previous: Option<&AgentBrokerState>,
) -> AgentBrokerState {
    let previous_by_id = previous
        .map(|item| {
            item.current
                .iter()
                .map(|entry| (entry.institution_id, entry))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();
    let current_ids = current
        .iter()
        .map(|item| item.institution_id)
        .collect::<HashSet<_>>();
    let previous_ids = previous_by_id.keys().copied().collect::<HashSet<_>>();

    let entered = current
        .iter()
        .filter(|item| !previous_ids.contains(&item.institution_id))
        .map(|item| item.name.clone())
        .collect::<Vec<_>>();

    let exited = previous
        .map(|item| {
            item.current
                .iter()
                .filter(|entry| !current_ids.contains(&entry.institution_id))
                .map(|entry| entry.name.clone())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let switched_to_bid = current
        .iter()
        .filter(|item| {
            !item.bid_positions.is_empty()
                && previous_by_id
                    .get(&item.institution_id)
                    .map(|prev| prev.bid_positions.is_empty())
                    .unwrap_or(true)
        })
        .map(|item| item.name.clone())
        .collect::<Vec<_>>();

    let switched_to_ask = current
        .iter()
        .filter(|item| {
            !item.ask_positions.is_empty()
                && previous_by_id
                    .get(&item.institution_id)
                    .map(|prev| prev.ask_positions.is_empty())
                    .unwrap_or(true)
        })
        .map(|item| item.name.clone())
        .collect::<Vec<_>>();

    AgentBrokerState {
        current: current.to_vec(),
        entered,
        exited,
        switched_to_bid,
        switched_to_ask,
    }
}

pub(super) fn institutions_by_symbol(
    activities: &[InstitutionActivity],
    store: &ObjectStore,
) -> HashMap<String, Vec<AgentBrokerInstitution>> {
    let mut result: HashMap<String, Vec<AgentBrokerInstitution>> = HashMap::new();
    for activity in activities {
        result
            .entry(activity.symbol.0.clone())
            .or_default()
            .push(AgentBrokerInstitution {
                institution_id: activity.institution_id.0,
                name: store
                    .institutions
                    .get(&activity.institution_id)
                    .map(|item| item.name_en.clone())
                    .unwrap_or_else(|| format!("institution:{}", activity.institution_id.0)),
                bid_positions: activity.bid_positions.clone(),
                ask_positions: activity.ask_positions.clone(),
                seat_count: activity.seat_count,
            });
    }
    for value in result.values_mut() {
        value.sort_by(|a, b| a.name.cmp(&b.name));
    }
    result
}

pub(super) fn hk_events_by_symbol(events: &[LiveEvent]) -> HashMap<String, Vec<LiveEvent>> {
    let mut result = HashMap::new();
    for event in events {
        for symbol in extract_symbols(&event.summary) {
            result
                .entry(symbol)
                .or_insert_with(Vec::new)
                .push(event.clone());
        }
    }
    result
}

pub(super) fn us_events_by_symbol(events: &[LiveEvent]) -> HashMap<String, Vec<LiveEvent>> {
    hk_events_by_symbol(events)
}

pub(crate) fn hk_recent_transitions(
    history: &TickHistory,
    store: &ObjectStore,
    window: usize,
) -> Vec<AgentTransition> {
    let records = history.latest_n(window.max(2));
    let mut transitions = Vec::new();

    for pair in records.windows(2) {
        let previous = pair[0];
        let current = pair[1];
        let previous_map = previous
            .hypothesis_tracks
            .iter()
            .map(|item| (item.setup_id.as_str(), item))
            .collect::<HashMap<_, _>>();
        let current_ids = current
            .hypothesis_tracks
            .iter()
            .map(|item| item.setup_id.as_str())
            .collect::<HashSet<_>>();

        for track in &current.hypothesis_tracks {
            let Some(symbol) = scope_symbol(&track.scope) else {
                continue;
            };
            let sector = store.sector_name_for_symbol(symbol).map(str::to_string);
            let previous_track = previous_map.get(track.setup_id.as_str()).copied();
            let changed = match previous_track {
                Some(prev) => {
                    prev.status != track.status
                        || prev.action != track.action
                        || prev.hypothesis_id != track.hypothesis_id
                }
                None => track.status != HypothesisTrackStatus::Stable || track.action == "enter",
            };
            if !changed {
                continue;
            }
            transitions.push(AgentTransition {
                from_tick: previous.tick_number,
                to_tick: current.tick_number,
                symbol: symbol.0.clone(),
                sector,
                setup_id: Some(track.setup_id.clone()),
                title: track.title.clone(),
                from_state: previous_track.map(render_track_state),
                to_state: render_track_state(track),
                confidence: track.confidence,
                summary: render_hk_transition_summary(previous_track, track),
                transition_reason: track.transition_reason.clone(),
            });
        }

        for track in &previous.hypothesis_tracks {
            if current_ids.contains(track.setup_id.as_str()) {
                continue;
            }
            let Some(symbol) = scope_symbol(&track.scope) else {
                continue;
            };
            transitions.push(AgentTransition {
                from_tick: previous.tick_number,
                to_tick: current.tick_number,
                symbol: symbol.0.clone(),
                sector: store.sector_name_for_symbol(symbol).map(str::to_string),
                setup_id: Some(track.setup_id.clone()),
                title: track.title.clone(),
                from_state: Some(render_track_state(track)),
                to_state: "absent".into(),
                confidence: track.confidence,
                summary: format!("{} left the active structure set", track.title),
                transition_reason: track.transition_reason.clone(),
            });
        }
    }

    transitions.sort_by(|a, b| {
        b.to_tick
            .cmp(&a.to_tick)
            .then_with(|| b.confidence.cmp(&a.confidence))
    });
    transitions.truncate(64);
    transitions
}

pub(crate) fn us_recent_transitions(
    history: &UsTickHistory,
    store: &ObjectStore,
    window: usize,
) -> Vec<AgentTransition> {
    let records = history.latest_n(window.max(2));
    let mut transitions = Vec::new();

    for pair in records.windows(2) {
        let previous = pair[0];
        let current = pair[1];
        let previous_map = previous
            .tactical_setups
            .iter()
            .map(|item| (item.setup_id.as_str(), item))
            .collect::<HashMap<_, _>>();
        let current_ids = current
            .tactical_setups
            .iter()
            .map(|item| item.setup_id.as_str())
            .collect::<HashSet<_>>();

        for setup in &current.tactical_setups {
            let Some(symbol) = scope_symbol(&setup.scope) else {
                continue;
            };
            let previous_setup = previous_map.get(setup.setup_id.as_str()).copied();
            let changed = match previous_setup {
                Some(prev) => {
                    prev.action != setup.action || prev.hypothesis_id != setup.hypothesis_id
                }
                None => true,
            };
            if !changed {
                continue;
            }
            transitions.push(AgentTransition {
                from_tick: previous.tick_number,
                to_tick: current.tick_number,
                symbol: symbol.0.clone(),
                sector: store.sector_name_for_symbol(symbol).map(str::to_string),
                setup_id: Some(setup.setup_id.clone()),
                title: setup.title.clone(),
                from_state: previous_setup.map(|item| item.action.to_string()),
                to_state: setup.action.to_string(),
                confidence: setup.confidence,
                summary: match previous_setup {
                    Some(prev) if prev.action != setup.action => {
                        format!("{} action {} -> {}", setup.title, prev.action, setup.action)
                    }
                    Some(_) => format!("{} rotated into a new hypothesis", setup.title),
                    None => format!("{} entered the active structure set", setup.title),
                },
                transition_reason: None,
            });
        }

        for setup in &previous.tactical_setups {
            if current_ids.contains(setup.setup_id.as_str()) {
                continue;
            }
            let Some(symbol) = scope_symbol(&setup.scope) else {
                continue;
            };
            transitions.push(AgentTransition {
                from_tick: previous.tick_number,
                to_tick: current.tick_number,
                symbol: symbol.0.clone(),
                sector: store.sector_name_for_symbol(symbol).map(str::to_string),
                setup_id: Some(setup.setup_id.clone()),
                title: setup.title.clone(),
                from_state: Some(setup.action.to_string()),
                to_state: "absent".into(),
                confidence: setup.confidence,
                summary: match setup.review_reason_code {
                    Some(code) => format!(
                        "{} left the active structure set ({})",
                        setup.title,
                        code.as_str()
                    ),
                    None => format!("{} left the active structure set", setup.title),
                },
                transition_reason: setup
                    .review_reason_code
                    .map(|code| code.as_str().to_string())
                    .or_else(|| {
                        setup
                            .policy_verdict
                            .as_ref()
                            .map(|verdict| verdict.primary.as_str().to_string())
                    }),
            });
        }
    }

    transitions.sort_by(|a, b| {
        b.to_tick
            .cmp(&a.to_tick)
            .then_with(|| b.confidence.cmp(&a.confidence))
    });
    transitions.truncate(64);
    transitions
}

pub(super) fn hk_signal_state(signal: &SymbolSignals) -> AgentSignalState {
    AgentSignalState {
        composite: signal.composite,
        mark_price: signal.mark_price,
        capital_flow_direction: signal.capital_flow_direction,
        price_momentum: Decimal::ZERO,
        volume_profile: Decimal::ZERO,
        pre_post_market_anomaly: Decimal::ZERO,
        valuation: Decimal::ZERO,
        sector_coherence: signal.sector_coherence,
        cross_stock_correlation: Some(signal.cross_stock_correlation),
        cross_market_propagation: None,
    }
}

pub(super) fn us_signal_state(signal: &UsSymbolSignals) -> AgentSignalState {
    AgentSignalState {
        composite: signal.composite,
        mark_price: signal.mark_price,
        capital_flow_direction: signal.capital_flow_direction,
        price_momentum: signal.price_momentum,
        volume_profile: signal.volume_profile,
        pre_post_market_anomaly: signal.pre_post_market_anomaly,
        valuation: signal.valuation,
        sector_coherence: None,
        cross_stock_correlation: None,
        cross_market_propagation: None,
    }
}

pub(super) fn build_hk_invalidation(
    track: Option<&HypothesisTrack>,
    hypothesis: Option<&Hypothesis>,
    backward: Option<&BackwardInvestigation>,
    setup: Option<&TacticalSetup>,
    live_case: Option<&crate::live_snapshot::LiveTacticalCase>,
) -> Option<AgentInvalidationState> {
    let status = track
        .map(|item| item.status.as_str().to_string())
        .unwrap_or_else(|| "unknown".into());
    let invalidated = track
        .map(|item| {
            item.invalidated_at.is_some() || item.status == HypothesisTrackStatus::Invalidated
        })
        .unwrap_or(false);

    let mut rules = Vec::new();
    if let Some(hypothesis) = hypothesis {
        rules.extend(
            hypothesis
                .invalidation_conditions
                .iter()
                .map(|item| item.description.clone()),
        );
    }
    if let Some(setup) = setup {
        rules.extend(setup.risk_notes.iter().cloned());
    }
    if let Some(live_case) = live_case {
        if let Some(code) = &live_case.review_reason_code {
            rules.push(format!("review_reason_code={code}"));
        }
        if let Some(primary) = &live_case.policy_primary {
            rules.push(format!("policy_primary={primary}"));
        }
        if let Some(reason) = &live_case.policy_reason {
            rules.push(format!("policy_reason={reason}"));
        }
        if let Some(reason) = &live_case.multi_horizon_gate_reason {
            rules.push(format!("multi_horizon_gate=blocked: {reason}"));
        }
    }
    if let Some(backward) = backward {
        if let Some(falsifier) = &backward.leading_falsifier {
            rules.push(falsifier.clone());
        }
    }
    dedupe_strings(&mut rules);

    if track.is_none() && rules.is_empty() {
        return None;
    }

    Some(AgentInvalidationState {
        status,
        invalidated,
        transition_reason: track.and_then(|item| item.transition_reason.clone()),
        leading_falsifier: backward.and_then(|item| item.leading_falsifier.clone()),
        rules,
    })
}

pub(super) fn build_us_invalidation(
    setup: Option<&TacticalSetup>,
    hypothesis: Option<&Hypothesis>,
    backward: Option<&crate::live_snapshot::LiveBackwardChain>,
    live_case: Option<&crate::live_snapshot::LiveTacticalCase>,
) -> Option<AgentInvalidationState> {
    if setup.is_none() && hypothesis.is_none() && backward.is_none() && live_case.is_none() {
        return None;
    }

    let mut rules = Vec::new();
    if let Some(hypothesis) = hypothesis {
        rules.extend(
            hypothesis
                .invalidation_conditions
                .iter()
                .map(|item| item.description.clone()),
        );
    }
    if let Some(setup) = setup {
        rules.extend(setup.risk_notes.iter().cloned());
    }
    if let Some(live_case) = live_case {
        if let Some(code) = &live_case.review_reason_code {
            rules.push(format!("review_reason_code={code}"));
        }
        if let Some(primary) = &live_case.policy_primary {
            rules.push(format!("policy_primary={primary}"));
        }
        if let Some(reason) = &live_case.policy_reason {
            rules.push(format!("policy_reason={reason}"));
        }
        if let Some(reason) = &live_case.multi_horizon_gate_reason {
            rules.push(format!("multi_horizon_gate=blocked: {reason}"));
        }
    }
    if let Some(backward) = backward {
        rules.push(format!("主因反轉則失效: {}", backward.primary_driver));
    }
    dedupe_strings(&mut rules);

    Some(AgentInvalidationState {
        status: setup
            .map(|item| item.action.to_string())
            .unwrap_or_else(|| "watch".into()),
        invalidated: false,
        transition_reason: None,
        leading_falsifier: backward.map(|item| item.primary_driver.clone()),
        rules,
    })
}

pub(super) fn build_hk_depth_state(
    current: &SymbolSignals,
    previous: Option<&SymbolSignals>,
    order_book: &OrderBookObservation,
    previous_depth: Option<&AgentDepthState>,
) -> AgentDepthState {
    let previous_spread = previous_depth.and_then(|item| item.spread);
    let spread_change = match (order_book.spread, previous_spread) {
        (Some(current), Some(previous)) => Some(current - previous),
        _ => None,
    };

    let summary = format!(
        "imbalance={:+} bid_top3={:.3} ask_top3={:.3} bid_vol={} ask_vol={}",
        current.depth_structure_imbalance.round_dp(3),
        current.bid_top3_ratio.round_dp(3),
        current.ask_top3_ratio.round_dp(3),
        order_book.total_bid_volume,
        order_book.total_ask_volume,
    );

    AgentDepthState {
        imbalance: current.depth_structure_imbalance,
        imbalance_change: current.depth_structure_imbalance
            - previous
                .map(|item| item.depth_structure_imbalance)
                .unwrap_or(Decimal::ZERO),
        bid_best_ratio: current.bid_best_ratio,
        bid_best_ratio_change: current.bid_best_ratio
            - previous
                .map(|item| item.bid_best_ratio)
                .unwrap_or(Decimal::ZERO),
        ask_best_ratio: current.ask_best_ratio,
        ask_best_ratio_change: current.ask_best_ratio
            - previous
                .map(|item| item.ask_best_ratio)
                .unwrap_or(Decimal::ZERO),
        bid_top3_ratio: current.bid_top3_ratio,
        bid_top3_ratio_change: current.bid_top3_ratio
            - previous
                .map(|item| item.bid_top3_ratio)
                .unwrap_or(Decimal::ZERO),
        ask_top3_ratio: current.ask_top3_ratio,
        ask_top3_ratio_change: current.ask_top3_ratio
            - previous
                .map(|item| item.ask_top3_ratio)
                .unwrap_or(Decimal::ZERO),
        spread: order_book.spread,
        spread_change,
        bid_total_volume: order_book.total_bid_volume,
        ask_total_volume: order_book.total_ask_volume,
        bid_total_volume_change: order_book.total_bid_volume
            - previous_depth
                .map(|item| item.bid_total_volume)
                .unwrap_or(0),
        ask_total_volume_change: order_book.total_ask_volume
            - previous_depth
                .map(|item| item.ask_total_volume)
                .unwrap_or(0),
        summary,
    }
}

pub(super) fn build_us_structure_state(
    symbol: &str,
    store: &ObjectStore,
    setup: Option<&TacticalSetup>,
    hypothesis: Option<&Hypothesis>,
    status: Option<String>,
    age_ticks: Option<u64>,
    status_streak: Option<u64>,
    causal_leader: Option<String>,
    live_case: Option<&crate::live_snapshot::LiveTacticalCase>,
) -> Option<AgentStructureState> {
    let setup = setup?;
    Some(AgentStructureState {
        symbol: symbol.to_string(),
        sector: store
            .sector_name_for_symbol(&Symbol(symbol.to_string()))
            .map(str::to_string),
        setup_id: Some(setup.setup_id.clone()),
        title: setup.title.clone(),
        action: setup.action.to_string(),
        status,
        age_ticks,
        status_streak,
        confidence: setup.confidence,
        confidence_change: None,
        confidence_gap: Some(setup.confidence_gap),
        transition_reason: None,
        contest_state: None,
        current_leader: causal_leader,
        leader_streak: None,
        leader_transition_summary: None,
        thesis_family: hypothesis
            .map(|item| item.family_label.clone())
            .or_else(|| setup_family(setup)),
        action_expectancies: crate::agent::AgentActionExpectancies::default(),
        expected_net_alpha: None,
        alpha_horizon: Some(alpha_horizon_label(
            setup.horizon.primary.to_legacy_string(),
            age_ticks.unwrap_or(0),
        )),
        invalidation_rule: primary_invalidation_rule(
            hypothesis,
            None,
            Some(setup),
        )
        .or_else(|| live_case.and_then(|item| item.multi_horizon_gate_reason.clone())),
    })
}

pub(super) fn build_hk_structure_state(
    symbol: &str,
    store: &ObjectStore,
    setup: Option<&TacticalSetup>,
    track: Option<&HypothesisTrack>,
    backward: Option<&BackwardInvestigation>,
    hypothesis: Option<&Hypothesis>,
    context_prior: Option<&AgentContextPrior>,
    live_case: Option<&crate::live_snapshot::LiveTacticalCase>,
) -> Option<AgentStructureState> {
    let setup = setup?;
    let matched_pattern = live_case.and_then(|item| item.matched_success_pattern_signature.clone());
    Some(AgentStructureState {
        symbol: symbol.to_string(),
        sector: store
            .sector_name_for_symbol(&Symbol(symbol.to_string()))
            .map(str::to_string),
        setup_id: Some(setup.setup_id.clone()),
        title: setup.title.clone(),
        action: setup.action.to_string(),
        status: track.map(|item| item.status.as_str().to_string()),
        age_ticks: track.map(|item| item.age_ticks),
        status_streak: track.map(|item| item.status_streak),
        confidence: track
            .map(|item| item.confidence)
            .unwrap_or(setup.confidence),
        confidence_change: track.map(|item| item.confidence_change),
        confidence_gap: Some(
            track
                .map(|item| item.confidence_gap)
                .unwrap_or(setup.confidence_gap),
        ),
        transition_reason: track.and_then(|item| item.transition_reason.clone()),
        contest_state: backward.map(|item| item.contest_state.as_str().to_string()),
        current_leader: backward.and_then(|item| {
            item.leading_cause
                .as_ref()
                .map(|cause| cause.explanation.clone())
        }),
        leader_streak: backward.map(|item| item.leading_cause_streak),
        leader_transition_summary: append_pattern_fragment(
            backward.and_then(|item| item.leader_transition_summary.clone()),
            matched_pattern.clone(),
        ),
        thesis_family: hypothesis
            .map(|item| item.family_label.clone())
            .or_else(|| setup_family(setup)),
        action_expectancies: context_prior
            .map(|item| item.action_expectancies.clone())
            .unwrap_or_default(),
        expected_net_alpha: context_prior.map(|item| item.expected_net_alpha),
        alpha_horizon: Some(alpha_horizon_label(
            setup.horizon.primary.to_legacy_string(),
            10,
        )),
        invalidation_rule: primary_invalidation_rule(
            hypothesis,
            backward.and_then(|item| item.leading_falsifier.as_deref()),
            Some(setup),
        )
        .or_else(|| live_case.and_then(|item| item.multi_horizon_gate_reason.clone())),
    })
}

pub(super) fn append_pattern_fragment(
    base: Option<String>,
    matched_pattern: Option<String>,
) -> Option<String> {
    match (base, matched_pattern) {
        (Some(base), Some(pattern)) if !pattern.is_empty() => {
            Some(format!("{base} | pattern={pattern}"))
        }
        (None, Some(pattern)) if !pattern.is_empty() => Some(format!("pattern={pattern}")),
        (base, _) => base,
    }
}

