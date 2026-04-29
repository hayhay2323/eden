use super::*;
use std::collections::VecDeque;

use rust_decimal::prelude::FromPrimitive;
use rust_decimal_macros::dec;

const REPLENISH_WINDOW: u64 = 3;
const REPLENISH_MEMORY: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BrokerSymbolId {
    pub broker_id: BrokerId,
    pub symbol: Symbol,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerTemporalState {
    pub broker_symbol_id: BrokerSymbolId,
    pub active: bool,
    pub first_seen_tick: u64,
    pub last_seen_tick: u64,
    pub seen_count: u64,
    pub last_appeared_tick: u64,
    pub last_disappeared_tick: Option<u64>,
    pub appearance_count: u64,
    pub disappearance_count: u64,
    pub replenish_count: u64,
    pub last_replenished_tick: Option<u64>,
    pub last_replenish_side: Option<Side>,
    pub replenish_intervals: VecDeque<u64>,
    pub replenish_positions: VecDeque<i32>,
    pub depth_recovery_ratios: VecDeque<Decimal>,
    pub replenish_side_consistent: bool,
    pub last_side: Side,
    pub side_flip_count: u64,
    pub last_position: i32,
    pub position_sum: i64,
    pub position_observations: u64,
    pub last_visible_volume: Option<i64>,
    pub last_disappeared_visible_volume: Option<i64>,
    pub institution_id: Option<InstitutionId>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BrokerTransitionKind {
    Appeared,
    Reappeared,
    Disappeared,
    SideFlipped,
    Replenished,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BrokerTransition {
    pub kind: BrokerTransitionKind,
    pub broker_symbol_id: BrokerSymbolId,
    pub tick: u64,
    pub side: Side,
    pub position: i32,
    pub institution_id: Option<InstitutionId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replenish_count: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replenish_interval: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replenish_position_consistency: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replenish_interval_regularity: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replenish_frequency: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub replenish_side_consistent: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub depth_recovery_ratio: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iceberg_confidence: Option<Decimal>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BrokerTemporalDelta {
    pub active_broker_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub transitions: Vec<BrokerTransition>,
}

pub struct TemporalBrokerRegistry {
    records: HashMap<BrokerSymbolId, BrokerTemporalState>,
}

impl TemporalBrokerRegistry {
    pub fn new() -> Self {
        Self {
            records: HashMap::new(),
        }
    }

    pub fn update(
        &mut self,
        entries: &[BrokerQueueEntry],
        order_books: &[crate::ontology::links::OrderBookObservation],
        store: &crate::ontology::store::ObjectStore,
        tick: u64,
    ) -> BrokerTemporalDelta {
        let mut seen_ids = HashSet::new();
        let mut transitions = Vec::new();

        for entry in entries {
            let id = BrokerSymbolId {
                broker_id: entry.broker_id,
                symbol: entry.symbol.clone(),
            };
            seen_ids.insert(id.clone());

            let inst_id = store.broker_to_institution.get(&entry.broker_id).copied();
            let visible_volume =
                visible_volume_at_level(order_books, &entry.symbol, entry.side, entry.position);

            match self.records.get_mut(&id) {
                Some(state) if state.active => {
                    state.last_seen_tick = tick;
                    state.seen_count += 1;
                    state.position_sum += entry.position as i64;
                    state.position_observations += 1;
                    if entry.side != state.last_side {
                        state.side_flip_count += 1;
                        transitions.push(BrokerTransition::basic(
                            BrokerTransitionKind::SideFlipped,
                            id.clone(),
                            tick,
                            entry.side,
                            entry.position,
                            inst_id,
                        ));
                        state.last_side = entry.side;
                    }
                    state.last_position = entry.position;
                    state.last_visible_volume = visible_volume;
                }
                Some(state) => {
                    state.active = true;
                    state.last_seen_tick = tick;
                    state.seen_count += 1;
                    state.last_appeared_tick = tick;
                    state.appearance_count += 1;
                    state.last_side = entry.side;
                    state.last_position = entry.position;
                    state.position_sum += entry.position as i64;
                    state.position_observations += 1;
                    state.last_visible_volume = visible_volume;

                    let is_replenish = state
                        .last_disappeared_tick
                        .map(|d| tick.saturating_sub(d) <= REPLENISH_WINDOW)
                        .unwrap_or(false);
                    if is_replenish {
                        let replenish_interval = state
                            .last_replenished_tick
                            .map(|last| tick.saturating_sub(last));
                        let depth_recovery_ratio = visible_volume.and_then(|current_volume| {
                            state
                                .last_disappeared_visible_volume
                                .and_then(|previous_volume| {
                                    depth_recovery_ratio(previous_volume, current_volume)
                                })
                        });
                        state.replenish_count += 1;
                        state.last_replenished_tick = Some(tick);
                        if let Some(last_side) = state.last_replenish_side {
                            if last_side != entry.side {
                                state.replenish_side_consistent = false;
                            }
                        }
                        state.last_replenish_side = Some(entry.side);
                        push_bounded(&mut state.replenish_positions, entry.position);
                        if let Some(interval) = replenish_interval {
                            push_bounded(&mut state.replenish_intervals, interval);
                        }
                        if let Some(ratio) = depth_recovery_ratio {
                            push_bounded(&mut state.depth_recovery_ratios, ratio);
                        }

                        transitions.push(BrokerTransition::replenished(
                            id.clone(),
                            tick,
                            entry.side,
                            entry.position,
                            inst_id,
                            state,
                        ));
                    } else {
                        transitions.push(BrokerTransition::basic(
                            BrokerTransitionKind::Reappeared,
                            id.clone(),
                            tick,
                            entry.side,
                            entry.position,
                            inst_id,
                        ));
                    }
                }
                None => {
                    let state = BrokerTemporalState {
                        broker_symbol_id: id.clone(),
                        active: true,
                        first_seen_tick: tick,
                        last_seen_tick: tick,
                        seen_count: 1,
                        last_appeared_tick: tick,
                        last_disappeared_tick: None,
                        appearance_count: 1,
                        disappearance_count: 0,
                        replenish_count: 0,
                        last_replenished_tick: None,
                        last_replenish_side: None,
                        replenish_intervals: VecDeque::new(),
                        replenish_positions: VecDeque::new(),
                        depth_recovery_ratios: VecDeque::new(),
                        replenish_side_consistent: true,
                        last_side: entry.side,
                        side_flip_count: 0,
                        last_position: entry.position,
                        position_sum: entry.position as i64,
                        position_observations: 1,
                        last_visible_volume: visible_volume,
                        last_disappeared_visible_volume: None,
                        institution_id: inst_id,
                    };
                    transitions.push(BrokerTransition::basic(
                        BrokerTransitionKind::Appeared,
                        id.clone(),
                        tick,
                        entry.side,
                        entry.position,
                        inst_id,
                    ));
                    self.records.insert(id, state);
                }
            }
        }

        let disappeared: Vec<_> = self
            .records
            .iter()
            .filter(|(id, s)| s.active && !seen_ids.contains(id))
            .map(|(id, s)| (id.clone(), s.last_side, s.last_position, s.institution_id))
            .collect();

        for (id, side, position, inst_id) in disappeared {
            if let Some(state) = self.records.get_mut(&id) {
                state.active = false;
                state.disappearance_count += 1;
                state.last_disappeared_tick = Some(tick);
                state.last_disappeared_visible_volume = state.last_visible_volume;
                transitions.push(BrokerTransition::basic(
                    BrokerTransitionKind::Disappeared,
                    id,
                    tick,
                    side,
                    position,
                    inst_id,
                ));
            }
        }

        let active_broker_count = self.records.values().filter(|s| s.active).count();
        BrokerTemporalDelta {
            active_broker_count,
            transitions,
        }
    }

    pub fn broker_state(&self, id: &BrokerSymbolId) -> Option<&BrokerTemporalState> {
        self.records.get(id)
    }

    pub fn active_brokers(&self) -> Vec<BrokerTemporalState> {
        self.records
            .values()
            .filter(|s| s.active)
            .cloned()
            .collect()
    }
}

impl BrokerTransition {
    fn basic(
        kind: BrokerTransitionKind,
        broker_symbol_id: BrokerSymbolId,
        tick: u64,
        side: Side,
        position: i32,
        institution_id: Option<InstitutionId>,
    ) -> Self {
        Self {
            kind,
            broker_symbol_id,
            tick,
            side,
            position,
            institution_id,
            replenish_count: None,
            replenish_interval: None,
            replenish_position_consistency: None,
            replenish_interval_regularity: None,
            replenish_frequency: None,
            replenish_side_consistent: None,
            depth_recovery_ratio: None,
            iceberg_confidence: None,
        }
    }

    fn replenished(
        broker_symbol_id: BrokerSymbolId,
        tick: u64,
        side: Side,
        position: i32,
        institution_id: Option<InstitutionId>,
        state: &BrokerTemporalState,
    ) -> Self {
        let replenish_interval = state.replenish_intervals.back().copied();
        let replenish_position_consistency =
            Some(replenish_position_consistency(&state.replenish_positions));
        let replenish_interval_regularity =
            Some(replenish_interval_regularity(&state.replenish_intervals));
        let replenish_frequency = Some(replenish_frequency(state, tick));
        let depth_recovery_ratio = state.depth_recovery_ratios.back().copied();
        let iceberg_confidence = Some(iceberg_confidence(state, tick));

        Self {
            kind: BrokerTransitionKind::Replenished,
            broker_symbol_id,
            tick,
            side,
            position,
            institution_id,
            replenish_count: Some(state.replenish_count),
            replenish_interval,
            replenish_position_consistency,
            replenish_interval_regularity,
            replenish_frequency,
            replenish_side_consistent: Some(state.replenish_side_consistent),
            depth_recovery_ratio,
            iceberg_confidence,
        }
    }
}

fn push_bounded<T>(queue: &mut VecDeque<T>, value: T) {
    if queue.len() >= REPLENISH_MEMORY {
        queue.pop_front();
    }
    queue.push_back(value);
}

fn replenish_interval_regularity(intervals: &VecDeque<u64>) -> Decimal {
    match intervals.len() {
        0 => Decimal::ZERO,
        1 => dec!(0.5),
        _ => {
            let values = intervals
                .iter()
                .map(|value| *value as f64)
                .collect::<Vec<_>>();
            let mean = values.iter().sum::<f64>() / values.len() as f64;
            if mean <= f64::EPSILON {
                return Decimal::ZERO;
            }
            let variance = values
                .iter()
                .map(|value| {
                    let diff = *value - mean;
                    diff * diff
                })
                .sum::<f64>()
                / values.len() as f64;
            let cv = variance.sqrt() / mean;
            Decimal::from_f64((1.0 - cv).clamp(0.0, 1.0)).unwrap_or(Decimal::ZERO)
        }
    }
}

fn replenish_position_consistency(positions: &VecDeque<i32>) -> Decimal {
    if positions.is_empty() {
        return Decimal::ZERO;
    }
    let mut counts: HashMap<i32, usize> = HashMap::new();
    for position in positions {
        *counts.entry(*position).or_default() += 1;
    }
    let max_count = counts.values().copied().max().unwrap_or(0);
    Decimal::from(max_count as i64) / Decimal::from(positions.len() as i64)
}

fn replenish_frequency(state: &BrokerTemporalState, tick: u64) -> Decimal {
    let lifespan = tick.saturating_sub(state.first_seen_tick) + 1;
    if lifespan == 0 {
        return Decimal::ZERO;
    }
    let ratio = Decimal::from(state.replenish_count as i64) / Decimal::from(lifespan as i64);
    crate::math::clamp_unit_interval(ratio)
}

fn iceberg_confidence(state: &BrokerTemporalState, tick: u64) -> Decimal {
    let interval_regularity = replenish_interval_regularity(&state.replenish_intervals);
    let position_consistency = replenish_position_consistency(&state.replenish_positions);
    let frequency = replenish_frequency(state, tick);
    let side_consistency = if state.replenish_side_consistent {
        Decimal::ONE
    } else {
        dec!(0.35)
    };
    let evidence_strength = crate::math::clamp_unit_interval(
        Decimal::from(state.replenish_count.min(4) as i64) / dec!(4),
    );
    let base = crate::math::clamp_unit_interval(
        interval_regularity * dec!(0.30)
            + position_consistency * dec!(0.35)
            + frequency * dec!(0.20)
            + side_consistency * dec!(0.15),
    );
    let confidence =
        crate::math::clamp_unit_interval(base * (dec!(0.25) + evidence_strength * dec!(0.75)));
    let confidence = if let Some(depth_recovery_ratio) = state.depth_recovery_ratios.back().copied()
    {
        crate::math::clamp_unit_interval(
            confidence * dec!(0.85) + depth_recovery_ratio * dec!(0.15),
        )
    } else {
        confidence
    };
    confidence.round_dp(4)
}

fn depth_recovery_ratio(
    previous_visible_volume: i64,
    current_visible_volume: i64,
) -> Option<Decimal> {
    if previous_visible_volume <= 0 || current_visible_volume < 0 {
        return None;
    }
    Some(
        crate::math::clamp_unit_interval(
            Decimal::from(current_visible_volume) / Decimal::from(previous_visible_volume),
        )
        .round_dp(4),
    )
}

fn visible_volume_at_level(
    order_books: &[crate::ontology::links::OrderBookObservation],
    symbol: &Symbol,
    side: Side,
    position: i32,
) -> Option<i64> {
    let order_book = order_books.iter().find(|book| book.symbol == *symbol)?;
    let levels = match side {
        Side::Ask => &order_book.ask_levels,
        Side::Bid => &order_book.bid_levels,
    };
    levels
        .iter()
        .find(|level| level.position == position)
        .map(|level| level.volume)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::links::{DepthLevel, DepthProfile, OrderBookObservation};
    use std::collections::HashMap;

    fn sym(value: &str) -> Symbol {
        Symbol(value.into())
    }

    fn store() -> crate::ontology::store::ObjectStore {
        crate::ontology::store::ObjectStore {
            institutions: HashMap::new(),
            brokers: HashMap::new(),
            stocks: HashMap::new(),
            sectors: HashMap::new(),
            broker_to_institution: HashMap::new(),
            knowledge: std::sync::RwLock::new(crate::ontology::store::AccumulatedKnowledge::empty()),
        }
    }

    fn entry(symbol: &str, broker_id: i32, side: Side, position: i32) -> BrokerQueueEntry {
        BrokerQueueEntry {
            symbol: sym(symbol),
            broker_id: BrokerId(broker_id),
            side,
            position,
        }
    }

    fn order_book(
        symbol: &str,
        bid_levels: Vec<(i32, i64)>,
        ask_levels: Vec<(i32, i64)>,
    ) -> OrderBookObservation {
        let bid_levels = bid_levels
            .into_iter()
            .map(|(position, volume)| DepthLevel {
                position,
                price: None,
                volume,
                order_num: 1,
            })
            .collect::<Vec<_>>();
        let ask_levels = ask_levels
            .into_iter()
            .map(|(position, volume)| DepthLevel {
                position,
                price: None,
                volume,
                order_num: 1,
            })
            .collect::<Vec<_>>();
        OrderBookObservation {
            symbol: sym(symbol),
            total_ask_volume: ask_levels.iter().map(|level| level.volume).sum(),
            total_bid_volume: bid_levels.iter().map(|level| level.volume).sum(),
            total_ask_orders: ask_levels.len() as i64,
            total_bid_orders: bid_levels.len() as i64,
            ask_level_count: ask_levels.len(),
            bid_level_count: bid_levels.len(),
            spread: None,
            bid_profile: DepthProfile::empty(),
            ask_profile: DepthProfile::empty(),
            ask_levels,
            bid_levels,
        }
    }

    #[test]
    fn replenish_state_tracks_intervals_positions_and_confidence() {
        let mut registry = TemporalBrokerRegistry::new();
        let store = store();
        registry.update(
            &[entry("700.HK", 1, Side::Bid, 1)],
            &[order_book("700.HK", vec![(1, 100)], vec![])],
            &store,
            1,
        );
        registry.update(&[], &[order_book("700.HK", vec![], vec![])], &store, 2);
        let delta_1 = registry.update(
            &[entry("700.HK", 1, Side::Bid, 1)],
            &[order_book("700.HK", vec![(1, 100)], vec![])],
            &store,
            3,
        );
        registry.update(&[], &[order_book("700.HK", vec![], vec![])], &store, 4);
        registry.update(
            &[entry("700.HK", 1, Side::Bid, 1)],
            &[order_book("700.HK", vec![(1, 100)], vec![])],
            &store,
            5,
        );
        registry.update(&[], &[order_book("700.HK", vec![], vec![])], &store, 6);
        let delta_3 = registry.update(
            &[entry("700.HK", 1, Side::Bid, 1)],
            &[order_book("700.HK", vec![(1, 100)], vec![])],
            &store,
            7,
        );

        let state = registry
            .broker_state(&BrokerSymbolId {
                broker_id: BrokerId(1),
                symbol: sym("700.HK"),
            })
            .expect("state");
        assert_eq!(state.replenish_count, 3);
        assert_eq!(state.replenish_positions.len(), 3);
        assert_eq!(state.replenish_intervals.len(), 2);
        assert_eq!(state.depth_recovery_ratios.len(), 3);
        assert!(state.replenish_side_consistent);

        let first_confidence = delta_1
            .transitions
            .iter()
            .find_map(|item| item.iceberg_confidence)
            .expect("first confidence");
        let later_confidence = delta_3
            .transitions
            .iter()
            .find_map(|item| item.iceberg_confidence)
            .expect("later confidence");
        let depth_ratio = delta_3
            .transitions
            .iter()
            .find_map(|item| item.depth_recovery_ratio)
            .expect("depth ratio");
        assert_eq!(depth_ratio, Decimal::ONE);
        assert!(later_confidence > first_confidence);
        assert!(later_confidence >= dec!(0.45));
    }

    #[test]
    fn replenish_confidence_drops_when_side_and_position_vary() {
        let mut registry = TemporalBrokerRegistry::new();
        let store = store();
        registry.update(
            &[entry("700.HK", 2, Side::Bid, 1)],
            &[order_book("700.HK", vec![(1, 100)], vec![])],
            &store,
            1,
        );
        registry.update(&[], &[order_book("700.HK", vec![], vec![])], &store, 2);
        registry.update(
            &[entry("700.HK", 2, Side::Bid, 1)],
            &[order_book("700.HK", vec![(1, 100)], vec![])],
            &store,
            3,
        );
        registry.update(&[], &[order_book("700.HK", vec![], vec![])], &store, 4);
        registry.update(
            &[entry("700.HK", 2, Side::Ask, 4)],
            &[order_book("700.HK", vec![], vec![(4, 55)])],
            &store,
            5,
        );
        registry.update(&[], &[order_book("700.HK", vec![], vec![])], &store, 6);
        let delta = registry.update(
            &[entry("700.HK", 2, Side::Bid, 6)],
            &[order_book("700.HK", vec![(6, 40)], vec![])],
            &store,
            7,
        );

        let transition = delta
            .transitions
            .iter()
            .find(|item| matches!(item.kind, BrokerTransitionKind::Replenished))
            .expect("replenished transition");
        let confidence = transition.iceberg_confidence.expect("confidence");
        assert!(confidence < dec!(0.55));
        assert_eq!(transition.replenish_side_consistent, Some(false));
        assert!(
            transition
                .replenish_position_consistency
                .expect("position consistency")
                < dec!(0.5)
        );
    }
}
