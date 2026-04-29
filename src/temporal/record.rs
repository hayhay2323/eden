use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::serde::rfc3339;
use time::OffsetDateTime;

use crate::action::workflow::ActionWorkflowSnapshot;
use crate::graph::temporal::{GraphEdgeTransition, GraphNodeTransition};
use crate::ontology::domain::{DerivedSignal, Event, Observation};
use crate::ontology::links::{QuoteObservation, TradeActivity};
use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::{
    CaseCluster, Hypothesis, HypothesisTrack, PropagationPath, TacticalSetup,
};
use crate::ontology::world::{BackwardReasoningSnapshot, WorldStateSnapshot};
use crate::pipeline::reasoning::ReasoningSnapshot;
use crate::pipeline::signals::{
    DerivedSignalRecord, DerivedSignalSnapshot, EventSnapshot, MarketEventRecord,
    ObservationRecord, ObservationSnapshot,
};

/// Compact snapshot of one pipeline tick's key signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickRecord {
    pub tick_number: u64,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub signals: HashMap<Symbol, SymbolSignals>,
    pub observations: Vec<Observation<ObservationRecord>>,
    pub events: Vec<Event<MarketEventRecord>>,
    pub derived_signals: Vec<DerivedSignal<DerivedSignalRecord>>,
    pub action_workflows: Vec<ActionWorkflowSnapshot>,
    pub hypotheses: Vec<Hypothesis>,
    pub propagation_paths: Vec<PropagationPath>,
    pub tactical_setups: Vec<TacticalSetup>,
    pub hypothesis_tracks: Vec<HypothesisTrack>,
    pub case_clusters: Vec<CaseCluster>,
    pub world_state: WorldStateSnapshot,
    pub backward_reasoning: BackwardReasoningSnapshot,
    #[serde(default)]
    pub graph_edge_transitions: Vec<GraphEdgeTransition>,
    #[serde(default)]
    pub graph_node_transitions: Vec<GraphNodeTransition>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub microstructure_deltas: Option<crate::ontology::microstructure::MicrostructureDeltas>,
}

/// Per-symbol signals captured at one tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolSignals {
    pub mark_price: Option<Decimal>,
    pub composite: Decimal,
    pub institutional_alignment: Decimal,
    pub sector_coherence: Option<Decimal>,
    pub cross_stock_correlation: Decimal,
    pub order_book_pressure: Decimal,
    pub capital_flow_direction: Decimal,
    pub capital_size_divergence: Decimal,
    pub institutional_direction: Decimal,
    pub depth_structure_imbalance: Decimal,
    pub bid_top3_ratio: Decimal,
    pub ask_top3_ratio: Decimal,
    pub bid_best_ratio: Decimal,
    pub ask_best_ratio: Decimal,
    pub spread: Option<Decimal>,
    pub trade_count: usize,
    pub trade_volume: i64,
    pub buy_volume: i64,
    pub sell_volume: i64,
    pub vwap: Option<Decimal>,
    pub convergence_score: Option<Decimal>,
    pub composite_degradation: Option<Decimal>,
    pub institution_retention: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub edge_stability: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub temporal_weight: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub microstructure_confirmation: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component_spread: Option<Decimal>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub institutional_edge_age: Option<Decimal>,
}

impl TickRecord {
    pub fn capture(
        tick_number: u64,
        timestamp: OffsetDateTime,
        convergence: &HashMap<Symbol, crate::graph::decision::ConvergenceScore>,
        dimensions: &HashMap<Symbol, crate::pipeline::dimensions::SymbolDimensions>,
        order_books: &[crate::ontology::links::OrderBookObservation],
        quotes: &[QuoteObservation],
        trade_activities: &[TradeActivity],
        degradations: &HashMap<Symbol, crate::graph::decision::StructuralDegradation>,
        observations: &ObservationSnapshot,
        events: &EventSnapshot,
        derived_signals: &DerivedSignalSnapshot,
        action_workflows: &[ActionWorkflowSnapshot],
        reasoning: &ReasoningSnapshot,
        world_state: &WorldStateSnapshot,
        backward_reasoning: &BackwardReasoningSnapshot,
        graph_edge_transitions: &[GraphEdgeTransition],
        graph_node_transitions: &[GraphNodeTransition],
    ) -> Self {
        let mut signals = HashMap::new();
        let setup_map = reasoning
            .tactical_setups
            .iter()
            .filter_map(|setup| match &setup.scope {
                crate::ontology::reasoning::ReasoningScope::Symbol(symbol) => {
                    Some((symbol.clone(), setup))
                }
                _ => None,
            })
            .collect::<HashMap<_, _>>();

        let ob_map: HashMap<&Symbol, &crate::ontology::links::OrderBookObservation> =
            order_books.iter().map(|ob| (&ob.symbol, ob)).collect();
        let quote_map: HashMap<&Symbol, &QuoteObservation> =
            quotes.iter().map(|quote| (&quote.symbol, quote)).collect();
        let ta_map: HashMap<&Symbol, &TradeActivity> =
            trade_activities.iter().map(|ta| (&ta.symbol, ta)).collect();

        for (symbol, conv) in convergence {
            let dims = dimensions.get(symbol);
            let ob = ob_map.get(symbol);
            let quote = quote_map.get(symbol);
            let ta = ta_map.get(symbol);
            let deg = degradations.get(symbol);
            let setup = setup_map.get(symbol);
            let mark_price = quote
                .map(|quote| quote.last_done)
                .filter(|price| *price > Decimal::ZERO)
                .or_else(|| ta.and_then(|activity| activity.last_price))
                .filter(|price| *price > Decimal::ZERO)
                .or_else(|| {
                    ta.map(|activity| activity.vwap)
                        .filter(|price| *price > Decimal::ZERO)
                });

            signals.insert(
                symbol.clone(),
                SymbolSignals {
                    mark_price,
                    composite: conv.composite,
                    institutional_alignment: conv.institutional_alignment,
                    sector_coherence: conv.sector_coherence,
                    cross_stock_correlation: conv.cross_stock_correlation,
                    order_book_pressure: dims
                        .map(|d| d.order_book_pressure)
                        .unwrap_or(Decimal::ZERO),
                    capital_flow_direction: dims
                        .map(|d| d.capital_flow_direction)
                        .unwrap_or(Decimal::ZERO),
                    capital_size_divergence: dims
                        .map(|d| d.capital_size_divergence)
                        .unwrap_or(Decimal::ZERO),
                    institutional_direction: dims
                        .map(|d| d.institutional_direction)
                        .unwrap_or(Decimal::ZERO),
                    depth_structure_imbalance: dims
                        .map(|d| d.depth_structure_imbalance)
                        .unwrap_or(Decimal::ZERO),
                    bid_top3_ratio: ob
                        .map(|o| o.bid_profile.top3_volume_ratio)
                        .unwrap_or(Decimal::ZERO),
                    ask_top3_ratio: ob
                        .map(|o| o.ask_profile.top3_volume_ratio)
                        .unwrap_or(Decimal::ZERO),
                    bid_best_ratio: ob
                        .map(|o| o.bid_profile.best_level_ratio)
                        .unwrap_or(Decimal::ZERO),
                    ask_best_ratio: ob
                        .map(|o| o.ask_profile.best_level_ratio)
                        .unwrap_or(Decimal::ZERO),
                    spread: ob.and_then(|o| o.spread),
                    trade_count: ta.map(|t| t.trade_count).unwrap_or(0),
                    trade_volume: ta.map(|t| t.total_volume).unwrap_or(0),
                    buy_volume: ta.map(|t| t.buy_volume).unwrap_or(0),
                    sell_volume: ta.map(|t| t.sell_volume).unwrap_or(0),
                    vwap: ta.map(|t| t.vwap).filter(|v| *v != Decimal::ZERO),
                    convergence_score: setup.and_then(|setup| {
                        setup.convergence_score.or_else(|| {
                            setup.risk_notes.iter().find_map(|note| {
                                note.strip_prefix("convergence_score=")
                                    .and_then(|value| value.parse::<Decimal>().ok())
                            })
                        })
                    }),
                    composite_degradation: deg.map(|d| d.composite_degradation),
                    institution_retention: deg.map(|d| d.institution_retention),
                    edge_stability: conv.edge_stability,
                    temporal_weight: conv.temporal_weight,
                    microstructure_confirmation: conv.microstructure_confirmation,
                    component_spread: conv.component_spread,
                    institutional_edge_age: conv.institutional_edge_age,
                },
            );
        }

        TickRecord {
            tick_number,
            timestamp,
            signals,
            observations: observations.observations.clone(),
            events: events.events.clone(),
            derived_signals: derived_signals.signals.clone(),
            action_workflows: action_workflows.to_vec(),
            hypotheses: reasoning.hypotheses.clone(),
            propagation_paths: reasoning.propagation_paths.clone(),
            tactical_setups: reasoning.tactical_setups.clone(),
            hypothesis_tracks: reasoning.hypothesis_tracks.clone(),
            case_clusters: reasoning.case_clusters.clone(),
            world_state: world_state.clone(),
            backward_reasoning: backward_reasoning.clone(),
            graph_edge_transitions: graph_edge_transitions.to_vec(),
            graph_node_transitions: graph_node_transitions.to_vec(),
            microstructure_deltas: None,
        }
    }
}
