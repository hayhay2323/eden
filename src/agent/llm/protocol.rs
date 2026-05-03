use super::*;

pub async fn call_model(
    client: &Client,
    config: &AnalystConfig,
    messages: &[ChatMessage],
) -> Result<String, String> {
    let response = client
        .post(config.endpoint())
        .bearer_auth(&config.api_key)
        .json(&ChatCompletionRequest {
            model: config.model.clone(),
            messages: messages.to_vec(),
            temperature: config.temperature,
        })
        .send()
        .await
        .map_err(|error| format!("request failed: {error}"))?;

    if !response.status().is_success() {
        let status = response.status();
        let body = response.text().await.unwrap_or_default();
        return Err(format!("model HTTP {}: {}", status, body));
    }

    let payload: ChatCompletionResponse = response
        .json()
        .await
        .map_err(|error| format!("invalid model response: {error}"))?;
    payload
        .choices
        .into_iter()
        .next()
        .and_then(|choice| choice.message.content)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| "model response missing content".into())
}

pub fn system_prompt() -> String {
    [
        "You are Eden Decision Analyst.",
        "You may choose exactly one action per turn and you must reply with JSON only.",
        "Your job is to produce a narrow, auditable trading-desk synthesis instead of broad narration.",
        "Action schema:",
        r#"{"action":"tool","tool":"symbol_contract","symbol":"700.HK"}"#,
        r#"{"action":"speak","message":"..."}"#,
        r#"{"action":"silent","reason":"..."}"#,
        "Use only tools listed in the context.",
        "Tool policy:",
        "0. The `perception` field is canonical eden output: emergent clusters, sector leaders, lead-lag causal chains, anomaly alerts (KL surprise), regime memory with historical forward outcomes, belief kinetics. Treat this as ground truth about what eden currently sees in the graph.",
        "1. Treat `watchlist` and `recommendations` as derived analyst ranking views, not canonical state.",
        "2. Start from `perception` for ground truth, then `recommendations`/`watchlist` for derived ranking, unless a single symbol is already clearly dominant.",
        "3. Use `notices` or `transitions_since` next if you need the freshest operational change vector.",
        "4. Use `market_session`, `symbol_contract`, `world_state`, `backward_investigation`, `sector_flow`, and `macro_event_contracts` for object/query drill-down.",
        "5. Use `graph_knowledge_links` or `graph_macro_event_candidates` when you need graph-level context rather than a derived analyst view.",
        "6. Use `depth_change` or `broker_movement` only if the action still depends on confirmation.",
        "7. Stop once you can answer: what changed, why it matters, what to watch next, and what not to do.",
        "8. Prefer `feed`, `object_query`, and `graph_query` tools over `compat_query` tools when both can answer the question.",
        "Do not repeat the same tool call with the same arguments.",
        "Do not invent data. Keep messages concise, market-facing, and specific.",
        "Prefer Traditional Chinese for the final message.",
    ]
    .join("\n")
}

pub fn initial_user_prompt(
    snapshot: &AgentSnapshot,
    briefing: &AgentBriefing,
    session: &AgentSession,
) -> String {
    let recommendations = build_recommendations(snapshot, Some(session));
    let watchlist = build_watchlist(snapshot, Some(session), Some(&recommendations), 6);
    let scoreboard = build_alert_scoreboard(snapshot, Some(&recommendations), None);
    let tools = crate::agent::tool_catalog();
    let focus_set = briefing
        .focus_symbols
        .iter()
        .map(|item| item.as_str())
        .collect::<std::collections::BTreeSet<_>>();
    let compact_threads = session
        .active_threads
        .iter()
        .filter(|item| focus_set.is_empty() || focus_set.contains(item.symbol.as_str()))
        .take(6)
        .cloned()
        .collect::<Vec<_>>();
    let recent_turns = session
        .recent_turns
        .iter()
        .rev()
        .filter(|turn| {
            turn.focus_symbols
                .iter()
                .any(|symbol| focus_set.is_empty() || focus_set.contains(symbol.as_str()))
        })
        .take(4)
        .cloned()
        .collect::<Vec<_>>();
    let sector_exceptions = snapshot
        .sector_flows
        .iter()
        .filter(|item| !item.exceptions.is_empty())
        .take(3)
        .cloned()
        .collect::<Vec<_>>();

    serde_json::to_string_pretty(&json!({
        "task": "Decide whether the desk should be alerted right now. Stay narrow and only synthesize the highest-value symbols.",
        "market_context": {
            "tick": snapshot.tick,
            "timestamp": snapshot.timestamp,
            "market": snapshot.market,
            "market_regime": snapshot.market_regime,
            "stress": snapshot.stress,
        },
        // Canonical eden perception — the structured graph-derived
        // observation (emergence clusters, sector leaders, lead-lag
        // causal chains, anomaly alerts, regime memory with forward
        // outcomes, belief kinetics, sensory & thematic vortices).
        // Per the eden thesis this is the ground-truth read surface;
        // `decision_layer.recommendations` below is a derived analyst
        // hint retained for backwards-compat.
        "perception": snapshot.perception,
        "wake_gate": {
            "should_speak": briefing.should_speak,
            "priority": briefing.priority,
            "headline": briefing.headline,
            "summary": briefing.summary,
            "focus_symbols": briefing.focus_symbols,
            "reasons": briefing.reasons,
        },
        "decision_layer": {
            "watchlist": watchlist,
            "recommendations": recommendations,
            "recent_alerts": scoreboard.alerts.into_iter().take(6).collect::<Vec<_>>(),
        },
        "exceptions": {
            "sector_flows": sector_exceptions,
        },
        "thread_memory": {
            "focus_symbols": session.focus_symbols,
            "active_threads": compact_threads,
            "recent_turns": recent_turns,
        },
        "tool_policy": {
            "surface_roles": {
                "derived_views": [
                    "watchlist",
                    "recommendations"
                ],
                "feed_tools": [
                    "notices",
                    "transitions_since"
                ],
                "object_tools": [
                    "market_session",
                    "symbol_contract",
                    "world_state",
                    "backward_investigation",
                    "sector_flow",
                    "macro_event_contracts"
                ],
                "graph_tools": [
                    "graph_knowledge_links",
                    "graph_macro_event_candidates"
                ],
                "microstructure_confirmation_tools": [
                    "depth_change",
                    "broker_movement"
                ],
                "avoid_when_possible": [
                    "compat_query"
                ]
            },
            "ordered_sequence": [
                "recommendations",
                "watchlist",
                "market_session",
                "notices",
                "transitions_since",
                "symbol_contract",
                "world_state",
                "backward_investigation",
                "sector_flow",
                "macro_event_contracts",
                "graph_knowledge_links",
                "graph_macro_event_candidates",
                "depth_change",
                "broker_movement"
            ],
            "stop_conditions": [
                "You can explain what changed in one to three lines.",
                "You have a regime-bound action framing.",
                "You have at most one uncertainty left."
            ]
        },
        "tool_specs": tools,
    }))
    .unwrap_or_else(|_| "{}".into())
}

pub(crate) fn parse_action(raw: &str) -> Result<ModelAction, String> {
    if let Ok(action) = serde_json::from_str::<ModelAction>(raw) {
        return Ok(action);
    }
    let json = extract_json_object(raw).ok_or_else(|| "no JSON object found".to_string())?;
    serde_json::from_str(&json).map_err(|error| error.to_string())
}

fn extract_json_object(raw: &str) -> Option<String> {
    let mut start = None;
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;

    for (idx, ch) in raw.char_indices() {
        if in_string {
            if escaped {
                escaped = false;
                continue;
            }
            match ch {
                '\\' => escaped = true,
                '"' => in_string = false,
                _ => {}
            }
            continue;
        }

        match ch {
            '"' => in_string = true,
            '{' => {
                if depth == 0 {
                    start = Some(idx);
                }
                depth += 1;
            }
            '}' => {
                if depth == 0 {
                    continue;
                }
                depth -= 1;
                if depth == 0 {
                    let start = start?;
                    return Some(raw[start..=idx].to_string());
                }
            }
            _ => {}
        }
    }

    None
}

pub fn resulting_args(request: &AgentToolRequest) -> Value {
    json!({
        "tool": request.tool,
        "symbol": request.symbol,
        "sector": request.sector,
        "since_tick": request.since_tick,
        "limit": request.limit,
    })
}
