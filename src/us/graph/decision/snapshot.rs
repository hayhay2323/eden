use super::*;
use crate::pipeline::pressure::reasoning::StructuralEvidence;
use crate::us::pipeline::dimensions::UsSymbolDimensions;

#[derive(Debug, Clone)]
pub struct UsDecisionSnapshot {
    pub timestamp: OffsetDateTime,
    pub convergence_scores: HashMap<Symbol, UsConvergenceScore>,
    pub market_regime: UsMarketRegimeFilter,
    pub order_suggestions: Vec<UsOrderSuggestion>,
}

impl UsDecisionSnapshot {
    pub fn compute(
        graph: &UsGraph,
        cross_market_signals: &[CrossMarketSignal],
        tick_number: u64,
        edge_ledger: Option<&crate::graph::edge_learning::EdgeLearningLedger>,
    ) -> Self {
        Self::compute_with_evidence(
            graph,
            cross_market_signals,
            &HashMap::new(),
            tick_number,
            edge_ledger,
        )
    }

    pub fn compute_with_evidence(
        graph: &UsGraph,
        cross_market_signals: &[CrossMarketSignal],
        structural_evidence: &HashMap<Symbol, StructuralEvidence>,
        tick_number: u64,
        edge_ledger: Option<&crate::graph::edge_learning::EdgeLearningLedger>,
    ) -> Self {
        let mut convergence_scores = HashMap::new();
        for symbol in graph.stock_nodes.keys() {
            if let Some(score) = UsConvergenceScore::compute_with_evidence(
                symbol,
                graph,
                cross_market_signals,
                Some(structural_evidence),
                edge_ledger,
            ) {
                convergence_scores.insert(symbol.clone(), score);
            }
        }

        let macro_symbols = vec![Symbol("SPY.US".into()), Symbol("QQQ.US".into())];
        let all_dims: HashMap<Symbol, UsSymbolDimensions> = graph
            .stock_nodes
            .keys()
            .filter_map(|sym| {
                let &idx = graph.stock_nodes.get(sym)?;
                match &graph.graph[idx] {
                    UsNodeKind::Stock(s) => Some((sym.clone(), s.dimensions.clone())),
                    _ => None,
                }
            })
            .collect();
        let market_regime = UsMarketRegimeFilter::compute(&all_dims, &macro_symbols);

        let mut order_suggestions = Vec::new();
        for (symbol, score) in &convergence_scores {
            if score.composite == Decimal::ZERO {
                continue;
            }
            let direction = if score.composite > Decimal::ZERO {
                UsOrderDirection::Buy
            } else {
                UsOrderDirection::Sell
            };

            let suggested_quantity = 1;
            let estimated_cost = Decimal::new(1, 3);
            let heuristic_edge = score.composite.abs() - estimated_cost;

            let macro_blocks = market_regime.blocks(direction);
            let low_confidence = score.composite.abs() < Decimal::new(25, 2);
            let requires_confirmation = low_confidence || macro_blocks;

            let mut inputs = vec![
                format!("dim_composite={}", score.dimension_composite.round_dp(4)),
                format!("cross_stock={}", score.cross_stock_correlation.round_dp(4)),
            ];
            if let Some(sc) = score.sector_coherence {
                inputs.push(format!("sector={}", sc.round_dp(4)));
            }
            if let Some(cm) = score.cross_market_propagation {
                inputs.push(format!("hk_propagation={}", cm.round_dp(4)));
            }

            order_suggestions.push(UsOrderSuggestion {
                symbol: symbol.clone(),
                direction,
                convergence: score.clone(),
                suggested_quantity,
                estimated_cost,
                heuristic_edge,
                requires_confirmation,
                provenance: ProvenanceMetadata {
                    trace_id: format!("us-t{}-{}", tick_number, symbol.0),
                    inputs,
                },
            });
        }

        UsDecisionSnapshot {
            timestamp: graph.timestamp,
            convergence_scores,
            market_regime,
            order_suggestions,
        }
    }
}
