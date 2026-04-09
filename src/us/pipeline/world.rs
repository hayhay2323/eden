//! US backward reasoning: traces WHY a stock is moving by gathering all
//! available evidence sources and ranking them by absolute contribution.
//!
//! For each stock with a significant convergence score or tactical setup,
//! `derive_backward_snapshot` constructs a `UsBackwardChain` that explains
//! the stock's direction in human-readable Chinese strings (user-facing),
//! while all code logic and comments remain in English.

use std::collections::HashMap;

use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::{InvestigationSelection, ReasoningScope};
use petgraph::visit::EdgeRef;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::us::graph::decision::UsDecisionSnapshot;
use crate::us::graph::graph::{UsEdgeKind, UsGraph, UsNodeKind};
use crate::us::graph::propagation::CrossMarketSignal;
use crate::us::pipeline::dimensions::UsSymbolDimensions;

// ── Evidence types ──

/// One piece of backward evidence: "this happened, which contributed to the signal."
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsBackwardEvidence {
    /// Source dimension / channel name (ASCII, code-facing).
    pub source: String,
    /// Human-readable Chinese description shown to the user.
    pub description: String,
    /// Absolute contribution weight in [0, 1].
    pub weight: Decimal,
    /// Direction: positive = bullish, negative = bearish.
    pub direction: Decimal,
}

/// Complete backward reasoning chain for one stock.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsBackwardChain {
    pub symbol: Symbol,
    /// Top-level conclusion in Chinese, e.g. "XPEV.US 看空，主因：…".
    pub conclusion: String,
    /// All evidence items, sorted by |weight| descending.
    pub evidence: Vec<UsBackwardEvidence>,
    /// Overall confidence (mean of |weight| across all evidence).
    pub confidence: Decimal,
    /// The source name of the strongest evidence item.
    pub primary_driver: String,
}

/// Full backward reasoning snapshot for the US pipeline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsBackwardSnapshot {
    pub chains: Vec<UsBackwardChain>,
}

// ── Derivation ──

/// Minimum absolute composite score to include a stock in backward reasoning.
/// Stocks below this threshold have no meaningful signal to explain.
/// Derived principle: 0.10 = two dimensions at half-strength, one dimension
/// must be active before we bother explaining it.
const MIN_COMPOSITE_FOR_BACKWARD: &str = "0.03";

/// Build a full backward snapshot from the current decision snapshot and graph.
///
/// `cross_market_signals`: current HK→US propagation signals (may be empty).
/// `sector_names`: optional map of SectorId → display name string.
pub fn derive_backward_snapshot(
    decision: &UsDecisionSnapshot,
    graph: &UsGraph,
    cross_market_signals: &[CrossMarketSignal],
    investigation_selections: &[InvestigationSelection],
    sector_names: &HashMap<String, String>,
) -> UsBackwardSnapshot {
    let min_composite: Decimal = MIN_COMPOSITE_FOR_BACKWARD.parse().expect("constant parses");
    let investigation_symbols = investigation_selections
        .iter()
        .filter_map(|selection| match &selection.scope {
            ReasoningScope::Symbol(symbol) => Some(symbol),
            _ => None,
        })
        .collect::<Vec<_>>();

    let mut chains: Vec<UsBackwardChain> = decision
        .convergence_scores
        .iter()
        .filter(|(symbol, score)| {
            score.composite.abs() >= min_composite
                || score.dimension_composite.abs() >= min_composite
                || investigation_symbols
                    .iter()
                    .any(|candidate| *candidate == *symbol)
                || cross_market_signals.iter().any(|signal| {
                    &signal.us_symbol == *symbol
                        && signal.propagation_confidence.abs() >= min_composite
                })
        })
        .filter_map(|(symbol, score)| {
            // Look up the raw dimensions from the graph node.
            let dims = graph
                .stock_nodes
                .get(symbol)
                .and_then(|&idx| match &graph.graph[idx] {
                    UsNodeKind::Stock(s) => Some(s.dimensions.clone()),
                    _ => None,
                })?;

            // Collect graph-derived context.
            let neighbors =
                collect_aligned_neighbors(symbol, graph, score.composite > Decimal::ZERO);
            let sector_info = collect_sector_info(symbol, graph, sector_names);
            let cm_signal = cross_market_signals
                .iter()
                .find(|s| &s.us_symbol == symbol)
                .cloned();

            let evidence = build_evidence(
                symbol,
                &dims,
                score.sector_coherence,
                &neighbors,
                &sector_info,
                cm_signal.as_ref(),
            );
            if evidence.is_empty() {
                return None;
            }

            let confidence = mean_abs_weight(&evidence);
            let primary_driver = evidence
                .iter()
                .find(|item| item.source != "relative_valuation")
                .or_else(|| evidence.first())
                .map(|e| e.source.clone())
                .unwrap_or_else(|| "unknown".into());

            let is_bullish = score.composite > Decimal::ZERO;
            let conclusion = build_conclusion(symbol, is_bullish, &evidence);

            Some(UsBackwardChain {
                symbol: symbol.clone(),
                conclusion,
                evidence,
                confidence,
                primary_driver,
            })
        })
        .collect();

    // Sort chains: highest confidence first.
    chains.sort_by(|a, b| b.confidence.cmp(&a.confidence));

    UsBackwardSnapshot { chains }
}

// ── Evidence builders ──

/// Build the full sorted evidence list for one symbol.
fn build_evidence(
    _symbol: &Symbol,
    dims: &UsSymbolDimensions,
    sector_coherence: Option<Decimal>,
    neighbors: &[Symbol],
    sector_info: &Option<SectorInfo>,
    cm_signal: Option<&CrossMarketSignal>,
) -> Vec<UsBackwardEvidence> {
    let mut items: Vec<UsBackwardEvidence> = Vec::new();

    // ── Dimension 1: Capital flow ──
    if dims.capital_flow_direction.abs() > Decimal::ZERO {
        let pct = (dims.capital_flow_direction * Decimal::from(100)).round_dp(1);
        let direction_str = if dims.capital_flow_direction > Decimal::ZERO {
            "流入"
        } else {
            "流出"
        };
        items.push(UsBackwardEvidence {
            source: "capital_flow".into(),
            description: format!("資金{}{}%", direction_str, pct.abs()),
            weight: dims.capital_flow_direction.abs(),
            direction: dims.capital_flow_direction,
        });
    }

    // ── Dimension 2: Price momentum ──
    if dims.price_momentum.abs() > Decimal::ZERO {
        let pct = (dims.price_momentum * Decimal::from(100)).round_dp(1);
        items.push(UsBackwardEvidence {
            source: "momentum".into(),
            description: format!("價格動量{}%", pct),
            weight: dims.price_momentum.abs(),
            direction: dims.price_momentum,
        });
    }

    // ── Dimension 3: Volume profile ──
    if dims.volume_profile.abs() > Decimal::ZERO {
        let factor = (dims.volume_profile.abs() * Decimal::from(100)).round_dp(0);
        let vol_str = if dims.volume_profile > Decimal::ZERO {
            "放量上攻"
        } else {
            "放量下跌"
        };
        items.push(UsBackwardEvidence {
            source: "volume_profile".into(),
            description: format!("{}(強度{}%)", vol_str, factor),
            weight: dims.volume_profile.abs(),
            direction: dims.volume_profile,
        });
    }

    // ── Dimension 4: Pre/post market anomaly ──
    if dims.pre_post_market_anomaly.abs() > Decimal::ZERO {
        let pct = (dims.pre_post_market_anomaly * Decimal::from(100)).round_dp(1);
        let gap_str = if dims.pre_post_market_anomaly > Decimal::ZERO {
            "跳空高開"
        } else {
            "跳空低開"
        };
        items.push(UsBackwardEvidence {
            source: "pre_market_gap".into(),
            description: format!("盤前異動{}（{}%）", gap_str, pct),
            weight: dims.pre_post_market_anomaly.abs(),
            direction: dims.pre_post_market_anomaly,
        });
    }

    // ── Dimension 5: Valuation ──
    if dims.valuation.abs() > Decimal::ZERO {
        let pct = (dims.valuation * Decimal::from(100)).round_dp(1);
        let val_str = if dims.valuation > Decimal::ZERO {
            "相對同業估值較低"
        } else {
            "相對同業估值較高"
        };
        items.push(UsBackwardEvidence {
            // This is a relative peer factor, not a standalone intrinsic-value judgment.
            source: "relative_valuation".into(),
            description: format!("{}（相對偏差{}%）", val_str, pct.abs()),
            weight: (dims.valuation.abs() * Decimal::new(35, 2)).round_dp(4),
            direction: dims.valuation,
        });
    }

    // ── Cross-market signal (HK → US) ──
    if let Some(cm) = cm_signal {
        if cm.propagation_confidence.abs() > Decimal::ZERO {
            let conf_pct = (cm.propagation_confidence.abs() * Decimal::from(100)).round_dp(0);
            let direction_str = if cm.propagation_confidence > Decimal::ZERO {
                "看多"
            } else {
                "看空"
            };
            items.push(UsBackwardEvidence {
                source: "cross_market".into(),
                description: format!(
                    "港股 {} {}信號傳導（強度{}%）",
                    cm.hk_symbol, direction_str, conf_pct
                ),
                weight: cm.propagation_confidence.abs(),
                direction: cm.propagation_confidence,
            });
        }
    }

    // ── Graph neighbors: stocks moving in the same direction ──
    if !neighbors.is_empty() {
        // Weight is proportional to # of high-similarity aligned neighbors.
        // With median filter + cap at 5, max weight is 5/10 = 0.5.
        let neighbor_weight = Decimal::from(neighbors.len() as i64) / Decimal::from(10);
        // Use composite direction inferred from the first dimension we have.
        let overall_dir = items.first().map(|e| e.direction).unwrap_or(Decimal::ZERO);
        let names: Vec<&str> = neighbors.iter().take(3).map(|s| s.0.as_str()).collect();
        items.push(UsBackwardEvidence {
            source: "graph_neighbors".into(),
            description: format!("相關股 [{}] 同方向", names.join(", ")),
            weight: neighbor_weight,
            direction: overall_dir,
        });
    }

    // ── Sector coherence ──
    if let (Some(sc), Some(info)) = (sector_coherence, sector_info) {
        if sc.abs() > Decimal::ZERO {
            let pct = (sc.abs() * Decimal::from(100)).round_dp(0);
            let dir_str = if sc > Decimal::ZERO {
                "上行"
            } else {
                "下行"
            };
            items.push(UsBackwardEvidence {
                source: "sector_coherence".into(),
                description: format!("板塊 {} {}一致性{}%", info.name, dir_str, pct),
                weight: sc.abs(),
                direction: sc,
            });
        }
    }

    // Sort by absolute weight descending so the most significant evidence is first.
    items.sort_by(|a, b| b.weight.cmp(&a.weight));
    items
}

/// Build the top-level conclusion string from the top 2-3 evidence items.
fn build_conclusion(symbol: &Symbol, is_bullish: bool, evidence: &[UsBackwardEvidence]) -> String {
    let direction = if is_bullish { "多" } else { "空" };

    match evidence {
        [] => format!("{} 看{}，無明確主因", symbol, direction),
        [primary] => format!("{} 看{}，主因：{}", symbol, direction, primary.description),
        [primary, secondary, ..] => format!(
            "{} 看{}，主因：{}，佐證：{}",
            symbol, direction, primary.description, secondary.description
        ),
    }
}

/// Compute mean of |weight| across all evidence items.
fn mean_abs_weight(evidence: &[UsBackwardEvidence]) -> Decimal {
    if evidence.is_empty() {
        return Decimal::ZERO;
    }
    let sum: Decimal = evidence.iter().map(|e| e.weight).sum();
    sum / Decimal::from(evidence.len() as i64)
}

// ── Graph context helpers ──

/// Basic sector information for description strings.
struct SectorInfo {
    name: String,
}

/// Find graph neighbors moving in the same direction.
/// The underlying graph already enforces economic-relation gating plus strong
/// positive similarity, so this step only keeps the strongest aligned names.
fn collect_aligned_neighbors(
    symbol: &Symbol,
    graph: &UsGraph,
    target_bullish: bool,
) -> Vec<Symbol> {
    let Some(&stock_idx) = graph.stock_nodes.get(symbol) else {
        return Vec::new();
    };

    // Collect all neighbors with their similarity
    let mut candidates: Vec<(Symbol, Decimal)> = Vec::new();
    for edge in graph
        .graph
        .edges_directed(stock_idx, petgraph::Direction::Outgoing)
    {
        if let UsEdgeKind::StockToStock(e) = edge.weight() {
            if let UsNodeKind::Stock(neighbor) = &graph.graph[edge.target()] {
                let neighbor_bullish = neighbor.mean_direction > Decimal::ZERO;
                if neighbor_bullish == target_bullish {
                    candidates.push((neighbor.symbol.clone(), e.similarity));
                }
            }
        }
    }

    if candidates.is_empty() {
        return Vec::new();
    }

    // Filter by median similarity — only keep above-median neighbors
    let mut sims: Vec<Decimal> = candidates.iter().map(|(_, s)| *s).collect();
    sims.sort();
    let median = sims[sims.len() / 2];

    let mut strong: Vec<(Symbol, Decimal)> = candidates
        .into_iter()
        .filter(|(_, s)| *s >= median)
        .collect();
    strong.sort_by(|a, b| b.1.cmp(&a.1)); // sort by similarity descending
    strong.truncate(5); // only top 5 most similar

    strong.into_iter().map(|(sym, _)| sym).collect()
}

/// Look up the sector name for a stock if it has a sector edge in the graph.
fn collect_sector_info(
    symbol: &Symbol,
    graph: &UsGraph,
    sector_names: &HashMap<String, String>,
) -> Option<SectorInfo> {
    let &stock_idx = graph.stock_nodes.get(symbol)?;
    for edge in graph
        .graph
        .edges_directed(stock_idx, petgraph::Direction::Outgoing)
    {
        if let UsEdgeKind::StockToSector(_) = edge.weight() {
            if let UsNodeKind::Sector(sector) = &graph.graph[edge.target()] {
                let name = sector_names
                    .get(&sector.sector_id.0)
                    .cloned()
                    .unwrap_or_else(|| sector.sector_id.0.clone());
                return Some(SectorInfo { name });
            }
        }
    }
    None
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::objects::SectorId;
    use crate::us::graph::graph::UsGraph;
    use crate::us::pipeline::dimensions::{UsDimensionSnapshot, UsSymbolDimensions};
    use rust_decimal_macros::dec;
    use std::collections::HashMap;
    use time::OffsetDateTime;

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
            multi_horizon_momentum: Decimal::ZERO,
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

    #[allow(dead_code)]
    fn make_graph_with_sector(
        entries: Vec<(Symbol, UsSymbolDimensions)>,
        sector_map: HashMap<Symbol, SectorId>,
    ) -> UsGraph {
        let snap = make_snapshot(entries);
        UsGraph::compute(&snap, &sector_map, &HashMap::new())
    }

    // ── Unit tests for helpers ──

    #[test]
    fn mean_abs_weight_empty() {
        assert_eq!(mean_abs_weight(&[]), Decimal::ZERO);
    }

    #[test]
    fn mean_abs_weight_single() {
        let evidence = vec![UsBackwardEvidence {
            source: "capital_flow".into(),
            description: "資金流入10%".into(),
            weight: dec!(0.5),
            direction: dec!(0.5),
        }];
        assert_eq!(mean_abs_weight(&evidence), dec!(0.5));
    }

    #[test]
    fn build_conclusion_bullish() {
        let evidence = vec![
            UsBackwardEvidence {
                source: "capital_flow".into(),
                description: "資金流入50%".into(),
                weight: dec!(0.5),
                direction: dec!(0.5),
            },
            UsBackwardEvidence {
                source: "momentum".into(),
                description: "價格動量30%".into(),
                weight: dec!(0.3),
                direction: dec!(0.3),
            },
        ];
        let conclusion = build_conclusion(&sym("NVDA.US"), true, &evidence);
        assert!(conclusion.contains("看多"));
        assert!(conclusion.contains("NVDA.US"));
        assert!(conclusion.contains("主因"));
        assert!(conclusion.contains("佐證"));
    }

    #[test]
    fn build_conclusion_bearish_single_evidence() {
        let evidence = vec![UsBackwardEvidence {
            source: "momentum".into(),
            description: "價格動量-30%".into(),
            weight: dec!(0.3),
            direction: dec!(-0.3),
        }];
        let conclusion = build_conclusion(&sym("XPEV.US"), false, &evidence);
        assert!(conclusion.contains("看空"));
        assert!(conclusion.contains("XPEV.US"));
        assert!(!conclusion.contains("佐證")); // no secondary evidence
    }

    #[test]
    fn build_evidence_capital_flow_only() {
        let dims = make_dims(
            dec!(0.4),
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::ZERO,
        );
        let evidence = build_evidence(&sym("AAPL.US"), &dims, None, &[], &None, None);
        assert_eq!(evidence.len(), 1);
        assert_eq!(evidence[0].source, "capital_flow");
        assert!(evidence[0].description.contains("流入"));
        assert_eq!(evidence[0].direction, dec!(0.4));
    }

    #[test]
    fn build_evidence_sorts_by_weight() {
        // momentum (0.8) > capital_flow (0.3)
        let dims = make_dims(
            dec!(0.3),
            dec!(0.8),
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::ZERO,
        );
        let evidence = build_evidence(&sym("TSLA.US"), &dims, None, &[], &None, None);
        assert_eq!(evidence[0].source, "momentum");
        assert_eq!(evidence[1].source, "capital_flow");
    }

    #[test]
    fn build_evidence_cross_market_included() {
        let dims = make_dims(
            dec!(0.2),
            dec!(0.3),
            Decimal::ZERO,
            Decimal::ZERO,
            Decimal::ZERO,
        );
        let cm = CrossMarketSignal {
            hk_symbol: sym("9988.HK"),
            us_symbol: sym("BABA.US"),
            hk_composite: dec!(0.6),
            hk_inst_alignment: dec!(0.7),
            hk_timestamp: "2026-03-20T08:00:00Z".into(),
            time_since_hk_close_minutes: 0,
            propagation_confidence: dec!(0.6),
        };
        let evidence = build_evidence(&sym("BABA.US"), &dims, None, &[], &None, Some(&cm));
        let cm_ev = evidence.iter().find(|e| e.source == "cross_market");
        assert!(cm_ev.is_some());
        assert!(cm_ev.unwrap().description.contains("9988.HK"));
    }

    // ── Integration tests for derive_backward_snapshot ──

    #[test]
    fn snapshot_derives_chain_for_strong_signal() {
        let graph = make_graph(vec![(
            sym("NVDA.US"),
            make_dims(dec!(0.5), dec!(0.8), dec!(0.3), dec!(0.2), dec!(0.0)),
        )]);
        let decision =
            crate::us::graph::decision::UsDecisionSnapshot::compute(&graph, &[], 1, None);
        let snapshot = derive_backward_snapshot(&decision, &graph, &[], &[], &HashMap::new());

        assert_eq!(snapshot.chains.len(), 1);
        let chain = &snapshot.chains[0];
        assert_eq!(chain.symbol, sym("NVDA.US"));
        assert!(chain.confidence > Decimal::ZERO);
        assert!(!chain.evidence.is_empty());
        assert!(chain.conclusion.contains("NVDA.US"));
    }

    #[test]
    fn snapshot_skips_weak_signal_stocks() {
        // composite will be near zero for a balanced stock.
        let graph = make_graph(vec![(
            sym("AAPL.US"),
            make_dims(dec!(0.01), dec!(0.01), dec!(0.0), dec!(0.0), dec!(0.0)),
        )]);
        let decision =
            crate::us::graph::decision::UsDecisionSnapshot::compute(&graph, &[], 1, None);
        let snapshot = derive_backward_snapshot(&decision, &graph, &[], &[], &HashMap::new());
        // Composite = 0.004, below MIN_COMPOSITE_FOR_BACKWARD = 0.10
        assert!(snapshot.chains.is_empty());
    }

    #[test]
    fn snapshot_includes_cross_market_in_evidence() {
        let graph = make_graph(vec![(
            sym("BABA.US"),
            make_dims(dec!(0.2), dec!(0.3), dec!(0.1), dec!(0.1), dec!(0.0)),
        )]);
        let cm_signals = vec![CrossMarketSignal {
            hk_symbol: sym("9988.HK"),
            us_symbol: sym("BABA.US"),
            hk_composite: dec!(0.7),
            hk_inst_alignment: dec!(0.8),
            hk_timestamp: "2026-03-20T08:00:00Z".into(),
            time_since_hk_close_minutes: 0,
            propagation_confidence: dec!(0.7),
        }];
        let decision =
            crate::us::graph::decision::UsDecisionSnapshot::compute(&graph, &cm_signals, 1, None);
        let snapshot =
            derive_backward_snapshot(&decision, &graph, &cm_signals, &[], &HashMap::new());

        assert!(!snapshot.chains.is_empty());
        let chain = &snapshot.chains[0];
        assert!(chain.evidence.iter().any(|e| e.source == "cross_market"));
    }

    #[test]
    fn chains_sorted_by_confidence_descending() {
        let graph = make_graph(vec![
            (
                sym("AAPL.US"),
                make_dims(dec!(0.2), dec!(0.3), dec!(0.0), dec!(0.0), dec!(0.0)),
            ),
            (
                sym("NVDA.US"),
                make_dims(dec!(0.7), dec!(0.9), dec!(0.5), dec!(0.4), dec!(0.3)),
            ),
        ]);
        let decision =
            crate::us::graph::decision::UsDecisionSnapshot::compute(&graph, &[], 1, None);
        let snapshot = derive_backward_snapshot(&decision, &graph, &[], &[], &HashMap::new());

        // At least one chain should be present.
        assert!(!snapshot.chains.is_empty());
        // Chains should be in descending confidence order.
        for window in snapshot.chains.windows(2) {
            assert!(window[0].confidence >= window[1].confidence);
        }
    }

    #[test]
    fn primary_driver_matches_highest_weight_evidence() {
        let graph = make_graph(vec![(
            sym("TSLA.US"),
            // momentum dominates
            make_dims(dec!(0.1), dec!(0.9), dec!(0.2), dec!(0.05), dec!(0.0)),
        )]);
        let decision =
            crate::us::graph::decision::UsDecisionSnapshot::compute(&graph, &[], 1, None);
        let snapshot = derive_backward_snapshot(&decision, &graph, &[], &[], &HashMap::new());

        let chain = snapshot
            .chains
            .iter()
            .find(|c| c.symbol == sym("TSLA.US"))
            .unwrap();
        assert_eq!(chain.primary_driver, "momentum");
    }

    #[test]
    fn tactical_case_symbol_forces_backward_chain() {
        let graph = make_graph(vec![(
            sym("AAPL.US"),
            make_dims(dec!(0.01), dec!(0.01), dec!(0.0), dec!(0.0), dec!(0.0)),
        )]);
        let decision =
            crate::us::graph::decision::UsDecisionSnapshot::compute(&graph, &[], 1, None);
        let investigations = vec![crate::ontology::reasoning::InvestigationSelection {
            investigation_id: "investigation:aapl".into(),
            hypothesis_id: "hyp:aapl".into(),
            runner_up_hypothesis_id: None,
            provenance: crate::ontology::ProvenanceMetadata::new(
                crate::ontology::ProvenanceSource::Computed,
                time::OffsetDateTime::UNIX_EPOCH,
            ),
            scope: crate::ontology::ReasoningScope::Symbol(sym("AAPL.US")),
            title: "AAPL review".into(),
            family_key: "test".into(),
            family_label: "Test".into(),
            confidence: dec!(0.55),
            confidence_gap: dec!(0.10),
            priority_score: dec!(0.05),
            attention_hint: "review".into(),
            rationale: "test".into(),
            review_reason_code: None,
            notes: vec![],
        }];
        let snapshot =
            derive_backward_snapshot(&decision, &graph, &[], &investigations, &HashMap::new());

        assert_eq!(snapshot.chains.len(), 1);
        assert_eq!(snapshot.chains[0].symbol, sym("AAPL.US"));
    }
}
