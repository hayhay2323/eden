use super::*;

pub(super) fn order_direction_label(direction: OrderDirection) -> &'static str {
    match direction {
        OrderDirection::Buy => "buy",
        OrderDirection::Sell => "sell",
    }
}

pub(super) fn build_action_workflows(
    timestamp: time::OffsetDateTime,
    suggestions: &[eden::graph::decision::OrderSuggestion],
    active_fps: &[StructuralFingerprint],
    degradations: &HashMap<Symbol, eden::graph::decision::StructuralDegradation>,
    event_snapshot: &EventSnapshot,
    tracks: &[HypothesisTrack],
    setups: &[TacticalSetup],
) -> (
    Vec<ActionWorkflowSnapshot>,
    Vec<ActionWorkflowRecord>,
    Vec<ActionWorkflowEventRecord>,
) {
    let mut snapshots = Vec::new();
    let mut records = Vec::new();
    let mut events = Vec::new();

    for suggestion in suggestions {
        let track = symbol_track(&suggestion.symbol, tracks);
        let setup = symbol_setup(&suggestion.symbol, setups);
        let gate_reason =
            suggestion_gate_reason(&suggestion.symbol, suggestion, event_snapshot, track);
        let descriptor = ActionDescriptor::new(
            format!(
                "order:{}:{}",
                suggestion.symbol,
                order_direction_label(suggestion.direction)
            ),
            format!(
                "{} {}",
                order_direction_label(suggestion.direction).to_uppercase(),
                suggestion.symbol
            ),
            serde_json::json!({
                "symbol": suggestion.symbol,
                "direction": order_direction_label(suggestion.direction),
                "suggested_quantity": suggestion.suggested_quantity,
                "price_low": suggestion.price_low,
                "price_high": suggestion.price_high,
                "composite": suggestion.convergence.composite,
                "requires_confirmation": suggestion.requires_confirmation,
                "track_status": track.map(|track| track.status.as_str()),
                "track_streak": track.map(|track| track.status_streak),
                "decision_lineage": setup.map(|setup| serde_json::to_value(&setup.lineage).unwrap_or(serde_json::Value::Null)),
            }),
        );
        let suggested = SuggestedAction::new(
            descriptor,
            timestamp,
            Some("eden".into()),
            Some(gate_reason.clone().unwrap_or_else(|| {
                setup
                    .and_then(workflow_lineage_summary)
                    .unwrap_or_else(|| "generated from convergence pipeline".into())
            })),
        );
        let suggested_snapshot = ActionWorkflowSnapshot::from_state(&suggested);
        events.push(ActionWorkflowEventRecord::from_snapshot(
            &suggested_snapshot,
        ));

        if gate_reason.is_some() {
            snapshots.push(suggested_snapshot);
            records.push(ActionWorkflowRecord::from_state(&suggested));
        } else {
            let confirmed = suggested.clone().confirm(
                timestamp,
                Some("eden-auto".into()),
                Some(
                    setup
                        .and_then(workflow_lineage_summary)
                        .or_else(|| track.and_then(|track| track.transition_reason.clone()))
                        .unwrap_or_else(|| "auto-confirmed by structural consensus".into()),
                ),
            );
            events.push(ActionWorkflowEventRecord::from_transition(
                &suggested, &confirmed,
            ));
            snapshots.push(ActionWorkflowSnapshot::from_state(&confirmed));
            records.push(ActionWorkflowRecord::from_state(&confirmed));
        }
    }

    for fingerprint in active_fps {
        let track = symbol_track(&fingerprint.symbol, tracks);
        let setup = symbol_setup(&fingerprint.symbol, setups);
        let review_reason =
            position_review_reason(&fingerprint.symbol, degradations, event_snapshot, track);
        let descriptor = ActionDescriptor::new(
            format!("position:{}", fingerprint.symbol),
            format!("Position {}", fingerprint.symbol),
            serde_json::json!({
                "symbol": fingerprint.symbol,
                "entry_timestamp": fingerprint.entry_timestamp,
                "entry_composite": fingerprint.entry_composite,
                "entry_regime": fingerprint.entry_regime.to_string(),
                "track_status": track.map(|track| track.status.as_str()),
                "track_action": track.map(|track| track.action.as_str()),
                "decision_lineage": setup.map(|setup| serde_json::to_value(&setup.lineage).unwrap_or(serde_json::Value::Null)),
            }),
        );
        let suggested = SuggestedAction::new(
            descriptor,
            fingerprint.entry_timestamp,
            Some("tracker".into()),
            Some(
                setup
                    .and_then(workflow_lineage_summary)
                    .unwrap_or_else(|| "position entered".into()),
            ),
        );
        events.push(ActionWorkflowEventRecord::from_snapshot(
            &ActionWorkflowSnapshot::from_state(&suggested),
        ));
        let confirmed = suggested.clone().confirm(
            fingerprint.entry_timestamp,
            Some("tracker".into()),
            Some("position acknowledged".into()),
        );
        events.push(ActionWorkflowEventRecord::from_transition(
            &suggested, &confirmed,
        ));
        let executed = confirmed.clone().execute(
            fingerprint.entry_timestamp,
            Some("tracker".into()),
            Some("position active".into()),
        );
        events.push(ActionWorkflowEventRecord::from_transition(
            &confirmed, &executed,
        ));
        let monitored = executed.clone().monitor(
            timestamp,
            Some("eden".into()),
            Some("position still monitored".into()),
        );
        events.push(ActionWorkflowEventRecord::from_transition(
            &executed, &monitored,
        ));

        if let Some(reason) = review_reason {
            let reviewed = monitored
                .clone()
                .review(timestamp, Some("eden".into()), Some(reason));
            events.push(ActionWorkflowEventRecord::from_transition(
                &monitored, &reviewed,
            ));
            snapshots.push(ActionWorkflowSnapshot::from_state(&reviewed));
            records.push(ActionWorkflowRecord::from_state(&reviewed));
            continue;
        }

        snapshots.push(ActionWorkflowSnapshot::from_state(&monitored));
        records.push(ActionWorkflowRecord::from_state(&monitored));
    }

    (snapshots, records, events)
}

pub(super) fn suggestion_gate_reason(
    symbol: &Symbol,
    suggestion: &eden::graph::decision::OrderSuggestion,
    event_snapshot: &EventSnapshot,
    track: Option<&HypothesisTrack>,
) -> Option<String> {
    if suggestion.requires_confirmation {
        return Some("manual confirmation required by decision policy".into());
    }

    if let Some(track) = track {
        if track.action != "enter" {
            return Some(
                track
                    .transition_reason
                    .clone()
                    .unwrap_or_else(|| track.policy_reason.clone()),
            );
        }
    }

    critical_event_reason(symbol, event_snapshot)
}

pub(super) fn position_review_reason(
    symbol: &Symbol,
    degradations: &HashMap<Symbol, eden::graph::decision::StructuralDegradation>,
    event_snapshot: &EventSnapshot,
    track: Option<&HypothesisTrack>,
) -> Option<String> {
    if let Some(track) = track {
        if matches!(track.status.as_str(), "weakening" | "invalidated") || track.action == "review"
        {
            return Some(
                track
                    .transition_reason
                    .clone()
                    .unwrap_or_else(|| track.policy_reason.clone()),
            );
        }
    }

    if let Some(degradation) = degradations.get(symbol) {
        if degradation.composite_degradation >= Decimal::new(45, 2) {
            return Some(format!(
                "structural degradation reached {}",
                degradation.composite_degradation.round_dp(2)
            ));
        }
    }

    critical_event_reason(symbol, event_snapshot)
}

pub(super) fn symbol_track<'a>(
    symbol: &Symbol,
    tracks: &'a [HypothesisTrack],
) -> Option<&'a HypothesisTrack> {
    tracks.iter().find(|track| {
        matches!(
            &track.scope,
            eden::ReasoningScope::Symbol(track_symbol) if track_symbol == symbol
        ) && track.invalidated_at.is_none()
    })
}

pub(super) fn symbol_setup<'a>(
    symbol: &Symbol,
    setups: &'a [TacticalSetup],
) -> Option<&'a TacticalSetup> {
    setups.iter().find(|setup| {
        matches!(
            &setup.scope,
            eden::ReasoningScope::Symbol(setup_symbol) if setup_symbol == symbol
        )
    })
}

pub(super) fn workflow_lineage_summary(setup: &TacticalSetup) -> Option<String> {
    if !setup.lineage.promoted_by.is_empty() {
        Some(format!(
            "promoted_by {}",
            setup.lineage.promoted_by.join(" + ")
        ))
    } else if !setup.lineage.blocked_by.is_empty() {
        Some(format!(
            "blocked_by {}",
            setup.lineage.blocked_by.join(" + ")
        ))
    } else if !setup.lineage.based_on.is_empty() {
        Some(format!("based_on {}", setup.lineage.based_on.join(" + ")))
    } else if !setup.lineage.falsified_by.is_empty() {
        Some(format!(
            "falsified_by {}",
            setup.lineage.falsified_by.join(" + ")
        ))
    } else {
        None
    }
}

pub(super) fn critical_event_reason(
    symbol: &Symbol,
    event_snapshot: &EventSnapshot,
) -> Option<String> {
    event_snapshot.events.iter().find_map(|event| {
    let symbol_match = matches!(&event.value.scope, SignalScope::Symbol(event_symbol) if event_symbol == symbol);
    let market_match = matches!(event.value.scope, SignalScope::Market);
    let is_critical = matches!(
        event.value.kind,
        MarketEventKind::InstitutionalFlip
            | MarketEventKind::StressRegimeShift
            | MarketEventKind::MarketStressElevated
            | MarketEventKind::ManualReviewRequired
    );

    if is_critical && (symbol_match || market_match) {
        Some(event.value.summary.clone())
    } else {
        None
    }
})
}

pub(super) struct HkActionStage {
    pub(super) actionable_order_suggestions: Vec<eden::graph::decision::OrderSuggestion>,
    pub(super) newly_entered: Vec<Symbol>,
    pub(super) workflow_snapshots: Vec<ActionWorkflowSnapshot>,
    pub(super) workflow_records: Vec<ActionWorkflowRecord>,
    pub(super) workflow_events: Vec<ActionWorkflowEventRecord>,
}

pub(super) fn compute_readiness(links: &LinkSnapshot) -> ReadinessReport {
    let quoted_symbols: HashSet<Symbol> = links
        .quotes
        .iter()
        .filter(|q| q.last_done > Decimal::ZERO)
        .map(|q| q.symbol.clone())
        .collect();
    let order_book_symbols: HashSet<Symbol> = links
        .order_books
        .iter()
        .filter(|ob| ob.total_bid_volume + ob.total_ask_volume > 0)
        .map(|ob| ob.symbol.clone())
        .collect();

    let mut context_symbols: HashSet<Symbol> = HashSet::new();
    context_symbols.extend(links.calc_indexes.iter().map(|obs| obs.symbol.clone()));
    context_symbols.extend(
        links
            .candlesticks
            .iter()
            .filter(|obs| obs.candle_count >= 2)
            .map(|obs| obs.symbol.clone()),
    );
    context_symbols.extend(links.capital_flows.iter().map(|obs| obs.symbol.clone()));
    context_symbols.extend(
        links
            .capital_breakdowns
            .iter()
            .map(|obs| obs.symbol.clone()),
    );

    let ready_symbols = quoted_symbols
        .iter()
        .filter(|symbol| order_book_symbols.contains(*symbol) && context_symbols.contains(*symbol))
        .cloned()
        .collect();

    ReadinessReport {
        ready_symbols,
        quote_symbols: quoted_symbols.len(),
        order_book_symbols: order_book_symbols.len(),
        context_symbols: context_symbols.len(),
    }
}

pub(super) fn build_hk_action_stage(
    now: time::OffsetDateTime,
    brain: &BrainGraph,
    tracker: &mut PositionTracker,
    readiness: &ReadinessReport,
    decision: &DecisionSnapshot,
    ready_convergence_scores: &HashMap<Symbol, eden::graph::decision::ConvergenceScore>,
    ready_order_suggestions: &[eden::graph::decision::OrderSuggestion],
    aged_degradations: &HashMap<Symbol, eden::graph::decision::StructuralDegradation>,
    event_snapshot: &EventSnapshot,
    reasoning_snapshot: &ReasoningSnapshot,
) -> HkActionStage {
    let actionable_setups = reasoning_snapshot
        .tactical_setups
        .iter()
        .filter(|setup| setup.action == "enter")
        .collect::<Vec<_>>();
    let actionable_symbols: HashSet<Symbol> = actionable_setups
        .iter()
        .filter_map(|setup| match &setup.scope {
            eden::ReasoningScope::Symbol(symbol) => Some(symbol.clone()),
            _ => None,
        })
        .collect();
    let actionable_order_suggestions = ready_order_suggestions
        .iter()
        .filter(|suggestion| actionable_symbols.contains(&suggestion.symbol))
        .cloned()
        .collect::<Vec<_>>();

    let zero_syms: Vec<Symbol> = tracker
        .active_fingerprints()
        .iter()
        .filter(|fp| {
            readiness.ready_symbols.contains(&fp.symbol)
                && decision
                    .convergence_scores
                    .get(&fp.symbol)
                    .map(|c| c.composite == Decimal::ZERO)
                    .unwrap_or(true)
        })
        .map(|fp| fp.symbol.clone())
        .collect();
    for sym in &zero_syms {
        tracker.exit(sym);
    }

    let newly_entered =
        tracker.auto_enter_allowed(ready_convergence_scores, Some(&actionable_symbols), brain);
    let (workflow_snapshots, workflow_records, workflow_events) = build_action_workflows(
        now,
        &actionable_order_suggestions,
        &tracker.active_fingerprints(),
        aged_degradations,
        event_snapshot,
        &reasoning_snapshot.hypothesis_tracks,
        &reasoning_snapshot.tactical_setups,
    );

    HkActionStage {
        actionable_order_suggestions,
        newly_entered,
        workflow_snapshots,
        workflow_records,
        workflow_events,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn setup(symbol: &str, action: &str) -> TacticalSetup {
        TacticalSetup {
            setup_id: format!("setup:{symbol}"),
            hypothesis_id: format!("hyp:{symbol}:flow"),
            runner_up_hypothesis_id: None,
            provenance: eden::ProvenanceMetadata::new(
                eden::ProvenanceSource::Computed,
                time::OffsetDateTime::UNIX_EPOCH,
            ),
            lineage: eden::DecisionLineage::default(),
            scope: eden::ReasoningScope::Symbol(Symbol(symbol.into())),
            title: format!("Long {symbol}"),
            action: action.into(),
            time_horizon: "intraday".into(),
            confidence: dec!(0.7),
            confidence_gap: dec!(0.2),
            heuristic_edge: dec!(0.1),
            convergence_score: Some(dec!(0.4)),
            convergence_detail: None,
            workflow_id: Some(format!("order:{symbol}:buy")),
            entry_rationale: "flow leads".into(),
            causal_narrative: None,
            risk_notes: vec![],
            review_reason_code: None,
            policy_verdict: None,
        }
    }

    fn track(symbol: &str, action: &str) -> HypothesisTrack {
        HypothesisTrack {
            track_id: format!("track:{symbol}"),
            setup_id: format!("setup:{symbol}"),
            hypothesis_id: format!("hyp:{symbol}:flow"),
            runner_up_hypothesis_id: None,
            scope: eden::ReasoningScope::Symbol(Symbol(symbol.into())),
            title: format!("Long {symbol}"),
            action: action.into(),
            status: eden::HypothesisTrackStatus::Stable,
            age_ticks: 6,
            status_streak: 3,
            confidence: dec!(0.7),
            previous_confidence: Some(dec!(0.7)),
            confidence_change: Decimal::ZERO,
            confidence_gap: dec!(0.2),
            previous_confidence_gap: Some(dec!(0.2)),
            confidence_gap_change: Decimal::ZERO,
            heuristic_edge: dec!(0.1),
            policy_reason: "holding enter".into(),
            transition_reason: None,
            first_seen_at: time::OffsetDateTime::UNIX_EPOCH,
            last_updated_at: time::OffsetDateTime::UNIX_EPOCH,
            invalidated_at: None,
        }
    }

    fn reasoning_snapshot(symbol: &str, action: &str) -> ReasoningSnapshot {
        ReasoningSnapshot {
            timestamp: time::OffsetDateTime::UNIX_EPOCH,
            hypotheses: vec![],
            propagation_paths: vec![],
            investigation_selections: vec![],
            tactical_setups: vec![setup(symbol, action)],
            hypothesis_tracks: vec![track(symbol, action)],
            case_clusters: vec![],
        }
    }

}
