use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::external::polymarket::{PolymarketBias, PolymarketPrior, PolymarketSnapshot};
use crate::math::{clamp_unit_interval, median};
use crate::ontology::objects::{SectorId, Symbol};

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

pub(super) fn estimate_transaction_cost(price_low: Option<Decimal>, price_high: Option<Decimal>) -> Decimal {
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
    pub external_confirmation: Option<String>,
    pub external_conflict: Option<String>,
    pub external_support_slug: Option<String>,
    pub external_support_probability: Option<Decimal>,
    pub external_conflict_slug: Option<String>,
    pub external_conflict_probability: Option<Decimal>,
}

pub(super) fn apply_external_convergence_to_suggestion(
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
        crate::ontology::ReasoningScope::Market(_) => true,
        crate::ontology::ReasoningScope::Sector(sector) => {
            sector_id.map(|value| value.0.as_str()) == Some(sector.0.as_str())
        }
        crate::ontology::ReasoningScope::Symbol(scope_symbol) => scope_symbol == symbol,
        crate::ontology::ReasoningScope::Region(region) => {
            matches!(region.0.as_str(), "china" | "hk" | "hong_kong")
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
