use crate::ontology::knowledge::{AgentKnowledgeNodeRef, KnowledgeNodeKind};
use crate::ontology::objects::{InstitutionId, Symbol};
use crate::ontology::reasoning::ReasoningScope;
use crate::ontology::store::canonical_sector_id;

pub fn normalize_node_component(raw: &str) -> String {
    let mut out = String::new();
    let mut last_was_sep = false;

    for ch in raw.trim().chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
            last_was_sep = false;
        } else if matches!(ch, '.' | '-' | ':') {
            out.push(ch);
            last_was_sep = false;
        } else if !last_was_sep {
            out.push('_');
            last_was_sep = true;
        }
    }

    let out = out.trim_matches('_');
    if out.is_empty() {
        "unknown".into()
    } else {
        out.into()
    }
}

pub fn market_node_id(label: &str) -> String {
    format!("market:{}", normalize_node_component(label))
}

pub fn symbol_node_id(symbol: &str) -> String {
    format!("symbol:{}", normalize_node_component(symbol))
}

pub fn sector_node_id(sector: &str) -> String {
    let canonical = canonical_sector_id(sector)
        .map(str::to_string)
        .unwrap_or_else(|| normalize_node_component(sector));
    format!("sector:{canonical}")
}

pub fn institution_node_id(value: &str) -> String {
    format!("institution:{}", normalize_node_component(value))
}

pub fn institution_numeric_node_id(institution_id: InstitutionId) -> String {
    institution_node_id(&institution_id.0.to_string())
}

pub fn theme_node_id(theme: &str) -> String {
    format!("theme:{}", normalize_node_component(theme))
}

pub fn region_node_id(region: &str) -> String {
    format!("region:{}", normalize_node_component(region))
}

pub fn custom_node_id(value: &str) -> String {
    format!("custom:{}", normalize_node_component(value))
}

pub fn macro_event_node_id(event_id: &str) -> String {
    if event_id.starts_with("macro_event:") {
        event_id.to_string()
    } else {
        format!("macro_event:{event_id}")
    }
}

pub fn decision_node_id(recommendation_id: &str) -> String {
    if recommendation_id.starts_with("decision:") {
        recommendation_id.to_string()
    } else {
        format!("decision:{recommendation_id}")
    }
}

pub fn hypothesis_node_id(hypothesis_id: &str) -> String {
    if hypothesis_id.starts_with("hypothesis:") {
        hypothesis_id.to_string()
    } else {
        format!("hypothesis:{hypothesis_id}")
    }
}

pub fn setup_node_id(setup_id: &str) -> String {
    if setup_id.starts_with("setup:") {
        setup_id.to_string()
    } else {
        format!("setup:{setup_id}")
    }
}

pub fn mechanism_node_id(label: &str) -> String {
    format!("mechanism:{}", normalize_node_component(label))
}

pub fn world_entity_node_id(entity_id: &str) -> String {
    if entity_id.starts_with("world_entity:") {
        entity_id.to_string()
    } else {
        format!("world_entity:{entity_id}")
    }
}

pub fn backward_cause_node_id(cause_id: &str) -> String {
    if cause_id.starts_with("backward_cause:") {
        cause_id.to_string()
    } else {
        format!("backward_cause:{cause_id}")
    }
}

pub fn position_node_id(workflow_id: &str) -> String {
    if workflow_id.starts_with("position:") {
        workflow_id.to_string()
    } else {
        format!("position:{workflow_id}")
    }
}

pub fn scope_node_id(scope: &ReasoningScope) -> String {
    match scope {
        ReasoningScope::Market(market_id) => market_node_id(&market_id.0),
        ReasoningScope::Symbol(symbol) => symbol_node_id(&symbol.0),
        ReasoningScope::Sector(sector) => sector_node_id(&sector.0),
        ReasoningScope::Institution(institution) => institution_numeric_node_id(*institution),
        ReasoningScope::Theme(theme) => theme_node_id(&theme.0),
        ReasoningScope::Region(region) => region_node_id(&region.0),
        ReasoningScope::Custom(value) => custom_node_id(&value.0),
    }
}

pub fn scope_node_label(scope: &ReasoningScope) -> String {
    match scope {
        ReasoningScope::Market(market_id) => market_id.to_string(),
        ReasoningScope::Symbol(Symbol(symbol)) => symbol.clone(),
        ReasoningScope::Sector(sector) => sector.to_string(),
        ReasoningScope::Institution(institution) => institution.to_string(),
        ReasoningScope::Theme(theme) => theme.to_string(),
        ReasoningScope::Region(region) => region.to_string(),
        ReasoningScope::Custom(value) => value.to_string(),
    }
}

pub fn knowledge_node_ref(
    node_kind: impl Into<KnowledgeNodeKind>,
    node_id: String,
    label: String,
) -> AgentKnowledgeNodeRef {
    AgentKnowledgeNodeRef {
        node_kind: node_kind.into(),
        node_id,
        label,
    }
}

pub fn macro_event_knowledge_node_ref(event_id: &str, headline: &str) -> AgentKnowledgeNodeRef {
    knowledge_node_ref(
        KnowledgeNodeKind::MacroEvent,
        macro_event_node_id(event_id),
        headline.into(),
    )
}

pub fn market_knowledge_node_ref(label: &str) -> AgentKnowledgeNodeRef {
    knowledge_node_ref(KnowledgeNodeKind::Market, market_node_id(label), label.into())
}

pub fn sector_knowledge_node_ref(sector: &str) -> AgentKnowledgeNodeRef {
    knowledge_node_ref(KnowledgeNodeKind::Sector, sector_node_id(sector), sector.into())
}

pub fn symbol_knowledge_node_ref(symbol: &str) -> AgentKnowledgeNodeRef {
    knowledge_node_ref(KnowledgeNodeKind::Symbol, symbol_node_id(symbol), symbol.into())
}

pub fn decision_knowledge_node_ref(
    recommendation_id: &str,
    label: String,
) -> AgentKnowledgeNodeRef {
    knowledge_node_ref(KnowledgeNodeKind::Decision, decision_node_id(recommendation_id), label)
}

pub fn world_entity_knowledge_node_ref(entity_id: &str, label: String) -> AgentKnowledgeNodeRef {
    knowledge_node_ref(KnowledgeNodeKind::WorldEntity, world_entity_node_id(entity_id), label)
}

pub fn backward_cause_knowledge_node_ref(cause_id: &str, label: String) -> AgentKnowledgeNodeRef {
    knowledge_node_ref(KnowledgeNodeKind::BackwardCause, backward_cause_node_id(cause_id), label)
}

pub fn position_knowledge_node_ref(workflow_id: &str, label: String) -> AgentKnowledgeNodeRef {
    knowledge_node_ref(KnowledgeNodeKind::Position, position_node_id(workflow_id), label)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scope_node_ids_are_canonical() {
        assert_eq!(market_node_id("HK"), "market:hk");
        assert_eq!(
            sector_node_id("Property Developers"),
            "sector:property_developers"
        );
        assert_eq!(symbol_node_id("700.HK"), "symbol:700.hk");
        assert_eq!(symbol_node_id("700.hk"), "symbol:700.hk");
        assert_eq!(
            scope_node_id(&ReasoningScope::Sector("Property Developers".into())),
            "sector:property_developers"
        );
        assert_eq!(scope_node_id(&ReasoningScope::market()), "market:market");
        assert_eq!(sector_node_id("Technology"), "sector:tech");
        assert_eq!(sector_node_id("科技"), "sector:tech");
        assert_eq!(sector_node_id("中概股"), "sector:china_adr");
        assert_eq!(sector_node_id("金融"), "sector:finance");
        assert_eq!(sector_node_id("加密"), "sector:crypto");
        assert_eq!(sector_node_id("消費"), "sector:consumer");
        assert_eq!(sector_node_id("ETF"), "sector:etf");
        assert_eq!(sector_node_id("電訊傳媒"), "sector:telecom_media");
    }
}
