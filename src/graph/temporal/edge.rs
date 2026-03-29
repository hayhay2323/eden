use super::*;
use crate::graph::temporal::shared::{
    canonical_pair, institution_key, institution_label, sector_key, sector_label, stock_key,
    stock_label,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphEdgeKind {
    InstitutionToStock,
    StockToSector,
    StockToStock,
    InstitutionToInstitution,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GraphEdgeId {
    pub kind: GraphEdgeKind,
    pub source_key: String,
    pub target_key: String,
}

impl GraphEdgeId {
    pub fn institution_to_stock(institution_id: InstitutionId, symbol: &Symbol) -> Self {
        Self {
            kind: GraphEdgeKind::InstitutionToStock,
            source_key: institution_key(institution_id),
            target_key: stock_key(symbol),
        }
    }

    pub fn stock_to_sector(symbol: &Symbol, sector_id: &SectorId) -> Self {
        Self {
            kind: GraphEdgeKind::StockToSector,
            source_key: stock_key(symbol),
            target_key: sector_key(sector_id),
        }
    }

    pub fn stock_to_stock(left: &Symbol, right: &Symbol) -> Self {
        let left_key = stock_key(left);
        let right_key = stock_key(right);
        let (source_key, target_key) = canonical_pair(left_key, right_key);
        Self {
            kind: GraphEdgeKind::StockToStock,
            source_key,
            target_key,
        }
    }

    pub fn institution_to_institution(left: InstitutionId, right: InstitutionId) -> Self {
        let left_key = institution_key(left);
        let right_key = institution_key(right);
        let (source_key, target_key) = canonical_pair(left_key, right_key);
        Self {
            kind: GraphEdgeKind::InstitutionToInstitution,
            source_key,
            target_key,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdgeState {
    pub edge_id: GraphEdgeId,
    pub source_label: String,
    pub target_label: String,
    pub active: bool,
    #[serde(with = "rfc3339")]
    pub first_seen_at: OffsetDateTime,
    pub first_seen_tick: u64,
    #[serde(with = "rfc3339")]
    pub last_seen_at: OffsetDateTime,
    pub last_seen_tick: u64,
    #[serde(with = "rfc3339")]
    pub last_appeared_at: OffsetDateTime,
    pub last_appeared_tick: u64,
    #[serde(default, with = "time::serde::rfc3339::option")]
    pub last_disappeared_at: Option<OffsetDateTime>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_disappeared_tick: Option<u64>,
    pub seen_count: u64,
    pub appearance_count: u64,
    pub disappearance_count: u64,
    pub last_value: Decimal,
    pub last_magnitude: Decimal,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphEdgeTransitionKind {
    Appeared,
    Reappeared,
    Disappeared,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphEdgeTransition {
    pub kind: GraphEdgeTransitionKind,
    pub edge_id: GraphEdgeId,
    pub source_label: String,
    pub target_label: String,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub tick: u64,
    pub value: Decimal,
    pub magnitude: Decimal,
    pub first_seen_tick: u64,
    pub last_seen_tick: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphTemporalDelta {
    pub active_edge_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transitions: Vec<GraphEdgeTransition>,
}

#[derive(Debug, Clone)]
struct GraphEdgeObservation {
    edge_id: GraphEdgeId,
    source_label: String,
    target_label: String,
    value: Decimal,
    magnitude: Decimal,
}

#[derive(Debug, Default)]
pub struct TemporalEdgeRegistry {
    records: HashMap<GraphEdgeId, GraphEdgeState>,
}

impl TemporalEdgeRegistry {
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
        }
    }

    pub fn update(&mut self, brain: &BrainGraph, tick: u64) -> GraphTemporalDelta {
        let observed = collect_edge_observations(brain);
        let now = brain.timestamp;
        let mut seen_ids = HashSet::new();
        let mut transitions = Vec::new();

        for observation in observed.values() {
            seen_ids.insert(observation.edge_id.clone());
            match self.records.get_mut(&observation.edge_id) {
                Some(record) if record.active => {
                    record.last_seen_at = now;
                    record.last_seen_tick = tick;
                    record.seen_count += 1;
                    record.last_value = observation.value;
                    record.last_magnitude = observation.magnitude;
                    record.source_label = observation.source_label.clone();
                    record.target_label = observation.target_label.clone();
                }
                Some(record) => {
                    record.active = true;
                    record.last_seen_at = now;
                    record.last_seen_tick = tick;
                    record.last_appeared_at = now;
                    record.last_appeared_tick = tick;
                    record.seen_count += 1;
                    record.appearance_count += 1;
                    record.last_value = observation.value;
                    record.last_magnitude = observation.magnitude;
                    record.source_label = observation.source_label.clone();
                    record.target_label = observation.target_label.clone();
                    transitions.push(GraphEdgeTransition {
                        kind: GraphEdgeTransitionKind::Reappeared,
                        edge_id: record.edge_id.clone(),
                        source_label: record.source_label.clone(),
                        target_label: record.target_label.clone(),
                        timestamp: now,
                        tick,
                        value: record.last_value,
                        magnitude: record.last_magnitude,
                        first_seen_tick: record.first_seen_tick,
                        last_seen_tick: record.last_seen_tick,
                    });
                }
                None => {
                    let state = GraphEdgeState {
                        edge_id: observation.edge_id.clone(),
                        source_label: observation.source_label.clone(),
                        target_label: observation.target_label.clone(),
                        active: true,
                        first_seen_at: now,
                        first_seen_tick: tick,
                        last_seen_at: now,
                        last_seen_tick: tick,
                        last_appeared_at: now,
                        last_appeared_tick: tick,
                        last_disappeared_at: None,
                        last_disappeared_tick: None,
                        seen_count: 1,
                        appearance_count: 1,
                        disappearance_count: 0,
                        last_value: observation.value,
                        last_magnitude: observation.magnitude,
                    };
                    transitions.push(GraphEdgeTransition {
                        kind: GraphEdgeTransitionKind::Appeared,
                        edge_id: state.edge_id.clone(),
                        source_label: state.source_label.clone(),
                        target_label: state.target_label.clone(),
                        timestamp: now,
                        tick,
                        value: state.last_value,
                        magnitude: state.last_magnitude,
                        first_seen_tick: state.first_seen_tick,
                        last_seen_tick: state.last_seen_tick,
                    });
                    self.records.insert(observation.edge_id.clone(), state);
                }
            }
        }

        let disappeared_ids = self
            .records
            .iter()
            .filter(|(edge_id, record)| record.active && !seen_ids.contains(*edge_id))
            .map(|(edge_id, _)| edge_id.clone())
            .collect::<Vec<_>>();

        for edge_id in disappeared_ids {
            if let Some(record) = self.records.get_mut(&edge_id) {
                record.active = false;
                record.disappearance_count += 1;
                record.last_disappeared_at = Some(now);
                record.last_disappeared_tick = Some(tick);
                transitions.push(GraphEdgeTransition {
                    kind: GraphEdgeTransitionKind::Disappeared,
                    edge_id: record.edge_id.clone(),
                    source_label: record.source_label.clone(),
                    target_label: record.target_label.clone(),
                    timestamp: now,
                    tick,
                    value: record.last_value,
                    magnitude: record.last_magnitude,
                    first_seen_tick: record.first_seen_tick,
                    last_seen_tick: record.last_seen_tick,
                });
            }
        }

        transitions.sort_by(|left, right| {
            left.tick
                .cmp(&right.tick)
                .then_with(|| left.edge_id.cmp(&right.edge_id))
                .then_with(|| transition_rank(left.kind).cmp(&transition_rank(right.kind)))
        });

        GraphTemporalDelta {
            active_edge_count: self.records.values().filter(|record| record.active).count(),
            transitions,
        }
    }

    pub fn edge_state(&self, edge_id: &GraphEdgeId) -> Option<&GraphEdgeState> {
        self.records.get(edge_id)
    }

    pub fn active_edges(&self) -> Vec<GraphEdgeState> {
        let mut items = self
            .records
            .values()
            .filter(|record| record.active)
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by(|left, right| {
            right
                .last_magnitude
                .cmp(&left.last_magnitude)
                .then_with(|| left.edge_id.cmp(&right.edge_id))
        });
        items
    }
}

fn collect_edge_observations(brain: &BrainGraph) -> HashMap<GraphEdgeId, GraphEdgeObservation> {
    let mut observed = HashMap::new();

    for edge in brain.graph.edge_references() {
        let Some(source_node) = brain.graph.node_weight(edge.source()) else {
            continue;
        };
        let Some(target_node) = brain.graph.node_weight(edge.target()) else {
            continue;
        };

        let observation = match (source_node, target_node, edge.weight()) {
            (
                NodeKind::Institution(source),
                NodeKind::Stock(target),
                EdgeKind::InstitutionToStock(item),
            ) => Some(GraphEdgeObservation {
                edge_id: GraphEdgeId::institution_to_stock(source.institution_id, &target.symbol),
                source_label: institution_label(source.institution_id),
                target_label: stock_label(&target.symbol),
                value: item.direction,
                magnitude: item.direction.abs(),
            }),
            (NodeKind::Stock(source), NodeKind::Sector(target), EdgeKind::StockToSector(_)) => {
                Some(GraphEdgeObservation {
                    edge_id: GraphEdgeId::stock_to_sector(&source.symbol, &target.sector_id),
                    source_label: stock_label(&source.symbol),
                    target_label: sector_label(&target.sector_id),
                    value: Decimal::ONE,
                    magnitude: Decimal::ONE,
                })
            }
            (NodeKind::Stock(source), NodeKind::Stock(target), EdgeKind::StockToStock(item)) => {
                Some(GraphEdgeObservation {
                    edge_id: GraphEdgeId::stock_to_stock(&source.symbol, &target.symbol),
                    source_label: stock_label(&source.symbol),
                    target_label: stock_label(&target.symbol),
                    value: item.similarity,
                    magnitude: item.similarity.abs(),
                })
            }
            (
                NodeKind::Institution(source),
                NodeKind::Institution(target),
                EdgeKind::InstitutionToInstitution(item),
            ) => Some(GraphEdgeObservation {
                edge_id: GraphEdgeId::institution_to_institution(
                    source.institution_id,
                    target.institution_id,
                ),
                source_label: institution_label(source.institution_id),
                target_label: institution_label(target.institution_id),
                value: item.jaccard,
                magnitude: item.jaccard.abs(),
            }),
            _ => None,
        };

        if let Some(observation) = observation {
            observed
                .entry(observation.edge_id.clone())
                .or_insert(observation);
        }
    }

    observed
}

fn transition_rank(kind: GraphEdgeTransitionKind) -> u8 {
    match kind {
        GraphEdgeTransitionKind::Appeared => 0,
        GraphEdgeTransitionKind::Reappeared => 1,
        GraphEdgeTransitionKind::Disappeared => 2,
    }
}
