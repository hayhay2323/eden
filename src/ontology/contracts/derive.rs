use super::*;

fn pattern_phrase(signature: Option<&str>) -> Option<String> {
    signature
        .filter(|value| !value.is_empty())
        .map(|value| format!("matched historical pattern {value}"))
}

pub fn derive_agent_session(snapshot: &OperationalSnapshot) -> AgentSession {
    AgentSession {
        tick: snapshot.source_tick,
        timestamp: format_timestamp(snapshot.observed_at),
        market: snapshot.market,
        should_speak: snapshot.market_session.should_speak,
        active_thread_count: snapshot.market_session.active_thread_count,
        focus_symbols: snapshot.market_session.focus_symbols.clone(),
        active_threads: snapshot
            .threads
            .iter()
            .map(|item| item.thread.clone())
            .collect(),
        current_investigations: vec![],
        current_judgments: vec![],
        recent_turns: snapshot.recent_turns.clone(),
    }
}

pub fn derive_agent_briefing(snapshot: &OperationalSnapshot) -> AgentBriefing {
    let mut summary = snapshot.market_session.wake_summary.clone();
    let pattern_summary = snapshot
        .recommendations
        .iter()
        .find_map(|item| pattern_phrase(item.summary.matched_success_pattern_signature.as_deref()));
    if let Some(pattern_summary) = pattern_summary.clone() {
        if !summary.iter().any(|item| item == &pattern_summary) {
            summary.insert(0, pattern_summary);
        }
    }
    if let Some(headline) = snapshot.market_session.wake_headline.as_ref() {
        if !summary.iter().any(|item| item == headline) {
            summary.insert(0, headline.clone());
        }
    }
    summary.truncate(6);

    let spoken_message = if snapshot.market_session.should_speak {
        (!summary.is_empty()).then(|| summary.join(" "))
    } else {
        None
    };

    AgentBriefing {
        tick: snapshot.source_tick,
        timestamp: format_timestamp(snapshot.observed_at),
        market: snapshot.market,
        should_speak: snapshot.market_session.should_speak,
        priority: snapshot.market_session.priority,
        headline: snapshot
            .market_session
            .wake_headline
            .clone()
            .or(pattern_summary),
        summary,
        spoken_message,
        focus_symbols: snapshot.market_session.focus_symbols.clone(),
        reasons: snapshot.market_session.wake_reasons.clone(),
        current_investigations: vec![],
        current_judgments: vec![],
        executed_tools: snapshot
            .market_session
            .suggested_tools
            .iter()
            .cloned()
            .map(|tool| AgentExecutedTool {
                tool: tool.tool,
                args: tool.args,
                preview: Some(tool.reason),
                result: serde_json::Value::Null,
            })
            .collect(),
    }
}

pub fn derive_agent_recommendations(snapshot: &OperationalSnapshot) -> AgentRecommendations {
    let mut decisions = Vec::new();
    if let Some(item) = snapshot.market_recommendation.clone() {
        decisions.push(AgentDecision::Market(item));
    }
    decisions.extend(
        snapshot
            .sector_recommendations
            .iter()
            .cloned()
            .map(AgentDecision::Sector),
    );
    decisions.extend(
        snapshot
            .recommendations
            .iter()
            .cloned()
            .map(|item| AgentDecision::Symbol(item.recommendation)),
    );

    AgentRecommendations {
        tick: snapshot.source_tick,
        timestamp: format_timestamp(snapshot.observed_at),
        market: snapshot.market,
        regime_bias: snapshot.market_session.market_regime.bias.clone(),
        total: decisions.len(),
        market_recommendation: snapshot.market_recommendation.clone(),
        decisions,
        items: snapshot
            .recommendations
            .iter()
            .map(|item| item.recommendation.clone())
            .collect(),
        knowledge_links: vec![],
    }
}

pub fn derive_agent_watchlist(snapshot: &OperationalSnapshot, limit: usize) -> AgentWatchlist {
    let recommendations = derive_agent_recommendations(snapshot);
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
            .market_session
            .focus_symbols
            .iter()
            .take(limit.max(1))
            .enumerate()
        {
            let symbol_contract = snapshot.symbol(symbol);
            entries.push(AgentWatchlistEntry {
                rank: index + 1,
                scope_kind: "symbol".into(),
                symbol: symbol.clone(),
                sector: symbol_contract.and_then(|item| item.sector.clone()),
                edge_layer: None,
                title: symbol_contract
                    .and_then(|item| item.state.structure.as_ref())
                    .map(|item| item.title.clone()),
                action: "watch".into(),
                action_label: None,
                bias: "neutral".into(),
                severity: "normal".into(),
                score: symbol_contract
                    .and_then(|item| item.state.structure.as_ref())
                    .map(|item| item.confidence)
                    .unwrap_or(Decimal::ZERO),
                status: symbol_contract
                    .and_then(|item| item.state.structure.as_ref())
                    .and_then(|item| item.status.clone()),
                why: pattern_phrase(
                    symbol_contract
                        .and_then(|item| item.state.structure.as_ref())
                        .and_then(|item| item.leader_transition_summary.as_deref())
                        .and_then(|summary| {
                            summary
                                .split('|')
                                .map(str::trim)
                                .find_map(|part| part.strip_prefix("pattern=").map(str::to_string))
                        })
                        .as_deref(),
                )
                .map(|pattern| format!("{symbol} is in the current session focus | {pattern}"))
                .unwrap_or_else(|| format!("{symbol} is in the current session focus.")),
                why_components: vec![],
                primary_lens: None,
                supporting_lenses: vec![],
                review_lens: None,
                transition: None,
                watch_next: vec![],
                do_not: vec![],
                recommendation_id: format!("rec:{}:{}:watch", snapshot.source_tick, symbol),
                thesis_family: symbol_contract
                    .and_then(|item| item.state.structure.as_ref())
                    .and_then(|item| item.thesis_family.clone()),
                matched_success_pattern_signature: symbol_contract
                    .and_then(|item| item.state.structure.as_ref())
                    .and_then(|item| {
                        item.leader_transition_summary
                            .as_deref()
                            .and_then(|summary| {
                                summary.split('|').map(str::trim).find_map(|part| {
                                    part.strip_prefix("pattern=").map(str::to_string)
                                })
                            })
                    }),
                state_transition: symbol_contract
                    .and_then(|item| item.state.structure.as_ref())
                    .and_then(|item| item.transition_reason.clone()),
                best_action: "wait".into(),
                action_expectancies: AgentActionExpectancies {
                    wait_expectancy: Some(Decimal::ZERO),
                    ..AgentActionExpectancies::default()
                },
                decision_attribution: AgentDecisionAttribution::default(),
                expected_net_alpha: symbol_contract
                    .and_then(|item| item.state.structure.as_ref())
                    .and_then(|item| item.expected_net_alpha),
                alpha_horizon: symbol_contract
                    .and_then(|item| item.state.structure.as_ref())
                    .and_then(|item| item.alpha_horizon.clone())
                    .unwrap_or_else(|| "intraday:8t".into()),
                preferred_expression: None,
                reference_symbols: vec![symbol.clone()],
                invalidation_rule: symbol_contract
                    .and_then(|item| item.state.structure.as_ref())
                    .and_then(|item| item.invalidation_rule.clone()),
                invalidation_components: vec![],
                execution_policy: Some(ActionExecutionPolicy::ManualOnly),
                governance: Some(ActionGovernanceContract::for_recommendation(
                    ActionExecutionPolicy::ManualOnly,
                )),
                governance_reason_code: Some(ActionGovernanceReasonCode::AdvisoryAction),
                governance_reason: Some(
                    "focus-only symbol remains advisory until a formal recommendation exists"
                        .into(),
                ),
            });
        }
    }

    AgentWatchlist {
        tick: snapshot.source_tick,
        timestamp: format_timestamp(snapshot.observed_at),
        market: snapshot.market,
        regime_bias: snapshot.market_session.market_regime.bias.clone(),
        total: entries.len(),
        entries,
    }
}

pub fn derive_agent_narration(
    snapshot: &OperationalSnapshot,
    analysis: Option<&AgentAnalysis>,
) -> AgentNarration {
    let briefing = derive_agent_briefing(snapshot);
    let recommendations = derive_agent_recommendations(snapshot);
    let watchlist = derive_agent_watchlist(snapshot, 8);
    let top_decision = recommendations.decisions.first();

    let message = analysis
        .and_then(|item| item.message.clone())
        .or_else(|| briefing.spoken_message.clone());
    let market_recommendation = snapshot.market_recommendation.clone();
    let mut bullets = watchlist
        .entries
        .iter()
        .take(3)
        .map(|entry| format!("{}: {} | {}", entry.symbol, entry.action, entry.why))
        .collect::<Vec<_>>();
    if let Some(pattern_summary) = top_decision.and_then(|decision| match decision {
        AgentDecision::Symbol(item) => {
            pattern_phrase(item.matched_success_pattern_signature.as_deref())
        }
        _ => None,
    }) {
        if !bullets.iter().any(|item| item == &pattern_summary) {
            bullets.insert(0, pattern_summary);
        }
    }
    for item in briefing.summary.iter().take(2) {
        if !bullets.iter().any(|candidate| candidate == item) {
            bullets.push(item.clone());
        }
    }
    if let Some(item) = market_recommendation.as_ref() {
        bullets.insert(
            0,
            format!(
                "Market: {} via {} | {}",
                item.best_action, item.preferred_expression, item.summary
            ),
        );
    }
    if let Some(pattern_summary) = top_decision.and_then(|decision| match decision {
        AgentDecision::Symbol(item) => {
            pattern_phrase(item.matched_success_pattern_signature.as_deref())
        }
        _ => None,
    }) {
        if !bullets.iter().any(|item| item == &pattern_summary) {
            bullets.insert(0, pattern_summary);
        }
    }
    bullets.truncate(5);

    let mut tags = snapshot
        .notices
        .iter()
        .map(|item| item.kind.clone())
        .collect::<Vec<_>>();
    for decision in recommendations.decisions.iter().take(3) {
        let tag = match decision {
            AgentDecision::Market(item) => format!("market_{}", item.best_action),
            AgentDecision::Sector(item) => format!("sector_{}", item.best_action),
            AgentDecision::Symbol(item) => item.action.clone(),
        };
        if !tags.iter().any(|candidate| candidate == &tag) {
            tags.push(tag);
        }
    }
    tags.sort();
    tags.dedup();
    tags.truncate(6);

    let alert_level = top_decision
        .map(|decision| match decision {
            AgentDecision::Market(item) => {
                if item.best_action == "wait" {
                    "normal"
                } else {
                    "high"
                }
            }
            AgentDecision::Sector(item) => {
                if item.best_action == "wait" {
                    "normal"
                } else {
                    "high"
                }
            }
            AgentDecision::Symbol(item) => item.severity.as_str(),
        })
        .unwrap_or_else(|| {
            if briefing.should_speak || snapshot.market_session.active_thread_count > 0 {
                "high"
            } else {
                "normal"
            }
        })
        .to_string();

    let what_changed = snapshot
        .recent_transitions
        .iter()
        .take(3)
        .map(|item| item.summary.clone())
        .collect::<Vec<_>>();

    let watch_next = top_decision
        .map(|decision| match decision {
            AgentDecision::Market(item) => item.decisive_factors.iter().take(3).cloned().collect(),
            AgentDecision::Sector(item) => item.decisive_factors.iter().take(3).cloned().collect(),
            AgentDecision::Symbol(item) => item.watch_next.clone(),
        })
        .unwrap_or_default();
    let what_not_to_do = top_decision
        .map(|decision| match decision {
            AgentDecision::Market(item) => item
                .why_not_single_name
                .clone()
                .into_iter()
                .collect::<Vec<_>>(),
            AgentDecision::Sector(_) => vec![],
            AgentDecision::Symbol(item) => item.do_not.clone(),
        })
        .unwrap_or_default();
    let fragility = top_decision
        .and_then(|decision| {
            if let AgentDecision::Symbol(item) = decision {
                Some(item.fragility.clone())
            } else {
                None
            }
        })
        .unwrap_or_default();
    let confidence_band = top_decision.map(|decision| {
        let score = match decision {
            AgentDecision::Market(item) => item.market_impulse_score,
            AgentDecision::Sector(item) => item.sector_impulse_score,
            AgentDecision::Symbol(item) => item.confidence,
        };
        if score >= Decimal::new(8, 2) {
            "high"
        } else if score >= Decimal::new(4, 2) {
            "medium"
        } else {
            "low"
        }
        .to_string()
    });

    let action_cards = recommendations
        .decisions
        .iter()
        .take(12)
        .map(|decision| narration_action_card(decision, snapshot.market))
        .collect::<Vec<_>>();
    let dominant_lenses = aggregate_dominant_lenses(&action_cards);
    if let Some(summary) = dominant_lens_summary(&dominant_lenses) {
        if !bullets.iter().any(|item| item == &summary) {
            bullets.push(summary);
            bullets.truncate(5);
        }
    }

    AgentNarration {
        tick: snapshot.source_tick,
        timestamp: format_timestamp(snapshot.observed_at),
        market: snapshot.market,
        should_alert: snapshot.market_session.should_speak
            || snapshot.market_session.active_thread_count > 0
            || top_decision
                .map(|decision| match decision {
                    AgentDecision::Market(item) => item.best_action != "wait",
                    AgentDecision::Sector(item) => item.best_action != "wait",
                    AgentDecision::Symbol(item) => item.action != "ignore",
                })
                .unwrap_or(false)
            || market_recommendation
                .as_ref()
                .map(|item| item.best_action != "wait")
                .unwrap_or(false),
        alert_level,
        source: analysis
            .map(|item| item.provider.clone())
            .unwrap_or_else(|| "operational_snapshot".into()),
        headline: analysis
            .and_then(|item| item.message.clone())
            .or_else(|| {
                market_recommendation.as_ref().and_then(|item| {
                    (item.best_action != "wait").then(|| {
                        format!(
                            "{} {} via {}",
                            market_label(snapshot.market),
                            item.best_action,
                            item.preferred_expression
                        )
                    })
                })
            })
            .or_else(|| {
                top_decision.and_then(|decision| match decision {
                    AgentDecision::Symbol(item) => {
                        pattern_phrase(item.matched_success_pattern_signature.as_deref())
                            .map(|pattern| format!("{} {} | {}", item.symbol, item.action, pattern))
                    }
                    _ => None,
                })
            })
            .or_else(|| briefing.headline.clone()),
        message,
        bullets,
        focus_symbols: if watchlist.entries.is_empty() {
            briefing.focus_symbols.clone()
        } else {
            let mut focus = Vec::new();
            for entry in watchlist.entries.iter().take(4) {
                if entry.reference_symbols.is_empty() {
                    if entry.scope_kind == "symbol" {
                        focus.push(entry.symbol.clone());
                    }
                } else {
                    for symbol in &entry.reference_symbols {
                        if !focus.iter().any(|value| value == symbol) {
                            focus.push(symbol.clone());
                        }
                    }
                }
                if focus.len() >= 4 {
                    break;
                }
            }
            if focus.is_empty() {
                briefing.focus_symbols.clone()
            } else {
                focus
            }
        },
        tags,
        primary_action: top_decision.and_then(|decision| match decision {
            AgentDecision::Market(item) => {
                (item.best_action != "wait").then(|| format!("market_{}", item.best_action))
            }
            AgentDecision::Sector(item) => {
                (item.best_action != "wait").then(|| format!("sector_{}", item.best_action))
            }
            AgentDecision::Symbol(item) => Some(item.action.clone()),
        }),
        confidence_band,
        what_changed,
        why_it_matters: top_decision
            .map(|decision| match decision {
                AgentDecision::Market(item) => item.summary.clone(),
                AgentDecision::Sector(item) => item.summary.clone(),
                AgentDecision::Symbol(item) => item.why.clone(),
            })
            .or_else(|| snapshot.market_session.market_summary.clone()),
        watch_next,
        what_not_to_do,
        fragility,
        recommendation_ids: recommendations
            .decisions
            .iter()
            .take(3)
            .map(|decision| match decision {
                AgentDecision::Market(item) => item.recommendation_id.clone(),
                AgentDecision::Sector(item) => item.recommendation_id.clone(),
                AgentDecision::Symbol(item) => item.recommendation_id.clone(),
            })
            .collect(),
        market_summary_5m: snapshot.market_session.market_summary.clone(),
        market_recommendation,
        dominant_lenses,
        action_cards,
    }
}

pub fn derive_stale_agent_narration(
    snapshot: &OperationalSnapshot,
    analysis: Option<&AgentAnalysis>,
) -> AgentNarration {
    let stale_tick = analysis.map(|item| item.tick);
    let stale_note = stale_tick
        .map(|tick| format!("目前沒有新鮮的 Codex 分析可用；最後一輪 Codex 停在 tick {tick}。"))
        .unwrap_or_else(|| "目前沒有可用的 Codex 分析。".into());
    AgentNarration {
        tick: snapshot.source_tick,
        timestamp: format_timestamp(snapshot.observed_at),
        market: snapshot.market,
        should_alert: false,
        alert_level: "normal".into(),
        source: "codex-stale".into(),
        headline: Some("等待新的 Codex 分析".into()),
        message: Some(stale_note.clone()),
        bullets: vec![
            "前端已切到 Codex-first 模式。".into(),
            "因為目前沒有 fresh Codex，暫不展示行動建議。".into(),
        ],
        focus_symbols: vec![],
        tags: vec!["codex".into(), "stale".into()],
        primary_action: None,
        confidence_band: None,
        what_changed: vec![],
        why_it_matters: Some(stale_note),
        watch_next: vec![
            "等待下一輪 Codex 成功完成。".into(),
            "確認 Codex 最新 tick 追上 live tick。".into(),
        ],
        what_not_to_do: vec!["不要把舊的 Codex 分析當成現在的行動依據。".into()],
        fragility: vec!["Codex analysis is stale relative to current live tick.".into()],
        recommendation_ids: vec![],
        market_summary_5m: Some(
            analysis
                .and_then(|item| item.message.clone())
                .map(|message| format!("舊的 Codex 摘要已過時：{message}"))
                .unwrap_or_else(|| {
                    format!(
                        "{} 大市仍在更新，但 Codex 尚未跟上。",
                        snapshot.market_session.market_regime.bias
                    )
                }),
        ),
        market_recommendation: None,
        dominant_lenses: vec![],
        action_cards: vec![],
    }
}
