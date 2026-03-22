use std::collections::HashMap;

use petgraph::visit::EdgeRef;
use petgraph::Direction as GraphDirection;
use rust_decimal::prelude::Signed;
use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::external::polymarket::{PolymarketBias, PolymarketPrior, PolymarketSnapshot};
use crate::math::{clamp_unit_interval, cosine_similarity, median};
use crate::ontology::links::LinkSnapshot;
use crate::ontology::objects::{InstitutionId, SectorId, Symbol};
use crate::ontology::reasoning::{ActionDirection, ActionNode, ActionNodeStage};
use crate::ontology::store::ObjectStore;
use crate::pipeline::dimensions::SymbolDimensions;

use super::graph::{dims_to_array, BrainGraph, EdgeKind, NodeKind};

// ── Convergence ──

#[derive(Debug, Clone)]
pub struct ConvergenceScore {
    pub symbol: Symbol,
    pub institutional_alignment: Decimal,
    pub sector_coherence: Option<Decimal>,
    pub cross_stock_correlation: Decimal,
    pub composite: Decimal,
}

impl ConvergenceScore {
    /// Compute convergence score for a stock in the BrainGraph.
    fn compute(symbol: &Symbol, brain: &BrainGraph) -> Option<Self> {
        let &stock_idx = brain.stock_nodes.get(symbol)?;

        // 1. institutional_alignment: weighted avg of institution edge directions, weighted by seat_count
        let mut weighted_sum = Decimal::ZERO;
        let mut weight_total = Decimal::ZERO;
        for edge in brain
            .graph
            .edges_directed(stock_idx, GraphDirection::Incoming)
        {
            if let EdgeKind::InstitutionToStock(e) = edge.weight() {
                let w = Decimal::from(e.seat_count as i64);
                weighted_sum += e.direction * w;
                weight_total += w;
            }
        }
        let institutional_alignment = if weight_total > Decimal::ZERO {
            weighted_sum / weight_total
        } else {
            Decimal::ZERO
        };

        // 2. sector_coherence: sector node's mean_coherence via stock→sector edge
        let mut sector_coherence = None;
        for edge in brain
            .graph
            .edges_directed(stock_idx, GraphDirection::Outgoing)
        {
            if let EdgeKind::StockToSector(_) = edge.weight() {
                let target = edge.target();
                if let NodeKind::Sector(s) = &brain.graph[target] {
                    sector_coherence = Some(s.mean_coherence);
                }
            }
        }

        // 3. cross_stock_correlation: mean of (similarity * neighbor.mean_direction) across stock↔stock
        let mut corr_sum = Decimal::ZERO;
        let mut corr_count = 0i64;
        for edge in brain
            .graph
            .edges_directed(stock_idx, GraphDirection::Outgoing)
        {
            if let EdgeKind::StockToStock(e) = edge.weight() {
                let target = edge.target();
                if let NodeKind::Stock(neighbor) = &brain.graph[target] {
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

        // 4. composite: mean of nonzero components (equal weight)
        let mut components = Vec::new();
        if institutional_alignment != Decimal::ZERO {
            components.push(institutional_alignment);
        }
        if let Some(sc) = sector_coherence {
            if sc != Decimal::ZERO {
                components.push(sc);
            }
        }
        if cross_stock_correlation != Decimal::ZERO {
            components.push(cross_stock_correlation);
        }
        let composite = if components.is_empty() {
            Decimal::ZERO
        } else {
            let sum: Decimal = components.iter().sum();
            sum / Decimal::from(components.len() as i64)
        };

        Some(ConvergenceScore {
            symbol: symbol.clone(),
            institutional_alignment,
            sector_coherence,
            cross_stock_correlation,
            composite,
        })
    }
}

// ── Structural Fingerprint (captured at entry) ──

#[derive(Debug, Clone)]
pub struct StructuralFingerprint {
    pub symbol: Symbol,
    pub entry_timestamp: OffsetDateTime,
    pub entry_composite: Decimal,
    pub entry_regime: crate::action::narrative::Regime,
    pub institutional_directions: Vec<(InstitutionId, Decimal)>,
    pub sector_mean_coherence: Option<Decimal>,
    pub correlated_stocks: Vec<(Symbol, Decimal)>,
    pub entry_dimensions: SymbolDimensions,
}

impl StructuralFingerprint {
    /// Capture the structural fingerprint of a stock at entry time.
    pub fn capture(symbol: &Symbol, brain: &BrainGraph) -> Option<Self> {
        let &stock_idx = brain.stock_nodes.get(symbol)?;
        let stock_node = match &brain.graph[stock_idx] {
            NodeKind::Stock(s) => s,
            _ => return None,
        };

        // Institutional directions
        let mut institutional_directions = Vec::new();
        for edge in brain
            .graph
            .edges_directed(stock_idx, GraphDirection::Incoming)
        {
            if let EdgeKind::InstitutionToStock(e) = edge.weight() {
                let source = edge.source();
                if let NodeKind::Institution(inst) = &brain.graph[source] {
                    institutional_directions.push((inst.institution_id, e.direction));
                }
            }
        }

        // Sector coherence
        let mut sector_mean_coherence = None;
        for edge in brain
            .graph
            .edges_directed(stock_idx, GraphDirection::Outgoing)
        {
            if let EdgeKind::StockToSector(_) = edge.weight() {
                let target = edge.target();
                if let NodeKind::Sector(s) = &brain.graph[target] {
                    sector_mean_coherence = Some(s.mean_coherence);
                }
            }
        }

        // Correlated stocks
        let mut correlated_stocks = Vec::new();
        for edge in brain
            .graph
            .edges_directed(stock_idx, GraphDirection::Outgoing)
        {
            if let EdgeKind::StockToStock(e) = edge.weight() {
                let target = edge.target();
                if let NodeKind::Stock(neighbor) = &brain.graph[target] {
                    correlated_stocks.push((neighbor.symbol.clone(), e.similarity));
                }
            }
        }

        Some(StructuralFingerprint {
            symbol: symbol.clone(),
            entry_timestamp: brain.timestamp,
            entry_composite: Decimal::ZERO, // Filled by caller with convergence score
            entry_regime: stock_node.regime,
            institutional_directions,
            sector_mean_coherence,
            correlated_stocks,
            entry_dimensions: stock_node.dimensions.clone(),
        })
    }
}

impl ActionNode {
    pub fn from_hk_fingerprint(symbol: &Symbol, fingerprint: &StructuralFingerprint) -> Self {
        let direction = if fingerprint.entry_composite > Decimal::ZERO {
            ActionDirection::Long
        } else if fingerprint.entry_composite < Decimal::ZERO {
            ActionDirection::Short
        } else {
            ActionDirection::Neutral
        };

        Self {
            workflow_id: format!("hk-position:{}", symbol),
            symbol: symbol.clone(),
            market: symbol.market(),
            sector: None,
            stage: ActionNodeStage::Monitoring,
            direction,
            // TODO: Phase 3d — thread current composite from BrainGraph so
            // current_confidence reflects live state, not frozen entry.
            entry_confidence: fingerprint.entry_composite.abs(),
            current_confidence: fingerprint.entry_composite.abs(),
            // TODO: Phase 3d — StructuralFingerprint does not carry entry_price,
            // pnl, or tick age. Requires PositionTracker to supply these.
            entry_price: None,
            pnl: None,
            age_ticks: 0,
            degradation_score: None,
            exit_forming: false,
        }
    }
}

// ── Structural Degradation ──

#[derive(Debug, Clone)]
pub struct StructuralDegradation {
    pub symbol: Symbol,
    pub institution_retention: Decimal,
    pub sector_coherence_change: Decimal,
    pub correlation_retention: Decimal,
    pub dimension_drift: Decimal,
    pub composite_degradation: Decimal,
}

impl StructuralDegradation {
    /// Compute how much the structure has degraded since entry.
    pub fn compute(fingerprint: &StructuralFingerprint, brain: &BrainGraph) -> Self {
        let symbol = &fingerprint.symbol;

        // institution_retention: fraction of original institutions still present with same direction sign
        let institution_retention = if fingerprint.institutional_directions.is_empty() {
            Decimal::ONE // No institutions at entry → nothing to lose
        } else {
            let mut retained = 0i64;
            for (inst_id, entry_dir) in &fingerprint.institutional_directions {
                if let Some(&inst_idx) = brain.institution_nodes.get(inst_id) {
                    // Check if this institution still has an edge to this stock with same sign
                    if let Some(&stock_idx) = brain.stock_nodes.get(symbol) {
                        let still_present = brain
                            .graph
                            .edges_directed(inst_idx, GraphDirection::Outgoing)
                            .any(|edge| {
                                if let EdgeKind::InstitutionToStock(e) = edge.weight() {
                                    edge.target() == stock_idx && same_sign(e.direction, *entry_dir)
                                } else {
                                    false
                                }
                            });
                        if still_present {
                            retained += 1;
                        }
                    }
                }
            }
            Decimal::from(retained)
                / Decimal::from(fingerprint.institutional_directions.len() as i64)
        };

        // sector_coherence_change: current minus entry
        let sector_coherence_change = if let Some(entry_sc) = fingerprint.sector_mean_coherence {
            let current_sc = brain
                .stock_nodes
                .get(symbol)
                .and_then(|&idx| {
                    brain
                        .graph
                        .edges_directed(idx, GraphDirection::Outgoing)
                        .find_map(|edge| {
                            if let EdgeKind::StockToSector(_) = edge.weight() {
                                if let NodeKind::Sector(s) = &brain.graph[edge.target()] {
                                    return Some(s.mean_coherence);
                                }
                            }
                            None
                        })
                })
                .unwrap_or(Decimal::ZERO);
            current_sc - entry_sc
        } else {
            Decimal::ZERO
        };

        // correlation_retention: fraction of correlated stocks still correlated
        let correlation_retention = if fingerprint.correlated_stocks.is_empty() {
            Decimal::ONE
        } else {
            let mut retained = 0i64;
            if let Some(&stock_idx) = brain.stock_nodes.get(symbol) {
                let current_neighbors: HashMap<Symbol, Decimal> = brain
                    .graph
                    .edges_directed(stock_idx, GraphDirection::Outgoing)
                    .filter_map(|edge| {
                        if let EdgeKind::StockToStock(e) = edge.weight() {
                            if let NodeKind::Stock(neighbor) = &brain.graph[edge.target()] {
                                return Some((neighbor.symbol.clone(), e.similarity));
                            }
                        }
                        None
                    })
                    .collect();
                for (sym, _) in &fingerprint.correlated_stocks {
                    if current_neighbors.contains_key(sym) {
                        retained += 1;
                    }
                }
            }
            Decimal::from(retained) / Decimal::from(fingerprint.correlated_stocks.len() as i64)
        };

        // dimension_drift: 1 - cosine_similarity(entry, current)
        let dimension_drift = if let Some(&stock_idx) = brain.stock_nodes.get(symbol) {
            if let NodeKind::Stock(current) = &brain.graph[stock_idx] {
                let entry_vec = dims_to_array(&fingerprint.entry_dimensions);
                let current_vec = dims_to_array(&current.dimensions);
                Decimal::ONE - cosine_similarity(entry_vec, current_vec)
            } else {
                Decimal::ZERO
            }
        } else {
            Decimal::ONE // Stock gone → max drift
        };

        // composite_degradation: mean of degradation signals
        // Convert retentions to degradation (1 - retention), keep change/drift as-is
        let inst_degradation = Decimal::ONE - institution_retention;
        let corr_degradation = Decimal::ONE - correlation_retention;
        let signals = [
            inst_degradation,
            sector_coherence_change.abs(),
            corr_degradation,
            dimension_drift,
        ];
        let composite_degradation =
            signals.iter().sum::<Decimal>() / Decimal::from(signals.len() as i64);

        StructuralDegradation {
            symbol: symbol.clone(),
            institution_retention,
            sector_coherence_change,
            correlation_retention,
            dimension_drift,
            composite_degradation,
        }
    }
}

fn same_sign(a: Decimal, b: Decimal) -> bool {
    (a > Decimal::ZERO && b > Decimal::ZERO)
        || (a < Decimal::ZERO && b < Decimal::ZERO)
        || (a == Decimal::ZERO && b == Decimal::ZERO)
}

#[derive(Debug, Clone, Copy)]
struct ConfirmationPolicy {
    low_confidence_cutoff: Decimal,
    wide_spread_cutoff: Decimal,
}

impl ConfirmationPolicy {
    fn from_market(
        convergence_scores: &HashMap<Symbol, ConvergenceScore>,
        best_bid: &HashMap<Symbol, Decimal>,
        best_ask: &HashMap<Symbol, Decimal>,
    ) -> Self {
        let low_confidence_cutoff = median(
            convergence_scores
                .values()
                .map(|score| score.composite.abs())
                .filter(|value| *value > Decimal::ZERO)
                .collect(),
        )
        .unwrap_or(Decimal::ZERO);
        let wide_spread_cutoff = median(
            convergence_scores
                .keys()
                .filter_map(|symbol| {
                    quoted_spread_ratio(
                        best_bid.get(symbol).copied(),
                        best_ask.get(symbol).copied(),
                    )
                })
                .collect(),
        )
        .unwrap_or(Decimal::ZERO);
        Self {
            low_confidence_cutoff,
            wide_spread_cutoff,
        }
    }
}

fn quoted_spread_ratio(price_low: Option<Decimal>, price_high: Option<Decimal>) -> Option<Decimal> {
    match (price_low, price_high) {
        (Some(low), Some(high)) if high > low => {
            let mid = (low + high) / Decimal::TWO;
            if mid > Decimal::ZERO {
                Some((high - low) / mid)
            } else {
                None
            }
        }
        _ => None,
    }
}

fn requires_manual_confirmation(
    score: &ConvergenceScore,
    price_low: Option<Decimal>,
    price_high: Option<Decimal>,
    policy: ConfirmationPolicy,
) -> bool {
    let low_confidence = score.composite.abs() < policy.low_confidence_cutoff;
    let structural_disagreement = (score.institutional_alignment != Decimal::ZERO
        && score.cross_stock_correlation != Decimal::ZERO
        && !same_sign(score.institutional_alignment, score.cross_stock_correlation))
        || score
            .sector_coherence
            .map(|value| value < Decimal::ZERO)
            .unwrap_or(false);
    let missing_price = price_low.is_none() || price_high.is_none();
    let wide_spread = quoted_spread_ratio(price_low, price_high)
        .map(|spread| spread > policy.wide_spread_cutoff)
        .unwrap_or(false);

    low_confidence || structural_disagreement || missing_price || wide_spread
}

fn estimate_transaction_cost(price_low: Option<Decimal>, price_high: Option<Decimal>) -> Decimal {
    quoted_spread_ratio(price_low, price_high).unwrap_or(Decimal::new(5, 3)) // fallback 0.5% when the book is too sparse to estimate
}

fn scale_to_unit(value: Decimal, floor: Decimal, ceiling: Decimal) -> Decimal {
    if ceiling <= floor {
        return Decimal::ZERO;
    }
    clamp_unit_interval((value - floor) / (ceiling - floor))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketRegimeBias {
    RiskOn,
    Neutral,
    RiskOff,
}

impl MarketRegimeBias {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RiskOn => "risk_on",
            Self::Neutral => "neutral",
            Self::RiskOff => "risk_off",
        }
    }
}

impl std::fmt::Display for MarketRegimeBias {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct MarketRegimeFilter {
    pub bias: MarketRegimeBias,
    pub confidence: Decimal,
    pub breadth_up: Decimal,
    pub breadth_down: Decimal,
    pub average_return: Decimal,
    pub leader_return: Option<Decimal>,
    pub directional_consensus: Decimal,
    pub external_bias: Option<MarketRegimeBias>,
    pub external_confidence: Option<Decimal>,
    pub external_driver: Option<String>,
}

impl MarketRegimeFilter {
    pub fn neutral() -> Self {
        Self {
            bias: MarketRegimeBias::Neutral,
            confidence: Decimal::ZERO,
            breadth_up: Decimal::ZERO,
            breadth_down: Decimal::ZERO,
            average_return: Decimal::ZERO,
            leader_return: None,
            directional_consensus: Decimal::ZERO,
            external_bias: None,
            external_confidence: None,
            external_driver: None,
        }
    }

    fn compute(
        links: &LinkSnapshot,
        convergence_scores: &HashMap<Symbol, ConvergenceScore>,
    ) -> Self {
        const LEADER_SYMBOLS: &[&str] = &[
            "700.HK", "9988.HK", "3690.HK", "1810.HK", "388.HK", "5.HK", "939.HK", "883.HK",
        ];

        let returns = links
            .quotes
            .iter()
            .filter_map(|quote| {
                if quote.prev_close > Decimal::ZERO {
                    Some((
                        quote.symbol.clone(),
                        (quote.last_done - quote.prev_close) / quote.prev_close,
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let total_returns = Decimal::from(returns.len() as i64);
        let breadth_up = if total_returns > Decimal::ZERO {
            Decimal::from(returns.iter().filter(|item| item.1 > Decimal::ZERO).count() as i64)
                / total_returns
        } else {
            Decimal::ZERO
        };
        let breadth_down = if total_returns > Decimal::ZERO {
            Decimal::from(returns.iter().filter(|item| item.1 < Decimal::ZERO).count() as i64)
                / total_returns
        } else {
            Decimal::ZERO
        };
        let average_return = if total_returns > Decimal::ZERO {
            returns.iter().map(|(_, value)| *value).sum::<Decimal>() / total_returns
        } else {
            Decimal::ZERO
        };

        let leader_returns = returns
            .iter()
            .filter_map(|(symbol, value)| {
                LEADER_SYMBOLS
                    .contains(&symbol.0.as_str())
                    .then_some(*value)
            })
            .collect::<Vec<_>>();
        let leader_return = if leader_returns.is_empty() {
            None
        } else {
            Some(
                leader_returns.iter().copied().sum::<Decimal>()
                    / Decimal::from(leader_returns.len() as i64),
            )
        };

        let directional_consensus = if convergence_scores.is_empty() {
            Decimal::ZERO
        } else {
            convergence_scores
                .values()
                .map(|score| {
                    score.composite.signum()
                        * clamp_unit_interval(score.composite.abs() / Decimal::new(4, 1))
                })
                .sum::<Decimal>()
                / Decimal::from(convergence_scores.len() as i64)
        };

        let leader_proxy = leader_return.unwrap_or(average_return);
        let risk_off_score = [
            scale_to_unit(breadth_down, Decimal::new(58, 2), Decimal::new(82, 2)),
            scale_to_unit(-average_return, Decimal::new(6, 3), Decimal::new(3, 2)),
            scale_to_unit(-leader_proxy, Decimal::new(12, 3), Decimal::new(5, 2)),
            scale_to_unit(
                -directional_consensus,
                Decimal::new(15, 2),
                Decimal::new(75, 2),
            ),
        ]
        .iter()
        .copied()
        .sum::<Decimal>()
            / Decimal::from(4);
        let risk_on_score = [
            scale_to_unit(breadth_up, Decimal::new(58, 2), Decimal::new(82, 2)),
            scale_to_unit(average_return, Decimal::new(6, 3), Decimal::new(3, 2)),
            scale_to_unit(leader_proxy, Decimal::new(12, 3), Decimal::new(5, 2)),
            scale_to_unit(
                directional_consensus,
                Decimal::new(15, 2),
                Decimal::new(75, 2),
            ),
        ]
        .iter()
        .copied()
        .sum::<Decimal>()
            / Decimal::from(4);

        let min_score = Decimal::new(60, 2);
        let min_gap = Decimal::new(15, 2);
        let bias = if risk_off_score >= min_score && risk_off_score - risk_on_score >= min_gap {
            MarketRegimeBias::RiskOff
        } else if risk_on_score >= min_score && risk_on_score - risk_off_score >= min_gap {
            MarketRegimeBias::RiskOn
        } else {
            MarketRegimeBias::Neutral
        };
        let confidence = match bias {
            MarketRegimeBias::RiskOff => risk_off_score,
            MarketRegimeBias::RiskOn => risk_on_score,
            MarketRegimeBias::Neutral => risk_off_score.max(risk_on_score),
        };

        Self {
            bias,
            confidence,
            breadth_up,
            breadth_down,
            average_return,
            leader_return,
            directional_consensus,
            external_bias: None,
            external_confidence: None,
            external_driver: None,
        }
    }

    pub fn apply_polymarket_snapshot(&mut self, snapshot: &PolymarketSnapshot) {
        let strongest = snapshot
            .priors
            .iter()
            .filter(|prior| prior.active && !prior.closed)
            .filter(|prior| matches!(prior.scope, crate::ontology::ReasoningScope::Market))
            .filter(|prior| prior.is_material())
            .max_by(|a, b| a.probability.cmp(&b.probability));

        let Some(prior) = strongest else {
            self.external_bias = None;
            self.external_confidence = None;
            self.external_driver = None;
            return;
        };

        self.external_bias = match prior.bias {
            PolymarketBias::RiskOn => Some(MarketRegimeBias::RiskOn),
            PolymarketBias::RiskOff => Some(MarketRegimeBias::RiskOff),
            PolymarketBias::Neutral => Some(MarketRegimeBias::Neutral),
        };
        self.external_confidence = Some(prior.probability);
        self.external_driver = Some(format!(
            "polymarket {}={} on {}",
            prior.selected_outcome,
            prior.probability.round_dp(3),
            prior.label
        ));
    }

    fn effective_blocking_bias(&self) -> Option<MarketRegimeBias> {
        let local_bias = (!matches!(self.bias, MarketRegimeBias::Neutral)).then_some(self.bias);
        let external_bias = self
            .external_bias
            .filter(|bias| !matches!(bias, MarketRegimeBias::Neutral));
        let external_confidence = self.external_confidence.unwrap_or(Decimal::ZERO);

        match (local_bias, external_bias) {
            (Some(local), Some(external)) if local == external => Some(local),
            (Some(_local), Some(external))
                if external_confidence >= Decimal::new(75, 2)
                    && self.confidence < Decimal::new(70, 2) =>
            {
                Some(external)
            }
            (Some(local), _) => Some(local),
            (None, Some(external)) if external_confidence >= Decimal::new(65, 2) => Some(external),
            _ => None,
        }
    }

    pub fn blocks(&self, direction: OrderDirection) -> bool {
        matches!(
            (self.effective_blocking_bias(), direction),
            (Some(MarketRegimeBias::RiskOff), OrderDirection::Buy)
                | (Some(MarketRegimeBias::RiskOn), OrderDirection::Sell)
        )
    }

    pub fn gate_reason(&self, direction: OrderDirection) -> Option<String> {
        if !self.blocks(direction) {
            return None;
        }

        let blocked_side = match direction {
            OrderDirection::Buy => "long",
            OrderDirection::Sell => "short",
        };
        let blocking_bias = self.effective_blocking_bias().unwrap_or(self.bias);
        let leader_fragment = self
            .leader_return
            .map(|value| {
                format!(
                    " leader_return={:+.2}%",
                    (value * Decimal::from(100)).round_dp(2)
                )
            })
            .unwrap_or_default();
        let external_fragment = self
            .external_driver
            .as_ref()
            .map(|driver| {
                format!(
                    " external={} ext_conf={:.0}%",
                    driver,
                    (self.external_confidence.unwrap_or(Decimal::ZERO) * Decimal::from(100))
                        .round_dp(0)
                )
            })
            .unwrap_or_default();

        Some(format!(
            "market regime {} blocks {} entries (breadth_down={:.0}% breadth_up={:.0}% avg_return={:+.2}% consensus={:+.2}{}{} conf={:.0}%)",
            blocking_bias,
            blocked_side,
            (self.breadth_down * Decimal::from(100)).round_dp(0),
            (self.breadth_up * Decimal::from(100)).round_dp(0),
            (self.average_return * Decimal::from(100)).round_dp(2),
            self.directional_consensus.round_dp(2),
            leader_fragment,
            external_fragment,
            (self.confidence * Decimal::from(100)).round_dp(0),
        ))
    }
}

// ── Order Suggestion ──

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OrderDirection {
    Buy,
    Sell,
}

#[derive(Debug, Clone)]
pub struct OrderSuggestion {
    pub symbol: Symbol,
    pub direction: OrderDirection,
    pub convergence: ConvergenceScore,
    pub suggested_quantity: i32,
    pub price_low: Option<Decimal>,
    pub price_high: Option<Decimal>,
    pub estimated_cost: Decimal,
    pub heuristic_edge: Decimal,
    pub requires_confirmation: bool,
    pub convergence_score: Decimal,
    pub effective_confidence: Decimal,
    pub external_confirmation: Option<String>,
    pub external_conflict: Option<String>,
    pub external_support_slug: Option<String>,
    pub external_support_probability: Option<Decimal>,
    pub external_conflict_slug: Option<String>,
    pub external_conflict_probability: Option<Decimal>,
}

// ── Decision Snapshot ──

#[derive(Debug)]
pub struct DecisionSnapshot {
    pub timestamp: OffsetDateTime,
    pub convergence_scores: HashMap<Symbol, ConvergenceScore>,
    pub market_regime: MarketRegimeFilter,
    pub order_suggestions: Vec<OrderSuggestion>,
    pub degradations: HashMap<Symbol, StructuralDegradation>,
}

impl DecisionSnapshot {
    /// Compute all convergence scores, order suggestions, and structural degradations.
    pub fn compute(
        brain: &BrainGraph,
        links: &LinkSnapshot,
        active_fingerprints: &[StructuralFingerprint],
        store: &ObjectStore,
    ) -> Self {
        // Compute ConvergenceScore for all stock nodes
        let mut convergence_scores = HashMap::new();
        for symbol in brain.stock_nodes.keys() {
            if let Some(score) = ConvergenceScore::compute(symbol, brain) {
                convergence_scores.insert(symbol.clone(), score);
            }
        }
        let market_regime = MarketRegimeFilter::compute(links, &convergence_scores);

        // Build order book price lookup
        let mut best_bid: HashMap<Symbol, Decimal> = HashMap::new();
        let mut best_ask: HashMap<Symbol, Decimal> = HashMap::new();
        for ob in &links.order_books {
            if let Some(level) = ob.bid_levels.first() {
                if let Some(price) = level.price {
                    best_bid.insert(ob.symbol.clone(), price);
                }
            }
            if let Some(level) = ob.ask_levels.first() {
                if let Some(price) = level.price {
                    best_ask.insert(ob.symbol.clone(), price);
                }
            }
        }
        let confirmation_policy =
            ConfirmationPolicy::from_market(&convergence_scores, &best_bid, &best_ask);

        // Generate OrderSuggestion for stocks with |composite| > 0
        let mut order_suggestions = Vec::new();
        for (symbol, score) in &convergence_scores {
            if score.composite == Decimal::ZERO {
                continue;
            }
            let direction = if score.composite > Decimal::ZERO {
                OrderDirection::Buy
            } else {
                OrderDirection::Sell
            };
            let lot_size = store.stocks.get(symbol).map(|s| s.lot_size).unwrap_or(100);
            let price_low = best_bid.get(symbol).copied();
            let price_high = best_ask.get(symbol).copied();
            let estimated_cost = estimate_transaction_cost(price_low, price_high);
            let heuristic_edge = score.composite.abs() - estimated_cost;
            let macro_requires_review = market_regime.blocks(direction);

            order_suggestions.push(OrderSuggestion {
                symbol: symbol.clone(),
                direction,
                convergence: score.clone(),
                suggested_quantity: lot_size,
                price_low,
                price_high,
                estimated_cost,
                heuristic_edge,
                requires_confirmation: requires_manual_confirmation(
                    score,
                    price_low,
                    price_high,
                    confirmation_policy,
                ) || macro_requires_review,
                convergence_score: clamp_unit_interval(score.composite.abs()),
                effective_confidence: clamp_unit_interval(score.composite.abs()),
                external_confirmation: None,
                external_conflict: None,
                external_support_slug: None,
                external_support_probability: None,
                external_conflict_slug: None,
                external_conflict_probability: None,
            });
        }

        // Compute StructuralDegradation for all active fingerprints
        let mut degradations = HashMap::new();
        for fp in active_fingerprints {
            let deg = StructuralDegradation::compute(fp, brain);
            degradations.insert(fp.symbol.clone(), deg);
        }

        DecisionSnapshot {
            timestamp: brain.timestamp,
            convergence_scores,
            market_regime,
            order_suggestions,
            degradations,
        }
    }

    pub fn apply_polymarket_snapshot(
        &mut self,
        snapshot: &PolymarketSnapshot,
        store: &ObjectStore,
    ) {
        self.market_regime.apply_polymarket_snapshot(snapshot);
        for suggestion in &mut self.order_suggestions {
            let sector_id = store
                .stocks
                .get(&suggestion.symbol)
                .and_then(|stock| stock.sector_id.as_ref());
            apply_external_convergence_to_suggestion(suggestion, snapshot, sector_id);
            if self.market_regime.blocks(suggestion.direction) {
                suggestion.requires_confirmation = true;
            }
        }
    }
}

fn apply_external_convergence_to_suggestion(
    suggestion: &mut OrderSuggestion,
    snapshot: &PolymarketSnapshot,
    sector_id: Option<&SectorId>,
) {
    let supportive = strongest_relevant_prior(
        snapshot,
        &suggestion.symbol,
        sector_id,
        suggestion.direction,
        true,
    );
    let conflicting = strongest_relevant_prior(
        snapshot,
        &suggestion.symbol,
        sector_id,
        suggestion.direction,
        false,
    );
    let base_confidence = clamp_unit_interval(suggestion.convergence.composite.abs());

    suggestion.effective_confidence = base_confidence;
    suggestion.convergence_score = base_confidence;
    suggestion.external_confirmation = None;
    suggestion.external_conflict = None;
    suggestion.external_support_slug = None;
    suggestion.external_support_probability = None;
    suggestion.external_conflict_slug = None;
    suggestion.external_conflict_probability = None;

    if let Some(prior) = supportive {
        suggestion.external_support_slug = Some(prior.slug.clone());
        suggestion.external_support_probability = Some(prior.probability);
        suggestion.external_confirmation = Some(format!(
            "{} confirms {} at {:.0}%",
            prior.label,
            direction_label(suggestion.direction),
            (prior.probability * Decimal::from(100)).round_dp(0),
        ));
        if conflicting.is_none() {
            suggestion.convergence_score = Decimal::ONE
                - (Decimal::ONE - base_confidence) * (Decimal::ONE - prior.probability);
            suggestion.effective_confidence = suggestion.convergence_score;
        }
    }

    if let Some(prior) = conflicting {
        suggestion.external_conflict_slug = Some(prior.slug.clone());
        suggestion.external_conflict_probability = Some(prior.probability);
        suggestion.external_conflict = Some(format!(
            "{} contradicts {} at {:.0}%",
            prior.label,
            direction_label(suggestion.direction),
            (prior.probability * Decimal::from(100)).round_dp(0),
        ));
        suggestion.requires_confirmation = true;
    }
}

fn strongest_relevant_prior<'a>(
    snapshot: &'a PolymarketSnapshot,
    symbol: &Symbol,
    sector_id: Option<&SectorId>,
    direction: OrderDirection,
    support: bool,
) -> Option<&'a PolymarketPrior> {
    snapshot
        .priors
        .iter()
        .filter(|prior| prior.active && !prior.closed && prior.is_material())
        .filter(|prior| prior_relevant_to_symbol(prior, symbol, sector_id))
        .filter(|prior| prior_supports_direction(prior, direction) == support)
        .max_by(|a, b| a.probability.cmp(&b.probability))
}

fn prior_relevant_to_symbol(
    prior: &PolymarketPrior,
    symbol: &Symbol,
    sector_id: Option<&SectorId>,
) -> bool {
    let explicit_targets = prior.parsed_target_scopes();
    if !explicit_targets.is_empty() {
        return explicit_targets
            .iter()
            .any(|scope| scope_matches_symbol(scope, symbol, sector_id));
    }

    scope_matches_symbol(&prior.scope, symbol, sector_id)
}

fn scope_matches_symbol(
    scope: &crate::ontology::ReasoningScope,
    symbol: &Symbol,
    sector_id: Option<&SectorId>,
) -> bool {
    match scope {
        crate::ontology::ReasoningScope::Market => true,
        crate::ontology::ReasoningScope::Sector(sector) => {
            sector_id.map(|value| value.0.as_str()) == Some(sector.as_str())
        }
        crate::ontology::ReasoningScope::Symbol(scope_symbol) => scope_symbol == symbol,
        crate::ontology::ReasoningScope::Region(region) => {
            matches!(region.as_str(), "china" | "hk" | "hong_kong")
        }
        crate::ontology::ReasoningScope::Theme(_)
        | crate::ontology::ReasoningScope::Custom(_)
        | crate::ontology::ReasoningScope::Institution(_) => false,
    }
}

fn prior_supports_direction(prior: &PolymarketPrior, direction: OrderDirection) -> bool {
    matches!(
        (prior.bias, direction),
        (PolymarketBias::RiskOn, OrderDirection::Buy)
            | (PolymarketBias::RiskOff, OrderDirection::Sell)
    )
}

fn direction_label(direction: OrderDirection) -> &'static str {
    match direction {
        OrderDirection::Buy => "long",
        OrderDirection::Sell => "short",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::action::narrative::{
        DimensionReading, Direction, NarrativeSnapshot, Regime, SymbolNarrative,
    };
    use crate::external::polymarket::{PolymarketBias, PolymarketPrior, PolymarketSnapshot};
    use crate::graph::graph::BrainGraph;
    use crate::logic::tension::Dimension;
    use crate::ontology::links::*;
    use crate::ontology::objects::*;
    use crate::ReasoningScope;
    use rust_decimal_macros::dec;

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
    }

    fn make_store_with_stocks(stocks: Vec<Stock>) -> ObjectStore {
        let mut stock_map = HashMap::new();
        for s in stocks {
            stock_map.insert(s.symbol.clone(), s);
        }
        ObjectStore {
            institutions: HashMap::new(),
            brokers: HashMap::new(),
            stocks: stock_map,
            sectors: HashMap::new(),
            broker_to_institution: HashMap::new(),
        }
    }

    fn make_stock(symbol: &str, lot_size: i32) -> Stock {
        let symbol_id = sym(symbol);
        Stock {
            market: symbol_id.market(),
            symbol: symbol_id,
            name_en: symbol.into(),
            name_cn: String::new(),
            name_hk: String::new(),
            exchange: "SEHK".into(),
            lot_size,
            sector_id: None,
            total_shares: 0,
            circulating_shares: 0,
            eps_ttm: rust_decimal::Decimal::ZERO,
            bps: rust_decimal::Decimal::ZERO,
            dividend_yield: rust_decimal::Decimal::ZERO,
        }
    }

    fn make_narrative(coherence: Decimal, mean_direction: Decimal) -> SymbolNarrative {
        SymbolNarrative {
            regime: Regime::classify(coherence, mean_direction),
            coherence,
            mean_direction,
            readings: vec![DimensionReading {
                dimension: Dimension::OrderBookPressure,
                value: mean_direction,
                direction: Direction::from_value(mean_direction),
            }],
            agreements: vec![],
            contradictions: vec![],
        }
    }

    fn make_dims(obp: Decimal, cfd: Decimal, csd: Decimal, id: Decimal) -> SymbolDimensions {
        SymbolDimensions {
            order_book_pressure: obp,
            capital_flow_direction: cfd,
            capital_size_divergence: csd,
            institutional_direction: id,
            ..Default::default()
        }
    }

    fn empty_links() -> LinkSnapshot {
        LinkSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            broker_queues: vec![],
            calc_indexes: vec![],
            candlesticks: vec![],
            institution_activities: vec![],
            cross_stock_presences: vec![],
            capital_flows: vec![],
            capital_breakdowns: vec![],
            market_temperature: None,
            order_books: vec![],
            quotes: vec![],
            trade_activities: vec![],
        }
    }

    fn build_brain(
        narratives: HashMap<Symbol, SymbolNarrative>,
        dimensions: HashMap<Symbol, SymbolDimensions>,
        links: &LinkSnapshot,
        store: &ObjectStore,
    ) -> BrainGraph {
        let narrative = NarrativeSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            narratives,
        };
        let dims = crate::pipeline::dimensions::DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions,
        };
        BrainGraph::compute(&narrative, &dims, links, store)
    }

    // ── Convergence Tests ──

    #[test]
    fn all_bullish_convergence() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.8), dec!(0.6)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.7), dec!(0.5)));
        narratives.insert(sym("5.HK"), make_narrative(dec!(0.3), dec!(-0.2)));

        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
        );
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0.4), dec!(0.4), dec!(0.4), dec!(0.4)),
        );
        dimensions.insert(
            sym("5.HK"),
            make_dims(dec!(0.5), dec!(-0.5), dec!(0.5), dec!(-0.5)),
        );

        let mut links = empty_links();
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(100),
            symbols: vec![sym("700.HK"), sym("9988.HK")],
            ask_symbols: vec![],
            bid_symbols: vec![sym("700.HK"), sym("9988.HK")],
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![],
            bid_positions: vec![1, 2, 3],
            seat_count: 3,
        });

        let store = make_store_with_stocks(vec![]);
        let brain = build_brain(narratives, dimensions, &links, &store);

        let score = ConvergenceScore::compute(&sym("700.HK"), &brain).unwrap();
        // All bullish → positive composite
        assert!(score.composite > Decimal::ZERO);
        assert!(score.institutional_alignment > Decimal::ZERO);
        assert!(score.cross_stock_correlation > Decimal::ZERO);
    }

    #[test]
    fn conflicted_signals() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.3), dec!(-0.5)));

        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
        );
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(-0.4), dec!(-0.4), dec!(-0.4), dec!(-0.4)),
        );

        let mut links = empty_links();
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(100),
            symbols: vec![sym("700.HK"), sym("9988.HK")],
            ask_symbols: vec![sym("700.HK")],
            bid_symbols: vec![],
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![1, 2],
            bid_positions: vec![],
            seat_count: 2,
        });

        let store = make_store_with_stocks(vec![]);
        let brain = build_brain(narratives, dimensions, &links, &store);

        let score = ConvergenceScore::compute(&sym("700.HK"), &brain).unwrap();
        // Institution selling, correlated stock bearish → negative composite
        assert!(score.composite < Decimal::ZERO);
    }

    #[test]
    fn no_institutions_convergence() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));

        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
        );

        let links = empty_links();
        let store = make_store_with_stocks(vec![]);
        let brain = build_brain(narratives, dimensions, &links, &store);

        let score = ConvergenceScore::compute(&sym("700.HK"), &brain).unwrap();
        assert_eq!(score.institutional_alignment, Decimal::ZERO);
        // No neighbors either, so composite = 0
        assert_eq!(score.composite, Decimal::ZERO);
    }

    // ── Fingerprint + Degradation Tests ──

    #[test]
    fn fingerprint_no_degradation() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));

        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
        );

        let links = empty_links();
        let store = make_store_with_stocks(vec![]);
        let brain = build_brain(narratives, dimensions, &links, &store);

        let fp = StructuralFingerprint::capture(&sym("700.HK"), &brain).unwrap();
        // Same brain → no degradation
        let deg = StructuralDegradation::compute(&fp, &brain);
        // dimension_drift should be ~0 (same dims)
        assert!(deg.dimension_drift.abs() < dec!(0.001));
        assert_eq!(deg.institution_retention, Decimal::ONE); // no institutions → 1
        assert_eq!(deg.correlation_retention, Decimal::ONE); // no correlations → 1
    }

    #[test]
    fn full_degradation() {
        // Build entry brain
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.3), dec!(0.1)));

        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
        );
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0.4), dec!(0.4), dec!(0.4), dec!(0.4)),
        );

        let mut links = empty_links();
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(100),
            symbols: vec![sym("700.HK"), sym("9988.HK")],
            ask_symbols: vec![],
            bid_symbols: vec![sym("700.HK"), sym("9988.HK")],
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![],
            bid_positions: vec![1],
            seat_count: 1,
        });

        let store = make_store_with_stocks(vec![]);
        let entry_brain = build_brain(narratives.clone(), dimensions.clone(), &links, &store);
        let mut fp = StructuralFingerprint::capture(&sym("700.HK"), &entry_brain).unwrap();
        fp.entry_composite = dec!(0.5);

        // Build degraded brain — institution gone, dimensions flipped
        let mut narratives2 = HashMap::new();
        narratives2.insert(sym("700.HK"), make_narrative(dec!(-0.3), dec!(-0.5)));

        let mut dimensions2 = HashMap::new();
        dimensions2.insert(
            sym("700.HK"),
            make_dims(dec!(-0.5), dec!(-0.5), dec!(-0.5), dec!(-0.5)),
        );

        let empty = empty_links();
        let degraded_brain = build_brain(narratives2, dimensions2, &empty, &store);

        let deg = StructuralDegradation::compute(&fp, &degraded_brain);
        // Institution gone → retention = 0
        assert_eq!(deg.institution_retention, Decimal::ZERO);
        // Dimensions flipped → drift should be ~2
        assert!(deg.dimension_drift > dec!(1.5));
        // Overall high degradation
        assert!(deg.composite_degradation > dec!(0.5));
    }

    // ── Order Suggestion Tests ──

    #[test]
    fn order_direction_from_composite() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.8), dec!(0.6)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.7), dec!(0.5)));

        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
        );
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0.4), dec!(0.4), dec!(0.4), dec!(0.4)),
        );

        let mut links = empty_links();
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(100),
            symbols: vec![sym("700.HK"), sym("9988.HK")],
            ask_symbols: vec![],
            bid_symbols: vec![sym("700.HK"), sym("9988.HK")],
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![],
            bid_positions: vec![1, 2],
            seat_count: 2,
        });

        let store = make_store_with_stocks(vec![make_stock("700.HK", 100)]);
        let brain = build_brain(narratives, dimensions, &links, &store);
        let snapshot = DecisionSnapshot::compute(&brain, &links, &[], &store);

        let suggestion = snapshot
            .order_suggestions
            .iter()
            .find(|o| o.symbol == sym("700.HK"));
        assert!(suggestion.is_some());
        let s = suggestion.unwrap();
        assert_eq!(s.direction, OrderDirection::Buy);
        assert_eq!(s.suggested_quantity, 100);
        assert!(s.requires_confirmation);
    }

    #[test]
    fn price_range_from_order_book() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0.8), dec!(0.6)));
        narratives.insert(sym("9988.HK"), make_narrative(dec!(0.7), dec!(0.5)));

        let mut dimensions = HashMap::new();
        dimensions.insert(
            sym("700.HK"),
            make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
        );
        dimensions.insert(
            sym("9988.HK"),
            make_dims(dec!(0.4), dec!(0.4), dec!(0.4), dec!(0.4)),
        );

        let mut links = empty_links();
        links.cross_stock_presences.push(CrossStockPresence {
            institution_id: InstitutionId(100),
            symbols: vec![sym("700.HK"), sym("9988.HK")],
            ask_symbols: vec![],
            bid_symbols: vec![sym("700.HK"), sym("9988.HK")],
        });
        links.institution_activities.push(InstitutionActivity {
            symbol: sym("700.HK"),
            institution_id: InstitutionId(100),
            ask_positions: vec![],
            bid_positions: vec![1],
            seat_count: 1,
        });
        links.order_books.push(OrderBookObservation {
            symbol: sym("700.HK"),
            bid_levels: vec![DepthLevel {
                position: 1,
                price: Some(dec!(350)),
                volume: 1000,
                order_num: 10,
            }],
            ask_levels: vec![DepthLevel {
                position: 1,
                price: Some(dec!(351)),
                volume: 800,
                order_num: 8,
            }],
            total_bid_volume: 1000,
            total_ask_volume: 800,
            total_bid_orders: 10,
            total_ask_orders: 8,
            spread: Some(dec!(1)),
            bid_level_count: 1,
            ask_level_count: 1,
            bid_profile: DepthProfile::empty(),
            ask_profile: DepthProfile::empty(),
        });

        let store = make_store_with_stocks(vec![make_stock("700.HK", 100)]);
        let brain = build_brain(narratives, dimensions, &links, &store);
        let snapshot = DecisionSnapshot::compute(&brain, &links, &[], &store);

        let s = snapshot
            .order_suggestions
            .iter()
            .find(|o| o.symbol == sym("700.HK"))
            .unwrap();
        assert_eq!(s.price_low, Some(dec!(350)));
        assert_eq!(s.price_high, Some(dec!(351)));
    }

    #[test]
    fn confirmation_logic_only_flags_risky_orders() {
        let confident = ConvergenceScore {
            symbol: sym("700.HK"),
            institutional_alignment: dec!(0.7),
            sector_coherence: Some(dec!(0.6)),
            cross_stock_correlation: dec!(0.5),
            composite: dec!(0.6),
        };
        let policy = ConfirmationPolicy {
            low_confidence_cutoff: dec!(0.4),
            wide_spread_cutoff: dec!(0.01),
        };
        assert!(!requires_manual_confirmation(
            &confident,
            Some(dec!(350)),
            Some(dec!(351)),
            policy,
        ));

        let conflicted = ConvergenceScore {
            cross_stock_correlation: dec!(-0.5),
            ..confident.clone()
        };
        assert!(requires_manual_confirmation(
            &conflicted,
            Some(dec!(350)),
            Some(dec!(351)),
            policy,
        ));
    }

    #[test]
    fn confirmation_policy_derives_cutoffs_from_market_samples() {
        let scores = HashMap::from([
            (
                sym("700.HK"),
                ConvergenceScore {
                    symbol: sym("700.HK"),
                    institutional_alignment: dec!(0.7),
                    sector_coherence: Some(dec!(0.6)),
                    cross_stock_correlation: dec!(0.5),
                    composite: dec!(0.2),
                },
            ),
            (
                sym("388.HK"),
                ConvergenceScore {
                    symbol: sym("388.HK"),
                    institutional_alignment: dec!(0.5),
                    sector_coherence: Some(dec!(0.4)),
                    cross_stock_correlation: dec!(0.3),
                    composite: dec!(0.6),
                },
            ),
            (
                sym("9988.HK"),
                ConvergenceScore {
                    symbol: sym("9988.HK"),
                    institutional_alignment: dec!(0.6),
                    sector_coherence: Some(dec!(0.5)),
                    cross_stock_correlation: dec!(0.2),
                    composite: dec!(0.9),
                },
            ),
        ]);
        let best_bid = HashMap::from([
            (sym("700.HK"), dec!(350)),
            (sym("388.HK"), dec!(100)),
            (sym("9988.HK"), dec!(80)),
        ]);
        let best_ask = HashMap::from([
            (sym("700.HK"), dec!(351)),
            (sym("388.HK"), dec!(100.4)),
            (sym("9988.HK"), dec!(80.8)),
        ]);

        let policy = ConfirmationPolicy::from_market(&scores, &best_bid, &best_ask);

        assert_eq!(policy.low_confidence_cutoff, dec!(0.6));
        assert!(policy.wide_spread_cutoff > dec!(0.003));
        assert!(policy.wide_spread_cutoff < dec!(0.01));
    }

    #[test]
    fn action_node_from_hk_fingerprint_maps_direction_and_market() {
        let symbol = sym("700.HK");
        let fingerprint = StructuralFingerprint {
            symbol: symbol.clone(),
            entry_timestamp: OffsetDateTime::UNIX_EPOCH,
            entry_composite: dec!(0.6),
            entry_regime: crate::action::narrative::Regime::CoherentBullish,
            institutional_directions: vec![],
            sector_mean_coherence: Some(dec!(0.2)),
            correlated_stocks: vec![],
            entry_dimensions: SymbolDimensions::default(),
        };

        let node = ActionNode::from_hk_fingerprint(&symbol, &fingerprint);

        assert_eq!(node.market, crate::ontology::Market::Hk);
        assert_eq!(node.direction, ActionDirection::Long);
        assert_eq!(node.stage, ActionNodeStage::Monitoring);
    }

    #[test]
    fn market_regime_flags_broad_selloff_as_risk_off() {
        let mut links = empty_links();
        links.quotes = vec![
            QuoteObservation {
                symbol: sym("700.HK"),
                last_done: dec!(519),
                prev_close: dec!(550.5),
                open: dec!(545),
                high: dec!(546),
                low: dec!(515),
                volume: 100,
                turnover: dec!(52000),
                timestamp: OffsetDateTime::UNIX_EPOCH,
                market_status: MarketStatus::Normal,
            },
            QuoteObservation {
                symbol: sym("9988.HK"),
                last_done: dec!(72.3),
                prev_close: dec!(75.0),
                open: dec!(74.8),
                high: dec!(74.9),
                low: dec!(71.9),
                volume: 100,
                turnover: dec!(7230),
                timestamp: OffsetDateTime::UNIX_EPOCH,
                market_status: MarketStatus::Normal,
            },
            QuoteObservation {
                symbol: sym("3690.HK"),
                last_done: dec!(118),
                prev_close: dec!(123),
                open: dec!(122),
                high: dec!(122.5),
                low: dec!(117),
                volume: 100,
                turnover: dec!(11800),
                timestamp: OffsetDateTime::UNIX_EPOCH,
                market_status: MarketStatus::Normal,
            },
            QuoteObservation {
                symbol: sym("1810.HK"),
                last_done: dec!(14.8),
                prev_close: dec!(15.2),
                open: dec!(15.1),
                high: dec!(15.1),
                low: dec!(14.6),
                volume: 100,
                turnover: dec!(1480),
                timestamp: OffsetDateTime::UNIX_EPOCH,
                market_status: MarketStatus::Normal,
            },
            QuoteObservation {
                symbol: sym("883.HK"),
                last_done: dec!(19.1),
                prev_close: dec!(19.8),
                open: dec!(19.7),
                high: dec!(19.7),
                low: dec!(18.9),
                volume: 100,
                turnover: dec!(1910),
                timestamp: OffsetDateTime::UNIX_EPOCH,
                market_status: MarketStatus::Normal,
            },
            QuoteObservation {
                symbol: sym("939.HK"),
                last_done: dec!(5.91),
                prev_close: dec!(6.05),
                open: dec!(6.02),
                high: dec!(6.03),
                low: dec!(5.88),
                volume: 100,
                turnover: dec!(591),
                timestamp: OffsetDateTime::UNIX_EPOCH,
                market_status: MarketStatus::Normal,
            },
            QuoteObservation {
                symbol: sym("6060.HK"),
                last_done: dec!(14.96),
                prev_close: dec!(14.5),
                open: dec!(14.6),
                high: dec!(15.1),
                low: dec!(14.4),
                volume: 100,
                turnover: dec!(1496),
                timestamp: OffsetDateTime::UNIX_EPOCH,
                market_status: MarketStatus::Normal,
            },
            QuoteObservation {
                symbol: sym("688.HK"),
                last_done: dec!(11.9),
                prev_close: dec!(12.3),
                open: dec!(12.2),
                high: dec!(12.2),
                low: dec!(11.8),
                volume: 100,
                turnover: dec!(1190),
                timestamp: OffsetDateTime::UNIX_EPOCH,
                market_status: MarketStatus::Normal,
            },
        ];

        let convergence_scores = HashMap::from([
            (
                sym("700.HK"),
                ConvergenceScore {
                    symbol: sym("700.HK"),
                    institutional_alignment: dec!(-0.4),
                    sector_coherence: Some(dec!(-0.3)),
                    cross_stock_correlation: dec!(-0.5),
                    composite: dec!(-0.4),
                },
            ),
            (
                sym("9988.HK"),
                ConvergenceScore {
                    symbol: sym("9988.HK"),
                    institutional_alignment: dec!(-0.3),
                    sector_coherence: Some(dec!(-0.2)),
                    cross_stock_correlation: dec!(-0.4),
                    composite: dec!(-0.3),
                },
            ),
            (
                sym("6060.HK"),
                ConvergenceScore {
                    symbol: sym("6060.HK"),
                    institutional_alignment: dec!(0.2),
                    sector_coherence: Some(dec!(0.1)),
                    cross_stock_correlation: dec!(0.1),
                    composite: dec!(0.15),
                },
            ),
            (
                sym("688.HK"),
                ConvergenceScore {
                    symbol: sym("688.HK"),
                    institutional_alignment: dec!(-0.2),
                    sector_coherence: Some(dec!(-0.1)),
                    cross_stock_correlation: dec!(-0.2),
                    composite: dec!(-0.2),
                },
            ),
        ]);

        let regime = MarketRegimeFilter::compute(&links, &convergence_scores);
        assert_eq!(regime.bias, MarketRegimeBias::RiskOff);
        assert!(regime.blocks(OrderDirection::Buy));
        assert!(!regime.blocks(OrderDirection::Sell));
    }

    #[test]
    fn zero_composite_no_suggestions() {
        let mut narratives = HashMap::new();
        narratives.insert(sym("700.HK"), make_narrative(dec!(0), dec!(0)));

        let mut dimensions = HashMap::new();
        dimensions.insert(sym("700.HK"), make_dims(dec!(0), dec!(0), dec!(0), dec!(0)));

        let links = empty_links();
        let store = make_store_with_stocks(vec![]);
        let brain = build_brain(narratives, dimensions, &links, &store);
        let snapshot = DecisionSnapshot::compute(&brain, &links, &[], &store);

        // Composite is zero → no suggestion
        let suggestion = snapshot
            .order_suggestions
            .iter()
            .find(|o| o.symbol == sym("700.HK"));
        assert!(suggestion.is_none());
    }

    #[test]
    fn polymarket_market_prior_soft_blocks_when_local_regime_is_neutral() {
        let mut regime = MarketRegimeFilter::neutral();
        regime.apply_polymarket_snapshot(&PolymarketSnapshot {
            fetched_at: OffsetDateTime::UNIX_EPOCH,
            priors: vec![PolymarketPrior {
                slug: "fed-cut".into(),
                label: "Fed cut".into(),
                question: "Will the Fed cut?".into(),
                scope: ReasoningScope::Market,
                target_scopes: vec![],
                bias: PolymarketBias::RiskOn,
                selected_outcome: "Yes".into(),
                probability: dec!(0.72),
                conviction_threshold: dec!(0.60),
                active: true,
                closed: false,
                category: None,
                volume: None,
                liquidity: None,
                end_date: None,
            }],
        });

        assert!(regime.blocks(OrderDirection::Sell));
        assert!(!regime.blocks(OrderDirection::Buy));
        assert!(regime
            .gate_reason(OrderDirection::Sell)
            .unwrap_or_default()
            .contains("external="));
    }

    #[test]
    fn explicit_target_scopes_drive_polymarket_symbol_relevance() {
        let store = make_store_with_stocks(vec![Stock {
            market: crate::ontology::Market::Hk,
            symbol: sym("981.HK"),
            name_en: "SMIC".into(),
            name_cn: String::new(),
            name_hk: String::new(),
            exchange: "SEHK".into(),
            lot_size: 100,
            sector_id: Some(SectorId("semiconductor".into())),
            total_shares: 0,
            circulating_shares: 0,
            eps_ttm: Decimal::ZERO,
            bps: Decimal::ZERO,
            dividend_yield: Decimal::ZERO,
        }]);
        let mut suggestion = OrderSuggestion {
            symbol: sym("981.HK"),
            direction: OrderDirection::Sell,
            convergence: ConvergenceScore {
                symbol: sym("981.HK"),
                institutional_alignment: dec!(-0.5),
                sector_coherence: Some(dec!(-0.4)),
                cross_stock_correlation: dec!(-0.3),
                composite: dec!(-0.55),
            },
            suggested_quantity: 100,
            price_low: Some(dec!(20)),
            price_high: Some(dec!(20.1)),
            estimated_cost: dec!(0.005),
            heuristic_edge: dec!(0.54),
            requires_confirmation: false,
            convergence_score: dec!(0.55),
            effective_confidence: dec!(0.55),
            external_confirmation: None,
            external_conflict: None,
            external_support_slug: None,
            external_support_probability: None,
            external_conflict_slug: None,
            external_conflict_probability: None,
        };
        let snapshot = PolymarketSnapshot {
            fetched_at: OffsetDateTime::UNIX_EPOCH,
            priors: vec![PolymarketPrior {
                slug: "chip-sanctions".into(),
                label: "AI chip sanctions".into(),
                question: "Will AI chip sanctions tighten?".into(),
                scope: ReasoningScope::Theme("ai_semis".into()),
                target_scopes: vec!["sector:semiconductor".into()],
                bias: PolymarketBias::RiskOff,
                selected_outcome: "Yes".into(),
                probability: dec!(0.66),
                conviction_threshold: dec!(0.60),
                active: true,
                closed: false,
                category: None,
                volume: None,
                liquidity: None,
                end_date: None,
            }],
        };

        let sector_id = store
            .stocks
            .get(&sym("981.HK"))
            .and_then(|stock| stock.sector_id.as_ref());
        apply_external_convergence_to_suggestion(&mut suggestion, &snapshot, sector_id);

        assert!(suggestion.external_confirmation.is_some());
        assert_eq!(
            suggestion.external_support_slug.as_deref(),
            Some("chip-sanctions")
        );
        assert!(suggestion.convergence_score > dec!(0.55));
    }
}
