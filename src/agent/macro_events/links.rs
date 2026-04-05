use super::routing::impact_market_label;
use super::*;

pub(crate) fn build_macro_event_knowledge_links(
    macro_events: &[AgentMacroEvent],
) -> Vec<AgentKnowledgeLink> {
    let mut links = Vec::new();
    let mut seen = HashSet::new();
    for event in macro_events {
        let source = macro_event_node_ref(event);
        for market in &event.impact.affected_markets {
            let target = market_node_ref(market);
            let link_id = format!("{}:{}", source.node_id, target.node_id);
            if seen.insert(link_id.clone()) {
                links.push(AgentKnowledgeLink {
                    link_id,
                    relation: KnowledgeRelation::ImpactsMarket,
                    source: source.clone(),
                    target,
                    confidence: event.confidence,
                    attributes: KnowledgeLinkAttributes::ImpactsMarket {
                        event_type: event.event_type.clone(),
                        authority_level: event.authority_level.clone(),
                        primary_scope: event.impact.primary_scope.clone(),
                        preferred_expression: event.impact.preferred_expression.clone(),
                    },
                    rationale: Some(event.summary.clone()),
                });
            }
        }
        for sector in &event.impact.affected_sectors {
            let target = sector_node_ref(sector);
            let link_id = format!("{}:{}", source.node_id, target.node_id);
            if seen.insert(link_id.clone()) {
                links.push(AgentKnowledgeLink {
                    link_id,
                    relation: KnowledgeRelation::ImpactsSector,
                    source: source.clone(),
                    target,
                    confidence: event.confidence,
                    attributes: KnowledgeLinkAttributes::ImpactsSector {
                        event_type: event.event_type.clone(),
                        authority_level: event.authority_level.clone(),
                        primary_scope: event.impact.primary_scope.clone(),
                        preferred_expression: event.impact.preferred_expression.clone(),
                    },
                    rationale: Some(event.summary.clone()),
                });
            }
        }
        for symbol in &event.impact.affected_symbols {
            let target = symbol_node_ref(symbol);
            let link_id = format!("{}:{}", source.node_id, target.node_id);
            if seen.insert(link_id.clone()) {
                links.push(AgentKnowledgeLink {
                    link_id,
                    relation: KnowledgeRelation::ImpactsSymbol,
                    source: source.clone(),
                    target,
                    confidence: event.confidence,
                    attributes: KnowledgeLinkAttributes::ImpactsSymbol {
                        event_type: event.event_type.clone(),
                        authority_level: event.authority_level.clone(),
                        primary_scope: event.impact.primary_scope.clone(),
                        preferred_expression: event.impact.preferred_expression.clone(),
                    },
                    rationale: Some(event.summary.clone()),
                });
            }
        }
    }
    links
}

pub(crate) fn build_decision_knowledge_links(
    snapshot: &AgentSnapshot,
    decisions: &[AgentDecision],
) -> Vec<AgentKnowledgeLink> {
    let mut links = Vec::new();
    let mut seen = HashSet::new();
    for event in &snapshot.macro_events {
        let source = macro_event_node_ref(event);
        for decision in decisions {
            let Some(relation) = event_supports_decision(snapshot, event, decision) else {
                continue;
            };
            let target = decision_node_ref(decision);
            let link_id = format!("{}:{}:{}", relation, source.node_id, target.node_id);
            if seen.insert(link_id.clone()) {
                links.push(AgentKnowledgeLink {
                    link_id,
                    relation,
                    source: source.clone(),
                    target,
                    confidence: event.confidence,
                    attributes: match decision {
                        AgentDecision::Market(item) => match relation {
                            KnowledgeRelation::SupportsDecision => {
                                KnowledgeLinkAttributes::SupportsDecision {
                                    decision_scope_kind: "market".into(),
                                    primary_scope: event.impact.primary_scope.clone(),
                                    best_action: item.best_action.clone(),
                                }
                            }
                            KnowledgeRelation::DominatesScope => {
                                KnowledgeLinkAttributes::DominatesScope {
                                    decision_scope_kind: "market".into(),
                                    primary_scope: event.impact.primary_scope.clone(),
                                    best_action: item.best_action.clone(),
                                }
                            }
                            _ => KnowledgeLinkAttributes::Generic,
                        },
                        AgentDecision::Sector(item) => match relation {
                            KnowledgeRelation::SupportsDecision => {
                                KnowledgeLinkAttributes::SupportsDecision {
                                    decision_scope_kind: "sector".into(),
                                    primary_scope: event.impact.primary_scope.clone(),
                                    best_action: item.best_action.clone(),
                                }
                            }
                            KnowledgeRelation::DominatesScope => {
                                KnowledgeLinkAttributes::DominatesScope {
                                    decision_scope_kind: "sector".into(),
                                    primary_scope: event.impact.primary_scope.clone(),
                                    best_action: item.best_action.clone(),
                                }
                            }
                            _ => KnowledgeLinkAttributes::Generic,
                        },
                        AgentDecision::Symbol(item) => match relation {
                            KnowledgeRelation::SupportsDecision => {
                                KnowledgeLinkAttributes::SupportsDecision {
                                    decision_scope_kind: "symbol".into(),
                                    primary_scope: event.impact.primary_scope.clone(),
                                    best_action: item.best_action.clone(),
                                }
                            }
                            KnowledgeRelation::DominatesScope => {
                                KnowledgeLinkAttributes::DominatesScope {
                                    decision_scope_kind: "symbol".into(),
                                    primary_scope: event.impact.primary_scope.clone(),
                                    best_action: item.best_action.clone(),
                                }
                            }
                            _ => KnowledgeLinkAttributes::Generic,
                        },
                    },
                    rationale: Some(event.summary.clone()),
                });
            }
        }
    }
    links
}

pub(crate) fn knowledge_link_matches_filters(
    link: &AgentKnowledgeLink,
    symbol: Option<&str>,
    sector: Option<&str>,
) -> bool {
    let symbol_match = match symbol {
        Some(target) => {
            link.source.label.eq_ignore_ascii_case(target)
                || link.target.label.eq_ignore_ascii_case(target)
                || link
                    .source
                    .node_id
                    .eq_ignore_ascii_case(&format!("symbol:{target}"))
                || link
                    .target
                    .node_id
                    .eq_ignore_ascii_case(&format!("symbol:{target}"))
        }
        None => true,
    };
    let sector_match = match sector {
        Some(target) => {
            let sector_id = format!("sector:{}", target.to_ascii_lowercase().replace(' ', "_"));
            link.source.label.eq_ignore_ascii_case(target)
                || link.target.label.eq_ignore_ascii_case(target)
                || link.source.node_id.eq_ignore_ascii_case(&sector_id)
                || link.target.node_id.eq_ignore_ascii_case(&sector_id)
        }
        None => true,
    };
    symbol_match && sector_match
}

fn macro_event_node_ref(event: &AgentMacroEvent) -> AgentKnowledgeNodeRef {
    macro_event_knowledge_node_ref(&event.event_id, &event.headline)
}

fn market_node_ref(label: &str) -> AgentKnowledgeNodeRef {
    market_knowledge_node_ref(label)
}

fn sector_node_ref(sector: &str) -> AgentKnowledgeNodeRef {
    sector_knowledge_node_ref(sector)
}

fn symbol_node_ref(symbol: &str) -> AgentKnowledgeNodeRef {
    symbol_knowledge_node_ref(symbol)
}

fn decision_node_ref(decision: &AgentDecision) -> AgentKnowledgeNodeRef {
    match decision {
        AgentDecision::Market(item) => decision_knowledge_node_ref(
            &item.recommendation_id,
            format!("{} {}", market_scope_symbol(item.market), item.best_action),
        ),
        AgentDecision::Sector(item) => decision_knowledge_node_ref(
            &item.recommendation_id,
            format!("{} {}", item.sector, item.best_action),
        ),
        AgentDecision::Symbol(item) => decision_knowledge_node_ref(
            &item.recommendation_id,
            format!("{} {}", item.symbol, item.best_action),
        ),
    }
}

fn event_supports_decision(
    snapshot: &AgentSnapshot,
    event: &AgentMacroEvent,
    decision: &AgentDecision,
) -> Option<KnowledgeRelation> {
    let market_label = impact_market_label(snapshot.market);
    match decision {
        AgentDecision::Market(_) => event
            .impact
            .affected_markets
            .iter()
            .any(|value| value == &market_label)
            .then_some(KnowledgeRelation::SupportsDecision),
        AgentDecision::Sector(item) => {
            if event
                .impact
                .affected_sectors
                .iter()
                .any(|value| value.eq_ignore_ascii_case(&item.sector))
            {
                Some(KnowledgeRelation::SupportsDecision)
            } else if item.best_action == "wait"
                && event.impact.primary_scope == "market"
                && event
                    .impact
                    .affected_markets
                    .iter()
                    .any(|value| value == &market_label)
            {
                Some(KnowledgeRelation::DominatesScope)
            } else {
                None
            }
        }
        AgentDecision::Symbol(item) => {
            if event
                .impact
                .affected_symbols
                .iter()
                .any(|value| value.eq_ignore_ascii_case(&item.symbol))
            {
                Some(KnowledgeRelation::SupportsDecision)
            } else if item.best_action == "wait"
                && event.impact.primary_scope == "market"
                && event
                    .impact
                    .affected_markets
                    .iter()
                    .any(|value| value == &market_label)
            {
                Some(KnowledgeRelation::DominatesScope)
            } else {
                None
            }
        }
    }
}
