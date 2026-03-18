use std::collections::HashMap;

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::logic::tension::{DimensionPair, Dimension, SymbolTension, TensionSnapshot};
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
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_dims(book: Decimal, capital: Decimal, size: Decimal, inst: Decimal) -> SymbolDimensions {
        SymbolDimensions {
            order_book_pressure: book,
            capital_flow_direction: capital,
            capital_size_divergence: size,
            institutional_direction: inst,
        }
    }

    fn make_tension(dims: &SymbolDimensions) -> SymbolTension {
        // Recompute tension from dimensions using the same logic as tension.rs.
        let all = Dimension::ALL;
        let four = Decimal::from(4);
        let six = Decimal::from(6);

        let sum_vals: Decimal = all.iter().map(|&d| get_dimension_value(dims, d)).sum();
        let mean_direction = sum_vals / four;

        let mut pairs = Vec::with_capacity(6);
        for i in 0..4 {
            for j in (i + 1)..4 {
                let val_a = get_dimension_value(dims, all[i]);
                let val_b = get_dimension_value(dims, all[j]);
                pairs.push(DimensionPair {
                    dim_a: all[i],
                    dim_b: all[j],
                    val_a,
                    val_b,
                    product: val_a * val_b,
                });
            }
        }

        let sum_products: Decimal = pairs.iter().map(|p| p.product).sum();
        let coherence = sum_products / six;
        pairs.sort_by(|a, b| a.product.cmp(&b.product));

        SymbolTension { coherence, mean_direction, pairs }
    }

    // ── Regime classification ──

    #[test]
    fn regime_coherent_bullish() {
        assert_eq!(Regime::classify(dec!(0.5), dec!(0.3)), Regime::CoherentBullish);
        assert_eq!(Regime::classify(dec!(0), dec!(0.1)), Regime::CoherentBullish);
    }

    #[test]
    fn regime_coherent_bearish() {
        assert_eq!(Regime::classify(dec!(0.5), dec!(-0.3)), Regime::CoherentBearish);
        assert_eq!(Regime::classify(dec!(0), dec!(-0.1)), Regime::CoherentBearish);
    }

    #[test]
    fn regime_coherent_neutral() {
        assert_eq!(Regime::classify(dec!(0.5), dec!(0)), Regime::CoherentNeutral);
        assert_eq!(Regime::classify(dec!(0), dec!(0)), Regime::CoherentNeutral);
    }

    #[test]
    fn regime_conflicted() {
        assert_eq!(Regime::classify(dec!(-0.1), dec!(0.5)), Regime::Conflicted);
        assert_eq!(Regime::classify(dec!(-0.1), dec!(-0.5)), Regime::Conflicted);
        assert_eq!(Regime::classify(dec!(-0.1), dec!(0)), Regime::Conflicted);
    }

    #[test]
    fn regime_three_vs_one() {
        // 3 positive, 1 large negative → cross-products dominate → negative coherence → Conflicted
        let dims = make_dims(dec!(0.3), dec!(0.3), dec!(0.3), dec!(-0.9));
        let tension = make_tension(&dims);
        assert!(tension.coherence < Decimal::ZERO, "coherence should be negative: {}", tension.coherence);
        let regime = Regime::classify(tension.coherence, tension.mean_direction);
        assert_eq!(regime, Regime::Conflicted);
    }

    // ── Direction ──

    #[test]
    fn direction_positive() {
        assert_eq!(Direction::from_value(dec!(0.5)), Direction::Positive);
        assert_eq!(Direction::from_value(dec!(0.001)), Direction::Positive);
    }

    #[test]
    fn direction_negative() {
        assert_eq!(Direction::from_value(dec!(-0.5)), Direction::Negative);
        assert_eq!(Direction::from_value(dec!(-0.001)), Direction::Negative);
    }

    #[test]
    fn direction_neutral() {
        assert_eq!(Direction::from_value(dec!(0)), Direction::Neutral);
    }

    // ── Readings ──

    #[test]
    fn readings_count_always_four() {
        let dims = make_dims(dec!(0.1), dec!(0.2), dec!(0.3), dec!(0.4));
        let tension = make_tension(&dims);
        let narrative = compute_symbol_narrative(&tension, &dims);
        assert_eq!(narrative.readings.len(), 4);
    }

    #[test]
    fn readings_sorted_by_magnitude() {
        let dims = make_dims(dec!(0.1), dec!(-0.8), dec!(0.3), dec!(0.5));
        let tension = make_tension(&dims);
        let narrative = compute_symbol_narrative(&tension, &dims);
        // |values| should be descending: 0.8, 0.5, 0.3, 0.1
        for w in narrative.readings.windows(2) {
            assert!(w[0].value.abs() >= w[1].value.abs());
        }
        assert_eq!(narrative.readings[0].dimension, Dimension::CapitalFlowDirection);
    }

    // ── Pair partitioning ──

    #[test]
    fn partition_always_six() {
        let dims = make_dims(dec!(0.4), dec!(-0.2), dec!(0.3), dec!(-0.1));
        let tension = make_tension(&dims);
        let narrative = compute_symbol_narrative(&tension, &dims);
        assert_eq!(narrative.agreements.len() + narrative.contradictions.len(), 6);
    }

    #[test]
    fn all_positive_no_contradictions() {
        let dims = make_dims(dec!(0.5), dec!(0.3), dec!(0.4), dec!(0.6));
        let tension = make_tension(&dims);
        let narrative = compute_symbol_narrative(&tension, &dims);
        assert_eq!(narrative.contradictions.len(), 0);
        assert_eq!(narrative.agreements.len(), 6);
    }

    #[test]
    fn split_signals_has_contradictions() {
        // 2 positive, 2 negative → 4 cross-sign pairs (contradictions)
        let dims = make_dims(dec!(0.5), dec!(0.3), dec!(-0.4), dec!(-0.6));
        let tension = make_tension(&dims);
        let narrative = compute_symbol_narrative(&tension, &dims);
        assert_eq!(narrative.contradictions.len(), 4);
        assert_eq!(narrative.agreements.len(), 2);
    }

    // ── Snapshot integration ──

    #[test]
    fn snapshot_all_symbols() {
        let dim_snap = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::from([
                (Symbol("700.HK".into()), make_dims(dec!(0.4), dec!(-0.2), dec!(-0.3), dec!(0.1))),
                (Symbol("9988.HK".into()), make_dims(dec!(0.5), dec!(0.3), dec!(0.4), dec!(0.6))),
            ]),
        };
        let tension_snap = TensionSnapshot::compute(&dim_snap);
        let narrative_snap = NarrativeSnapshot::compute(&tension_snap, &dim_snap);
        assert_eq!(narrative_snap.narratives.len(), 2);
        assert!(narrative_snap.narratives.contains_key(&Symbol("700.HK".into())));
        assert!(narrative_snap.narratives.contains_key(&Symbol("9988.HK".into())));
    }

    #[test]
    fn snapshot_skips_missing() {
        // Tension has both symbols, but dimensions only has one.
        let full_dims = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::from([
                (Symbol("700.HK".into()), make_dims(dec!(0.4), dec!(-0.2), dec!(-0.3), dec!(0.1))),
                (Symbol("9988.HK".into()), make_dims(dec!(0.5), dec!(0.3), dec!(0.4), dec!(0.6))),
            ]),
        };
        let tension_snap = TensionSnapshot::compute(&full_dims);

        // Partial dimensions — only 700.HK
        let partial_dims = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::from([
                (Symbol("700.HK".into()), make_dims(dec!(0.4), dec!(-0.2), dec!(-0.3), dec!(0.1))),
            ]),
        };

        let narrative_snap = NarrativeSnapshot::compute(&tension_snap, &partial_dims);
        assert_eq!(narrative_snap.narratives.len(), 1);
        assert!(narrative_snap.narratives.contains_key(&Symbol("700.HK".into())));
    }

    #[test]
    fn snapshot_preserves_timestamp() {
        let ts = OffsetDateTime::UNIX_EPOCH;
        let dim_snap = DimensionSnapshot {
            timestamp: ts,
            dimensions: HashMap::from([
                (Symbol("700.HK".into()), make_dims(dec!(0.1), dec!(0.2), dec!(0.3), dec!(0.4))),
            ]),
        };
        let tension_snap = TensionSnapshot::compute(&dim_snap);
        let narrative_snap = NarrativeSnapshot::compute(&tension_snap, &dim_snap);
        assert_eq!(narrative_snap.timestamp, ts);
    }
}
