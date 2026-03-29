use super::*;
use rust_decimal::Decimal;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroEventHistoryRecord {
    pub record_id: String,
    pub event_id: String,
    pub tick_number: u64,
    pub market: String,
    #[serde(with = "rfc3339")]
    pub recorded_at: OffsetDateTime,
    pub event_type: String,
    pub authority_level: String,
    pub headline: String,
    pub summary: String,
    pub confidence: Decimal,
    pub confirmation_state: String,
    pub primary_scope: String,
    pub affected_markets: Vec<String>,
    pub affected_sectors: Vec<String>,
    pub affected_symbols: Vec<String>,
    pub preferred_expression: String,
    pub requires_market_confirmation: bool,
    pub decisive_factors: Vec<String>,
    pub supporting_notice_ids: Vec<String>,
    pub promotion_reasons: Vec<String>,
}

impl MacroEventHistoryRecord {
    pub fn from_agent_event(event: &AgentMacroEvent, recorded_at: OffsetDateTime) -> Self {
        Self {
            record_id: macro_event_record_id(event.market, event.tick, &event.event_id),
            event_id: event.event_id.clone(),
            tick_number: event.tick,
            market: market_slug(event.market).into(),
            recorded_at,
            event_type: event.event_type.clone(),
            authority_level: event.authority_level.clone(),
            headline: event.headline.clone(),
            summary: event.summary.clone(),
            confidence: event.confidence,
            confirmation_state: event.confirmation_state.clone(),
            primary_scope: event.impact.primary_scope.clone(),
            affected_markets: event.impact.affected_markets.clone(),
            affected_sectors: event.impact.affected_sectors.clone(),
            affected_symbols: event.impact.affected_symbols.clone(),
            preferred_expression: event.impact.preferred_expression.clone(),
            requires_market_confirmation: event.impact.requires_market_confirmation,
            decisive_factors: event.impact.decisive_factors.clone(),
            supporting_notice_ids: event.supporting_notice_ids.clone(),
            promotion_reasons: event.promotion_reasons.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeLinkHistoryRecord {
    pub record_id: String,
    pub link_id: String,
    pub tick_number: u64,
    pub market: String,
    #[serde(with = "rfc3339")]
    pub recorded_at: OffsetDateTime,
    pub relation: KnowledgeRelation,
    pub source_node_kind: String,
    pub source_node_id: String,
    pub source_label: String,
    pub target_node_kind: String,
    pub target_node_id: String,
    pub target_label: String,
    pub confidence: Decimal,
    pub attributes: KnowledgeLinkAttributes,
    pub rationale: Option<String>,
}

impl KnowledgeLinkHistoryRecord {
    pub fn from_agent_link(
        market: LiveMarket,
        tick_number: u64,
        recorded_at: OffsetDateTime,
        link: &AgentKnowledgeLink,
    ) -> Self {
        Self {
            record_id: knowledge_link_record_id(market, tick_number, &link.link_id),
            link_id: link.link_id.clone(),
            tick_number,
            market: market_slug(market).into(),
            recorded_at,
            relation: link.relation.clone(),
            source_node_kind: link.source.node_kind.to_string(),
            source_node_id: link.source.node_id.clone(),
            source_label: link.source.label.clone(),
            target_node_kind: link.target.node_kind.to_string(),
            target_node_id: link.target.node_id.clone(),
            target_label: link.target.label.clone(),
            confidence: link.confidence,
            attributes: link.attributes.clone(),
            rationale: link.rationale.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroEventStateRecord {
    pub state_id: String,
    pub event_id: String,
    pub market: String,
    pub latest_tick_number: u64,
    #[serde(with = "rfc3339")]
    pub last_seen_at: OffsetDateTime,
    pub event_type: String,
    pub authority_level: String,
    pub headline: String,
    pub summary: String,
    pub confidence: Decimal,
    pub confirmation_state: String,
    pub primary_scope: String,
    pub affected_markets: Vec<String>,
    pub affected_sectors: Vec<String>,
    pub affected_symbols: Vec<String>,
    pub preferred_expression: String,
    pub requires_market_confirmation: bool,
    pub decisive_factors: Vec<String>,
    pub supporting_notice_ids: Vec<String>,
    pub promotion_reasons: Vec<String>,
}

impl MacroEventStateRecord {
    pub fn from_agent_event(event: &AgentMacroEvent, recorded_at: OffsetDateTime) -> Self {
        Self {
            state_id: macro_event_state_id(event.market, &event.event_id),
            event_id: event.event_id.clone(),
            market: market_slug(event.market).into(),
            latest_tick_number: event.tick,
            last_seen_at: recorded_at,
            event_type: event.event_type.clone(),
            authority_level: event.authority_level.clone(),
            headline: event.headline.clone(),
            summary: event.summary.clone(),
            confidence: event.confidence,
            confirmation_state: event.confirmation_state.clone(),
            primary_scope: event.impact.primary_scope.clone(),
            affected_markets: event.impact.affected_markets.clone(),
            affected_sectors: event.impact.affected_sectors.clone(),
            affected_symbols: event.impact.affected_symbols.clone(),
            preferred_expression: event.impact.preferred_expression.clone(),
            requires_market_confirmation: event.impact.requires_market_confirmation,
            decisive_factors: event.impact.decisive_factors.clone(),
            supporting_notice_ids: event.supporting_notice_ids.clone(),
            promotion_reasons: event.promotion_reasons.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeLinkStateRecord {
    pub state_id: String,
    pub link_id: String,
    pub market: String,
    pub latest_tick_number: u64,
    #[serde(with = "rfc3339")]
    pub last_seen_at: OffsetDateTime,
    pub relation: KnowledgeRelation,
    pub source_node_kind: String,
    pub source_node_id: String,
    pub source_label: String,
    pub target_node_kind: String,
    pub target_node_id: String,
    pub target_label: String,
    pub confidence: Decimal,
    pub attributes: KnowledgeLinkAttributes,
    pub rationale: Option<String>,
}

impl KnowledgeLinkStateRecord {
    pub fn from_agent_link(
        market: LiveMarket,
        tick_number: u64,
        recorded_at: OffsetDateTime,
        link: &AgentKnowledgeLink,
    ) -> Self {
        Self {
            state_id: knowledge_link_state_id(market, &link.link_id),
            link_id: link.link_id.clone(),
            market: market_slug(market).into(),
            latest_tick_number: tick_number,
            last_seen_at: recorded_at,
            relation: link.relation.clone(),
            source_node_kind: link.source.node_kind.to_string(),
            source_node_id: link.source.node_id.clone(),
            source_label: link.source.label.clone(),
            target_node_kind: link.target.node_kind.to_string(),
            target_node_id: link.target.node_id.clone(),
            target_label: link.target.label.clone(),
            confidence: link.confidence,
            attributes: link.attributes.clone(),
            rationale: link.rationale.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEventHistoryRecord {
    pub record_id: String,
    pub event_id: String,
    pub tick_number: u64,
    pub market: String,
    #[serde(with = "rfc3339")]
    pub recorded_at: OffsetDateTime,
    pub kind: KnowledgeEventKind,
    pub subject_node_kind: String,
    pub subject_node_id: String,
    pub subject_label: String,
    pub object_node_kind: Option<String>,
    pub object_node_id: Option<String>,
    pub object_label: Option<String>,
    pub confidence: Decimal,
    pub evidence: Vec<EvidenceRef>,
    pub attributes: KnowledgeEventAttributes,
    pub rationale: Option<String>,
}

impl KnowledgeEventHistoryRecord {
    pub fn from_agent_event(
        market: LiveMarket,
        tick_number: u64,
        recorded_at: OffsetDateTime,
        event: &AgentKnowledgeEvent,
    ) -> Self {
        Self {
            record_id: knowledge_event_record_id(market, tick_number, &event.event_id),
            event_id: event.event_id.clone(),
            tick_number,
            market: market_slug(market).into(),
            recorded_at,
            kind: event.kind,
            subject_node_kind: event.subject.node_kind.to_string(),
            subject_node_id: event.subject.node_id.clone(),
            subject_label: event.subject.label.clone(),
            object_node_kind: event.object.as_ref().map(|item| item.node_kind.to_string()),
            object_node_id: event.object.as_ref().map(|item| item.node_id.clone()),
            object_label: event.object.as_ref().map(|item| item.label.clone()),
            confidence: event.confidence,
            evidence: event.evidence.clone(),
            attributes: event.attributes.clone(),
            rationale: event.rationale.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeEventStateRecord {
    pub state_id: String,
    pub event_id: String,
    pub market: String,
    pub latest_tick_number: u64,
    #[serde(with = "rfc3339")]
    pub last_seen_at: OffsetDateTime,
    pub kind: KnowledgeEventKind,
    pub subject_node_kind: String,
    pub subject_node_id: String,
    pub subject_label: String,
    pub object_node_kind: Option<String>,
    pub object_node_id: Option<String>,
    pub object_label: Option<String>,
    pub confidence: Decimal,
    pub evidence: Vec<EvidenceRef>,
    pub attributes: KnowledgeEventAttributes,
    pub rationale: Option<String>,
}

impl KnowledgeEventStateRecord {
    pub fn from_agent_event(
        market: LiveMarket,
        tick_number: u64,
        recorded_at: OffsetDateTime,
        event: &AgentKnowledgeEvent,
    ) -> Self {
        Self {
            state_id: knowledge_event_state_id(market, &event.event_id),
            event_id: event.event_id.clone(),
            market: market_slug(market).into(),
            latest_tick_number: tick_number,
            last_seen_at: recorded_at,
            kind: event.kind,
            subject_node_kind: event.subject.node_kind.to_string(),
            subject_node_id: event.subject.node_id.clone(),
            subject_label: event.subject.label.clone(),
            object_node_kind: event.object.as_ref().map(|item| item.node_kind.to_string()),
            object_node_id: event.object.as_ref().map(|item| item.node_id.clone()),
            object_label: event.object.as_ref().map(|item| item.label.clone()),
            confidence: event.confidence,
            evidence: event.evidence.clone(),
            attributes: event.attributes.clone(),
            rationale: event.rationale.clone(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeNodeHistoryRecord {
    pub record_id: String,
    pub node_id: String,
    pub node_kind: String,
    pub label: String,
    pub market: String,
    pub tick_number: u64,
    #[serde(with = "rfc3339")]
    pub recorded_at: OffsetDateTime,
    pub attributes: KnowledgeNodeAttributes,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KnowledgeNodeStateRecord {
    pub state_id: String,
    pub node_id: String,
    pub node_kind: String,
    pub label: String,
    pub market: String,
    pub latest_tick_number: u64,
    #[serde(with = "rfc3339")]
    pub last_seen_at: OffsetDateTime,
    pub attributes: KnowledgeNodeAttributes,
}

pub fn macro_event_record_id(market: LiveMarket, tick_number: u64, event_id: &str) -> String {
    format!("{}:{}:{}", market_slug(market), tick_number, event_id)
}

pub fn knowledge_link_record_id(market: LiveMarket, tick_number: u64, link_id: &str) -> String {
    format!("{}:{}:{}", market_slug(market), tick_number, link_id)
}

pub fn macro_event_state_id(market: LiveMarket, event_id: &str) -> String {
    format!("{}:{}", market_slug(market), event_id)
}

pub fn knowledge_link_state_id(market: LiveMarket, link_id: &str) -> String {
    format!("{}:{}", market_slug(market), link_id)
}

pub fn knowledge_event_record_id(market: LiveMarket, tick_number: u64, event_id: &str) -> String {
    format!("{}:{}:{}", market_slug(market), tick_number, event_id)
}

pub fn knowledge_event_state_id(market: LiveMarket, event_id: &str) -> String {
    format!("{}:{}", market_slug(market), event_id)
}

pub fn knowledge_node_history_record(
    market: LiveMarket,
    tick_number: u64,
    recorded_at: OffsetDateTime,
    node: &AgentKnowledgeNodeRef,
    attributes: KnowledgeNodeAttributes,
) -> KnowledgeNodeHistoryRecord {
    KnowledgeNodeHistoryRecord {
        record_id: format!("{}:{}:{}", market_slug(market), tick_number, node.node_id),
        node_id: node.node_id.clone(),
        node_kind: node.node_kind.to_string(),
        label: node.label.clone(),
        market: market_slug(market).into(),
        tick_number,
        recorded_at,
        attributes,
    }
}

pub fn knowledge_node_state_record(
    market: LiveMarket,
    tick_number: u64,
    recorded_at: OffsetDateTime,
    node: &AgentKnowledgeNodeRef,
    attributes: KnowledgeNodeAttributes,
) -> KnowledgeNodeStateRecord {
    KnowledgeNodeStateRecord {
        state_id: format!("{}:{}", market_slug(market), node.node_id),
        node_id: node.node_id.clone(),
        node_kind: node.node_kind.to_string(),
        label: node.label.clone(),
        market: market_slug(market).into(),
        latest_tick_number: tick_number,
        last_seen_at: recorded_at,
        attributes,
    }
}
