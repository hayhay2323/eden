use super::*;
use crate::action::workflow::{ActionExecutionPolicy, ActionGovernanceContract};
use crate::ontology::ActionNodeStage;

fn pattern_phrase(signature: Option<&str>) -> Option<String> {
    signature
        .filter(|value| !value.is_empty())
        .map(|value| format!("matched historical pattern {value}"))
}

fn thread_pattern_note(symbol_state: Option<&AgentSymbolState>) -> Option<String> {
    symbol_state
        .and_then(super::shared::matched_success_pattern_signature)
        .map(|signature| format!("pattern={signature}"))
}

fn append_thread_pattern(
    base: Option<String>,
    symbol_state: Option<&AgentSymbolState>,
) -> Option<String> {
    match (base, thread_pattern_note(symbol_state)) {
        (Some(base), Some(pattern)) if !base.contains(&pattern) => {
            Some(format!("{base} | {pattern}"))
        }
        (Some(base), _) => Some(base),
        (None, Some(pattern)) => Some(pattern),
        (None, None) => None,
    }
}

pub fn build_briefing(snapshot: &AgentSnapshot) -> AgentBriefing {
    let mut executed_tools = Vec::new();
    let mut current_investigations = Vec::new();
    let mut current_judgments = Vec::new();
    for suggested in snapshot.wake.suggested_tools.iter().take(6) {
        let request = tool_request_from_suggested(suggested);
        if let Ok(result) = execute_tool(snapshot, None, &request) {
            if let AgentToolOutput::Investigations(investigations) = &result {
                current_investigations = investigations.items.clone();
            }
            if let AgentToolOutput::Judgments(judgments) = &result {
                current_judgments = judgments.items.clone();
            }
            executed_tools.push(AgentExecutedTool {
                tool: suggested.tool.clone(),
                args: suggested.args.clone(),
                preview: result.preview(),
                result: result.as_json(),
            });
        }
    }
    normalize_workflow_surface_items(
        snapshot,
        &mut current_investigations,
        &mut current_judgments,
    );

    let preferred_judgment_summary = current_judgments
        .iter()
        .find(|item| item.object_kind != "cross_market_dependency")
        .or_else(|| current_judgments.first())
        .map(|item| item.summary.clone());
    let preferred_investigation_summary = current_investigations
        .iter()
        .find(|item| item.object_kind != "cross_market_dependency")
        .or_else(|| current_investigations.first())
        .map(|item| item.summary.clone());
    let preferred_workflow_summary =
        preferred_workflow_focus_summary(snapshot, &current_investigations, &current_judgments);

    let mut summary = snapshot.wake.summary.clone();
    let pattern_summary = snapshot
        .symbols
        .iter()
        .find_map(super::shared::matched_success_pattern_signature)
        .as_deref()
        .and_then(|signature| pattern_phrase(Some(signature)));
    if let Some(pattern_summary) = pattern_summary.clone() {
        if !summary.iter().any(|item| item == &pattern_summary) {
            summary.insert(0, pattern_summary);
        }
    }
    if let Some(summary_line) = &preferred_investigation_summary {
        if !summary.iter().any(|item| item == summary_line) {
            summary.insert(0, summary_line.clone());
        }
    }
    if let Some(summary_line) = &preferred_judgment_summary {
        if !summary.iter().any(|item| item == summary_line) {
            summary.insert(0, summary_line.clone());
        }
    }
    if let Some(summary_line) = &preferred_workflow_summary {
        if !summary.iter().any(|item| item == summary_line) {
            summary.insert(0, summary_line.clone());
        }
    }
    for preview in executed_tools
        .iter()
        .filter_map(|item| item.preview.clone())
    {
        if !summary.iter().any(|item| item == &preview) {
            summary.push(preview);
        }
    }
    summary.truncate(6);

    let mut reasons = snapshot.wake.reasons.clone();
    for investigation in current_investigations.iter().take(5) {
        if !reasons.iter().any(|item| item == &investigation.summary) {
            reasons.insert(0, investigation.summary.clone());
        }
    }
    for judgment in current_judgments.iter().take(5) {
        if !reasons.iter().any(|item| item == &judgment.summary) {
            reasons.insert(0, judgment.summary.clone());
        }
    }
    if let Some(summary_line) = &preferred_workflow_summary {
        if !reasons.iter().any(|item| item == summary_line) {
            reasons.insert(0, summary_line.clone());
        }
    }
    reasons.truncate(6);

    let spoken_message = if snapshot.wake.should_speak {
        let mut lines = Vec::new();
        if let Some(headline) = preferred_workflow_summary
            .clone()
            .or(preferred_judgment_summary.clone())
            .clone()
            .or_else(|| snapshot.wake.headline.clone())
        {
            lines.push(headline.clone());
        }
        for item in &summary {
            if !lines.iter().any(|line| line == item) {
                lines.push(item.clone());
            }
        }
        (!lines.is_empty()).then(|| lines.join(" "))
    } else {
        None
    };

    AgentBriefing {
        tick: snapshot.tick,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        should_speak: snapshot.wake.should_speak,
        priority: snapshot.wake.priority,
        headline: preferred_workflow_summary
            .or(preferred_judgment_summary)
            .or(preferred_investigation_summary)
            .or_else(|| snapshot.wake.headline.clone())
            .or(pattern_summary),
        summary,
        dominant_intents: vec![],
        spoken_message,
        focus_symbols: snapshot.wake.focus_symbols.clone(),
        reasons,
        current_investigations,
        current_judgments,
        executed_tools,
    }
}

pub fn build_session(
    snapshot: &AgentSnapshot,
    briefing: &AgentBriefing,
    previous_session: Option<&AgentSession>,
) -> AgentSession {
    let previous_threads = previous_session
        .map(|session| {
            session
                .active_threads
                .iter()
                .map(|thread| (thread.symbol.as_str(), thread))
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default();

    let mut current_turn = AgentTurn {
        tick: briefing.tick,
        timestamp: briefing.timestamp.clone(),
        should_speak: briefing.should_speak,
        priority: briefing.priority,
        headline: briefing.headline.clone(),
        spoken_message: briefing.spoken_message.clone(),
        focus_symbols: briefing.focus_symbols.clone(),
        triggered_notice_ids: snapshot
            .notices
            .iter()
            .filter(|item| {
                item.tick == snapshot.tick
                    && item
                        .symbol
                        .as_ref()
                        .map(|symbol| briefing.focus_symbols.iter().any(|value| value == symbol))
                        .unwrap_or(true)
            })
            .map(|item| item.notice_id.clone())
            .take(8)
            .collect(),
        triggered_transition_summaries: snapshot
            .recent_transitions
            .iter()
            .filter(|item| item.to_tick == snapshot.tick)
            .filter(|item| {
                briefing
                    .focus_symbols
                    .iter()
                    .any(|value| value == &item.symbol)
            })
            .map(|item| item.summary.clone())
            .take(8)
            .collect(),
        investigations: briefing.current_investigations.clone(),
        judgments: briefing.current_judgments.clone(),
        executed_tools: briefing.executed_tools.clone(),
    };

    let mut focus_symbols = briefing.focus_symbols.clone();
    for transition in snapshot
        .recent_transitions
        .iter()
        .filter(|item| item.to_tick == snapshot.tick)
        .take(4)
    {
        push_unique(&mut focus_symbols, transition.symbol.clone());
    }
    for thread in previous_threads.values() {
        if thread.status != "resolved" && thread.idle_ticks < 3 {
            push_unique(&mut focus_symbols, thread.symbol.clone());
        }
    }

    let mut active_threads = Vec::new();
    let mut threaded_symbols = std::collections::HashSet::new();
    let investigations_by_symbol = briefing
        .current_investigations
        .iter()
        .filter_map(|item| {
            item.reference_symbols
                .first()
                .cloned()
                .map(|symbol| (symbol, item))
        })
        .collect::<HashMap<_, _>>();
    let judgments_by_symbol = briefing
        .current_judgments
        .iter()
        .filter(|item| item.object_kind == "symbol")
        .filter_map(|item| {
            item.reference_symbols
                .first()
                .cloned()
                .or_else(|| Some(item.object_id.clone()))
                .map(|symbol| (symbol, item))
        })
        .collect::<HashMap<_, _>>();

    for investigation in briefing
        .current_investigations
        .iter()
        .filter(|item| item.object_kind == "symbol")
    {
        let thread_symbol = investigation
            .reference_symbols
            .first()
            .cloned()
            .unwrap_or_else(|| investigation.object_id.clone());
        if !threaded_symbols.insert(thread_symbol.clone()) {
            continue;
        }

        let symbol_state = snapshot.symbol(&thread_symbol);
        let previous_thread = previous_threads.get(thread_symbol.as_str()).copied();
        let judgment = judgments_by_symbol.get(thread_symbol.as_str()).copied();
        let workflow_stage = thread_workflow_stage(symbol_state, judgment);
        let workflow_next_step = thread_workflow_next_step(
            symbol_state,
            judgment,
            Some(investigation.attention_hint.as_str()),
        );
        let blocked_reason = thread_blocked_reason(symbol_state, judgment);
        let unlock_condition = thread_unlock_condition(
            symbol_state,
            judgment,
            workflow_stage.as_deref(),
            workflow_next_step.as_deref(),
        );
        let workflow_summary = workflow_focus_summary(
            thread_symbol.as_str(),
            investigation.family_label.as_deref(),
            workflow_stage.as_deref(),
            workflow_next_step.as_deref(),
            judgment.and_then(|item| item.best_action.as_deref()),
        );
        let status = match judgment.map(|item| item.kind) {
            Some(AgentJudgmentKind::Execute) => "active",
            Some(AgentJudgmentKind::Govern | AgentJudgmentKind::Escalate) => "escalated",
            Some(AgentJudgmentKind::Investigate) => "monitoring",
            None if matches!(investigation.attention_hint.as_str(), "enter" | "review") => {
                "escalated"
            }
            _ => "monitoring",
        };

        let priority = judgment
            .map(|item| item.priority)
            .unwrap_or(investigation.priority);
        let reasons = merge_thread_reasons(
            investigation.reasons.iter(),
            judgment.into_iter().flat_map(|item| item.reasons.iter()),
            6,
        );

        active_threads.push(AgentThread {
            symbol: thread_symbol.clone(),
            sector: symbol_state
                .and_then(|item| item.sector.clone())
                .or_else(|| previous_thread.and_then(|thread| thread.sector.clone())),
            status: status.into(),
            first_tick: previous_thread
                .map(|thread| thread.first_tick)
                .unwrap_or(snapshot.tick),
            last_tick: snapshot.tick,
            idle_ticks: 0,
            turns_observed: previous_thread
                .map(|thread| thread.turns_observed.saturating_add(1))
                .unwrap_or(1),
            priority,
            title: judgment
                .map(|item| item.title.clone())
                .or_else(|| Some(investigation.title.clone())),
            headline: append_thread_pattern(
                workflow_summary.clone().or_else(|| {
                    judgment
                        .map(|item| item.summary.clone())
                        .or_else(|| Some(investigation.summary.clone()))
                }),
                symbol_state,
            ),
            latest_summary: append_thread_pattern(
                workflow_summary.or_else(|| {
                    judgment
                        .map(|item| item.summary.clone())
                        .or_else(|| Some(investigation.summary.clone()))
                }),
                symbol_state,
            ),
            last_transition: reasons.first().cloned(),
            current_leader: symbol_state
                .and_then(|item| {
                    item.structure
                        .as_ref()
                        .and_then(|structure| structure.current_leader.clone())
                })
                .or_else(|| previous_thread.and_then(|thread| thread.current_leader.clone())),
            invalidation_status: symbol_state
                .and_then(|item| {
                    item.invalidation
                        .as_ref()
                        .map(|invalidation| invalidation.status.clone())
                })
                .or_else(|| previous_thread.and_then(|thread| thread.invalidation_status.clone())),
            workflow_stage,
            workflow_next_step,
            execution_policy: judgment
                .and_then(|item| item.execution_policy)
                .map(|item| item.to_string())
                .or_else(|| previous_thread.and_then(|thread| thread.execution_policy.clone())),
            governance_reason: judgment
                .and_then(|item| item.governance_reason.clone())
                .or_else(|| previous_thread.and_then(|thread| thread.governance_reason.clone())),
            blocked_reason: blocked_reason
                .or_else(|| previous_thread.and_then(|thread| thread.blocked_reason.clone())),
            unlock_condition: unlock_condition
                .or_else(|| previous_thread.and_then(|thread| thread.unlock_condition.clone())),
            reasons,
        });
    }

    for judgment in briefing
        .current_judgments
        .iter()
        .filter(|item| item.object_kind == "symbol")
    {
        let thread_symbol = judgment
            .reference_symbols
            .first()
            .cloned()
            .unwrap_or_else(|| judgment.object_id.clone());
        if !threaded_symbols.insert(thread_symbol.clone()) {
            continue;
        }

        let symbol_state = snapshot.symbol(&thread_symbol);
        let previous_thread = previous_threads.get(thread_symbol.as_str()).copied();
        let investigation = investigations_by_symbol
            .get(thread_symbol.as_str())
            .copied();
        let workflow_stage = thread_workflow_stage(symbol_state, Some(judgment));
        let workflow_next_step = thread_workflow_next_step(
            symbol_state,
            Some(judgment),
            investigation.map(|item| item.attention_hint.as_str()),
        );
        let blocked_reason = thread_blocked_reason(symbol_state, Some(judgment));
        let unlock_condition = thread_unlock_condition(
            symbol_state,
            Some(judgment),
            workflow_stage.as_deref(),
            workflow_next_step.as_deref(),
        );
        let workflow_summary = workflow_focus_summary(
            thread_symbol.as_str(),
            investigation.and_then(|item| item.family_label.as_deref()),
            workflow_stage.as_deref(),
            workflow_next_step.as_deref(),
            judgment.best_action.as_deref(),
        );
        let status = match judgment.kind {
            AgentJudgmentKind::Execute => "active",
            AgentJudgmentKind::Govern | AgentJudgmentKind::Escalate => "escalated",
            AgentJudgmentKind::Investigate => "monitoring",
        };

        active_threads.push(AgentThread {
            symbol: thread_symbol.clone(),
            sector: symbol_state
                .and_then(|item| item.sector.clone())
                .or_else(|| previous_thread.and_then(|thread| thread.sector.clone())),
            status: status.into(),
            first_tick: previous_thread
                .map(|thread| thread.first_tick)
                .unwrap_or(snapshot.tick),
            last_tick: snapshot.tick,
            idle_ticks: 0,
            turns_observed: previous_thread
                .map(|thread| thread.turns_observed.saturating_add(1))
                .unwrap_or(1),
            priority: judgment.priority,
            title: Some(judgment.title.clone()),
            headline: append_thread_pattern(
                workflow_summary
                    .clone()
                    .or_else(|| Some(judgment.summary.clone())),
                symbol_state,
            ),
            latest_summary: append_thread_pattern(
                workflow_summary.or_else(|| Some(judgment.summary.clone())),
                symbol_state,
            ),
            last_transition: judgment.reasons.first().cloned(),
            current_leader: symbol_state
                .and_then(|item| {
                    item.structure
                        .as_ref()
                        .and_then(|structure| structure.current_leader.clone())
                })
                .or_else(|| previous_thread.and_then(|thread| thread.current_leader.clone())),
            invalidation_status: symbol_state
                .and_then(|item| {
                    item.invalidation
                        .as_ref()
                        .map(|invalidation| invalidation.status.clone())
                })
                .or_else(|| previous_thread.and_then(|thread| thread.invalidation_status.clone())),
            workflow_stage,
            workflow_next_step,
            execution_policy: judgment
                .execution_policy
                .map(|item| item.to_string())
                .or_else(|| previous_thread.and_then(|thread| thread.execution_policy.clone())),
            governance_reason: judgment
                .governance_reason
                .clone()
                .or_else(|| previous_thread.and_then(|thread| thread.governance_reason.clone())),
            blocked_reason: blocked_reason
                .or_else(|| previous_thread.and_then(|thread| thread.blocked_reason.clone())),
            unlock_condition: unlock_condition
                .or_else(|| previous_thread.and_then(|thread| thread.unlock_condition.clone())),
            reasons: merge_thread_reasons(
                investigation
                    .into_iter()
                    .flat_map(|item| item.reasons.iter()),
                judgment.reasons.iter(),
                6,
            ),
        });
    }

    for symbol in &focus_symbols {
        if threaded_symbols.contains(symbol.as_str()) {
            continue;
        }
        let symbol_state = snapshot.symbol(symbol);
        let previous_thread = previous_threads.get(symbol.as_str()).copied();
        let current_transition = snapshot
            .recent_transitions
            .iter()
            .find(|item| item.to_tick == snapshot.tick && item.symbol == *symbol);
        let current_notice = snapshot.notices.iter().find(|item| {
            item.tick == snapshot.tick && item.symbol.as_deref() == Some(symbol.as_str())
        });

        let status = match (
            symbol_state.and_then(|item| item.invalidation.as_ref()),
            current_transition,
            briefing.focus_symbols.iter().any(|value| value == symbol),
            symbol_state.is_some(),
        ) {
            (Some(invalidation), _, _, _) if invalidation.invalidated => "invalidated",
            (_, Some(_), _, true) => "escalated",
            (_, _, true, true) => "active",
            (_, _, false, true) => "monitoring",
            _ => "resolved",
        };

        let idle_ticks = if briefing.focus_symbols.iter().any(|value| value == symbol) {
            0
        } else {
            previous_thread
                .map(|thread| thread.idle_ticks.saturating_add(1))
                .unwrap_or(1)
        };

        if status == "resolved" && idle_ticks > 2 {
            continue;
        }

        let priority = symbol_state
            .and_then(symbol_priority)
            .or_else(|| current_transition.map(|item| item.confidence.abs()))
            .or_else(|| current_notice.map(|item| item.significance))
            .unwrap_or(Decimal::ZERO);

        let latest_summary = symbol_state
            .and_then(symbol_thread_summary)
            .or_else(|| current_transition.map(|item| item.summary.clone()))
            .or_else(|| current_notice.map(|item| item.summary.clone()))
            .or_else(|| previous_thread.and_then(|thread| thread.latest_summary.clone()));

        let reasons = collect_thread_reasons(snapshot, briefing, symbol);
        let workflow_stage = thread_workflow_stage(symbol_state, None);
        let workflow_next_step = thread_workflow_next_step(symbol_state, None, None);
        let blocked_reason = thread_blocked_reason(symbol_state, None);
        let unlock_condition = thread_unlock_condition(
            symbol_state,
            None,
            workflow_stage.as_deref(),
            workflow_next_step.as_deref(),
        );
        let workflow_summary = workflow_focus_summary(
            symbol.as_str(),
            symbol_state
                .and_then(|item| item.structure.as_ref())
                .and_then(|item| item.thesis_family.as_deref()),
            workflow_stage.as_deref(),
            workflow_next_step.as_deref(),
            None,
        );

        active_threads.push(AgentThread {
            symbol: symbol.clone(),
            sector: symbol_state
                .and_then(|item| item.sector.clone())
                .or_else(|| current_transition.and_then(|item| item.sector.clone()))
                .or_else(|| previous_thread.and_then(|thread| thread.sector.clone())),
            status: status.into(),
            first_tick: previous_thread
                .map(|thread| thread.first_tick)
                .unwrap_or(snapshot.tick),
            last_tick: snapshot.tick,
            idle_ticks,
            turns_observed: previous_thread
                .map(|thread| thread.turns_observed.saturating_add(1))
                .unwrap_or(1),
            priority,
            title: symbol_state
                .and_then(|item| {
                    item.structure
                        .as_ref()
                        .map(|structure| structure.title.clone())
                })
                .or_else(|| previous_thread.and_then(|thread| thread.title.clone())),
            headline: append_thread_pattern(
                workflow_summary
                    .clone()
                    .or_else(|| current_transition.map(|item| item.summary.clone()))
                    .or_else(|| current_notice.map(|item| item.title.clone()))
                    .or_else(|| previous_thread.and_then(|thread| thread.headline.clone())),
                symbol_state,
            ),
            latest_summary: append_thread_pattern(
                workflow_summary.or(latest_summary),
                symbol_state,
            ),
            last_transition: current_transition
                .map(|item| item.summary.clone())
                .or_else(|| previous_thread.and_then(|thread| thread.last_transition.clone())),
            current_leader: symbol_state
                .and_then(|item| {
                    item.structure
                        .as_ref()
                        .and_then(|structure| structure.current_leader.clone())
                })
                .or_else(|| previous_thread.and_then(|thread| thread.current_leader.clone())),
            invalidation_status: symbol_state
                .and_then(|item| {
                    item.invalidation
                        .as_ref()
                        .map(|invalidation| invalidation.status.clone())
                })
                .or_else(|| previous_thread.and_then(|thread| thread.invalidation_status.clone())),
            workflow_stage,
            workflow_next_step,
            execution_policy: previous_thread.and_then(|thread| thread.execution_policy.clone()),
            governance_reason: previous_thread.and_then(|thread| thread.governance_reason.clone()),
            blocked_reason: blocked_reason
                .or_else(|| previous_thread.and_then(|thread| thread.blocked_reason.clone())),
            unlock_condition: unlock_condition
                .or_else(|| previous_thread.and_then(|thread| thread.unlock_condition.clone())),
            reasons,
        });
    }

    active_threads.sort_by(|a, b| {
        b.priority
            .cmp(&a.priority)
            .then_with(|| a.symbol.cmp(&b.symbol))
    });
    active_threads.truncate(16);

    if let Some(workflow_headline) = active_threads
        .iter()
        .find_map(thread_workflow_focus_summary)
    {
        current_turn.headline = Some(workflow_headline.clone());
        if current_turn.should_speak {
            let mut lines = vec![workflow_headline];
            if let Some(existing) = current_turn.spoken_message.clone() {
                for part in existing.split(" ").take(0) {
                    let _ = part;
                }
                if !lines.iter().any(|item| item == &existing) {
                    lines.push(existing);
                }
            }
            current_turn.spoken_message = Some(lines.join(" "));
        }
    }

    let mut recent_turns = previous_session
        .map(|session| session.recent_turns.clone())
        .unwrap_or_default();
    recent_turns.push(current_turn);
    if recent_turns.len() > 24 {
        recent_turns.drain(..recent_turns.len() - 24);
    }

    let active_thread_count = active_threads
        .iter()
        .filter(|thread| thread.status != "resolved")
        .count();

    AgentSession {
        tick: snapshot.tick,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        should_speak: briefing.should_speak,
        active_thread_count,
        focus_symbols,
        active_threads,
        current_investigations: briefing.current_investigations.clone(),
        current_judgments: briefing.current_judgments.clone(),
        recent_turns,
    }
}

fn action_node_stage_label(stage: ActionNodeStage) -> &'static str {
    match stage {
        ActionNodeStage::Suggested => "suggested",
        ActionNodeStage::Confirmed => "confirmed",
        ActionNodeStage::Executed => "executed",
        ActionNodeStage::Monitoring => "monitoring",
        ActionNodeStage::Reviewed => "reviewed",
    }
}

fn thread_workflow_stage(
    symbol_state: Option<&AgentSymbolState>,
    judgment: Option<&AgentOperationalJudgment>,
) -> Option<String> {
    if let Some(position) = symbol_state.and_then(|item| item.active_position.as_ref()) {
        return Some(action_node_stage_label(position.stage).into());
    }
    if judgment
        .and_then(|item| item.best_action.as_deref())
        .map(|item| !matches!(item, "wait" | "ignore"))
        .unwrap_or(false)
    {
        return Some("suggested".into());
    }
    None
}

fn thread_workflow_next_step(
    symbol_state: Option<&AgentSymbolState>,
    judgment: Option<&AgentOperationalJudgment>,
    investigation_hint: Option<&str>,
) -> Option<String> {
    if let Some(position) = symbol_state.and_then(|item| item.active_position.as_ref()) {
        let next = match position.stage {
            ActionNodeStage::Suggested => "confirm",
            ActionNodeStage::Confirmed => "execute",
            ActionNodeStage::Executed => "monitor",
            ActionNodeStage::Monitoring if position.exit_forming => "review",
            ActionNodeStage::Monitoring => "monitor",
            ActionNodeStage::Reviewed => "complete",
        };
        return Some(next.into());
    }
    if let Some(judgment) = judgment {
        let next = match judgment.kind {
            AgentJudgmentKind::Execute => "execute",
            AgentJudgmentKind::Govern => "review_gate",
            AgentJudgmentKind::Escalate => "review_desk",
            AgentJudgmentKind::Investigate => match investigation_hint {
                Some("review" | "enter") => "review_desk",
                _ => "collect_confirmation",
            },
        };
        return Some(next.into());
    }
    if let Some(symbol_state) = symbol_state {
        if super::shared::policy_reason(symbol_state).is_some() {
            return Some("review_gate".into());
        }
        if super::shared::multi_horizon_gate_reason(symbol_state).is_some() {
            return Some("collect_confirmation".into());
        }
    }
    match investigation_hint {
        Some("review" | "enter") => Some("review_desk".into()),
        Some(_) => Some("collect_confirmation".into()),
        None => None,
    }
}

fn thread_blocked_reason(
    symbol_state: Option<&AgentSymbolState>,
    judgment: Option<&AgentOperationalJudgment>,
) -> Option<String> {
    if let Some(reason) = judgment.and_then(|item| item.governance_reason.clone()) {
        return Some(reason);
    }
    if let Some(reason) = symbol_state.and_then(super::shared::policy_reason) {
        return Some(reason);
    }
    if let Some(reason) = symbol_state.and_then(super::shared::multi_horizon_gate_reason) {
        return Some(format!("multi-horizon gate: {reason}"));
    }
    None
}

fn thread_unlock_condition(
    symbol_state: Option<&AgentSymbolState>,
    judgment: Option<&AgentOperationalJudgment>,
    workflow_stage: Option<&str>,
    workflow_next_step: Option<&str>,
) -> Option<String> {
    match workflow_stage {
        Some("monitoring") => {
            if symbol_state
                .and_then(|item| item.active_position.as_ref())
                .map(|item| item.exit_forming)
                .unwrap_or(false)
            {
                return Some("exit trigger confirms and moves to review".into());
            }
            return Some("ongoing monitoring or degradation signal".into());
        }
        Some("reviewed") => return Some("workflow already completed".into()),
        _ => {}
    }

    match workflow_next_step {
        Some("review_gate") => {
            if judgment
                .and_then(|item| item.best_action.as_deref())
                .map(|item| item == "wait")
                .unwrap_or(false)
            {
                return Some("thesis upgrades beyond advisory wait".into());
            }
            return Some("human review clears the gate".into());
        }
        Some("review_desk") => return Some("operator review promotes the workflow".into()),
        Some("collect_confirmation") => {
            if symbol_state
                .and_then(super::shared::multi_horizon_gate_reason)
                .is_some()
            {
                return Some("positive 5m/30m/session lineage".into());
            }
            return Some("confirming follow-through on the next ticks".into());
        }
        Some("execute") => return Some("execution workflow is opened".into()),
        Some("monitor") => return Some("position remains healthy while monitoring".into()),
        Some("review") => return Some("exit trigger or operator review".into()),
        Some("complete") => return Some("no further action required".into()),
        _ => {}
    }
    None
}

fn workflow_focus_summary(
    object_id: &str,
    family_label: Option<&str>,
    workflow_stage: Option<&str>,
    workflow_next_step: Option<&str>,
    best_action: Option<&str>,
) -> Option<String> {
    let family_suffix = family_label
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|item| format!(" ({item})"))
        .unwrap_or_default();
    let family_inline = family_label
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|item| format!(" in {item}"))
        .unwrap_or_default();
    match (workflow_stage, workflow_next_step) {
        (Some("monitoring"), _) => Some(format!("{object_id} is monitoring an active workflow")),
        (Some("reviewed"), _) => Some(format!("{object_id} has completed workflow review")),
        (Some("suggested"), Some(next)) => Some(format!("{object_id} is suggested; next={next}")),
        (Some("confirmed"), Some(next)) => Some(format!("{object_id} is confirmed; next={next}")),
        (Some("executed"), Some(next)) => Some(format!("{object_id} is executed; next={next}")),
        (_, Some("review_gate")) => Some(format!(
            "{object_id} is in review_gate before {}",
            best_action.unwrap_or("progress")
        )),
        (_, Some("review_desk")) => Some(format!(
            "{object_id} is queued for review_desk{}",
            family_suffix
        )),
        (_, Some("collect_confirmation")) => Some(format!(
            "{object_id} is collecting confirmation{}",
            family_inline
        )),
        (_, Some("execute")) => Some(format!(
            "{object_id} is execution_ready for {}",
            best_action.unwrap_or("action")
        )),
        (_, Some("complete")) => Some(format!("{object_id} is complete")),
        _ => None,
    }
}

fn preferred_workflow_focus_summary(
    snapshot: &AgentSnapshot,
    investigations: &[AgentInvestigation],
    judgments: &[AgentOperationalJudgment],
) -> Option<String> {
    let investigations_by_symbol = investigations
        .iter()
        .filter_map(|item| {
            item.reference_symbols
                .first()
                .cloned()
                .map(|symbol| (symbol, item))
        })
        .collect::<HashMap<_, _>>();

    judgments
        .iter()
        .find(|item| item.object_kind == "symbol")
        .and_then(|judgment| {
            let symbol = judgment
                .reference_symbols
                .first()
                .cloned()
                .unwrap_or_else(|| judgment.object_id.clone());
            let investigation = investigations_by_symbol.get(symbol.as_str()).copied();
            let workflow_stage = thread_workflow_stage(snapshot.symbol(&symbol), Some(judgment));
            let workflow_next_step = thread_workflow_next_step(
                snapshot.symbol(&symbol),
                Some(judgment),
                investigation.map(|item| item.attention_hint.as_str()),
            );
            workflow_focus_summary(
                symbol.as_str(),
                investigation.and_then(|item| item.family_label.as_deref()),
                workflow_stage.as_deref(),
                workflow_next_step.as_deref(),
                judgment.best_action.as_deref(),
            )
        })
        .or_else(|| {
            investigations
                .iter()
                .find(|item| item.object_kind == "symbol")
                .and_then(|investigation| {
                    let symbol = investigation
                        .reference_symbols
                        .first()
                        .cloned()
                        .unwrap_or_else(|| investigation.object_id.clone());
                    let workflow_stage = thread_workflow_stage(snapshot.symbol(&symbol), None);
                    let workflow_next_step = thread_workflow_next_step(
                        snapshot.symbol(&symbol),
                        None,
                        Some(investigation.attention_hint.as_str()),
                    );
                    workflow_focus_summary(
                        symbol.as_str(),
                        investigation.family_label.as_deref(),
                        workflow_stage.as_deref(),
                        workflow_next_step.as_deref(),
                        None,
                    )
                })
        })
}

fn normalize_workflow_surface_items(
    snapshot: &AgentSnapshot,
    investigations: &mut [AgentInvestigation],
    judgments: &mut [AgentOperationalJudgment],
) {
    let investigation_lookup = investigations
        .iter()
        .filter_map(|item| {
            item.reference_symbols
                .first()
                .cloned()
                .map(|symbol| (symbol, item.clone()))
        })
        .collect::<HashMap<_, _>>();

    for investigation in investigations.iter_mut() {
        let symbol = investigation
            .reference_symbols
            .first()
            .cloned()
            .unwrap_or_else(|| investigation.object_id.clone());
        let workflow_stage = thread_workflow_stage(snapshot.symbol(&symbol), None);
        let workflow_next_step = thread_workflow_next_step(
            snapshot.symbol(&symbol),
            None,
            Some(investigation.attention_hint.as_str()),
        );
        if let Some(summary) = workflow_focus_summary(
            symbol.as_str(),
            investigation.family_label.as_deref(),
            workflow_stage.as_deref(),
            workflow_next_step.as_deref(),
            None,
        ) {
            investigation.summary = summary;
        }
    }

    for judgment in judgments.iter_mut() {
        let symbol = judgment
            .reference_symbols
            .first()
            .cloned()
            .unwrap_or_else(|| judgment.object_id.clone());
        let investigation = investigation_lookup.get(symbol.as_str());
        let workflow_stage = thread_workflow_stage(snapshot.symbol(&symbol), Some(judgment));
        let workflow_next_step = thread_workflow_next_step(
            snapshot.symbol(&symbol),
            Some(judgment),
            investigation.map(|item| item.attention_hint.as_str()),
        );
        if let Some(summary) = workflow_focus_summary(
            symbol.as_str(),
            investigation.and_then(|item| item.family_label.as_deref()),
            workflow_stage.as_deref(),
            workflow_next_step.as_deref(),
            judgment.best_action.as_deref(),
        ) {
            judgment.summary = summary;
        }
    }
}

fn thread_workflow_focus_summary(thread: &AgentThread) -> Option<String> {
    workflow_focus_summary(
        thread.symbol.as_str(),
        None,
        thread.workflow_stage.as_deref(),
        thread.workflow_next_step.as_deref(),
        None,
    )
}

pub(super) fn recommendation_decisions(
    market_recommendation: Option<AgentMarketRecommendation>,
    items: &[AgentRecommendation],
    sector_recommendations: &[AgentSectorRecommendation],
) -> Vec<AgentDecision> {
    let mut decisions = Vec::new();
    if let Some(item) = market_recommendation {
        decisions.push(AgentDecision::Market(item));
    }
    decisions.extend(
        sector_recommendations
            .iter()
            .cloned()
            .map(AgentDecision::Sector),
    );
    decisions.extend(items.iter().cloned().map(AgentDecision::Symbol));
    sort_decisions(&mut decisions);
    decisions
}

fn sort_decisions(decisions: &mut [AgentDecision]) {
    decisions.sort_by(|a, b| {
        decision_priority_rank(b)
            .cmp(&decision_priority_rank(a))
            .then_with(|| decision_score(b).cmp(&decision_score(a)))
            .then_with(|| decision_scope_rank(b).cmp(&decision_scope_rank(a)))
            .then_with(|| decision_sort_label(a).cmp(&decision_sort_label(b)))
    });
}

fn decision_priority_rank(decision: &AgentDecision) -> u8 {
    if decision_best_action(decision) != "wait" {
        3
    } else if matches!(decision, AgentDecision::Symbol(item) if item.action != "ignore") {
        2
    } else {
        1
    }
}

fn decision_scope_rank(decision: &AgentDecision) -> u8 {
    match decision {
        AgentDecision::Market(_) => 3,
        AgentDecision::Sector(_) => 2,
        AgentDecision::Symbol(_) => 1,
    }
}

fn decision_best_action(decision: &AgentDecision) -> &str {
    match decision {
        AgentDecision::Market(item) => item.best_action.as_str(),
        AgentDecision::Sector(item) => item.best_action.as_str(),
        AgentDecision::Symbol(item) => item.best_action.as_str(),
    }
}

fn decision_score(decision: &AgentDecision) -> Decimal {
    match decision {
        AgentDecision::Market(item) => item.market_impulse_score,
        AgentDecision::Sector(item) => item.sector_impulse_score,
        AgentDecision::Symbol(item) => item.score,
    }
}

fn decision_sort_label(decision: &AgentDecision) -> String {
    match decision {
        AgentDecision::Market(item) => item.recommendation_id.clone(),
        AgentDecision::Sector(item) => item.sector.clone(),
        AgentDecision::Symbol(item) => item.symbol.clone(),
    }
}

fn decision_watchlist_visible(decision: &AgentDecision) -> bool {
    match decision {
        AgentDecision::Market(item) => item.best_action != "wait",
        AgentDecision::Sector(item) => item.best_action != "wait",
        AgentDecision::Symbol(item) => item.action != "ignore",
    }
}

pub(super) fn decision_matches_filters(
    decision: &AgentDecision,
    symbol: Option<&str>,
    sector: Option<&str>,
) -> bool {
    let symbol_match = match symbol {
        Some(target) => match decision {
            AgentDecision::Market(item) => item
                .reference_symbols
                .iter()
                .any(|value| value.eq_ignore_ascii_case(target)),
            AgentDecision::Sector(item) => item
                .reference_symbols
                .iter()
                .any(|value| value.eq_ignore_ascii_case(target)),
            AgentDecision::Symbol(item) => item.symbol.eq_ignore_ascii_case(target),
        },
        None => true,
    };
    let sector_match = match sector {
        Some(target) => match decision {
            AgentDecision::Market(item) => item
                .focus_sectors
                .iter()
                .any(|value| value.eq_ignore_ascii_case(target)),
            AgentDecision::Sector(item) => item.sector.eq_ignore_ascii_case(target),
            AgentDecision::Symbol(item) => item
                .sector
                .as_deref()
                .map(|value| value.eq_ignore_ascii_case(target))
                .unwrap_or(false),
        },
        None => true,
    };
    symbol_match && sector_match
}

pub(super) fn sync_recommendation_views(recommendations: &mut AgentRecommendations) {
    recommendations.market_recommendation = recommendations.decisions.iter().find_map(|decision| {
        if let AgentDecision::Market(item) = decision {
            Some(item.clone())
        } else {
            None
        }
    });
    recommendations.items = recommendations
        .decisions
        .iter()
        .filter_map(|decision| {
            if let AgentDecision::Symbol(item) = decision {
                Some(item.clone())
            } else {
                None
            }
        })
        .collect();
    recommendations.total = recommendations.decisions.len();
}

pub(super) fn market_scope_symbol(market: LiveMarket) -> String {
    match market {
        LiveMarket::Hk => "HK Market".into(),
        LiveMarket::Us => "US Market".into(),
    }
}

fn synthesized_action_expectancies(
    best_action: &str,
    expected_net_alpha: Option<Decimal>,
) -> AgentActionExpectancies {
    let mut action_expectancies = AgentActionExpectancies {
        wait_expectancy: Some(Decimal::ZERO),
        ..AgentActionExpectancies::default()
    };
    if let Some(alpha) = expected_net_alpha {
        match best_action {
            "follow" => action_expectancies.follow_expectancy = Some(alpha),
            "fade" => action_expectancies.fade_expectancy = Some(alpha),
            _ => {}
        }
    }
    action_expectancies
}

fn decision_watchlist_entry(
    snapshot: &AgentSnapshot,
    decision: &AgentDecision,
    rank: usize,
) -> AgentWatchlistEntry {
    match decision {
        AgentDecision::Market(item) => AgentWatchlistEntry {
            rank,
            scope_kind: "market".into(),
            symbol: market_scope_symbol(snapshot.market),
            sector: None,
            edge_layer: Some(item.edge_layer.clone()),
            title: Some(format!(
                "{} macro / market setup",
                market_scope_symbol(snapshot.market)
            )),
            action: item.best_action.clone(),
            action_label: Some(item.preferred_expression.clone()),
            bias: item.bias.clone(),
            severity: if item.best_action == "wait" {
                "normal".into()
            } else {
                "high".into()
            },
            score: item.market_impulse_score,
            status: Some(snapshot.market_regime.bias.clone()),
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
            matched_success_pattern_signature: None,
            state_transition: None,
            best_action: item.best_action.clone(),
            action_expectancies: synthesized_action_expectancies(
                &item.best_action,
                item.expected_net_alpha,
            ),
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
            severity: if item.best_action == "wait" {
                "normal".into()
            } else {
                "high".into()
            },
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
            matched_success_pattern_signature: None,
            state_transition: None,
            best_action: item.best_action.clone(),
            action_expectancies: synthesized_action_expectancies(
                &item.best_action,
                item.expected_net_alpha,
            ),
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
            why: pattern_phrase(item.matched_success_pattern_signature.as_deref())
                .map(|pattern| format!("{} | {}", item.why, pattern))
                .unwrap_or_else(|| item.why.clone()),
            why_components: item.why_components.clone(),
            primary_lens: item.primary_lens.clone(),
            supporting_lenses: item.supporting_lenses.clone(),
            review_lens: item.review_lens.clone(),
            transition: item.transition.clone(),
            watch_next: item.watch_next.clone(),
            do_not: item.do_not.clone(),
            recommendation_id: item.recommendation_id.clone(),
            thesis_family: item.thesis_family.clone(),
            matched_success_pattern_signature: item.matched_success_pattern_signature.clone(),
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

pub fn build_watchlist(
    snapshot: &AgentSnapshot,
    session: Option<&AgentSession>,
    recommendations: Option<&AgentRecommendations>,
    limit: usize,
) -> AgentWatchlist {
    // FP2: empty shell instead of running the deprecated heuristic
    // builder when no recommendations supplied. Watchlist still
    // builds; the entries derived from recommendations.decisions are
    // simply absent if no caller supplied them.
    let _ = session;
    let recommendations = recommendations
        .cloned()
        .unwrap_or_else(|| AgentRecommendations::empty(snapshot));
    let mut entries = recommendations
        .decisions
        .iter()
        .filter(|decision| decision_watchlist_visible(decision))
        .take(limit.max(1))
        .enumerate()
        .map(|(index, decision)| decision_watchlist_entry(snapshot, decision, index + 1))
        .collect::<Vec<_>>();

    if entries.is_empty() {
        for (index, symbol) in snapshot
            .wake
            .focus_symbols
            .iter()
            .take(limit.max(1))
            .enumerate()
        {
            entries.push(AgentWatchlistEntry {
                rank: index + 1,
                scope_kind: "symbol".into(),
                symbol: symbol.clone(),
                sector: snapshot.symbol(symbol).and_then(|item| item.sector.clone()),
                edge_layer: None,
                title: snapshot.symbol(symbol).and_then(|item| {
                    item.structure
                        .as_ref()
                        .map(|structure| structure.title.clone())
                }),
                action: "watch".into(),
                action_label: None,
                bias: snapshot
                    .symbol(symbol)
                    .and_then(agent_bias_for_symbol)
                    .unwrap_or("neutral")
                    .into(),
                severity: "normal".into(),
                score: snapshot
                    .symbol(symbol)
                    .and_then(symbol_priority)
                    .unwrap_or(Decimal::ZERO),
                status: snapshot.symbol(symbol).and_then(symbol_status).map(str::to_string),
                why: snapshot
                    .recent_transitions
                    .iter()
                    .find(|item| item.symbol.eq_ignore_ascii_case(symbol))
                    .map(|item| item.summary.clone())
                    .map(|base| {
                        pattern_phrase(
                            snapshot
                                .symbol(symbol)
                                .and_then(super::shared::matched_success_pattern_signature)
                                .as_deref(),
                        )
                        .map(|pattern| format!("{base} | {pattern}"))
                        .unwrap_or(base)
                    })
                    .unwrap_or_else(|| {
                        pattern_phrase(
                            snapshot
                                .symbol(symbol)
                                .and_then(super::shared::matched_success_pattern_signature)
                                .as_deref(),
                        )
                        .map(|pattern| format!("{symbol} is in the current wake focus | {pattern}"))
                        .unwrap_or_else(|| format!("{symbol} is in the current wake focus."))
                    }),
                why_components: vec![],
                primary_lens: None,
                supporting_lenses: vec![],
                review_lens: None,
                transition: snapshot
                    .recent_transitions
                    .iter()
                    .find(|item| item.symbol.eq_ignore_ascii_case(symbol))
                    .map(|item| item.summary.clone()),
                watch_next: vec![],
                do_not: vec![],
                recommendation_id: format!("rec:{}:{}:watch", snapshot.tick, symbol),
                thesis_family: snapshot
                    .symbol(symbol)
                    .and_then(|item| item.structure.as_ref())
                    .and_then(|item| item.thesis_family.clone()),
                matched_success_pattern_signature: snapshot
                    .symbol(symbol)
                    .and_then(super::shared::matched_success_pattern_signature),
                state_transition: snapshot
                    .symbol(symbol)
                    .and_then(|item| item.structure.as_ref())
                    .and_then(|item| item.transition_reason.clone()),
                best_action: "wait".into(),
                action_expectancies: snapshot
                    .symbol(symbol)
                    .and_then(|item| item.structure.as_ref())
                    .map(|item| item.action_expectancies.clone())
                    .unwrap_or_else(|| AgentActionExpectancies {
                        wait_expectancy: Some(Decimal::ZERO),
                        ..AgentActionExpectancies::default()
                    }),
                decision_attribution: snapshot
                    .symbol(symbol)
                    .and_then(|item| item.structure.as_ref())
                    .map(|item| AgentDecisionAttribution {
                        historical_expectancies: item.action_expectancies.clone(),
                        live_expectancy_shift: Decimal::ZERO,
                        decisive_factors: vec![],
                    })
                    .unwrap_or_default(),
                expected_net_alpha: snapshot
                    .symbol(symbol)
                    .and_then(|item| item.structure.as_ref())
                    .and_then(|item| item.expected_net_alpha),
                alpha_horizon: snapshot
                    .symbol(symbol)
                    .and_then(|item| item.structure.as_ref())
                    .and_then(|item| item.alpha_horizon.clone())
                    .unwrap_or_else(|| alpha_horizon_label("intraday", 8)),
                preferred_expression: None,
                reference_symbols: vec![symbol.clone()],
                invalidation_rule: snapshot
                    .symbol(symbol)
                    .and_then(|item| item.structure.as_ref())
                    .and_then(|item| item.invalidation_rule.clone()),
                invalidation_components: vec![],
                execution_policy: Some(ActionExecutionPolicy::ManualOnly),
                governance: Some(ActionGovernanceContract::for_recommendation(
                    ActionExecutionPolicy::ManualOnly,
                )),
                governance_reason_code: Some(
                    crate::action::workflow::ActionGovernanceReasonCode::AdvisoryAction,
                ),
                governance_reason: Some(
                    "wake-only symbol remains advisory until an explicit recommendation is produced"
                        .into(),
                ),
            });
        }
    }

    AgentWatchlist {
        tick: snapshot.tick,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        regime_bias: snapshot.market_regime.bias.clone(),
        total: entries.len(),
        entries,
    }
}

pub fn build_alert_scoreboard(
    snapshot: &AgentSnapshot,
    recommendations: Option<&AgentRecommendations>,
    previous: Option<&AgentAlertScoreboard>,
) -> AgentAlertScoreboard {
    // FP2: empty shell instead of legacy heuristic builder fallback.
    let recommendations = recommendations
        .cloned()
        .unwrap_or_else(|| AgentRecommendations::empty(snapshot));
    let mut alerts = previous
        .map(|scoreboard| scoreboard.alerts.clone())
        .unwrap_or_default();

    for alert in &mut alerts {
        backfill_alert_resolution_from_legacy_outcome(alert);
        sync_alert_views(alert);
    }

    for alert in alerts.iter_mut().filter(|item| item.resolution.is_none()) {
        alert.resolution = resolve_alert_resolution(snapshot, alert);
        sync_alert_views(alert);
    }

    for decision in &recommendations.decisions {
        if let Some(alert) = decision_alert_record(snapshot, decision, &alerts) {
            alerts.push(alert);
        }
    }

    alerts.sort_by(|a, b| {
        b.tick
            .cmp(&a.tick)
            .then_with(|| a.alert_id.cmp(&b.alert_id))
    });

    let unresolved = alerts
        .iter()
        .filter(|item| item.resolution.is_none())
        .count();
    if alerts.len() > 240 {
        let keep = alerts
            .iter()
            .take_while(|item| item.resolution.is_none())
            .count()
            .max(unresolved.min(40));
        alerts.truncate(keep.saturating_add(200).min(alerts.len()));
    }

    let stats = compute_alert_stats(&alerts);
    let by_kind = compute_alert_slice_stats(&alerts, |item| item.kind.clone());
    let by_action = compute_alert_slice_stats(&alerts, |item| item.suggested_action.clone());
    let by_scope = compute_alert_slice_stats(&alerts, |item| item.scope_kind.clone());
    let by_regime = compute_alert_slice_stats(&alerts, |item| item.regime_bias.clone());
    let by_sector = compute_alert_slice_stats(&alerts, |item| {
        item.sector.clone().unwrap_or_else(|| "unknown".into())
    });

    AgentAlertScoreboard {
        tick: snapshot.tick,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        regime_bias: snapshot.market_regime.bias.clone(),
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

pub fn build_eod_review(
    snapshot: &AgentSnapshot,
    scoreboard: &AgentAlertScoreboard,
) -> AgentEodReview {
    let effective_kinds = top_positive_slices(&scoreboard.by_kind, 3);
    let noisy_kinds = top_noisy_slices(&scoreboard.by_kind, 3);
    let effective_actions = top_positive_slices(&scoreboard.by_action, 3);
    let effective_sectors = top_positive_slices(&scoreboard.by_sector, 3);
    let effective_regimes = top_positive_slices(&scoreboard.by_regime, 3);
    let top_hits = top_resolved_alerts(&scoreboard.alerts, "hit", 3);
    let top_misses = top_resolved_alerts(&scoreboard.alerts, "miss", 3);

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
        tick: snapshot.tick,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        regime_bias: snapshot.market_regime.bias.clone(),
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

fn symbol_thread_summary(item: &AgentSymbolState) -> Option<String> {
    if let Some(structure) = &item.structure {
        let mut summary = format!(
            "{} {} conf={:+}",
            structure.title,
            structure.action,
            structure.confidence.round_dp(3)
        );
        if let Some(reason) = super::shared::multi_horizon_gate_reason(item) {
            summary.push_str(&format!(" gate={reason}"));
        }
        if let Some(primary) = super::shared::policy_primary(item) {
            summary.push_str(&format!(" policy={primary}"));
        }
        if let Some(pattern) = thread_pattern_note(Some(item)) {
            summary.push_str(&format!(" {pattern}"));
        }
        return Some(summary);
    }
    if let Some(signal) = &item.signal {
        return Some(format!(
            "{} composite={:+}",
            item.symbol,
            signal.composite.round_dp(3)
        ));
    }
    None
}

fn collect_thread_reasons(
    snapshot: &AgentSnapshot,
    briefing: &AgentBriefing,
    symbol: &str,
) -> Vec<String> {
    let mut reasons = snapshot
        .notices
        .iter()
        .filter(|item| item.symbol.as_deref() == Some(symbol))
        .map(|item| item.summary.clone())
        .take(3)
        .collect::<Vec<_>>();

    for summary in snapshot
        .recent_transitions
        .iter()
        .filter(|item| item.symbol == symbol)
        .map(|item| item.summary.clone())
        .take(2)
    {
        if !reasons.iter().any(|item| item == &summary) {
            reasons.push(summary);
        }
    }

    for reason in briefing.reasons.iter().take(2) {
        if reason.contains(symbol) && !reasons.iter().any(|item| item == reason) {
            reasons.push(reason.clone());
        }
    }

    if let Some(state) = snapshot.symbol(symbol) {
        if let Some(reason) = super::shared::multi_horizon_gate_reason(state) {
            let item = format!("multi_horizon_gate: {reason}");
            if !reasons.iter().any(|existing| existing == &item) {
                reasons.push(item);
            }
        }
        if let Some(reason) = super::shared::policy_reason(state) {
            let item = format!("policy_gate: {reason}");
            if !reasons.iter().any(|existing| existing == &item) {
                reasons.push(item);
            }
        }
    }

    reasons.truncate(5);
    reasons
}

fn merge_thread_reasons<'a>(
    primary: impl IntoIterator<Item = &'a String>,
    secondary: impl IntoIterator<Item = &'a String>,
    limit: usize,
) -> Vec<String> {
    let mut reasons = Vec::new();
    for reason in primary.into_iter().chain(secondary) {
        if !reasons.iter().any(|item| item == reason) {
            reasons.push(reason.clone());
        }
        if reasons.len() >= limit {
            break;
        }
    }
    reasons
}

fn tool_request_from_suggested(suggested: &AgentSuggestedToolCall) -> AgentToolRequest {
    AgentToolRequest {
        tool: suggested.tool.clone(),
        symbol: suggested
            .args
            .get("symbol")
            .and_then(Value::as_str)
            .map(str::to_string),
        sector: suggested
            .args
            .get("sector")
            .and_then(Value::as_str)
            .map(str::to_string),
        since_tick: suggested.args.get("since_tick").and_then(Value::as_u64),
        limit: suggested
            .args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|value| value as usize),
    }
}
