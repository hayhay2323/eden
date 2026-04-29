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
    /// A regime candidate seen recently but not yet confirmed across
    /// `REGIME_CONFIRM_TICKS` consecutive observations. Prevents the classifier
    /// from flip-flopping on tick-level noise while a symbol is cleanly trending
    /// or ranging — the observed regime must repeat before we commit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pending_regime: Option<String>,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub pending_regime_streak: u64,
}

fn is_zero_u64(value: &u64) -> bool {
    *value == 0
}

/// Number of consecutive ticks a new regime must persist before we emit
/// `RegimeChanged` and commit the transition. Chosen to damp snap-flips from
/// tick-level coherence noise on trending stocks while still catching real
/// regime shifts within ~9 seconds at the current tick cadence.
const REGIME_CONFIRM_TICKS: u64 = 3;

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
                    if let Some(transition) =
                        advance_regime_with_hysteresis(record, &observation.regime, now, tick)
                    {
                        transitions.push(transition);
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
                    record.pending_regime = None;
                    record.pending_regime_streak = 0;
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
                        pending_regime: None,
                        pending_regime_streak: 0,
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

    /// Per-symbol regime snapshot for downstream per-symbol fingerprinting.
    /// Returns `None` if the symbol has never been seen or the record
    /// has been evicted. Callers use this to derive a symbol-level
    /// `RegimeFingerprint` that can differ from the market-level bucket.
    pub fn stock_regime_stats(&self, symbol: &Symbol) -> Option<StockRegimeStats> {
        let id = GraphNodeId::stock(symbol);
        self.records.get(&id).map(|record| StockRegimeStats {
            regime: record.last_regime.clone(),
            regime_change_count: record.regime_change_count,
            first_seen_tick: record.first_seen_tick,
            last_seen_tick: record.last_seen_tick,
            pending_regime: record.pending_regime.clone(),
            pending_streak: record.pending_regime_streak,
            active: record.active,
        })
    }
}

/// Snapshot of a single stock's regime state as observed by the
/// temporal node registry. Downstream per-symbol fingerprint builders
/// use this to compute `turn_pressure` (normalised flip rate) and
/// `bull_bias` (direction inferred from `regime`).
#[derive(Debug, Clone)]
pub struct StockRegimeStats {
    pub regime: Option<String>,
    pub regime_change_count: u64,
    pub first_seen_tick: u64,
    pub last_seen_tick: u64,
    pub pending_regime: Option<String>,
    pub pending_streak: u64,
    pub active: bool,
}

impl StockRegimeStats {
    /// Flip rate = `regime_change_count / age_in_ticks`, clamped to [0, 1].
    /// 0.0 means a clean trend / range with no regime flips observed;
    /// values approaching 1.0 mean one or more flips per tick (chop).
    pub fn turn_pressure(&self) -> f64 {
        let age = self
            .last_seen_tick
            .saturating_sub(self.first_seen_tick)
            .max(1);
        let rate = self.regime_change_count as f64 / age as f64;
        rate.clamp(0.0, 1.0)
    }

    /// Long bias inferred from the currently committed regime label.
    /// `CoherentBullish` → 0.8, `CoherentBearish` → 0.2, any flavor of
    /// conflicted / unknown → 0.5 (neutral).
    pub fn bull_bias(&self) -> f64 {
        match self.regime.as_deref() {
            Some("CoherentBullish") => 0.8,
            Some("CoherentBearish") => 0.2,
            _ => 0.5,
        }
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

/// Apply a hysteresis-gated regime observation to an active record.
///
/// Returns `Some(transition)` only when the candidate regime has persisted
/// across `REGIME_CONFIRM_TICKS` consecutive observations — this dampens
/// tick-level noise on trending stocks. If the observation matches the
/// committed regime the pending candidate is cleared; if it is new, the
/// streak resets to 1.
fn advance_regime_with_hysteresis(
    record: &mut GraphNodeState,
    observed_regime: &Option<String>,
    now: OffsetDateTime,
    tick: u64,
) -> Option<GraphNodeTransition> {
    if *observed_regime == record.last_regime {
        record.pending_regime = None;
        record.pending_regime_streak = 0;
        return None;
    }

    if record.pending_regime == *observed_regime {
        record.pending_regime_streak += 1;
    } else {
        record.pending_regime = observed_regime.clone();
        record.pending_regime_streak = 1;
    }

    if record.pending_regime_streak < REGIME_CONFIRM_TICKS {
        return None;
    }

    let previous_regime = record.last_regime.clone();
    record.last_regime = observed_regime.clone();
    record.regime_change_count += 1;
    record.pending_regime = None;
    record.pending_regime_streak = 0;
    Some(GraphNodeTransition {
        kind: GraphNodeTransitionKind::RegimeChanged,
        node_id: record.node_id.clone(),
        label: record.label.clone(),
        timestamp: now,
        tick,
        first_seen_tick: record.first_seen_tick,
        last_seen_tick: record.last_seen_tick,
        previous_regime,
        new_regime: observed_regime.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_record(regime: Option<&str>) -> GraphNodeState {
        let now = OffsetDateTime::UNIX_EPOCH;
        GraphNodeState {
            node_id: GraphNodeId {
                kind: GraphNodeKind::Stock,
                key: "stock:TEST.HK".into(),
            },
            label: "TEST.HK".into(),
            active: true,
            first_seen_at: now,
            first_seen_tick: 0,
            last_seen_at: now,
            last_seen_tick: 0,
            last_appeared_at: now,
            last_appeared_tick: 0,
            last_disappeared_at: None,
            last_disappeared_tick: None,
            seen_count: 1,
            appearance_count: 1,
            disappearance_count: 0,
            last_regime: regime.map(|s| s.to_string()),
            regime_change_count: 0,
            pending_regime: None,
            pending_regime_streak: 0,
        }
    }

    #[test]
    fn hysteresis_suppresses_single_tick_flip() {
        let mut record = sample_record(Some("CoherentBullish"));
        let bear = Some("CoherentBearish".to_string());
        let out = advance_regime_with_hysteresis(&mut record, &bear, OffsetDateTime::UNIX_EPOCH, 1);
        assert!(out.is_none(), "first bearish tick must not emit transition");
        assert_eq!(record.last_regime.as_deref(), Some("CoherentBullish"));
        assert_eq!(record.pending_regime.as_deref(), Some("CoherentBearish"));
        assert_eq!(record.pending_regime_streak, 1);
    }

    #[test]
    fn hysteresis_commits_after_confirm_ticks() {
        let mut record = sample_record(Some("CoherentBullish"));
        let bear = Some("CoherentBearish".to_string());
        let now = OffsetDateTime::UNIX_EPOCH;
        for tick in 1..REGIME_CONFIRM_TICKS {
            assert!(advance_regime_with_hysteresis(&mut record, &bear, now, tick).is_none());
        }
        let out = advance_regime_with_hysteresis(&mut record, &bear, now, REGIME_CONFIRM_TICKS);
        let transition = out.expect("transition must fire at confirm threshold");
        assert_eq!(transition.kind, GraphNodeTransitionKind::RegimeChanged);
        assert_eq!(
            transition.previous_regime.as_deref(),
            Some("CoherentBullish")
        );
        assert_eq!(transition.new_regime.as_deref(), Some("CoherentBearish"));
        assert_eq!(record.last_regime.as_deref(), Some("CoherentBearish"));
        assert_eq!(record.regime_change_count, 1);
        assert_eq!(record.pending_regime_streak, 0);
    }

    #[test]
    fn hysteresis_resets_on_regime_return() {
        // Pending bearish for 2 ticks, then bullish held → pending cleared.
        let mut record = sample_record(Some("CoherentBullish"));
        let bear = Some("CoherentBearish".to_string());
        let bull = Some("CoherentBullish".to_string());
        let now = OffsetDateTime::UNIX_EPOCH;
        advance_regime_with_hysteresis(&mut record, &bear, now, 1);
        advance_regime_with_hysteresis(&mut record, &bear, now, 2);
        assert_eq!(record.pending_regime_streak, 2);
        // Now bullish again — should clear pending.
        assert!(advance_regime_with_hysteresis(&mut record, &bull, now, 3).is_none());
        assert_eq!(record.pending_regime, None);
        assert_eq!(record.pending_regime_streak, 0);
        assert_eq!(record.regime_change_count, 0);
    }

    #[test]
    fn hysteresis_resets_on_new_candidate() {
        // Bearish for 2 ticks, then Conflicted → streak restarts at 1.
        let mut record = sample_record(Some("CoherentBullish"));
        let bear = Some("CoherentBearish".to_string());
        let conflicted = Some("Conflicted".to_string());
        let now = OffsetDateTime::UNIX_EPOCH;
        advance_regime_with_hysteresis(&mut record, &bear, now, 1);
        advance_regime_with_hysteresis(&mut record, &bear, now, 2);
        let out = advance_regime_with_hysteresis(&mut record, &conflicted, now, 3);
        assert!(
            out.is_none(),
            "candidate switch must reset streak, not commit"
        );
        assert_eq!(record.pending_regime.as_deref(), Some("Conflicted"));
        assert_eq!(record.pending_regime_streak, 1);
        assert_eq!(record.last_regime.as_deref(), Some("CoherentBullish"));
    }
}
