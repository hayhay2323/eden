use std::collections::HashMap;

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::logic::tension::{Dimension, DimensionPair, SymbolTension, TensionSnapshot};
use crate::ontology::objects::Symbol;
use crate::pipeline::dimensions::{DimensionSnapshot, SymbolDimensions};

/// Market regime classified by sign of coherence and mean_direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Regime {
    CoherentBullish,
    CoherentBearish,
    CoherentNeutral,
    Conflicted,
}

impl Regime {
    pub fn classify(coherence: Decimal, mean_direction: Decimal) -> Self {
        if coherence < Decimal::ZERO {
            Regime::Conflicted
        } else if mean_direction > Decimal::ZERO {
            Regime::CoherentBullish
        } else if mean_direction < Decimal::ZERO {
            Regime::CoherentBearish
        } else {
            Regime::CoherentNeutral
        }
    }
}

impl std::fmt::Display for Regime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Regime::CoherentBullish => write!(f, "CoherentBullish"),
            Regime::CoherentBearish => write!(f, "CoherentBearish"),
            Regime::CoherentNeutral => write!(f, "CoherentNeutral"),
            Regime::Conflicted => write!(f, "Conflicted"),
        }
    }
}

/// Sign-based direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Positive,
    Negative,
    Neutral,
}

impl Direction {
    pub fn from_value(v: Decimal) -> Self {
        if v > Decimal::ZERO {
            Direction::Positive
        } else if v < Decimal::ZERO {
            Direction::Negative
        } else {
            Direction::Neutral
        }
    }
}

/// A single dimension's value and sign-based direction.
#[derive(Debug, Clone)]
pub struct DimensionReading {
    pub dimension: Dimension,
    pub value: Decimal,
    pub direction: Direction,
}

/// Structured narrative for a single symbol.
#[derive(Debug, Clone)]
pub struct SymbolNarrative {
    pub regime: Regime,
    pub coherence: Decimal,
    pub mean_direction: Decimal,
    pub readings: Vec<DimensionReading>,
    pub agreements: Vec<DimensionPair>,
    pub contradictions: Vec<DimensionPair>,
}

/// Market-wide narrative snapshot.
#[derive(Debug)]
pub struct NarrativeSnapshot {
    pub timestamp: OffsetDateTime,
    pub narratives: HashMap<Symbol, SymbolNarrative>,
}

impl NarrativeSnapshot {
    /// Pure function: combine tension and dimension snapshots into narratives.
    /// Symbols present in tensions but missing from dimensions are skipped.
    pub fn compute(tensions: &TensionSnapshot, dimensions: &DimensionSnapshot) -> Self {
        let narratives = tensions
            .tensions
            .iter()
            .filter_map(|(sym, tension)| {
                let dims = dimensions.dimensions.get(sym)?;
                Some((sym.clone(), compute_symbol_narrative(tension, dims)))
            })
            .collect();

        NarrativeSnapshot {
            timestamp: tensions.timestamp,
            narratives,
        }
    }
}

fn get_dimension_value(dims: &SymbolDimensions, d: Dimension) -> Decimal {
    match d {
        Dimension::OrderBookPressure => dims.order_book_pressure,
        Dimension::CapitalFlowDirection => dims.capital_flow_direction,
        Dimension::CapitalSizeDivergence => dims.capital_size_divergence,
        Dimension::InstitutionalDirection => dims.institutional_direction,
        Dimension::DepthStructureImbalance => dims.depth_structure_imbalance,
        Dimension::ValuationSupport => dims.valuation_support,
        Dimension::ActivityMomentum => dims.activity_momentum,
        Dimension::CandlestickConviction => dims.candlestick_conviction,
    }
}

fn compute_symbol_narrative(tension: &SymbolTension, dims: &SymbolDimensions) -> SymbolNarrative {
    let regime = Regime::classify(tension.coherence, tension.mean_direction);

    // Build readings sorted by |value| descending.
    let mut readings: Vec<DimensionReading> = Dimension::ALL
        .iter()
        .map(|&d| {
            let value = get_dimension_value(dims, d);
            DimensionReading {
                dimension: d,
                value,
                direction: Direction::from_value(value),
            }
        })
        .collect();
    readings.sort_by(|a, b| b.value.abs().cmp(&a.value.abs()));

    // Partition pairs into agreements and contradictions.
    let mut agreements = Vec::new();
    let mut contradictions = Vec::new();
    for pair in &tension.pairs {
        if pair.product < Decimal::ZERO {
            contradictions.push(pair.clone());
        } else {
            agreements.push(pair.clone());
        }
    }

    SymbolNarrative {
        regime,
        coherence: tension.coherence,
        mean_direction: tension.mean_direction,
        readings,
        agreements,
        contradictions,
    }
}

#[cfg(test)]
#[path = "narrative_tests.rs"]
mod tests;
