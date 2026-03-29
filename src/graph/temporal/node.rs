use super::*;
use crate::graph::temporal::shared::{
    institution_key, institution_label, sector_key, sector_label, stock_key, stock_label,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphNodeKind {
    Stock,
    Institution,
    Sector,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct GraphNodeId {
    pub kind: GraphNodeKind,
    pub key: String,
}

impl GraphNodeId {
    pub fn stock(symbol: &Symbol) -> Self {
        Self {
            kind: GraphNodeKind::Stock,
            key: stock_key(symbol),
        }
    }

    pub fn institution(institution_id: InstitutionId) -> Self {
        Self {
            kind: GraphNodeKind::Institution,
            key: institution_key(institution_id),
        }
    }

    pub fn sector(sector_id: &SectorId) -> Self {
        Self {
            kind: GraphNodeKind::Sector,
            key: sector_key(sector_id),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNodeState {
    pub node_id: GraphNodeId,
    pub label: String,
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
    pub last_regime: Option<String>,
    pub regime_change_count: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GraphNodeTransitionKind {
    Appeared,
    Reappeared,
    Disappeared,
    RegimeChanged,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphNodeTransition {
    pub kind: GraphNodeTransitionKind,
    pub node_id: GraphNodeId,
    pub label: String,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub tick: u64,
    pub first_seen_tick: u64,
    pub last_seen_tick: u64,
    pub previous_regime: Option<String>,
    pub new_regime: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct GraphNodeTemporalDelta {
    pub active_node_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transitions: Vec<GraphNodeTransition>,
}

#[derive(Debug, Clone)]
struct GraphNodeObservation {
    node_id: GraphNodeId,
    label: String,
    regime: Option<String>,
}

#[derive(Debug, Default)]
pub struct TemporalNodeRegistry {
    records: HashMap<GraphNodeId, GraphNodeState>,
}

impl TemporalNodeRegistry {
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
        }
    }

    pub fn update(&mut self, brain: &BrainGraph, tick: u64) -> GraphNodeTemporalDelta {
        let observed = collect_node_observations(brain);
        let now = brain.timestamp;
        let mut seen_ids = HashSet::new();
        let mut transitions = Vec::new();

        for observation in observed.values() {
            seen_ids.insert(observation.node_id.clone());
            match self.records.get_mut(&observation.node_id) {
                Some(record) if record.active => {
                    if observation.regime != record.last_regime {
                        let previous_regime = record.last_regime.clone();
                        record.last_regime = observation.regime.clone();
                        record.regime_change_count += 1;
                        transitions.push(GraphNodeTransition {
                            kind: GraphNodeTransitionKind::RegimeChanged,
                            node_id: record.node_id.clone(),
                            label: record.label.clone(),
                            timestamp: now,
                            tick,
                            first_seen_tick: record.first_seen_tick,
                            last_seen_tick: record.last_seen_tick,
                            previous_regime,
                            new_regime: observation.regime.clone(),
                        });
                    }
                    record.last_seen_at = now;
                    record.last_seen_tick = tick;
                    record.seen_count += 1;
                    record.label = observation.label.clone();
                }
                Some(record) => {
                    record.active = true;
                    record.last_seen_at = now;
                    record.last_seen_tick = tick;
                    record.last_appeared_at = now;
                    record.last_appeared_tick = tick;
                    record.seen_count += 1;
                    record.appearance_count += 1;
                    record.label = observation.label.clone();
                    record.last_regime = observation.regime.clone();
                    transitions.push(GraphNodeTransition {
                        kind: GraphNodeTransitionKind::Reappeared,
                        node_id: record.node_id.clone(),
                        label: record.label.clone(),
                        timestamp: now,
                        tick,
                        first_seen_tick: record.first_seen_tick,
                        last_seen_tick: record.last_seen_tick,
                        previous_regime: None,
                        new_regime: record.last_regime.clone(),
                    });
                }
                None => {
                    let state = GraphNodeState {
                        node_id: observation.node_id.clone(),
                        label: observation.label.clone(),
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
                        last_regime: observation.regime.clone(),
                        regime_change_count: 0,
                    };
                    transitions.push(GraphNodeTransition {
                        kind: GraphNodeTransitionKind::Appeared,
                        node_id: state.node_id.clone(),
                        label: state.label.clone(),
                        timestamp: now,
                        tick,
                        first_seen_tick: state.first_seen_tick,
                        last_seen_tick: state.last_seen_tick,
                        previous_regime: None,
                        new_regime: state.last_regime.clone(),
                    });
                    self.records.insert(observation.node_id.clone(), state);
                }
            }
        }

        let disappeared_ids = self
            .records
            .iter()
            .filter(|(node_id, record)| record.active && !seen_ids.contains(*node_id))
            .map(|(node_id, _)| node_id.clone())
            .collect::<Vec<_>>();

        for node_id in disappeared_ids {
            if let Some(record) = self.records.get_mut(&node_id) {
                record.active = false;
                record.disappearance_count += 1;
                record.last_disappeared_at = Some(now);
                record.last_disappeared_tick = Some(tick);
                transitions.push(GraphNodeTransition {
                    kind: GraphNodeTransitionKind::Disappeared,
                    node_id: record.node_id.clone(),
                    label: record.label.clone(),
                    timestamp: now,
                    tick,
                    first_seen_tick: record.first_seen_tick,
                    last_seen_tick: record.last_seen_tick,
                    previous_regime: record.last_regime.clone(),
                    new_regime: None,
                });
            }
        }

        transitions.sort_by(|left, right| {
            left.tick
                .cmp(&right.tick)
                .then_with(|| left.node_id.cmp(&right.node_id))
                .then_with(|| {
                    node_transition_rank(left.kind).cmp(&node_transition_rank(right.kind))
                })
        });

        GraphNodeTemporalDelta {
            active_node_count: self.records.values().filter(|record| record.active).count(),
            transitions,
        }
    }

    pub fn node_state(&self, node_id: &GraphNodeId) -> Option<&GraphNodeState> {
        self.records.get(node_id)
    }

    pub fn active_nodes(&self) -> Vec<GraphNodeState> {
        let mut items = self
            .records
            .values()
            .filter(|record| record.active)
            .cloned()
            .collect::<Vec<_>>();
        items.sort_by(|left, right| left.node_id.cmp(&right.node_id));
        items
    }
}

fn collect_node_observations(brain: &BrainGraph) -> HashMap<GraphNodeId, GraphNodeObservation> {
    let mut observed = HashMap::new();

    for node in brain.graph.node_weights() {
        let observation = match node {
            NodeKind::Stock(s) => GraphNodeObservation {
                node_id: GraphNodeId::stock(&s.symbol),
                label: stock_label(&s.symbol),
                regime: Some(format!("{:?}", s.regime)),
            },
            NodeKind::Institution(i) => GraphNodeObservation {
                node_id: GraphNodeId::institution(i.institution_id),
                label: institution_label(i.institution_id),
                regime: None,
            },
            NodeKind::Sector(s) => GraphNodeObservation {
                node_id: GraphNodeId::sector(&s.sector_id),
                label: sector_label(&s.sector_id),
                regime: None,
            },
        };

        observed
            .entry(observation.node_id.clone())
            .or_insert(observation);
    }

    observed
}

fn node_transition_rank(kind: GraphNodeTransitionKind) -> u8 {
    match kind {
        GraphNodeTransitionKind::Appeared => 0,
        GraphNodeTransitionKind::Reappeared => 1,
        GraphNodeTransitionKind::RegimeChanged => 2,
        GraphNodeTransitionKind::Disappeared => 3,
    }
}
