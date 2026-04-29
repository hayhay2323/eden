use super::*;
use crate::pipeline::pressure::reasoning::{AnomalyPhase, StructuralEvidence};
use rust_decimal::prelude::Signed;

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
    pub pressure_support: Option<Decimal>,
    pub peer_confirmation_ratio: Option<Decimal>,
    pub driver_class: Option<String>,
    pub lifecycle_phase: Option<String>,
    pub composite: Decimal,
}

impl UsConvergenceScore {
    #[allow(dead_code)]
    pub(super) fn compute(
        symbol: &Symbol,
        graph: &UsGraph,
        cross_market_signals: &[CrossMarketSignal],
        edge_ledger: Option<&crate::graph::edge_learning::EdgeLearningLedger>,
    ) -> Option<Self> {
        Self::compute_with_evidence(symbol, graph, cross_market_signals, None, edge_ledger)
    }

    pub(super) fn compute_with_evidence(
        symbol: &Symbol,
        graph: &UsGraph,
        cross_market_signals: &[CrossMarketSignal],
        structural_evidence: Option<&std::collections::HashMap<Symbol, StructuralEvidence>>,
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
        let pressure_evidence = structural_evidence.and_then(|items| items.get(symbol));
        let pressure_support = pressure_evidence.map(|evidence| {
            let lifecycle_support = match evidence.lifecycle_phase {
                AnomalyPhase::Growing => Decimal::new(8, 1),
                AnomalyPhase::Peaking => Decimal::new(6, 1),
                AnomalyPhase::New => Decimal::new(4, 1),
                AnomalyPhase::Fading => Decimal::new(1, 1),
            };
            ((evidence.tension
                + evidence.peer_confirmation_ratio
                + evidence.competition_margin
                + lifecycle_support)
                / Decimal::from(4))
            .min(Decimal::ONE)
        });

        // Assemble the SIGNED directional components. Each is in [-1, +1]
        // and carries its own bullish/bearish sign.
        let mut signed_components = Vec::new();
        if dimension_composite != Decimal::ZERO {
            signed_components.push(dimension_composite);
        }
        if cross_stock_correlation != Decimal::ZERO {
            signed_components.push(cross_stock_correlation);
        }
        if let Some(sc) = sector_coherence {
            if sc != Decimal::ZERO {
                signed_components.push(sc);
            }
        }
        if let Some(cm) = cross_market_propagation {
            if cm != Decimal::ZERO {
                signed_components.push(cm);
            }
        }

        let composite = fold_composite(&signed_components, pressure_support);

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
            pressure_support,
            peer_confirmation_ratio: pressure_evidence.map(|item| item.peer_confirmation_ratio),
            driver_class: pressure_evidence.map(|item| item.driver_class.as_str().to_string()),
            lifecycle_phase: pressure_evidence
                .map(|item| item.lifecycle_phase.as_str().to_string()),
            composite,
        })
    }
}

/// Fold the convergence components into a single signed composite.
///
/// `signed_components` contains the directional channels (each in [-1, +1]):
/// dimension_composite, cross_stock_correlation, sector_coherence,
/// cross_market_propagation.
///
/// `pressure_support` is a non-negative magnitude in [0, 1] derived from
/// (tension + peer_confirmation_ratio + competition_margin +
/// lifecycle_support) / 4. It carries structural confidence but NOT direction.
/// Treating it as just another signed vote (prior behavior) biased composite
/// toward positive because its sign was always `+`. Instead we use it as an
/// amplifier of the direction the signed components already agree on — if
/// they don't agree (their mean is zero), pressure contributes nothing
/// because there is no direction to amplify.
pub(super) fn fold_composite(
    signed_components: &[Decimal],
    pressure_support: Option<Decimal>,
) -> Decimal {
    let signed_mean = if signed_components.is_empty() {
        Decimal::ZERO
    } else {
        signed_components.iter().sum::<Decimal>() / Decimal::from(signed_components.len() as i64)
    };

    let pressure_contrib = match pressure_support {
        Some(pressure) if pressure > Decimal::ZERO && signed_mean != Decimal::ZERO => {
            Some(pressure * signed_mean.signum())
        }
        _ => None,
    };

    match pressure_contrib {
        Some(p) => {
            let n = signed_components.len() as i64 + 1;
            (signed_components.iter().sum::<Decimal>() + p) / Decimal::from(n)
        }
        None => signed_mean,
    }
}

#[cfg(test)]
mod tests {
    use super::fold_composite;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    #[test]
    fn fold_composite_empty_is_zero() {
        assert_eq!(fold_composite(&[], None), Decimal::ZERO);
        assert_eq!(fold_composite(&[], Some(dec!(0.8))), Decimal::ZERO);
    }

    #[test]
    fn fold_composite_without_pressure_is_plain_mean() {
        let signed = [dec!(0.4), dec!(-0.2), dec!(0.6)];
        // mean = 0.8 / 3 ≈ 0.2667
        let c = fold_composite(&signed, None);
        assert!((c - dec!(0.2667)).abs() < dec!(0.001));
    }

    #[test]
    fn fold_composite_pressure_does_not_flip_bearish_to_bullish() {
        // Prior bug: every signed component bearish, pressure_support = 0.8
        // was ADDED as a positive vote, pulling composite toward positive and
        // producing "看多, 主因: 資金流出" narratives.
        let signed = [dec!(-1.0), dec!(-1.0), dec!(-0.8)];
        let composite = fold_composite(&signed, Some(dec!(0.8)));
        assert!(
            composite < Decimal::ZERO,
            "bearish consensus + positive pressure must stay bearish, got {composite}"
        );
    }

    #[test]
    fn fold_composite_pressure_amplifies_bearish_consensus() {
        // With signed consensus negative, pressure should AMPLIFY bearish
        // (make composite more negative, not less).
        let signed = [dec!(-0.5), dec!(-0.5)];
        let without = fold_composite(&signed, None);
        let with = fold_composite(&signed, Some(dec!(0.6)));
        assert!(
            with < without,
            "pressure should deepen the bearish mean: with={with} without={without}"
        );
    }

    #[test]
    fn fold_composite_pressure_amplifies_bullish_consensus() {
        let signed = [dec!(0.5), dec!(0.3)];
        let without = fold_composite(&signed, None);
        let with = fold_composite(&signed, Some(dec!(0.6)));
        assert!(
            with > without,
            "pressure should lift the bullish mean: with={with} without={without}"
        );
    }

    #[test]
    fn fold_composite_pressure_noop_when_direction_cancels() {
        // signed mean = 0 exactly → no direction to amplify → pressure
        // contributes nothing, composite stays 0.
        let signed = [dec!(0.5), dec!(-0.5)];
        let composite = fold_composite(&signed, Some(dec!(0.9)));
        assert_eq!(composite, Decimal::ZERO);
    }

    #[test]
    fn fold_composite_pressure_is_direction_aligned_not_additive() {
        // Pressure=1 on a -0.2 signed mean should NOT flip sign.
        let signed = [dec!(-0.2)];
        let composite = fold_composite(&signed, Some(dec!(1.0)));
        assert!(composite < Decimal::ZERO);
        // And symmetrically:
        let signed = [dec!(0.2)];
        let composite = fold_composite(&signed, Some(dec!(1.0)));
        assert!(composite > Decimal::ZERO);
    }
}
