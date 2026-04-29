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

        if let Some(reason) = super::shared::multi_horizon_gate_reason(symbol) {
            notices.push(AgentNotice {
                notice_id: format!("lineage-gate:{}:{}", tick, symbol.symbol),
                tick,
                kind: "lineage_gate".into(),
                symbol: Some(symbol.symbol.clone()),
                sector: symbol.sector.clone(),
                title: format!("{} lineage gate", symbol.symbol),
                summary: format!("{} blocked: {}", symbol.symbol, reason),
                significance: symbol_priority(symbol)
                    .unwrap_or(Decimal::new(60, 2))
                    .max(Decimal::new(60, 2))
                    .min(Decimal::ONE),
            });
        }

        if let Some(reason) = super::shared::policy_reason(symbol) {
            let primary =
                super::shared::policy_primary(symbol).unwrap_or_else(|| "review_required".into());
            notices.push(AgentNotice {
                notice_id: format!("policy-gate:{}:{}", tick, symbol.symbol),
                tick,
                kind: "policy_gate".into(),
                symbol: Some(symbol.symbol.clone()),
                sector: symbol.sector.clone(),
                title: format!("{} policy {}", symbol.symbol, primary),
                summary: format!("{} policy[{}]: {}", symbol.symbol, primary, reason),
                significance: symbol_priority(symbol)
                    .unwrap_or(Decimal::new(55, 2))
                    .max(Decimal::new(55, 2))
                    .min(Decimal::ONE),
            });
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
    market: LiveMarket,
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
        if symbol_matches_market(market, &transition.symbol) {
            push_unique(&mut focus_symbols, transition.symbol.clone());
        }
    }
    for notice in &high_priority_notices {
        if let Some(symbol) = &notice.symbol {
            if symbol_matches_market(market, symbol) {
                push_unique(&mut focus_symbols, symbol.clone());
            }
        }
    }
    for signal in cross_market_signals.iter().take(3) {
        let candidate = match market {
            LiveMarket::Us => &signal.us_symbol,
            LiveMarket::Hk => &signal.hk_symbol,
        };
        if symbol_matches_market(market, candidate) {
            push_unique(&mut focus_symbols, candidate.clone());
        }
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
        .or_else(|| {
            high_priority_notices
                .first()
                .map(|item| item.summary.clone())
        })
        .or_else(|| {
            high_priority_notices
                .iter()
                .find(|item| {
                    item.symbol
                        .as_deref()
                        .map(|symbol| symbol_matches_market(market, symbol))
                        .unwrap_or(true)
                })
                .map(|item| item.summary.clone())
        });

    let priority = high_priority_notices
        .iter()
        .map(|item| item.significance)
        .chain(
            current_tick_transitions
                .iter()
                .map(|item| item.confidence.abs()),
        )
        .max()
        .unwrap_or(Decimal::ZERO);

    let should_speak = !current_tick_transitions.is_empty()
        || high_priority_notices
            .iter()
            .any(|item| item.significance >= speak_threshold);

    let mut suggested_tools = Vec::new();
    if should_speak {
        suggested_tools.push(AgentSuggestedToolCall {
            tool: "investigations".into(),
            args: json!({ "limit": 5 }),
            reason: "Start from the active investigations before compressing them into judgments."
                .into(),
        });
        suggested_tools.push(AgentSuggestedToolCall {
            tool: "judgments".into(),
            args: json!({ "limit": 5 }),
            reason: "Use operational judgments after reviewing the active investigations.".into(),
        });
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
                reason: format!(
                    "Check whether the order book confirms the move in {}",
                    symbol
                ),
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

/// Project HK momentum health (institutional flow / depth imbalance /
/// trade aggression peaks and collapses) into operator-readable lines.
///
/// Operates on raw `SignalMomentumEntry` refs so it can be called with any
/// subset of tracks (US convergence, HK institutional_flow, etc.) without
/// coupling this file to a specific tracker struct. Each input tuple is
/// (label, entry_option). Returns one line per track whose state is worse
/// than Healthy, capped to `max_symbols_per_track` symbols per track for
/// brevity.
pub(crate) fn describe_momentum_health<'a, I>(
    tracks: I,
    max_symbols_per_track: usize,
) -> Vec<String>
where
    I: IntoIterator<
        Item = (
            &'a str,
            &'a std::collections::HashMap<
                crate::ontology::objects::Symbol,
                crate::temporal::lineage::SignalMomentumEntry,
            >,
        ),
    >,
{
    let mut out = Vec::new();
    for (label, map) in tracks {
        let mut peaking = Vec::<String>::new();
        let mut collapsing = Vec::<String>::new();
        for (symbol, entry) in map {
            if entry.is_collapsing() {
                collapsing.push(symbol.0.clone());
            } else if entry.is_peaking() {
                peaking.push(symbol.0.clone());
            }
        }
        if !collapsing.is_empty() {
            collapsing.sort();
            let shown = collapsing
                .iter()
                .take(max_symbols_per_track)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            let suffix = if collapsing.len() > max_symbols_per_track {
                format!(" (+{} more)", collapsing.len() - max_symbols_per_track)
            } else {
                String::new()
            };
            out.push(format!(
                "{label} collapsing ({}): {shown}{suffix}",
                collapsing.len()
            ));
        }
        if !peaking.is_empty() {
            peaking.sort();
            let shown = peaking
                .iter()
                .take(max_symbols_per_track)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            let suffix = if peaking.len() > max_symbols_per_track {
                format!(" (+{} more)", peaking.len() - max_symbols_per_track)
            } else {
                String::new()
            };
            out.push(format!(
                "{label} peaking ({}): {shown}{suffix}",
                peaking.len()
            ));
        }
    }
    out
}

/// Project Y#2/Y#3 perception signals into operator-readable narrative strings
/// that can be injected into `AgentWakeState.reasons`.
///
/// Without this surfacing the whole persistent-state engine is invisible to the
/// analyst — the LLM reads `wake.reasons` / `wake.summary`, not raw
/// `cluster_states` / `perception_states`. For each output line the intent is
/// that a human reading it can immediately say "the market is doing X for Y
/// ticks with trend Z" without cross-referencing other fields.
///
/// Filters are intentionally conservative: only clusters with `age_ticks >= 3`
/// or a non-stable trend earn a line, because noisy single-tick flips would
/// drown out real signal. The exact thresholds are derived from
/// `state_persistence_ticks >= 2` (the raw-persistence gate already uses 2 as
/// its floor) and extended by one tick to let a trend register before we call
/// it out — not free parameters to tune per market.
pub(crate) fn derive_perception_reasons(
    world: Option<&LiveWorldSummary>,
    clusters: &[LiveClusterState],
    perceptions: &[AgentPerceptionState],
) -> Vec<String> {
    let mut out = Vec::new();

    if let Some(world) = world {
        let trend_tag = match world.trend.as_str() {
            "strengthening" | "weakening" => Some(world.trend.as_str()),
            _ => None,
        };
        let persists_meaningfully = world.age_ticks >= 3 || world.state_persistence_ticks >= 3;
        if persists_meaningfully || trend_tag.is_some() {
            let trend_suffix = match trend_tag {
                Some(tag) => format!(", {tag}"),
                None => String::new(),
            };
            out.push(format!(
                "world regime {} persisting {} ticks{}",
                world.regime, world.age_ticks, trend_suffix
            ));
        }
        if let Some(transition) = world.last_transition_summary.as_ref() {
            if world.state_persistence_ticks <= 2 {
                out.push(format!("world regime flip: {transition}"));
            }
        }
    }

    // Strengthening clusters: "Insurance cluster strengthening (5 members, age 31)"
    let mut strengthening = clusters
        .iter()
        .filter(|cluster| cluster.trend == "strengthening" && cluster.state != "low_information")
        .filter(|cluster| cluster.age_ticks >= 3 || cluster.state_persistence_ticks >= 3)
        .collect::<Vec<_>>();
    strengthening.sort_by(|a, b| b.confidence.cmp(&a.confidence));
    for cluster in strengthening.iter().take(2) {
        out.push(format!(
            "{} {} strengthening ({} members, age {})",
            cluster.label, cluster.state, cluster.member_count, cluster.age_ticks
        ));
    }

    // Weakening clusters
    let mut weakening = clusters
        .iter()
        .filter(|cluster| cluster.trend == "weakening" && cluster.state != "low_information")
        .filter(|cluster| cluster.age_ticks >= 3 || cluster.state_persistence_ticks >= 3)
        .collect::<Vec<_>>();
    weakening.sort_by(|a, b| b.confidence.cmp(&a.confidence));
    for cluster in weakening.iter().take(2) {
        out.push(format!(
            "{} {} weakening ({} members, age {})",
            cluster.label, cluster.state, cluster.member_count, cluster.age_ticks
        ));
    }

    // Freshly flipped clusters (state_persistence_ticks == 1 and have a transition)
    let mut freshly_flipped = clusters
        .iter()
        .filter(|cluster| {
            cluster.state_persistence_ticks <= 1
                && cluster.last_transition_summary.is_some()
                && cluster.state != "low_information"
                && cluster.member_count >= 2
        })
        .collect::<Vec<_>>();
    freshly_flipped.sort_by(|a, b| b.member_count.cmp(&a.member_count));
    for cluster in freshly_flipped.iter().take(2) {
        if let Some(transition) = cluster.last_transition_summary.as_ref() {
            out.push(format!("cluster flip: {transition}"));
        }
    }

    // Symbols demoted by absence this tick (Y#3 signal)
    let demoted = perceptions
        .iter()
        .filter(|state| {
            state
                .reason_codes
                .iter()
                .any(|code| code == "demoted_by_absence")
        })
        .collect::<Vec<_>>();
    if !demoted.is_empty() {
        let names = demoted
            .iter()
            .take(5)
            .map(|state| state.symbol.clone())
            .collect::<Vec<_>>()
            .join(", ");
        let suffix = if demoted.len() > 5 {
            format!(" (+{} more)", demoted.len() - 5)
        } else {
            String::new()
        };
        out.push(format!(
            "{} symbols demoted by absence: {names}{suffix}",
            demoted.len()
        ));
    }

    // Symbols whose peer confirmation withdrew this tick (reason_codes carries
    // the state_kind `turning_point` alongside no positive peer evidence —
    // hard to disambiguate pure turning_point from withdrawal without the
    // full evidence graph, so we surface all newly-turned symbols that had
    // explicit peer-related evidence in their reason codes).
    let peer_withdrew = perceptions
        .iter()
        .filter(|state| {
            state.state_kind == "turning_point"
                && state.state_persistence_ticks <= 1
                && state
                    .reason_codes
                    .iter()
                    .any(|code| code.contains("peer") || code.contains("expectation_missed"))
        })
        .collect::<Vec<_>>();
    if !peer_withdrew.is_empty() {
        let names = peer_withdrew
            .iter()
            .take(3)
            .map(|state| state.symbol.clone())
            .collect::<Vec<_>>()
            .join(", ");
        out.push(format!("peer confirmation withdrew: {names}"));
    }

    out
}

fn symbol_matches_market(market: LiveMarket, symbol: &str) -> bool {
    match market {
        LiveMarket::Us => symbol.ends_with(".US"),
        LiveMarket::Hk => symbol.ends_with(".HK"),
    }
}
