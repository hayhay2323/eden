use std::collections::HashMap;

use crate::ontology::objects::Symbol;
use petgraph::visit::EdgeRef;
use petgraph::Direction as GraphDirection;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::us::graph::graph::{UsEdgeKind, UsGraph, UsNodeKind};
use crate::us::graph::propagation::CrossMarketSignal;
use crate::us::pipeline::dimensions::UsSymbolDimensions;

// ── Convergence ──

#[derive(Debug, Clone)]
pub struct UsConvergenceScore {
    pub symbol: Symbol,
    /// Mean of the 5 US dimensions.
    pub dimension_composite: Decimal,
    pub capital_flow_direction: Decimal,
    pub price_momentum: Decimal,
    pub volume_profile: Decimal,
    pub pre_post_market_anomaly: Decimal,
    pub valuation: Decimal,
    /// Cross-market propagation from HK (only for dual-listed stocks).
    pub cross_market_propagation: Option<Decimal>,
    /// Mean of cross-stock similarity * neighbor direction.
    pub cross_stock_correlation: Decimal,
    /// Sector mean direction (if in a sector).
    pub sector_coherence: Option<Decimal>,
    /// Final composite: dimension_composite + cross_market + cross_stock + sector.
    pub composite: Decimal,
}

impl UsConvergenceScore {
    fn compute(
        symbol: &Symbol,
        graph: &UsGraph,
        cross_market_signals: &[CrossMarketSignal],
    ) -> Option<Self> {
        let &stock_idx = graph.stock_nodes.get(symbol)?;
        let dims = match &graph.graph[stock_idx] {
            UsNodeKind::Stock(s) => &s.dimensions,
            _ => return None,
        };

        let dimension_composite = average_dims(dims);

        // Cross-stock correlation: mean of (similarity * neighbor.mean_direction)
        let mut corr_sum = Decimal::ZERO;
        let mut corr_count = 0i64;
        for edge in graph
            .graph
            .edges_directed(stock_idx, GraphDirection::Outgoing)
        {
            if let UsEdgeKind::StockToStock(e) = edge.weight() {
                if let UsNodeKind::Stock(neighbor) = &graph.graph[edge.target()] {
                    corr_sum += e.similarity * neighbor.mean_direction;
                    corr_count += 1;
                }
            }
        }
        let cross_stock_correlation = if corr_count > 0 {
            corr_sum / Decimal::from(corr_count)
        } else {
            Decimal::ZERO
        };

        // Sector coherence: sector node's mean_direction
        let mut sector_coherence = None;
        for edge in graph
            .graph
            .edges_directed(stock_idx, GraphDirection::Outgoing)
        {
            if let UsEdgeKind::StockToSector(_) = edge.weight() {
                if let UsNodeKind::Sector(s) = &graph.graph[edge.target()] {
                    sector_coherence = Some(s.mean_direction);
                }
            }
        }

        // Cross-market propagation (HK -> US) for dual-listed stocks
        let cross_market_propagation = cross_market_signals
            .iter()
            .find(|s| &s.us_symbol == symbol)
            .map(|s| s.propagation_confidence);

        // Composite: mean of nonzero components
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

// ── Market Regime ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UsMarketRegimeBias {
    RiskOn,
    Neutral,
    RiskOff,
}

impl UsMarketRegimeBias {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RiskOn => "risk_on",
            Self::Neutral => "neutral",
            Self::RiskOff => "risk_off",
        }
    }
}

impl std::fmt::Display for UsMarketRegimeBias {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct UsMarketRegimeFilter {
    pub bias: UsMarketRegimeBias,
    pub confidence: Decimal,
    /// SPY/QQQ return proxy (macro).
    pub macro_return: Decimal,
    /// Fraction of watchlist stocks that are up.
    pub breadth_up: Decimal,
    /// Fraction of watchlist stocks that are down.
    pub breadth_down: Decimal,
    /// Average pre-market return across watchlist.
    pub pre_market_sentiment: Decimal,
}

impl UsMarketRegimeFilter {
    pub fn neutral() -> Self {
        Self {
            bias: UsMarketRegimeBias::Neutral,
            confidence: Decimal::ZERO,
            macro_return: Decimal::ZERO,
            breadth_up: Decimal::ZERO,
            breadth_down: Decimal::ZERO,
            pre_market_sentiment: Decimal::ZERO,
        }
    }

    /// Compute US market regime from dimension data.
    ///
    /// `macro_symbols`: SPY.US/QQQ.US dimensions used as macro proxy.
    /// `all_dims`: all symbol dimensions for breadth.
    pub fn compute(
        all_dims: &HashMap<Symbol, UsSymbolDimensions>,
        macro_symbols: &[Symbol],
    ) -> Self {
        if all_dims.is_empty() {
            return Self::neutral();
        }

        // Macro return: average price_momentum of SPY/QQQ
        let macro_momentums: Vec<Decimal> = macro_symbols
            .iter()
            .filter_map(|s| all_dims.get(s).map(|d| d.price_momentum))
            .collect();
        let macro_return = if macro_momentums.is_empty() {
            Decimal::ZERO
        } else {
            macro_momentums.iter().copied().sum::<Decimal>()
                / Decimal::from(macro_momentums.len() as i64)
        };

        // Breadth: fraction of stocks with positive/negative price_momentum
        let total = Decimal::from(all_dims.len() as i64);
        let up_count = all_dims
            .values()
            .filter(|d| d.price_momentum > Decimal::ZERO)
            .count();
        let down_count = all_dims
            .values()
            .filter(|d| d.price_momentum < Decimal::ZERO)
            .count();
        let breadth_up = Decimal::from(up_count as i64) / total;
        let breadth_down = Decimal::from(down_count as i64) / total;

        // Pre-market sentiment: average pre_post_market_anomaly
        let pre_market_sentiment = all_dims
            .values()
            .map(|d| d.pre_post_market_anomaly)
            .sum::<Decimal>()
            / total;

        // Scoring: each component scaled to [0, 1]
        let risk_off_score = [
            scale_to_unit(breadth_down, Decimal::new(55, 2), Decimal::new(80, 2)),
            scale_to_unit(-macro_return, Decimal::new(10, 2), Decimal::new(50, 2)),
            scale_to_unit(
                -pre_market_sentiment,
                Decimal::new(5, 2),
                Decimal::new(30, 2),
            ),
        ]
        .iter()
        .copied()
        .sum::<Decimal>()
            / Decimal::from(3);

        let risk_on_score = [
            scale_to_unit(breadth_up, Decimal::new(55, 2), Decimal::new(80, 2)),
            scale_to_unit(macro_return, Decimal::new(10, 2), Decimal::new(50, 2)),
            scale_to_unit(
                pre_market_sentiment,
                Decimal::new(5, 2),
                Decimal::new(30, 2),
            ),
        ]
        .iter()
        .copied()
        .sum::<Decimal>()
            / Decimal::from(3);

        let min_score = Decimal::new(55, 2);
        let min_gap = Decimal::new(15, 2);
        let bias = if risk_off_score >= min_score && risk_off_score - risk_on_score >= min_gap {
            UsMarketRegimeBias::RiskOff
        } else if risk_on_score >= min_score && risk_on_score - risk_off_score >= min_gap {
            UsMarketRegimeBias::RiskOn
        } else {
            UsMarketRegimeBias::Neutral
        };
        let confidence = match bias {
            UsMarketRegimeBias::RiskOff => risk_off_score,
            UsMarketRegimeBias::RiskOn => risk_on_score,
            UsMarketRegimeBias::Neutral => risk_off_score.max(risk_on_score),
        };

        UsMarketRegimeFilter {
            bias,
            confidence,
            macro_return,
            breadth_up,
            breadth_down,
            pre_market_sentiment,
        }
    }

    pub fn blocks(&self, direction: UsOrderDirection) -> bool {
        matches!(
            (self.bias, direction),
            (UsMarketRegimeBias::RiskOff, UsOrderDirection::Buy)
                | (UsMarketRegimeBias::RiskOn, UsOrderDirection::Sell)
        )
    }
}

// ── Provenance ──

/// Metadata linking a decision back to its contributing inputs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProvenanceMetadata {
    pub trace_id: String,
    pub inputs: Vec<String>,
}

// ── Order Suggestion ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum UsOrderDirection {
    Buy,
    Sell,
}

#[derive(Debug, Clone)]
pub struct UsOrderSuggestion {
    pub symbol: Symbol,
    pub direction: UsOrderDirection,
    pub convergence: UsConvergenceScore,
    pub suggested_quantity: i32,
    pub estimated_cost: Decimal,
    pub heuristic_edge: Decimal,
    pub requires_confirmation: bool,
    pub provenance: ProvenanceMetadata,
}

// ── Signal Scorecard ──

/// Records a signal at emission time for later resolution.
#[derive(Debug, Clone)]
pub struct UsSignalRecord {
    pub symbol: Symbol,
    pub tick_emitted: u64,
    pub direction: UsOrderDirection,
    pub composite_at_emission: Decimal,
    pub price_at_emission: Option<Decimal>,
    pub resolved: bool,
    pub price_at_resolution: Option<Decimal>,
    pub hit: Option<bool>,
    pub realized_return: Option<Decimal>,
}

/// Aggregated scorecard across resolved signals.
#[derive(Debug, Clone, Default)]
pub struct UsSignalScorecard {
    pub total_signals: usize,
    pub resolved_signals: usize,
    pub hits: usize,
    pub misses: usize,
    pub hit_rate: Decimal,
    pub mean_return: Decimal,
}

const RESOLUTION_LAG: u64 = 15;

impl UsSignalScorecard {
    /// Compute scorecard from a list of signal records.
    pub fn compute(records: &[UsSignalRecord]) -> Self {
        let resolved: Vec<&UsSignalRecord> = records.iter().filter(|r| r.resolved).collect();
        let resolved_signals = resolved.len();
        let total_signals = records.len();

        if resolved_signals == 0 {
            return UsSignalScorecard {
                total_signals,
                ..Default::default()
            };
        }

        let hits = resolved.iter().filter(|r| r.hit == Some(true)).count();
        let misses = resolved_signals - hits;
        let hit_rate = Decimal::from(hits as i64) / Decimal::from(resolved_signals as i64);
        let mean_return = resolved
            .iter()
            .filter_map(|r| r.realized_return)
            .sum::<Decimal>()
            / Decimal::from(resolved_signals as i64);

        UsSignalScorecard {
            total_signals,
            resolved_signals,
            hits,
            misses,
            hit_rate,
            mean_return,
        }
    }

    /// Try to resolve a signal record given the current tick and price.
    pub fn try_resolve(
        record: &mut UsSignalRecord,
        current_tick: u64,
        current_price: Option<Decimal>,
    ) {
        if record.resolved {
            return;
        }
        if current_tick < record.tick_emitted + RESOLUTION_LAG {
            return;
        }

        record.resolved = true;
        record.price_at_resolution = current_price;

        if let (Some(entry), Some(exit)) = (record.price_at_emission, current_price) {
            if entry > Decimal::ZERO {
                let ret = (exit - entry) / entry;
                let directional_return = match record.direction {
                    UsOrderDirection::Buy => ret,
                    UsOrderDirection::Sell => -ret,
                };
                record.realized_return = Some(directional_return);
                record.hit = Some(directional_return > Decimal::ZERO);
            }
        }
    }
}

// ── Decision Snapshot ──

#[derive(Debug)]
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
    ) -> Self {
        let mut convergence_scores = HashMap::new();
        for symbol in graph.stock_nodes.keys() {
            if let Some(score) = UsConvergenceScore::compute(symbol, graph, cross_market_signals) {
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

            // US stocks trade in lots of 1
            let suggested_quantity = 1;
            // No order book depth available for US via Longport; use fallback cost
            let estimated_cost = Decimal::new(1, 3); // 0.1% fallback spread
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

// ── Helpers ──

fn average_dims(dims: &UsSymbolDimensions) -> Decimal {
    let arr = [
        dims.capital_flow_direction,
        dims.price_momentum,
        dims.volume_profile,
        dims.pre_post_market_anomaly,
        dims.valuation,
    ];
    let sum: Decimal = arr.iter().copied().sum();
    sum / Decimal::from(arr.len() as i64)
}

fn clamp_unit(value: Decimal) -> Decimal {
    value.clamp(Decimal::ZERO, Decimal::ONE)
}

fn scale_to_unit(value: Decimal, floor: Decimal, ceiling: Decimal) -> Decimal {
    if ceiling <= floor {
        return Decimal::ZERO;
    }
    clamp_unit((value - floor) / (ceiling - floor))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::objects::SectorId;
    use crate::us::pipeline::dimensions::{UsDimensionSnapshot, UsSymbolDimensions};
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
    }

    fn make_dims(
        flow: Decimal,
        momentum: Decimal,
        volume: Decimal,
        prepost: Decimal,
        val: Decimal,
    ) -> UsSymbolDimensions {
        UsSymbolDimensions {
            capital_flow_direction: flow,
            price_momentum: momentum,
            volume_profile: volume,
            pre_post_market_anomaly: prepost,
            valuation: val,
        }
    }

    fn make_snapshot(entries: Vec<(Symbol, UsSymbolDimensions)>) -> UsDimensionSnapshot {
        UsDimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: entries.into_iter().collect(),
        }
    }

    fn make_graph(entries: Vec<(Symbol, UsSymbolDimensions)>) -> UsGraph {
        let snap = make_snapshot(entries);
        UsGraph::compute(&snap, &HashMap::new(), &HashMap::new())
    }

    // ── Convergence Tests ──

    #[test]
    fn convergence_basic_positive() {
        let g = make_graph(vec![(
            sym("AAPL.US"),
            make_dims(dec!(0.3), dec!(0.5), dec!(0.2), dec!(0.1), dec!(0.4)),
        )]);
        let score = UsConvergenceScore::compute(&sym("AAPL.US"), &g, &[]).unwrap();
        assert!(score.dimension_composite > Decimal::ZERO);
        assert!(score.composite > Decimal::ZERO);
        assert!(score.cross_market_propagation.is_none());
    }

    #[test]
    fn convergence_with_cross_market() {
        let g = make_graph(vec![(
            sym("BABA.US"),
            make_dims(dec!(0.1), dec!(0.2), dec!(0.1), dec!(0.05), dec!(0)),
        )]);
        let cm_signals = vec![CrossMarketSignal {
            hk_symbol: sym("9988.HK"),
            us_symbol: sym("BABA.US"),
            hk_composite: dec!(0.6),
            hk_inst_alignment: dec!(0.7),
            hk_timestamp: "2026-03-20T08:00:00Z".into(),
            time_since_hk_close_minutes: 0,
            propagation_confidence: dec!(0.6),
        }];
        let score = UsConvergenceScore::compute(&sym("BABA.US"), &g, &cm_signals).unwrap();
        assert_eq!(score.cross_market_propagation, Some(dec!(0.6)));
        // composite should include HK propagation
        assert!(score.composite > score.dimension_composite);
    }

    #[test]
    fn convergence_no_cross_market_for_non_dual() {
        let g = make_graph(vec![(
            sym("TSLA.US"),
            make_dims(dec!(0.5), dec!(0.8), dec!(0.3), dec!(0), dec!(0.1)),
        )]);
        let cm_signals = vec![CrossMarketSignal {
            hk_symbol: sym("9988.HK"),
            us_symbol: sym("BABA.US"),
            hk_composite: dec!(0.6),
            hk_inst_alignment: dec!(0.7),
            hk_timestamp: "2026-03-20T08:00:00Z".into(),
            time_since_hk_close_minutes: 0,
            propagation_confidence: dec!(0.6),
        }];
        let score = UsConvergenceScore::compute(&sym("TSLA.US"), &g, &cm_signals).unwrap();
        assert!(score.cross_market_propagation.is_none());
    }

    #[test]
    fn convergence_with_sector() {
        let snap = make_snapshot(vec![
            (
                sym("AAPL.US"),
                make_dims(dec!(0.3), dec!(0.5), dec!(0.2), dec!(0.1), dec!(0.4)),
            ),
            (
                sym("MSFT.US"),
                make_dims(dec!(0.2), dec!(0.4), dec!(0.1), dec!(0.05), dec!(0.3)),
            ),
        ]);
        let sector_map = HashMap::from([
            (sym("AAPL.US"), SectorId("tech".into())),
            (sym("MSFT.US"), SectorId("tech".into())),
        ]);
        let g = UsGraph::compute(&snap, &sector_map, &HashMap::new());
        let score = UsConvergenceScore::compute(&sym("AAPL.US"), &g, &[]).unwrap();
        assert!(score.sector_coherence.is_some());
    }

    #[test]
    fn convergence_zero_dims() {
        let g = make_graph(vec![(
            sym("AAPL.US"),
            make_dims(dec!(0), dec!(0), dec!(0), dec!(0), dec!(0)),
        )]);
        let score = UsConvergenceScore::compute(&sym("AAPL.US"), &g, &[]).unwrap();
        assert_eq!(score.composite, Decimal::ZERO);
    }

    #[test]
    fn convergence_missing_symbol() {
        let g = make_graph(vec![]);
        let score = UsConvergenceScore::compute(&sym("AAPL.US"), &g, &[]);
        assert!(score.is_none());
    }

    // ── Market Regime Tests ──

    #[test]
    fn regime_neutral_on_mixed_market() {
        let dims = HashMap::from([
            (
                sym("AAPL.US"),
                make_dims(dec!(0.1), dec!(0.2), dec!(0), dec!(0), dec!(0)),
            ),
            (
                sym("NVDA.US"),
                make_dims(dec!(-0.1), dec!(-0.2), dec!(0), dec!(0), dec!(0)),
            ),
        ]);
        let macro_syms = vec![sym("SPY.US")];
        let regime = UsMarketRegimeFilter::compute(&dims, &macro_syms);
        assert_eq!(regime.bias, UsMarketRegimeBias::Neutral);
    }

    #[test]
    fn regime_risk_off_on_broad_selloff() {
        let mut dims = HashMap::new();
        // Most stocks down heavily
        for i in 0..20 {
            dims.insert(
                Symbol(format!("STOCK{}.US", i)),
                make_dims(dec!(-0.5), dec!(-0.8), dec!(-0.3), dec!(-0.4), dec!(0)),
            );
        }
        // A few up
        for i in 20..22 {
            dims.insert(
                Symbol(format!("STOCK{}.US", i)),
                make_dims(dec!(0.1), dec!(0.1), dec!(0), dec!(0), dec!(0)),
            );
        }
        // Add macro
        dims.insert(
            sym("SPY.US"),
            make_dims(dec!(-0.5), dec!(-0.7), dec!(-0.3), dec!(-0.3), dec!(0)),
        );
        let macro_syms = vec![sym("SPY.US")];
        let regime = UsMarketRegimeFilter::compute(&dims, &macro_syms);
        assert_eq!(regime.bias, UsMarketRegimeBias::RiskOff);
        assert!(regime.blocks(UsOrderDirection::Buy));
        assert!(!regime.blocks(UsOrderDirection::Sell));
    }

    #[test]
    fn regime_empty_dims() {
        let regime = UsMarketRegimeFilter::compute(&HashMap::new(), &[]);
        assert_eq!(regime.bias, UsMarketRegimeBias::Neutral);
    }

    // ── Order Suggestion Tests ──

    #[test]
    fn suggestions_generated_for_nonzero_composite() {
        let g = make_graph(vec![(
            sym("AAPL.US"),
            make_dims(dec!(0.3), dec!(0.5), dec!(0.2), dec!(0.1), dec!(0.4)),
        )]);
        let snap = UsDecisionSnapshot::compute(&g, &[], 1);
        assert_eq!(snap.order_suggestions.len(), 1);
        let s = &snap.order_suggestions[0];
        assert_eq!(s.symbol, sym("AAPL.US"));
        assert_eq!(s.direction, UsOrderDirection::Buy);
        assert!(s.heuristic_edge > Decimal::ZERO);
        assert!(s.provenance.trace_id.contains("us-t1-AAPL.US"));
        assert!(!s.provenance.inputs.is_empty());
    }

    #[test]
    fn no_suggestion_for_zero_composite() {
        let g = make_graph(vec![(
            sym("AAPL.US"),
            make_dims(dec!(0), dec!(0), dec!(0), dec!(0), dec!(0)),
        )]);
        let snap = UsDecisionSnapshot::compute(&g, &[], 1);
        assert!(snap.order_suggestions.is_empty());
    }

    #[test]
    fn sell_suggestion_for_negative_composite() {
        let g = make_graph(vec![(
            sym("NVDA.US"),
            make_dims(dec!(-0.4), dec!(-0.6), dec!(-0.3), dec!(-0.2), dec!(-0.1)),
        )]);
        let snap = UsDecisionSnapshot::compute(&g, &[], 5);
        let s = &snap.order_suggestions[0];
        assert_eq!(s.direction, UsOrderDirection::Sell);
    }

    #[test]
    fn provenance_includes_cross_market() {
        let g = make_graph(vec![(
            sym("BABA.US"),
            make_dims(dec!(0.1), dec!(0.2), dec!(0.1), dec!(0.05), dec!(0)),
        )]);
        let cm = vec![CrossMarketSignal {
            hk_symbol: sym("9988.HK"),
            us_symbol: sym("BABA.US"),
            hk_composite: dec!(0.6),
            hk_inst_alignment: dec!(0.7),
            hk_timestamp: "2026-03-20T08:00:00Z".into(),
            time_since_hk_close_minutes: 0,
            propagation_confidence: dec!(0.5),
        }];
        let snap = UsDecisionSnapshot::compute(&g, &cm, 10);
        let s = snap
            .order_suggestions
            .iter()
            .find(|s| s.symbol == sym("BABA.US"))
            .unwrap();
        assert!(s
            .provenance
            .inputs
            .iter()
            .any(|i| i.contains("hk_propagation")));
    }

    // ── Scorecard Tests ──

    #[test]
    fn scorecard_empty() {
        let sc = UsSignalScorecard::compute(&[]);
        assert_eq!(sc.total_signals, 0);
        assert_eq!(sc.hit_rate, Decimal::ZERO);
    }

    #[test]
    fn scorecard_unresolved() {
        let records = vec![UsSignalRecord {
            symbol: sym("AAPL.US"),
            tick_emitted: 1,
            direction: UsOrderDirection::Buy,
            composite_at_emission: dec!(0.5),
            price_at_emission: Some(dec!(180)),
            resolved: false,
            price_at_resolution: None,
            hit: None,
            realized_return: None,
        }];
        let sc = UsSignalScorecard::compute(&records);
        assert_eq!(sc.total_signals, 1);
        assert_eq!(sc.resolved_signals, 0);
    }

    #[test]
    fn scorecard_resolved_hit() {
        let records = vec![UsSignalRecord {
            symbol: sym("AAPL.US"),
            tick_emitted: 1,
            direction: UsOrderDirection::Buy,
            composite_at_emission: dec!(0.5),
            price_at_emission: Some(dec!(180)),
            resolved: true,
            price_at_resolution: Some(dec!(185)),
            hit: Some(true),
            realized_return: Some(dec!(0.0278)),
        }];
        let sc = UsSignalScorecard::compute(&records);
        assert_eq!(sc.hits, 1);
        assert_eq!(sc.misses, 0);
        assert_eq!(sc.hit_rate, Decimal::ONE);
        assert!(sc.mean_return > Decimal::ZERO);
    }

    #[test]
    fn scorecard_mixed() {
        let records = vec![
            UsSignalRecord {
                symbol: sym("AAPL.US"),
                tick_emitted: 1,
                direction: UsOrderDirection::Buy,
                composite_at_emission: dec!(0.5),
                price_at_emission: Some(dec!(180)),
                resolved: true,
                price_at_resolution: Some(dec!(185)),
                hit: Some(true),
                realized_return: Some(dec!(0.028)),
            },
            UsSignalRecord {
                symbol: sym("NVDA.US"),
                tick_emitted: 2,
                direction: UsOrderDirection::Sell,
                composite_at_emission: dec!(-0.4),
                price_at_emission: Some(dec!(900)),
                resolved: true,
                price_at_resolution: Some(dec!(910)),
                hit: Some(false),
                realized_return: Some(dec!(-0.011)),
            },
        ];
        let sc = UsSignalScorecard::compute(&records);
        assert_eq!(sc.hits, 1);
        assert_eq!(sc.misses, 1);
        assert_eq!(sc.hit_rate, dec!(0.5));
    }

    #[test]
    fn try_resolve_before_lag() {
        let mut record = UsSignalRecord {
            symbol: sym("AAPL.US"),
            tick_emitted: 10,
            direction: UsOrderDirection::Buy,
            composite_at_emission: dec!(0.5),
            price_at_emission: Some(dec!(180)),
            resolved: false,
            price_at_resolution: None,
            hit: None,
            realized_return: None,
        };
        UsSignalScorecard::try_resolve(&mut record, 20, Some(dec!(185)));
        assert!(!record.resolved); // 20 < 10 + 15
    }

    #[test]
    fn try_resolve_after_lag() {
        let mut record = UsSignalRecord {
            symbol: sym("AAPL.US"),
            tick_emitted: 10,
            direction: UsOrderDirection::Buy,
            composite_at_emission: dec!(0.5),
            price_at_emission: Some(dec!(180)),
            resolved: false,
            price_at_resolution: None,
            hit: None,
            realized_return: None,
        };
        UsSignalScorecard::try_resolve(&mut record, 25, Some(dec!(185)));
        assert!(record.resolved);
        assert_eq!(record.hit, Some(true));
        assert!(record.realized_return.unwrap() > Decimal::ZERO);
    }

    #[test]
    fn try_resolve_sell_direction() {
        let mut record = UsSignalRecord {
            symbol: sym("NVDA.US"),
            tick_emitted: 5,
            direction: UsOrderDirection::Sell,
            composite_at_emission: dec!(-0.4),
            price_at_emission: Some(dec!(900)),
            resolved: false,
            price_at_resolution: None,
            hit: None,
            realized_return: None,
        };
        // Price went down: good for sell
        UsSignalScorecard::try_resolve(&mut record, 25, Some(dec!(880)));
        assert!(record.resolved);
        assert_eq!(record.hit, Some(true));
        assert!(record.realized_return.unwrap() > Decimal::ZERO);
    }

    #[test]
    fn try_resolve_already_resolved() {
        let mut record = UsSignalRecord {
            symbol: sym("AAPL.US"),
            tick_emitted: 1,
            direction: UsOrderDirection::Buy,
            composite_at_emission: dec!(0.5),
            price_at_emission: Some(dec!(180)),
            resolved: true,
            price_at_resolution: Some(dec!(185)),
            hit: Some(true),
            realized_return: Some(dec!(0.028)),
        };
        UsSignalScorecard::try_resolve(&mut record, 100, Some(dec!(170)));
        // Should not change
        assert_eq!(record.price_at_resolution, Some(dec!(185)));
        assert_eq!(record.hit, Some(true));
    }
}
