use super::*;
use rust_decimal::Decimal;
use serde_json::Value;

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
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
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
#[serde(try_from = "RawKnowledgeLinkHistoryRecord")]
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
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub confidence: Decimal,
    pub attributes: KnowledgeLinkAttributes,
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawKnowledgeLinkHistoryRecord {
    record_id: String,
    link_id: String,
    tick_number: u64,
    market: String,
    #[serde(with = "rfc3339")]
    recorded_at: OffsetDateTime,
    relation: KnowledgeRelation,
    source_node_kind: String,
    source_node_id: String,
    source_label: String,
    target_node_kind: String,
    target_node_id: String,
    target_label: String,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    confidence: Decimal,
    attributes: Value,
    rationale: Option<String>,
}

fn decode_knowledge_link_attributes(
    relation: &KnowledgeRelation,
    mut value: Value,
) -> Result<KnowledgeLinkAttributes, serde_json::Error> {
    if value.is_null() {
        return Ok(KnowledgeLinkAttributes::Generic);
    }
    if let Some(object) = value.as_object_mut() {
        if !object.contains_key("schema") && object.len() == 1 {
            if let Some((schema, nested)) = object.iter().next() {
                if let Some(nested_object) = nested.as_object() {
                    let mut promoted = nested_object.clone();
                    promoted
                        .entry("schema".to_string())
                        .or_insert_with(|| Value::String(schema.clone()));
                    return serde_json::from_value(Value::Object(promoted));
                }
            }
        }
        object
            .entry("schema")
            .or_insert_with(|| Value::String(relation.as_str().to_string()));
    }
    serde_json::from_value(value.clone()).or_else(|_| {
        let Some(object) = value.as_object() else {
            return Ok(KnowledgeLinkAttributes::Generic);
        };
        let string = |key: &str| {
            object
                .get(key)
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_default()
        };
        let bool_value = |key: &str| object.get(key).and_then(Value::as_bool).unwrap_or(false);
        let u64_value = |key: &str| {
            object
                .get(key)
                .and_then(|value| {
                    value
                        .as_u64()
                        .or_else(|| value.as_str().and_then(|item| item.parse::<u64>().ok()))
                })
                .unwrap_or(0)
        };
        let decimal_value = |key: &str| {
            object
                .get(key)
                .and_then(|value| match value {
                    Value::String(item) => item.parse::<Decimal>().ok(),
                    Value::Number(item) => item.to_string().parse::<Decimal>().ok(),
                    _ => None,
                })
                .unwrap_or(Decimal::ZERO)
        };
        let optional_decimal_value = |key: &str| {
            object.get(key).and_then(|value| match value {
                Value::Null => None,
                Value::String(item) => item.parse::<Decimal>().ok(),
                Value::Number(item) => item.to_string().parse::<Decimal>().ok(),
                _ => None,
            })
        };

        Ok(match relation {
            KnowledgeRelation::ImpactsMarket => KnowledgeLinkAttributes::ImpactsMarket {
                event_type: string("event_type"),
                authority_level: string("authority_level"),
                primary_scope: string("primary_scope"),
                preferred_expression: string("preferred_expression"),
            },
            KnowledgeRelation::ImpactsSector => KnowledgeLinkAttributes::ImpactsSector {
                event_type: string("event_type"),
                authority_level: string("authority_level"),
                primary_scope: string("primary_scope"),
                preferred_expression: string("preferred_expression"),
            },
            KnowledgeRelation::ImpactsSymbol => KnowledgeLinkAttributes::ImpactsSymbol {
                event_type: string("event_type"),
                authority_level: string("authority_level"),
                primary_scope: string("primary_scope"),
                preferred_expression: string("preferred_expression"),
            },
            KnowledgeRelation::SupportsDecision => KnowledgeLinkAttributes::SupportsDecision {
                decision_scope_kind: string("decision_scope_kind"),
                primary_scope: string("primary_scope"),
                best_action: string("best_action"),
            },
            KnowledgeRelation::DominatesScope => KnowledgeLinkAttributes::DominatesScope {
                decision_scope_kind: string("decision_scope_kind"),
                primary_scope: string("primary_scope"),
                best_action: string("best_action"),
            },
            KnowledgeRelation::DescribesScope => KnowledgeLinkAttributes::DescribesScope {
                layer: string("layer"),
                regime: string("regime"),
            },
            KnowledgeRelation::TargetsScope => KnowledgeLinkAttributes::TargetsScope {
                source_kind: string("source_kind"),
                scope_kind: string("scope_kind"),
            },
            KnowledgeRelation::DescribesCurrentStateOf => {
                KnowledgeLinkAttributes::DescribesCurrentStateOf {
                    state_kind: string("state_kind"),
                    trend: string("trend"),
                }
            }
            KnowledgeRelation::SupportedByEvidence => {
                KnowledgeLinkAttributes::SupportedByEvidence {
                    channel: string("channel"),
                    polarity: string("polarity"),
                }
            }
            KnowledgeRelation::ContradictedByEvidence => {
                KnowledgeLinkAttributes::ContradictedByEvidence {
                    channel: string("channel"),
                    polarity: string("polarity"),
                }
            }
            KnowledgeRelation::MissingExpectedEvidenceFor => {
                KnowledgeLinkAttributes::MissingExpectedEvidenceFor {
                    channel: string("channel"),
                    polarity: string("polarity"),
                }
            }
            KnowledgeRelation::ExpectsConfirmationFrom => {
                KnowledgeLinkAttributes::ExpectsConfirmationFrom {
                    expectation_kind: string("expectation_kind"),
                    expectation_status: string("expectation_status"),
                }
            }
            KnowledgeRelation::AttentionAllocatedTo => {
                KnowledgeLinkAttributes::AttentionAllocatedTo {
                    channel: string("channel"),
                }
            }
            KnowledgeRelation::UncertainDueTo => KnowledgeLinkAttributes::UncertainDueTo {
                uncertainty_level: decimal_value("uncertainty_level"),
            },
            KnowledgeRelation::CandidateForLeaf => KnowledgeLinkAttributes::CandidateForLeaf {
                leaf_regime: string("leaf_regime"),
                contest_state: string("contest_state"),
            },
            KnowledgeRelation::LeadingCauseForLeaf => {
                KnowledgeLinkAttributes::LeadingCauseForLeaf {
                    leaf_regime: string("leaf_regime"),
                    contest_state: string("contest_state"),
                    leader_streak: u64_value("leader_streak"),
                    cause_gap: optional_decimal_value("cause_gap"),
                }
            }
            KnowledgeRelation::TracksSymbol => KnowledgeLinkAttributes::TracksSymbol {
                stage: string("stage"),
                direction: string("direction"),
                age_ticks: u64_value("age_ticks"),
                exit_forming: bool_value("exit_forming"),
            },
            KnowledgeRelation::TracksSector => KnowledgeLinkAttributes::TracksSector {
                stage: string("stage"),
                direction: string("direction"),
                age_ticks: u64_value("age_ticks"),
                exit_forming: bool_value("exit_forming"),
            },
            KnowledgeRelation::InstantiatesHypothesis => {
                KnowledgeLinkAttributes::InstantiatesHypothesis {
                    action: string("action"),
                    confidence_gap: decimal_value("confidence_gap"),
                }
            }
            KnowledgeRelation::PrimaryMechanism => KnowledgeLinkAttributes::PrimaryMechanism {
                mechanism_score: decimal_value("mechanism_score"),
                case_action: string("case_action"),
            },
            KnowledgeRelation::CompetingMechanism => KnowledgeLinkAttributes::CompetingMechanism {
                mechanism_score: decimal_value("mechanism_score"),
                case_action: string("case_action"),
            },
        })
    })
}

impl TryFrom<RawKnowledgeLinkHistoryRecord> for KnowledgeLinkHistoryRecord {
    type Error = String;

    fn try_from(value: RawKnowledgeLinkHistoryRecord) -> Result<Self, Self::Error> {
        let attributes = decode_knowledge_link_attributes(&value.relation, value.attributes)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            record_id: value.record_id,
            link_id: value.link_id,
            tick_number: value.tick_number,
            market: value.market,
            recorded_at: value.recorded_at,
            relation: value.relation,
            source_node_kind: value.source_node_kind,
            source_node_id: value.source_node_id,
            source_label: value.source_label,
            target_node_kind: value.target_node_kind,
            target_node_id: value.target_node_id,
            target_label: value.target_label,
            confidence: value.confidence,
            attributes,
            rationale: value.rationale,
        })
    }
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
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
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
#[serde(try_from = "RawKnowledgeLinkStateRecord")]
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
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    pub confidence: Decimal,
    pub attributes: KnowledgeLinkAttributes,
    pub rationale: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawKnowledgeLinkStateRecord {
    state_id: String,
    link_id: String,
    market: String,
    latest_tick_number: u64,
    #[serde(with = "rfc3339")]
    last_seen_at: OffsetDateTime,
    relation: KnowledgeRelation,
    source_node_kind: String,
    source_node_id: String,
    source_label: String,
    target_node_kind: String,
    target_node_id: String,
    target_label: String,
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
    confidence: Decimal,
    attributes: Value,
    rationale: Option<String>,
}

impl TryFrom<RawKnowledgeLinkStateRecord> for KnowledgeLinkStateRecord {
    type Error = String;

    fn try_from(value: RawKnowledgeLinkStateRecord) -> Result<Self, Self::Error> {
        let attributes = decode_knowledge_link_attributes(&value.relation, value.attributes)
            .map_err(|error| error.to_string())?;
        Ok(Self {
            state_id: value.state_id,
            link_id: value.link_id,
            market: value.market,
            latest_tick_number: value.latest_tick_number,
            last_seen_at: value.last_seen_at,
            relation: value.relation,
            source_node_kind: value.source_node_kind,
            source_node_id: value.source_node_id,
            source_label: value.source_label,
            target_node_kind: value.target_node_kind,
            target_node_id: value.target_node_id,
            target_label: value.target_label,
            confidence: value.confidence,
            attributes,
            rationale: value.rationale,
        })
    }
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
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
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
    #[serde(with = "rust_decimal::serde::arbitrary_precision")]
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
