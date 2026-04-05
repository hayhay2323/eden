use std::collections::VecDeque;

use rust_decimal::Decimal;

use crate::graph::temporal::{GraphEdgeId, GraphEdgeTransition};
use crate::ontology::objects::Symbol;

use super::record::{SymbolSignals, TickRecord};

/// Ring buffer of recent tick records.
/// Capacity is fixed at creation; oldest ticks are evicted when full.
pub struct TickHistory {
    records: VecDeque<TickRecord>,
    capacity: usize,
}

impl TickHistory {
    pub fn new(capacity: usize) -> Self {
        Self {
            records: VecDeque::with_capacity(capacity),
            capacity,
        }
    }

    pub fn push(&mut self, record: TickRecord) {
        if self.records.len() >= self.capacity {
            self.records.pop_front();
        }
        self.records.push_back(record);
    }

    pub fn len(&self) -> usize {
        self.records.len()
    }

    pub fn latest(&self) -> Option<&TickRecord> {
        self.records.back()
    }

    pub fn oldest(&self) -> Option<&TickRecord> {
        self.records.front()
    }

    /// Return the last N records in chronological order.
    pub fn latest_n(&self, n: usize) -> Vec<&TickRecord> {
        let skip = self.records.len().saturating_sub(n);
        self.records.iter().skip(skip).collect()
    }

    /// Extract a time series of a specific field for a symbol.
    /// Returns values in chronological order, skipping ticks where the symbol is absent.
    pub fn signal_series<F>(&self, symbol: &Symbol, extractor: F) -> Vec<Decimal>
    where
        F: Fn(&SymbolSignals) -> Decimal,
    {
        self.records
            .iter()
            .filter_map(|r| r.signals.get(symbol).map(|s| extractor(s)))
            .collect()
    }

    pub fn graph_edge_transitions_for_id(
        &self,
        edge_id: &GraphEdgeId,
    ) -> Vec<&GraphEdgeTransition> {
        self.records
            .iter()
            .flat_map(|record| record.graph_edge_transitions.iter())
            .filter(|transition| &transition.edge_id == edge_id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::temporal::{
        GraphEdgeId, GraphEdgeKind, GraphEdgeTransition, GraphEdgeTransitionKind,
    };
    use crate::ontology::world::{BackwardReasoningSnapshot, WorldStateSnapshot};
    use rust_decimal_macros::dec;
    use std::collections::HashMap;
    use time::OffsetDateTime;

    fn make_signal(composite: Decimal) -> SymbolSignals {
        SymbolSignals {
            mark_price: None,
            composite,
            institutional_alignment: Decimal::ZERO,
            sector_coherence: None,
            cross_stock_correlation: Decimal::ZERO,
            order_book_pressure: Decimal::ZERO,
            capital_flow_direction: Decimal::ZERO,
            capital_size_divergence: Decimal::ZERO,
            institutional_direction: Decimal::ZERO,
            depth_structure_imbalance: Decimal::ZERO,
            bid_top3_ratio: Decimal::ZERO,
            ask_top3_ratio: Decimal::ZERO,
            bid_best_ratio: Decimal::ZERO,
            ask_best_ratio: Decimal::ZERO,
            spread: None,
            trade_count: 0,
            trade_volume: 0,
            buy_volume: 0,
            sell_volume: 0,
            vwap: None,
            convergence_score: None,
            composite_degradation: None,
            institution_retention: None,
            edge_stability: None,
            temporal_weight: None,
            microstructure_confirmation: None,
            component_spread: None,
            institutional_edge_age: None,
        }
    }

    fn make_tick(tick_number: u64, sym: &str, composite: Decimal) -> TickRecord {
        let mut signals = HashMap::new();
        signals.insert(Symbol(sym.into()), make_signal(composite));
        TickRecord {
            tick_number,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals,
            observations: vec![],
            events: vec![],
            derived_signals: vec![],
            action_workflows: vec![],
            polymarket_priors: vec![],
            hypotheses: vec![],
            propagation_paths: vec![],
            tactical_setups: vec![],
            hypothesis_tracks: vec![],
            case_clusters: vec![],
            world_state: WorldStateSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                entities: vec![],
                vortices: vec![],
            },
            backward_reasoning: BackwardReasoningSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                investigations: vec![],
            },
            graph_edge_transitions: vec![],
            graph_node_transitions: vec![],
            microstructure_deltas: None,
        }
    }

    fn edge_transition(tick: u64, kind: GraphEdgeTransitionKind) -> GraphEdgeTransition {
        GraphEdgeTransition {
            kind,
            edge_id: GraphEdgeId {
                kind: GraphEdgeKind::InstitutionToStock,
                source_key: "institution:100".into(),
                target_key: "symbol:700.HK".into(),
            },
            source_label: "100".into(),
            target_label: "700.HK".into(),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            tick,
            value: dec!(0.8),
            magnitude: dec!(0.8),
            first_seen_tick: 1,
            last_seen_tick: tick,
        }
    }

    #[test]
    fn push_and_len() {
        let mut h = TickHistory::new(10);
        assert_eq!(h.len(), 0);
        h.push(make_tick(1, "700.HK", dec!(0.1)));
        assert_eq!(h.len(), 1);
    }

    #[test]
    fn evicts_oldest_when_full() {
        let mut h = TickHistory::new(3);
        h.push(make_tick(1, "700.HK", dec!(0.1)));
        h.push(make_tick(2, "700.HK", dec!(0.2)));
        h.push(make_tick(3, "700.HK", dec!(0.3)));
        h.push(make_tick(4, "700.HK", dec!(0.4)));
        assert_eq!(h.len(), 3);
        assert_eq!(h.oldest().unwrap().tick_number, 2);
        assert_eq!(h.latest().unwrap().tick_number, 4);
    }

    #[test]
    fn latest_n() {
        let mut h = TickHistory::new(10);
        for i in 1..=5 {
            h.push(make_tick(i, "700.HK", Decimal::from(i)));
        }
        let last3 = h.latest_n(3);
        assert_eq!(last3.len(), 3);
        assert_eq!(last3[0].tick_number, 3);
        assert_eq!(last3[2].tick_number, 5);
    }

    #[test]
    fn signal_series() {
        let mut h = TickHistory::new(10);
        h.push(make_tick(1, "700.HK", dec!(0.1)));
        h.push(make_tick(2, "700.HK", dec!(0.3)));
        h.push(make_tick(3, "700.HK", dec!(0.5)));

        let series = h.signal_series(&Symbol("700.HK".into()), |s| s.composite);
        assert_eq!(series, vec![dec!(0.1), dec!(0.3), dec!(0.5)]);
    }

    #[test]
    fn signal_series_missing_symbol() {
        let mut h = TickHistory::new(10);
        h.push(make_tick(1, "700.HK", dec!(0.1)));

        let series = h.signal_series(&Symbol("9988.HK".into()), |s| s.composite);
        assert!(series.is_empty());
    }

    #[test]
    fn empty_buffer() {
        let h = TickHistory::new(10);
        assert!(h.latest().is_none());
        assert!(h.oldest().is_none());
        assert!(h.latest_n(5).is_empty());
    }

    #[test]
    fn graph_edge_transitions_are_queryable_by_id() {
        let mut h = TickHistory::new(10);
        let edge_id = GraphEdgeId {
            kind: GraphEdgeKind::InstitutionToStock,
            source_key: "institution:100".into(),
            target_key: "symbol:700.HK".into(),
        };

        let mut first = make_tick(1, "700.HK", dec!(0.1));
        first.graph_edge_transitions = vec![edge_transition(1, GraphEdgeTransitionKind::Appeared)];
        h.push(first);

        let mut second = make_tick(2, "700.HK", dec!(0.2));
        second.graph_edge_transitions =
            vec![edge_transition(2, GraphEdgeTransitionKind::Disappeared)];
        h.push(second);

        let transitions = h.graph_edge_transitions_for_id(&edge_id);
        assert_eq!(transitions.len(), 2);
        assert_eq!(transitions[0].kind, GraphEdgeTransitionKind::Appeared);
        assert_eq!(transitions[1].kind, GraphEdgeTransitionKind::Disappeared);
    }
}
