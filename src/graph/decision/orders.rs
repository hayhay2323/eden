use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::math::median;
use crate::ontology::objects::Symbol;

use super::{same_sign, ConvergenceScore};

#[derive(Debug, Clone, Copy)]
pub(super) struct ConfirmationPolicy {
    pub(crate) low_confidence_cutoff: Decimal,
    pub(crate) wide_spread_cutoff: Decimal,
}

impl ConfirmationPolicy {
    pub(super) fn from_market(
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

pub(super) fn requires_manual_confirmation(
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

pub(super) fn estimate_transaction_cost(
    price_low: Option<Decimal>,
    price_high: Option<Decimal>,
) -> Decimal {
    quoted_spread_ratio(price_low, price_high).unwrap_or(Decimal::new(5, 3)) // fallback 0.5% when the book is too sparse to estimate
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
}
