use std::collections::HashMap;

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::ontology::objects::Symbol;
use crate::pipeline::dimensions::{DimensionSnapshot, SymbolDimensions};

/// Pipeline dimensions, named for pairing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Dimension {
    OrderBookPressure,
    CapitalFlowDirection,
    CapitalSizeDivergence,
    InstitutionalDirection,
    DepthStructureImbalance,
    ValuationSupport,
    ActivityMomentum,
    CandlestickConviction,
}

impl Dimension {
    pub const ALL: [Dimension; 8] = [
        Dimension::OrderBookPressure,
        Dimension::CapitalFlowDirection,
        Dimension::CapitalSizeDivergence,
        Dimension::InstitutionalDirection,
        Dimension::DepthStructureImbalance,
        Dimension::ValuationSupport,
        Dimension::ActivityMomentum,
        Dimension::CandlestickConviction,
    ];

    pub fn short_name(&self) -> &'static str {
        match self {
            Dimension::OrderBookPressure => "book",
            Dimension::CapitalFlowDirection => "capital",
            Dimension::CapitalSizeDivergence => "size",
            Dimension::InstitutionalDirection => "inst",
            Dimension::DepthStructureImbalance => "depth",
            Dimension::ValuationSupport => "value",
            Dimension::ActivityMomentum => "activity",
            Dimension::CandlestickConviction => "candle",
        }
    }
}

impl std::fmt::Display for Dimension {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.short_name())
    }
}

/// A pair of dimensions and their product. Positive = agreement, negative = tension.
#[derive(Debug, Clone)]
pub struct DimensionPair {
    pub dim_a: Dimension,
    pub dim_b: Dimension,
    pub val_a: Decimal,
    pub val_b: Decimal,
    pub product: Decimal,
}

/// Per-symbol tension analysis.
#[derive(Debug, Clone)]
pub struct SymbolTension {
    /// Mean of all C(n,2) pairwise products. Positive = coherent, negative = conflicted.
    pub coherence: Decimal,
    /// Simple average of all dimensions. Overall directional lean.
    pub mean_direction: Decimal,
    /// All C(n,2) pairwise relationships, sorted by product (most tense first).
    pub pairs: Vec<DimensionPair>,
}

/// Market-wide tension snapshot.
#[derive(Debug)]
pub struct TensionSnapshot {
    pub timestamp: OffsetDateTime,
    pub tensions: HashMap<Symbol, SymbolTension>,
}

impl TensionSnapshot {
    /// Pure synchronous function — compute tensions from dimension vectors.
    pub fn compute(dims: &DimensionSnapshot) -> Self {
        let tensions = dims
            .dimensions
            .iter()
            .map(|(sym, sd)| (sym.clone(), compute_symbol_tension(sd)))
            .collect();

        TensionSnapshot {
            timestamp: dims.timestamp,
            tensions,
        }
    }
}

fn get_value(sd: &SymbolDimensions, dim: Dimension) -> Decimal {
    match dim {
        Dimension::OrderBookPressure => sd.order_book_pressure,
        Dimension::CapitalFlowDirection => sd.capital_flow_direction,
        Dimension::CapitalSizeDivergence => sd.capital_size_divergence,
        Dimension::InstitutionalDirection => sd.institutional_direction,
        Dimension::DepthStructureImbalance => sd.depth_structure_imbalance,
        Dimension::ValuationSupport => sd.valuation_support,
        Dimension::ActivityMomentum => sd.activity_momentum,
        Dimension::CandlestickConviction => sd.candlestick_conviction,
    }
}

fn compute_symbol_tension(sd: &SymbolDimensions) -> SymbolTension {
    let dims = Dimension::ALL;
    let n = Decimal::from(dims.len() as i64);
    let pair_count = Decimal::from((dims.len() * (dims.len() - 1) / 2) as i64); // C(n,2)

    // Mean direction: average of all values.
    let sum_vals: Decimal = dims.iter().map(|&d| get_value(sd, d)).sum();
    let mean_direction = sum_vals / n;

    // All C(n,2) pairs.
    let pair_count_usize = dims.len() * (dims.len() - 1) / 2;
    let mut pairs = Vec::with_capacity(pair_count_usize);
    for i in 0..dims.len() {
        for j in (i + 1)..dims.len() {
            let val_a = get_value(sd, dims[i]);
            let val_b = get_value(sd, dims[j]);
            pairs.push(DimensionPair {
                dim_a: dims[i],
                dim_b: dims[j],
                val_a,
                val_b,
                product: val_a * val_b,
            });
        }
    }

    // Coherence: mean of all pair products.
    let sum_products: Decimal = pairs.iter().map(|p| p.product).sum();
    let coherence = sum_products / pair_count;

    // Sort: most tense (most negative product) first.
    pairs.sort_by(|a, b| a.product.cmp(&b.product));

    SymbolTension {
        coherence,
        mean_direction,
        pairs,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_dims(
        book: Decimal,
        capital: Decimal,
        size: Decimal,
        inst: Decimal,
    ) -> SymbolDimensions {
        SymbolDimensions {
            order_book_pressure: book,
            capital_flow_direction: capital,
            capital_size_divergence: size,
            institutional_direction: inst,
            ..Default::default()
        }
    }

    // ── coherence ──

    #[test]
    fn all_positive_is_coherent() {
        let sd = make_dims(dec!(0.5), dec!(0.3), dec!(0.4), dec!(0.6));
        let t = compute_symbol_tension(&sd);
        assert!(
            t.coherence > Decimal::ZERO,
            "all positive → positive coherence"
        );
        assert!(t.mean_direction > Decimal::ZERO);
    }

    #[test]
    fn all_negative_is_coherent() {
        let sd = make_dims(dec!(-0.5), dec!(-0.3), dec!(-0.4), dec!(-0.6));
        let t = compute_symbol_tension(&sd);
        assert!(
            t.coherence > Decimal::ZERO,
            "all negative → positive coherence (products are positive)"
        );
        assert!(t.mean_direction < Decimal::ZERO);
    }

    #[test]
    fn mixed_signs_is_tense() {
        // 2 positive, 2 negative → 4 pairs disagree, 2 agree → net negative coherence
        let sd = make_dims(dec!(0.5), dec!(0.5), dec!(-0.5), dec!(-0.5));
        let t = compute_symbol_tension(&sd);
        assert!(
            t.coherence < Decimal::ZERO,
            "split signals → negative coherence"
        );
    }

    #[test]
    fn all_zero_is_neutral() {
        let sd = make_dims(dec!(0), dec!(0), dec!(0), dec!(0));
        let t = compute_symbol_tension(&sd);
        assert_eq!(t.coherence, dec!(0));
        assert_eq!(t.mean_direction, dec!(0));
    }

    #[test]
    fn one_outlier_creates_tension() {
        // Zero-valued dimensions are neutral, so only the nonzero outlier pairs turn tense.
        let sd = make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(-0.5));
        let t = compute_symbol_tension(&sd);
        let tense_count = t.pairs.iter().filter(|p| p.product < Decimal::ZERO).count();
        assert_eq!(tense_count, 3, "the outlier creates 3 tense pairs");
    }

    // ── pair count ──

    #[test]
    fn pair_count_matches_dimension_count() {
        let sd = make_dims(dec!(0.1), dec!(0.2), dec!(0.3), dec!(0.4));
        let t = compute_symbol_tension(&sd);
        assert_eq!(
            t.pairs.len(),
            Dimension::ALL.len() * (Dimension::ALL.len() - 1) / 2,
        );
    }

    // ── mean_direction ──

    #[test]
    fn mean_direction_calculation() {
        let sd = make_dims(dec!(0.4), dec!(0.2), dec!(-0.2), dec!(0.6));
        let t = compute_symbol_tension(&sd);
        // Four populated dimensions plus four zero-valued dimensions.
        assert_eq!(t.mean_direction, dec!(0.125));
    }

    // ── pair sorting ──

    #[test]
    fn pairs_sorted_most_tense_first() {
        let sd = make_dims(dec!(0.8), dec!(0.1), dec!(-0.9), dec!(0.5));
        let t = compute_symbol_tension(&sd);
        // book(+0.8) × size(-0.9) = -0.72 should be the most tense pair
        assert_eq!(t.pairs[0].dim_a, Dimension::OrderBookPressure);
        assert_eq!(t.pairs[0].dim_b, Dimension::CapitalSizeDivergence);
        // Verify sorting: each product ≤ next
        for w in t.pairs.windows(2) {
            assert!(w[0].product <= w[1].product);
        }
    }

    // ── specific product values ──

    #[test]
    fn product_values_correct() {
        let sd = make_dims(dec!(0.6), dec!(-0.4), dec!(0.3), dec!(0.2));
        let t = compute_symbol_tension(&sd);
        // Find book × capital pair
        let bc = t
            .pairs
            .iter()
            .find(|p| {
                p.dim_a == Dimension::OrderBookPressure
                    && p.dim_b == Dimension::CapitalFlowDirection
            })
            .unwrap();
        assert_eq!(bc.product, dec!(-0.24)); // 0.6 × -0.4
    }

    // ── snapshot integration ──

    #[test]
    fn tension_snapshot_from_dimensions() {
        let dim_snap = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::from([
                (
                    Symbol("700.HK".into()),
                    make_dims(dec!(0.4), dec!(-0.2), dec!(-0.3), dec!(0.1)),
                ),
                (
                    Symbol("9988.HK".into()),
                    make_dims(dec!(0.5), dec!(0.3), dec!(0.4), dec!(0.6)),
                ),
            ]),
        };

        let snap = TensionSnapshot::compute(&dim_snap);
        assert_eq!(snap.tensions.len(), 2);

        // 700.HK has mixed signals → should have some tension
        let t700 = &snap.tensions[&Symbol("700.HK".into())];
        let tense_count = t700
            .pairs
            .iter()
            .filter(|p| p.product < Decimal::ZERO)
            .count();
        assert!(tense_count > 0, "700.HK has mixed signals");

        // 9988.HK all positive → coherent
        let t9988 = &snap.tensions[&Symbol("9988.HK".into())];
        assert!(t9988.coherence > Decimal::ZERO);
    }

    // ── extreme cases ──

    #[test]
    fn perfect_agreement() {
        // Four explicit dims are 1, the remaining four dims are zero.
        // Sum of pairwise products = C(4,2) = 6, pair count = C(8,2) = 28.
        let sd = make_dims(dec!(1), dec!(1), dec!(1), dec!(1));
        let t = compute_symbol_tension(&sd);
        let expected = Decimal::from(6) / Decimal::from(28);
        assert_eq!(t.coherence.round_dp(10), expected.round_dp(10));
        assert_eq!(t.mean_direction, dec!(0.5));
    }

    #[test]
    fn perfect_split() {
        // [1, 1, -1, -1] plus four neutral dimensions.
        // Pairwise sum remains -2, but over C(8,2)=28 total pairs.
        let sd = make_dims(dec!(1), dec!(1), dec!(-1), dec!(-1));
        let t = compute_symbol_tension(&sd);
        let expected = Decimal::from(-2) / Decimal::from(28);
        assert_eq!(t.coherence.round_dp(10), expected.round_dp(10));
    }

    #[test]
    fn single_dimension_active() {
        // Only book has a value, the remaining seven dimensions are zero.
        let sd = make_dims(dec!(0.8), dec!(0), dec!(0), dec!(0));
        let t = compute_symbol_tension(&sd);
        assert_eq!(t.coherence, dec!(0));
        assert_eq!(t.mean_direction, dec!(0.1)); // 0.8/8
    }
}
