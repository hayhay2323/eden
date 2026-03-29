use super::*;

pub fn tool_catalog() -> Vec<AgentToolSpec> {
    vec![
        AgentToolSpec {
            name: "wake".into(),
            category: AgentToolCategory::DerivedView,
            route: "/api/agent/:market/wake".into(),
            method: "GET".into(),
            description:
                "Returns the speech gate decision, focus symbols, and suggested next queries."
                    .into(),
            args: vec![],
        },
        AgentToolSpec {
            name: "session".into(),
            category: AgentToolCategory::DerivedView,
            route: "/api/agent/:market/session".into(),
            method: "GET".into(),
            description: "Returns the current analyst session state with threads and recent turns."
                .into(),
            args: vec![],
        },
        AgentToolSpec {
            name: "watchlist".into(),
            category: AgentToolCategory::DerivedView,
            route: "/api/agent/:market/watchlist".into(),
            method: "GET".into(),
            description:
                "Returns the top symbols to watch right now, ranked by decision relevance."
                    .into(),
            args: vec![
                AgentToolArgSpec {
                    name: "symbol".into(),
                    required: false,
                    description: "Optional symbol filter.".into(),
                },
                AgentToolArgSpec {
                    name: "sector".into(),
                    required: false,
                    description: "Optional sector filter.".into(),
                },
                AgentToolArgSpec {
                    name: "limit".into(),
                    required: false,
                    description: "Maximum watchlist entries to return.".into(),
                },
            ],
        },
        AgentToolSpec {
            name: "recommendations".into(),
            category: AgentToolCategory::DerivedView,
            route: "/api/agent/:market/recommendations".into(),
            method: "GET".into(),
            description:
                "Returns standardized action recommendations tied to the current regime."
                    .into(),
            args: vec![
                AgentToolArgSpec {
                    name: "symbol".into(),
                    required: false,
                    description: "Optional symbol filter.".into(),
                },
                AgentToolArgSpec {
                    name: "sector".into(),
                    required: false,
                    description: "Optional sector filter.".into(),
                },
                AgentToolArgSpec {
                    name: "limit".into(),
                    required: false,
                    description: "Maximum recommendations to return.".into(),
                },
            ],
        },
        AgentToolSpec {
            name: "alert_scoreboard".into(),
            category: AgentToolCategory::DerivedView,
            route: "/api/agent/:market/scoreboard".into(),
            method: "GET".into(),
            description:
                "Returns auditable alerts plus hit-rate and outcome statistics by slice.".into(),
            args: vec![],
        },
        AgentToolSpec {
            name: "eod_review".into(),
            category: AgentToolCategory::DerivedView,
            route: "/api/agent/:market/eod-review".into(),
            method: "GET".into(),
            description:
                "Returns the latest end-of-day style review built from the alert scoreboard."
                    .into(),
            args: vec![],
        },
        AgentToolSpec {
            name: "threads".into(),
            category: AgentToolCategory::DerivedView,
            route: "/api/agent/:market/threads".into(),
            method: "GET".into(),
            description: "Returns the current analyst threads, optionally filtered by symbol or sector."
                .into(),
            args: vec![
                AgentToolArgSpec {
                    name: "symbol".into(),
                    required: false,
                    description: "Optional symbol filter.".into(),
                },
                AgentToolArgSpec {
                    name: "sector".into(),
                    required: false,
                    description: "Optional sector filter.".into(),
                },
                AgentToolArgSpec {
                    name: "limit".into(),
                    required: false,
                    description: "Maximum threads to return.".into(),
                },
            ],
        },
        AgentToolSpec {
            name: "turns".into(),
            category: AgentToolCategory::DerivedView,
            route: "/api/agent/:market/turns".into(),
            method: "GET".into(),
            description: "Returns recent analyst turns, optionally filtered by since_tick or symbol."
                .into(),
            args: vec![
                AgentToolArgSpec {
                    name: "since_tick".into(),
                    required: false,
                    description: "Only return turns newer than this tick.".into(),
                },
                AgentToolArgSpec {
                    name: "symbol".into(),
                    required: false,
                    description: "Optional focus symbol filter.".into(),
                },
                AgentToolArgSpec {
                    name: "limit".into(),
                    required: false,
                    description: "Maximum turns to return.".into(),
                },
            ],
        },
        AgentToolSpec {
            name: "active_structures".into(),
            category: AgentToolCategory::CompatQuery,
            route: "/api/agent/:market/structures".into(),
            method: "GET".into(),
            description: "Lists currently active structures ranked by confidence.".into(),
            args: vec![
                AgentToolArgSpec {
                    name: "limit".into(),
                    required: false,
                    description: "Maximum structures to return.".into(),
                },
                AgentToolArgSpec {
                    name: "sector".into(),
                    required: false,
                    description: "Optional sector filter.".into(),
                },
                AgentToolArgSpec {
                    name: "symbol".into(),
                    required: false,
                    description: "Optional symbol filter.".into(),
                },
            ],
        },
        AgentToolSpec {
            name: "structure_state".into(),
            category: AgentToolCategory::CompatQuery,
            route: "/api/agent/:market/structures/:symbol".into(),
            method: "GET".into(),
            description:
                "Returns the current structure state for one symbol, including age and leader streak."
                    .into(),
            args: vec![AgentToolArgSpec {
                name: "symbol".into(),
                required: true,
                description: "Ticker symbol.".into(),
            }],
        },
        AgentToolSpec {
            name: "transitions_since".into(),
            category: AgentToolCategory::Feed,
            route: "/api/feed/:market/transitions".into(),
            method: "GET".into(),
            description: "Returns recent structure transitions after an optional tick threshold."
                .into(),
            args: vec![
                AgentToolArgSpec {
                    name: "since_tick".into(),
                    required: false,
                    description: "Only return transitions newer than this tick.".into(),
                },
                AgentToolArgSpec {
                    name: "limit".into(),
                    required: false,
                    description: "Maximum transitions to return.".into(),
                },
                AgentToolArgSpec {
                    name: "symbol".into(),
                    required: false,
                    description: "Optional symbol filter.".into(),
                },
                AgentToolArgSpec {
                    name: "sector".into(),
                    required: false,
                    description: "Optional sector filter.".into(),
                },
            ],
        },
        AgentToolSpec {
            name: "symbol_state".into(),
            category: AgentToolCategory::CompatQuery,
            route: "/api/agent/:market/symbol/:symbol".into(),
            method: "GET".into(),
            description: "Returns the full current state for one symbol.".into(),
            args: vec![AgentToolArgSpec {
                name: "symbol".into(),
                required: true,
                description: "Ticker symbol.".into(),
            }],
        },
        AgentToolSpec {
            name: "depth_change".into(),
            category: AgentToolCategory::Microstructure,
            route: "/api/agent/:market/depth/:symbol".into(),
            method: "GET".into(),
            description: "Returns tick-to-tick depth and imbalance changes for one symbol."
                .into(),
            args: vec![AgentToolArgSpec {
                name: "symbol".into(),
                required: true,
                description: "Ticker symbol.".into(),
            }],
        },
        AgentToolSpec {
            name: "broker_movement".into(),
            category: AgentToolCategory::Microstructure,
            route: "/api/agent/:market/brokers/:symbol".into(),
            method: "GET".into(),
            description: "Returns institution entries, exits, and side switches for one symbol."
                .into(),
            args: vec![AgentToolArgSpec {
                name: "symbol".into(),
                required: true,
                description: "Ticker symbol.".into(),
            }],
        },
        AgentToolSpec {
            name: "invalidation_status".into(),
            category: AgentToolCategory::CompatQuery,
            route: "/api/agent/:market/invalidation/:symbol".into(),
            method: "GET".into(),
            description: "Returns invalidation status, rules, and leading falsifier for one symbol."
                .into(),
            args: vec![AgentToolArgSpec {
                name: "symbol".into(),
                required: true,
                description: "Ticker symbol.".into(),
            }],
        },
        AgentToolSpec {
            name: "sector_flow".into(),
            category: AgentToolCategory::ObjectQuery,
            route: "/api/ontology/:market/sector-flows".into(),
            method: "GET".into(),
            description: "Returns sector-level flow summaries and exceptions.".into(),
            args: vec![
                AgentToolArgSpec {
                    name: "sector".into(),
                    required: false,
                    description: "Optional sector filter.".into(),
                },
                AgentToolArgSpec {
                    name: "limit".into(),
                    required: false,
                    description: "Maximum sectors to return.".into(),
                },
            ],
        },
        AgentToolSpec {
            name: "world_state".into(),
            category: AgentToolCategory::ObjectQuery,
            route: "/api/ontology/:market/world".into(),
            method: "GET".into(),
            description:
                "Returns the current world-state canopy/trunk/leaf snapshot when available.".into(),
            args: vec![],
        },
        AgentToolSpec {
            name: "backward_investigation".into(),
            category: AgentToolCategory::ObjectQuery,
            route: "/api/ontology/:market/backward/:symbol".into(),
            method: "GET".into(),
            description: "Returns the current backward causal investigation for one symbol when available."
                .into(),
            args: vec![AgentToolArgSpec {
                name: "symbol".into(),
                required: true,
                description: "Ticker symbol.".into(),
            }],
        },
        AgentToolSpec {
            name: "notices".into(),
            category: AgentToolCategory::Feed,
            route: "/api/feed/:market/notices".into(),
            method: "GET".into(),
            description: "Returns the current notice feed, with optional since_tick and filters."
                .into(),
            args: vec![
                AgentToolArgSpec {
                    name: "since_tick".into(),
                    required: false,
                    description: "Only return notices newer than this tick.".into(),
                },
                AgentToolArgSpec {
                    name: "limit".into(),
                    required: false,
                    description: "Maximum notices to return.".into(),
                },
                AgentToolArgSpec {
                    name: "symbol".into(),
                    required: false,
                    description: "Optional symbol filter.".into(),
                },
                AgentToolArgSpec {
                    name: "sector".into(),
                    required: false,
                    description: "Optional sector filter.".into(),
                },
            ],
        },
        AgentToolSpec {
            name: "macro_event_candidates".into(),
            category: AgentToolCategory::CompatQuery,
            route: "/api/agent/:market/query?tool=macro_event_candidates".into(),
            method: "GET".into(),
            description:
                "Returns promoted-from-news/event candidates before final macro-event confirmation."
                    .into(),
            args: vec![
                AgentToolArgSpec {
                    name: "since_tick".into(),
                    required: false,
                    description: "Only return candidates newer than this tick.".into(),
                },
                AgentToolArgSpec {
                    name: "limit".into(),
                    required: false,
                    description: "Maximum candidates to return.".into(),
                },
                AgentToolArgSpec {
                    name: "symbol".into(),
                    required: false,
                    description: "Optional impacted symbol filter.".into(),
                },
                AgentToolArgSpec {
                    name: "sector".into(),
                    required: false,
                    description: "Optional impacted sector filter.".into(),
                },
            ],
        },
        AgentToolSpec {
            name: "macro_events".into(),
            category: AgentToolCategory::CompatQuery,
            route: "/api/agent/:market/query?tool=macro_events".into(),
            method: "GET".into(),
            description:
                "Returns confirmed macro events and their routed market/sector/symbol impact."
                    .into(),
            args: vec![
                AgentToolArgSpec {
                    name: "since_tick".into(),
                    required: false,
                    description: "Only return events newer than this tick.".into(),
                },
                AgentToolArgSpec {
                    name: "limit".into(),
                    required: false,
                    description: "Maximum macro events to return.".into(),
                },
                AgentToolArgSpec {
                    name: "symbol".into(),
                    required: false,
                    description: "Optional impacted symbol filter.".into(),
                },
                AgentToolArgSpec {
                    name: "sector".into(),
                    required: false,
                    description: "Optional impacted sector filter.".into(),
                },
            ],
        },
        AgentToolSpec {
            name: "knowledge_links".into(),
            category: AgentToolCategory::GraphQuery,
            route: "/api/agent/:market/query?tool=knowledge_links".into(),
            method: "GET".into(),
            description:
                "Returns explicit event-to-market/sector/symbol/decision knowledge-graph links."
                    .into(),
            args: vec![
                AgentToolArgSpec {
                    name: "symbol".into(),
                    required: false,
                    description: "Optional symbol filter.".into(),
                },
                AgentToolArgSpec {
                    name: "sector".into(),
                    required: false,
                    description: "Optional sector filter.".into(),
                },
                AgentToolArgSpec {
                    name: "limit".into(),
                    required: false,
                    description: "Maximum links to return.".into(),
                },
            ],
        },
    ]
}

pub fn execute_tool(
    snapshot: &AgentSnapshot,
    session: Option<&AgentSession>,
    request: &AgentToolRequest,
) -> Result<AgentToolOutput, String> {
    let symbol = request.symbol.as_deref();
    let sector = request.sector.as_deref();
    let since_tick = request.since_tick;
    let limit = request.limit.unwrap_or(120).max(1);

    match request.tool.as_str() {
        "wake" => Ok(AgentToolOutput::Wake(snapshot.wake.clone())),
        "tools" => Ok(AgentToolOutput::Tools(tool_catalog())),
        "session" => session
            .cloned()
            .map(AgentToolOutput::Session)
            .ok_or_else(|| "session state not available".to_string()),
        "watchlist" => {
            let recommendations = build_recommendations(snapshot, session);
            let mut watchlist = build_watchlist(snapshot, session, Some(&recommendations), limit);
            if let Some(symbol) = symbol {
                watchlist.entries.retain(|item| {
                    item.symbol.eq_ignore_ascii_case(symbol)
                        || item
                            .reference_symbols
                            .iter()
                            .any(|value| value.eq_ignore_ascii_case(symbol))
                });
            }
            if let Some(sector) = sector {
                watchlist.entries.retain(|item| {
                    item.sector
                        .as_deref()
                        .map(|value| value.eq_ignore_ascii_case(sector))
                        .unwrap_or(false)
                });
            }
            watchlist.total = watchlist.entries.len();
            if watchlist.entries.len() > limit {
                watchlist.entries.truncate(limit);
                watchlist.total = watchlist.entries.len();
            }
            for (index, item) in watchlist.entries.iter_mut().enumerate() {
                item.rank = index + 1;
            }
            Ok(AgentToolOutput::Watchlist(watchlist))
        }
        "recommendations" => {
            let mut recommendations = build_recommendations(snapshot, session);
            recommendations
                .decisions
                .retain(|decision| decision_matches_filters(decision, symbol, sector));
            if recommendations.decisions.len() > limit {
                recommendations.decisions.truncate(limit);
            }
            sync_recommendation_views(&mut recommendations);
            Ok(AgentToolOutput::Recommendations(recommendations))
        }
        "alert_scoreboard" => {
            let recommendations = build_recommendations(snapshot, session);
            Ok(AgentToolOutput::Scoreboard(build_alert_scoreboard(
                snapshot,
                Some(&recommendations),
                None,
            )))
        }
        "eod_review" => {
            let recommendations = build_recommendations(snapshot, session);
            let scoreboard = build_alert_scoreboard(snapshot, Some(&recommendations), None);
            Ok(AgentToolOutput::EodReview(build_eod_review(
                snapshot,
                &scoreboard,
            )))
        }
        "threads" => {
            let session = session.ok_or_else(|| "session state not available".to_string())?;
            let mut threads = session.active_threads.clone();
            if let Some(symbol) = symbol {
                threads.retain(|item| item.symbol.eq_ignore_ascii_case(symbol));
            }
            if let Some(sector) = sector {
                threads.retain(|item| {
                    item.sector
                        .as_deref()
                        .map(|value| value.eq_ignore_ascii_case(sector))
                        .unwrap_or(false)
                });
            }
            threads.truncate(limit);
            if let Some(symbol) = symbol {
                if let Some(thread) = threads
                    .iter()
                    .find(|item| item.symbol.eq_ignore_ascii_case(symbol))
                    .cloned()
                {
                    Ok(AgentToolOutput::Thread(thread))
                } else {
                    Err(format!("no thread found for `{symbol}`"))
                }
            } else {
                Ok(AgentToolOutput::Threads(threads))
            }
        }
        "turns" => {
            let session = session.ok_or_else(|| "session state not available".to_string())?;
            let mut turns = session.recent_turns.clone();
            if let Some(since_tick) = since_tick {
                turns.retain(|item| item.tick > since_tick);
            }
            if let Some(symbol) = symbol {
                turns.retain(|item| {
                    item.focus_symbols
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(symbol))
                });
            }
            if turns.len() > limit {
                turns = turns[turns.len().saturating_sub(limit)..].to_vec();
            }
            Ok(AgentToolOutput::Turns(turns))
        }
        "notices" => {
            let mut notices = snapshot.notices.clone();
            if let Some(since_tick) = since_tick {
                notices.retain(|item| item.tick > since_tick);
            }
            if let Some(symbol) = symbol {
                notices.retain(|item| {
                    item.symbol
                        .as_deref()
                        .map(|value| value.eq_ignore_ascii_case(symbol))
                        .unwrap_or(false)
                });
            }
            if let Some(sector) = sector {
                notices.retain(|item| {
                    item.sector
                        .as_deref()
                        .map(|value| value.eq_ignore_ascii_case(sector))
                        .unwrap_or(false)
                });
            }
            notices.truncate(limit);
            Ok(AgentToolOutput::Notices(notices))
        }
        "macro_event_candidates" => {
            let mut items = snapshot.macro_event_candidates.clone();
            if let Some(since_tick) = since_tick {
                items.retain(|item| item.tick > since_tick);
            }
            if let Some(symbol) = symbol {
                items.retain(|item| {
                    item.impact
                        .affected_symbols
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(symbol))
                });
            }
            if let Some(sector) = sector {
                items.retain(|item| {
                    item.impact
                        .affected_sectors
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(sector))
                });
            }
            items.truncate(limit);
            Ok(AgentToolOutput::MacroEventCandidates(items))
        }
        "macro_events" => {
            let mut items = snapshot.macro_events.clone();
            if let Some(since_tick) = since_tick {
                items.retain(|item| item.tick > since_tick);
            }
            if let Some(symbol) = symbol {
                items.retain(|item| {
                    item.impact
                        .affected_symbols
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(symbol))
                });
            }
            if let Some(sector) = sector {
                items.retain(|item| {
                    item.impact
                        .affected_sectors
                        .iter()
                        .any(|value| value.eq_ignore_ascii_case(sector))
                });
            }
            items.truncate(limit);
            Ok(AgentToolOutput::MacroEvents(items))
        }
        "knowledge_links" => {
            let recommendations = build_recommendations(snapshot, session);
            let mut links = snapshot.knowledge_links.clone();
            links.extend(recommendations.knowledge_links);
            if let Some(symbol) = symbol {
                links.retain(|item| knowledge_link_matches_filters(item, Some(symbol), None));
            }
            if let Some(sector) = sector {
                links.retain(|item| knowledge_link_matches_filters(item, None, Some(sector)));
            }
            links.truncate(limit);
            Ok(AgentToolOutput::KnowledgeLinks(links))
        }
        "transitions_since" => {
            let mut transitions = snapshot.recent_transitions.clone();
            if let Some(since_tick) = since_tick {
                transitions.retain(|item| item.to_tick > since_tick);
            }
            if let Some(symbol) = symbol {
                transitions.retain(|item| item.symbol.eq_ignore_ascii_case(symbol));
            }
            if let Some(sector) = sector {
                transitions.retain(|item| {
                    item.sector
                        .as_deref()
                        .map(|value| value.eq_ignore_ascii_case(sector))
                        .unwrap_or(false)
                });
            }
            transitions.truncate(limit);
            Ok(AgentToolOutput::Transitions(transitions))
        }
        "active_structures" => {
            let mut structures = snapshot.active_structures.clone();
            if let Some(symbol) = symbol {
                structures.retain(|item| item.symbol.eq_ignore_ascii_case(symbol));
            }
            if let Some(sector) = sector {
                structures.retain(|item| {
                    item.sector
                        .as_deref()
                        .map(|value| value.eq_ignore_ascii_case(sector))
                        .unwrap_or(false)
                });
            }
            structures.truncate(limit);
            Ok(AgentToolOutput::Structures(structures))
        }
        "structure_state" => {
            let symbol =
                symbol.ok_or_else(|| "tool `structure_state` requires `symbol`".to_string())?;
            let structure = snapshot
                .symbol(symbol)
                .and_then(|item| item.structure.clone())
                .ok_or_else(|| format!("no active structure found for `{symbol}`"))?;
            Ok(AgentToolOutput::Structure(structure))
        }
        "symbol_state" => {
            let symbol =
                symbol.ok_or_else(|| "tool `symbol_state` requires `symbol`".to_string())?;
            let state = snapshot
                .symbol(symbol)
                .cloned()
                .ok_or_else(|| format!("no symbol state found for `{symbol}`"))?;
            Ok(AgentToolOutput::Symbol(state))
        }
        "depth_change" => {
            let symbol =
                symbol.ok_or_else(|| "tool `depth_change` requires `symbol`".to_string())?;
            let depth = snapshot
                .symbol(symbol)
                .and_then(|item| item.depth.clone())
                .ok_or_else(|| format!("no depth state found for `{symbol}`"))?;
            Ok(AgentToolOutput::Depth(depth))
        }
        "broker_movement" => {
            let symbol =
                symbol.ok_or_else(|| "tool `broker_movement` requires `symbol`".to_string())?;
            let brokers = snapshot
                .symbol(symbol)
                .and_then(|item| item.brokers.clone())
                .ok_or_else(|| format!("no broker state found for `{symbol}`"))?;
            Ok(AgentToolOutput::Brokers(brokers))
        }
        "invalidation_status" => {
            let symbol =
                symbol.ok_or_else(|| "tool `invalidation_status` requires `symbol`".to_string())?;
            let invalidation = snapshot
                .symbol(symbol)
                .and_then(|item| item.invalidation.clone())
                .ok_or_else(|| format!("no invalidation state found for `{symbol}`"))?;
            Ok(AgentToolOutput::Invalidation(invalidation))
        }
        "sector_flow" => {
            let mut flows = snapshot.sector_flows.clone();
            if let Some(sector) = sector {
                flows.retain(|item| item.sector.eq_ignore_ascii_case(sector));
            }
            flows.truncate(limit);
            Ok(AgentToolOutput::SectorFlow(flows))
        }
        "world_state" => snapshot
            .world_state
            .clone()
            .map(AgentToolOutput::World)
            .ok_or_else(|| "world state not available for this market".to_string()),
        "backward_investigation" => {
            let symbol = symbol
                .ok_or_else(|| "tool `backward_investigation` requires `symbol`".to_string())?;
            let backward = snapshot
                .backward_investigation(symbol)
                .cloned()
                .ok_or_else(|| format!("no backward investigation found for `{symbol}`"))?;
            Ok(AgentToolOutput::Backward(backward))
        }
        other => Err(format!("unsupported agent tool `{other}`")),
    }
}
