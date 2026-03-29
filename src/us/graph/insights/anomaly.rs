use super::*;

pub fn compute_propagation_senses(
    graph: &UsGraph,
    dims: &UsDimensionSnapshot,
    dynamics: &HashMap<Symbol, UsSignalDynamics>,
) -> Vec<UsPropagationSense> {
    let mut senses = Vec::new();

    for (symbol, dynamics) in dynamics {
        let source_strength = dynamics
            .composite_delta
            .abs()
            .max(dynamics.composite_acceleration.abs());
        if source_strength < Decimal::new(4, 2) {
            continue;
        }

        let Some(&stock_idx) = graph.stock_nodes.get(symbol) else {
            continue;
        };
        let source_direction = if dynamics.composite_delta != Decimal::ZERO {
            dynamics.composite_delta.signum()
        } else {
            dynamics.composite_acceleration.signum()
        };
        if source_direction == Decimal::ZERO {
            continue;
        }

        for edge in graph
            .graph
            .edges_directed(stock_idx, GraphDirection::Outgoing)
        {
            let UsEdgeKind::StockToStock(link) = edge.weight() else {
                continue;
            };
            if link.similarity < Decimal::new(55, 2) {
                continue;
            }

            let UsNodeKind::Stock(target) = &graph.graph[edge.target()] else {
                continue;
            };
            let Some(target_dims) = dims.dimensions.get(&target.symbol) else {
                continue;
            };

            let target_momentum = target_dims.price_momentum;
            let lag_gap = (source_strength - target_momentum.abs()).max(Decimal::ZERO);
            let target_sign = target_momentum.signum();
            let lagging = target_sign == Decimal::ZERO
                || target_sign != source_direction
                || lag_gap >= Decimal::new(2, 2);
            if !lagging {
                continue;
            }

            let propagation_strength = (source_strength * link.similarity).min(Decimal::ONE);
            if propagation_strength < Decimal::new(3, 2) {
                continue;
            }

            senses.push(UsPropagationSense {
                source_symbol: symbol.clone(),
                target_symbol: target.symbol.clone(),
                channel: "stock_to_stock".into(),
                propagation_strength,
                target_momentum,
                lag_gap,
            });
        }
    }

    senses.sort_by(|left, right| {
        right
            .propagation_strength
            .cmp(&left.propagation_strength)
            .then_with(|| left.source_symbol.0.cmp(&right.source_symbol.0))
            .then_with(|| left.target_symbol.0.cmp(&right.target_symbol.0))
    });
    senses.truncate(32);
    senses
}

pub(super) fn compute_cross_market_anomalies(
    graph: &UsGraph,
    dims: &UsDimensionSnapshot,
    cross_market: &[CrossMarketSignal],
) -> Vec<UsCrossMarketAnomaly> {
    if cross_market.is_empty() {
        return Vec::new();
    }

    let mut all_divergences: Vec<(Symbol, Symbol, Decimal, Decimal, Decimal)> = Vec::new();

    for signal in cross_market {
        if signal.propagation_confidence == Decimal::ZERO {
            continue;
        }

        if !graph.stock_nodes.contains_key(&signal.us_symbol) {
            continue;
        }

        let actual_momentum = match dims.dimensions.get(&signal.us_symbol) {
            Some(d) => d.price_momentum,
            None => continue,
        };

        let expected_sign = if signal.propagation_confidence > Decimal::ZERO {
            Decimal::ONE
        } else {
            -Decimal::ONE
        };

        let actual_sign = if actual_momentum > Decimal::ZERO {
            Decimal::ONE
        } else if actual_momentum < Decimal::ZERO {
            -Decimal::ONE
        } else {
            Decimal::ZERO
        };

        let is_opposite = actual_sign != Decimal::ZERO && expected_sign != actual_sign;
        if !is_opposite {
            continue;
        }

        let divergence = (signal.propagation_confidence - actual_momentum).abs();

        all_divergences.push((
            signal.us_symbol.clone(),
            signal.hk_symbol.clone(),
            signal.propagation_confidence,
            actual_momentum,
            divergence,
        ));
    }

    if all_divergences.is_empty() {
        return Vec::new();
    }

    let median_div = if all_divergences.len() <= 1 {
        Decimal::ZERO
    } else {
        let div_vals: Vec<Decimal> = all_divergences.iter().map(|(_, _, _, _, d)| *d).collect();
        median_decimal(div_vals)
    };

    let mut results: Vec<UsCrossMarketAnomaly> = all_divergences
        .into_iter()
        .filter(|(_, _, _, _, divergence)| *divergence > median_div)
        .map(
            |(us_symbol, hk_symbol, propagation_confidence, actual_momentum, divergence)| {
                UsCrossMarketAnomaly {
                    us_symbol,
                    hk_symbol,
                    expected_direction: propagation_confidence,
                    actual_direction: actual_momentum,
                    divergence,
                }
            },
        )
        .collect();

    results.sort_by(|a, b| b.divergence.cmp(&a.divergence));
    results
}
