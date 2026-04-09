use super::*;

#[derive(Debug, Clone)]
pub struct UsConvergenceScore {
    pub symbol: Symbol,
    pub dimension_composite: Decimal,
    pub capital_flow_direction: Decimal,
    pub price_momentum: Decimal,
    pub volume_profile: Decimal,
    pub pre_post_market_anomaly: Decimal,
    pub valuation: Decimal,
    pub cross_market_propagation: Option<Decimal>,
    pub cross_stock_correlation: Decimal,
    pub sector_coherence: Option<Decimal>,
    pub composite: Decimal,
}

impl UsConvergenceScore {
    pub(super) fn compute(
        symbol: &Symbol,
        graph: &UsGraph,
        cross_market_signals: &[CrossMarketSignal],
        edge_ledger: Option<&crate::graph::edge_learning::EdgeLearningLedger>,
    ) -> Option<Self> {
        let &stock_idx = graph.stock_nodes.get(symbol)?;
        let dims = match &graph.graph[stock_idx] {
            UsNodeKind::Stock(s) => &s.dimensions,
            _ => return None,
        };

        let dimension_composite = dimension_composite(dims);

        let mut corr_sum = Decimal::ZERO;
        let mut corr_count = 0i64;
        for edge in graph
            .graph
            .edges_directed(stock_idx, GraphDirection::Outgoing)
        {
            if let UsEdgeKind::StockToStock(e) = edge.weight() {
                if let UsNodeKind::Stock(neighbor) = &graph.graph[edge.target()] {
                    let learned = edge_ledger
                        .map(|ledger| {
                            let (a, b) = if symbol.0 < neighbor.symbol.0 {
                                (symbol.clone(), neighbor.symbol.clone())
                            } else {
                                (neighbor.symbol.clone(), symbol.clone())
                            };
                            ledger.weight_multiplier(
                                &crate::graph::edge_learning::EdgeKey::StockToStock { a, b },
                            )
                        })
                        .unwrap_or(Decimal::ONE);
                    corr_sum += e.similarity * neighbor.mean_direction * learned;
                    corr_count += 1;
                }
            }
        }
        let cross_stock_correlation = if corr_count > 0 {
            corr_sum / Decimal::from(corr_count)
        } else {
            Decimal::ZERO
        };

        let mut sector_coherence = None;
        for edge in graph
            .graph
            .edges_directed(stock_idx, GraphDirection::Outgoing)
        {
            if let UsEdgeKind::StockToSector(_) = edge.weight() {
                if let UsNodeKind::Sector(s) = &graph.graph[edge.target()] {
                    let learned = edge_ledger
                        .map(|ledger| {
                            ledger.weight_multiplier(
                                &crate::graph::edge_learning::EdgeKey::StockToSector {
                                    symbol: symbol.clone(),
                                    sector_id: s.sector_id.clone(),
                                },
                            )
                        })
                        .unwrap_or(Decimal::ONE);
                    sector_coherence = Some(s.mean_direction * learned);
                }
            }
        }

        let cross_market_propagation = cross_market_signals
            .iter()
            .find(|s| &s.us_symbol == symbol)
            .map(|s| s.propagation_confidence);

        let mut components = Vec::new();
        if dimension_composite != Decimal::ZERO {
            components.push(dimension_composite);
        }
        if cross_stock_correlation != Decimal::ZERO {
            components.push(cross_stock_correlation);
        }
        if let Some(sc) = sector_coherence {
            if sc != Decimal::ZERO {
                components.push(sc);
            }
        }
        if let Some(cm) = cross_market_propagation {
            if cm != Decimal::ZERO {
                components.push(cm);
            }
        }

        let composite = if components.is_empty() {
            Decimal::ZERO
        } else {
            let sum: Decimal = components.iter().sum();
            sum / Decimal::from(components.len() as i64)
        };

        Some(UsConvergenceScore {
            symbol: symbol.clone(),
            dimension_composite,
            capital_flow_direction: dims.capital_flow_direction,
            price_momentum: dims.price_momentum,
            volume_profile: dims.volume_profile,
            pre_post_market_anomaly: dims.pre_post_market_anomaly,
            valuation: dims.valuation,
            cross_market_propagation,
            cross_stock_correlation,
            sector_coherence,
            composite,
        })
    }
}
