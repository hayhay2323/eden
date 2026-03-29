use super::*;

pub(crate) fn build_sector_flows(symbols: &[AgentSymbolState]) -> Vec<AgentSectorFlow> {
    let mut grouped: HashMap<String, Vec<&AgentSymbolState>> = HashMap::new();
    for symbol in symbols {
        let Some(sector) = &symbol.sector else {
            continue;
        };
        if symbol.signal.is_none() {
            continue;
        }
        grouped.entry(sector.clone()).or_default().push(symbol);
    }

    let mut flows = grouped
        .into_iter()
        .map(|(sector, items)| {
            let member_count = items.len();
            let average_composite = items
                .iter()
                .filter_map(|item| item.signal.as_ref().map(|signal| signal.composite))
                .sum::<Decimal>()
                / Decimal::from(member_count.max(1) as i64);
            let average_capital_flow = items
                .iter()
                .filter_map(|item| {
                    item.signal
                        .as_ref()
                        .map(|signal| signal.capital_flow_direction)
                })
                .sum::<Decimal>()
                / Decimal::from(member_count.max(1) as i64);

            let mut leaders = items
                .iter()
                .filter_map(|item| {
                    item.signal
                        .as_ref()
                        .map(|signal| (item.symbol.clone(), signal.composite.abs()))
                })
                .collect::<Vec<_>>();
            leaders.sort_by(|a, b| b.1.cmp(&a.1));
            let leaders = leaders
                .into_iter()
                .take(3)
                .map(|item| item.0)
                .collect::<Vec<_>>();

            let exceptions = items
                .iter()
                .filter_map(|item| {
                    let composite = item.signal.as_ref()?.composite;
                    let sector_sign = decimal_sign(average_composite);
                    let symbol_sign = decimal_sign(composite);
                    (sector_sign != 0 && symbol_sign != 0 && sector_sign != symbol_sign)
                        .then(|| item.symbol.clone())
                })
                .collect::<Vec<_>>();

            let summary = format!(
                "{} avg={:+} cap_flow={:+} leaders={}",
                sector,
                average_composite.round_dp(3),
                average_capital_flow.round_dp(3),
                leaders.join(", "),
            );

            AgentSectorFlow {
                sector,
                member_count,
                average_composite,
                average_capital_flow,
                leaders,
                exceptions,
                summary,
            }
        })
        .collect::<Vec<_>>();

    flows.sort_by(|a, b| {
        b.average_composite
            .abs()
            .cmp(&a.average_composite.abs())
            .then_with(|| a.sector.cmp(&b.sector))
    });
    flows
}

pub(crate) fn collect_active_structures(symbols: &[AgentSymbolState]) -> Vec<AgentStructureState> {
    let mut items = symbols
        .iter()
        .filter_map(|item| item.structure.clone())
        .filter(|item| {
            item.action != "observe"
                || item.transition_reason.is_some()
                || matches!(item.status.as_deref(), Some("strengthening" | "weakening"))
                || item.leader_transition_summary.is_some()
        })
        .collect::<Vec<_>>();
    items.sort_by(|a, b| {
        structure_attention_priority(b)
            .cmp(&structure_attention_priority(a))
            .then_with(|| b.confidence.cmp(&a.confidence))
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    items.truncate(64);
    items
}

fn structure_attention_priority(item: &AgentStructureState) -> i32 {
    let action_priority = match item.action.as_str() {
        "enter" | "add" | "hedge" | "trim" => 3,
        "review" => 2,
        "observe" => 1,
        _ => 0,
    };
    let status_priority = match item.status.as_deref() {
        Some("weakening") | Some("strengthening") => 1,
        _ => 0,
    };
    action_priority * 10 + status_priority
}

pub(crate) fn build_hk_notices(
    tick: u64,
    transitions: &[AgentTransition],
    symbols: &[AgentSymbolState],
    sectors: &[AgentSectorFlow],
    events: &[LiveEvent],
) -> Vec<AgentNotice> {
    let mut notices = Vec::new();

    for transition in transitions
        .iter()
        .filter(|item| item.to_tick == tick)
        .take(12)
    {
        notices.push(AgentNotice {
            notice_id: format!(
                "transition:{}:{}:{}",
                tick, transition.symbol, transition.to_state
            ),
            tick,
            kind: "transition".into(),
            symbol: Some(transition.symbol.clone()),
            sector: transition.sector.clone(),
            title: format!("{} transition", transition.symbol),
            summary: transition.summary.clone(),
            significance: transition.confidence.abs(),
        });
    }

    for symbol in symbols.iter().take(12) {
        if let Some(invalidation) = &symbol.invalidation {
            if invalidation.invalidated {
                notices.push(AgentNotice {
                    notice_id: format!("invalidated:{}:{}", tick, symbol.symbol),
                    tick,
                    kind: "invalidation".into(),
                    symbol: Some(symbol.symbol.clone()),
                    sector: symbol.sector.clone(),
                    title: format!("{} invalidated", symbol.symbol),
                    summary: invalidation
                        .transition_reason
                        .clone()
                        .unwrap_or_else(|| "leading hypothesis invalidated".into()),
                    significance: Decimal::ONE,
                });
            }
        }

        if let Some(depth) = &symbol.depth {
            if depth.imbalance_change.abs() >= Decimal::new(15, 2)
                || depth.bid_total_volume_change.abs() >= 10_000
                || depth.ask_total_volume_change.abs() >= 10_000
            {
                notices.push(AgentNotice {
                    notice_id: format!("depth:{}:{}", tick, symbol.symbol),
                    tick,
                    kind: "depth_shift".into(),
                    symbol: Some(symbol.symbol.clone()),
                    sector: symbol.sector.clone(),
                    title: format!("{} depth shifted", symbol.symbol),
                    summary: depth.summary.clone(),
                    significance: depth.imbalance_change.abs().min(Decimal::ONE),
                });
            }
        }

        if let Some(brokers) = &symbol.brokers {
            if !brokers.entered.is_empty() || !brokers.exited.is_empty() {
                notices.push(AgentNotice {
                    notice_id: format!("brokers:{}:{}", tick, symbol.symbol),
                    tick,
                    kind: "broker_movement".into(),
                    symbol: Some(symbol.symbol.clone()),
                    sector: symbol.sector.clone(),
                    title: format!("{} broker queue changed", symbol.symbol),
                    summary: format!(
                        "entered=[{}] exited=[{}]",
                        brokers.entered.join(", "),
                        brokers.exited.join(", "),
                    ),
                    significance: Decimal::new(
                        (brokers.entered.len() + brokers.exited.len()) as i64,
                        1,
                    )
                    .min(Decimal::ONE),
                });
            }
        }
    }

    for sector in sectors
        .iter()
        .filter(|item| !item.exceptions.is_empty())
        .take(6)
    {
        notices.push(AgentNotice {
            notice_id: format!("sector:{}:{}", tick, sector.sector),
            tick,
            kind: "sector_divergence".into(),
            symbol: None,
            sector: Some(sector.sector.clone()),
            title: format!("{} divergence", sector.sector),
            summary: format!("exceptions: {}", sector.exceptions.join(", ")),
            significance: sector.average_composite.abs().min(Decimal::ONE),
        });
    }

    for event in events.iter().take(6) {
        notices.push(AgentNotice {
            notice_id: format!("event:{}:{}", tick, event.kind),
            tick,
            kind: "market_event".into(),
            symbol: extract_symbols(&event.summary).into_iter().next(),
            sector: None,
            title: event.kind.clone(),
            summary: event.summary.clone(),
            significance: event.magnitude.abs().min(Decimal::ONE),
        });
    }

    notices.sort_by(|a, b| {
        b.significance
            .cmp(&a.significance)
            .then_with(|| a.notice_id.cmp(&b.notice_id))
    });
    notices.truncate(24);
    notices
}

pub(crate) fn build_us_notices(
    tick: u64,
    transitions: &[AgentTransition],
    symbols: &[AgentSymbolState],
    sectors: &[AgentSectorFlow],
    events: &[LiveEvent],
    cross_market_signals: &[LiveCrossMarketSignal],
) -> Vec<AgentNotice> {
    let mut notices = build_hk_notices(tick, transitions, symbols, sectors, events);
    for signal in cross_market_signals.iter().take(6) {
        notices.push(AgentNotice {
            notice_id: format!("cross:{}:{}:{}", tick, signal.us_symbol, signal.hk_symbol),
            tick,
            kind: "cross_market_signal".into(),
            symbol: Some(signal.us_symbol.clone()),
            sector: None,
            title: format!("{} <- {}", signal.us_symbol, signal.hk_symbol),
            summary: format!(
                "{} responding to {} with conf={:+}",
                signal.us_symbol,
                signal.hk_symbol,
                signal.propagation_confidence.round_dp(3)
            ),
            significance: signal.propagation_confidence.abs().min(Decimal::ONE),
        });
    }
    notices.sort_by(|a, b| {
        b.significance
            .cmp(&a.significance)
            .then_with(|| a.notice_id.cmp(&b.notice_id))
    });
    notices.truncate(24);
    notices
}

pub(crate) fn build_wake_state(
    tick: u64,
    notices: &[AgentNotice],
    transitions: &[AgentTransition],
    symbols: &[AgentSymbolState],
    sectors: &[AgentSectorFlow],
    cross_market_signals: &[LiveCrossMarketSignal],
) -> AgentWakeState {
    let speak_threshold = Decimal::new(55, 2);
    let current_tick_transitions = transitions
        .iter()
        .filter(|item| item.to_tick == tick)
        .collect::<Vec<_>>();
    let high_priority_notices = notices
        .iter()
        .filter(|item| {
            item.significance >= speak_threshold
                || matches!(
                    item.kind.as_str(),
                    "transition" | "invalidation" | "cross_market_signal" | "sector_divergence"
                )
        })
        .collect::<Vec<_>>();

    let mut focus_symbols = Vec::new();
    for transition in &current_tick_transitions {
        push_unique(&mut focus_symbols, transition.symbol.clone());
    }
    for notice in &high_priority_notices {
        if let Some(symbol) = &notice.symbol {
            push_unique(&mut focus_symbols, symbol.clone());
        }
    }
    for signal in cross_market_signals.iter().take(3) {
        push_unique(&mut focus_symbols, signal.us_symbol.clone());
    }
    focus_symbols.truncate(4);

    let mut reasons = high_priority_notices
        .iter()
        .take(5)
        .map(|item| item.summary.clone())
        .collect::<Vec<_>>();
    if reasons.is_empty() {
        reasons.extend(
            sectors
                .iter()
                .filter(|item| !item.exceptions.is_empty())
                .take(2)
                .map(|item| {
                    format!(
                        "{} diverging names: {}",
                        item.sector,
                        item.exceptions.join(", ")
                    )
                }),
        );
    }

    let summary = if !current_tick_transitions.is_empty() {
        current_tick_transitions
            .iter()
            .take(4)
            .map(|item| item.summary.clone())
            .collect::<Vec<_>>()
    } else {
        reasons.iter().take(4).cloned().collect::<Vec<_>>()
    };

    let headline = current_tick_transitions
        .first()
        .map(|item| item.summary.clone())
        .or_else(|| high_priority_notices.first().map(|item| item.summary.clone()));

    let priority = high_priority_notices
        .iter()
        .map(|item| item.significance)
        .chain(current_tick_transitions.iter().map(|item| item.confidence.abs()))
        .max()
        .unwrap_or(Decimal::ZERO);

    let should_speak = !current_tick_transitions.is_empty()
        || high_priority_notices
            .iter()
            .any(|item| item.significance >= speak_threshold);

    let mut suggested_tools = Vec::new();
    if should_speak {
        suggested_tools.push(AgentSuggestedToolCall {
            tool: "market_session".into(),
            args: json!({}),
            reason: "Start from the canonical market session object before derived analyst views."
                .into(),
        });
        suggested_tools.push(AgentSuggestedToolCall {
            tool: "watchlist".into(),
            args: json!({ "limit": 5 }),
            reason: "Start with the ranked watchlist instead of flattening every transition."
                .into(),
        });
        suggested_tools.push(AgentSuggestedToolCall {
            tool: "recommendations".into(),
            args: json!({ "limit": 5 }),
            reason: "Check the standardized regime-bound action policy before drilling down."
                .into(),
        });
        suggested_tools.push(AgentSuggestedToolCall {
            tool: "transitions_since".into(),
            args: json!({
                "since_tick": tick.saturating_sub(1),
                "limit": 10
            }),
            reason: "Review only the newest transitions after ranking the top names.".into(),
        });
    }
    for symbol in focus_symbols.iter().take(2) {
        suggested_tools.push(AgentSuggestedToolCall {
            tool: "symbol_contract".into(),
            args: json!({ "symbol": symbol }),
            reason: format!("Inspect the full state for {}", symbol),
        });
        if symbols
            .iter()
            .find(|item| item.symbol == *symbol)
            .and_then(|item| item.depth.as_ref())
            .is_some()
        {
            suggested_tools.push(AgentSuggestedToolCall {
                tool: "depth_change".into(),
                args: json!({ "symbol": symbol }),
                reason: format!("Check whether the order book confirms the move in {}", symbol),
            });
        }
        if symbols
            .iter()
            .find(|item| item.symbol == *symbol)
            .and_then(|item| item.brokers.as_ref())
            .is_some()
        {
            suggested_tools.push(AgentSuggestedToolCall {
                tool: "broker_movement".into(),
                args: json!({ "symbol": symbol }),
                reason: format!("Check whether new institutions entered {}", symbol),
            });
        }
    }
    if let Some(sector) = sectors.iter().find(|item| !item.exceptions.is_empty()) {
        suggested_tools.push(AgentSuggestedToolCall {
            tool: "sector_flow".into(),
            args: json!({ "sector": sector.sector }),
            reason: "Inspect the broader sector context behind the divergence.".into(),
        });
    }
    sort_suggested_tool_calls(&mut suggested_tools);
    suggested_tools.truncate(6);

    AgentWakeState {
        should_speak,
        priority,
        headline,
        summary,
        focus_symbols,
        reasons,
        suggested_tools,
    }
}
