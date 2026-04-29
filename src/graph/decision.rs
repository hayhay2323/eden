use std::collections::HashMap;

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::math::clamp_unit_interval;
use crate::ontology::links::LinkSnapshot;
use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::ActionDirection;
use crate::ontology::store::ObjectStore;

use super::graph::BrainGraph;

// Re-export extracted modules so existing `use crate::graph::decision::*` works
pub use super::convergence::*;
pub use super::fingerprint::*;
#[path = "decision/regime.rs"]
mod regime;
pub use regime::{MarketRegimeBias, MarketRegimeFilter};
#[path = "decision/orders.rs"]
mod orders;
use orders::{estimate_transaction_cost, requires_manual_confirmation, ConfirmationPolicy};
pub use orders::{OrderDirection, OrderSuggestion};

pub(crate) fn same_sign(a: Decimal, b: Decimal) -> bool {
    (a > Decimal::ZERO && b > Decimal::ZERO)
        || (a < Decimal::ZERO && b < Decimal::ZERO)
        || (a == Decimal::ZERO && b == Decimal::ZERO)
}

pub(crate) fn signed_position_return(
    entry_price: Decimal,
    current_price: Decimal,
    direction: ActionDirection,
) -> Option<Decimal> {
    if entry_price <= Decimal::ZERO || current_price <= Decimal::ZERO {
        return None;
    }

    let raw_return = (current_price - entry_price) / entry_price;
    Some(match direction {
        ActionDirection::Long => raw_return,
        ActionDirection::Short => -raw_return,
        ActionDirection::Neutral => Decimal::ZERO,
    })
}

// ── Decision Snapshot ──

#[derive(Debug, Clone)]
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
        temporal_ctx: Option<&TemporalConvergenceContext>,
        edge_ledger: Option<&crate::graph::edge_learning::EdgeLearningLedger>,
    ) -> Self {
        // Compute ConvergenceScore for all stock nodes
        let mut convergence_scores = HashMap::new();
        for symbol in brain.stock_nodes.keys() {
            if let Some(score) = ConvergenceScore::compute(symbol, brain, temporal_ctx, edge_ledger)
            {
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
}

#[cfg(test)]
#[path = "decision_tests.rs"]
mod tests;
