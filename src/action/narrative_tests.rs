use super::*;
use rust_decimal_macros::dec;

fn make_dims(book: Decimal, capital: Decimal, size: Decimal, inst: Decimal) -> SymbolDimensions {
    SymbolDimensions {
        order_book_pressure: book,
        capital_flow_direction: capital,
        capital_size_divergence: size,
        institutional_direction: inst,
        ..Default::default()
    }
}

fn make_tension(dims: &SymbolDimensions) -> SymbolTension {
    // Recompute tension from dimensions using the same logic as tension.rs.
    let all = Dimension::ALL;
    let n = Decimal::from(all.len() as i64);
    let pair_count = Decimal::from((all.len() * (all.len() - 1) / 2) as i64);

    let sum_vals: Decimal = all.iter().map(|&d| get_dimension_value(dims, d)).sum();
    let mean_direction = sum_vals / n;

    let mut pairs = Vec::new();
    for i in 0..all.len() {
        for j in (i + 1)..all.len() {
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
    let coherence = sum_products / pair_count;
    pairs.sort_by(|a, b| a.product.cmp(&b.product));

    SymbolTension {
        coherence,
        mean_direction,
        pairs,
    }
}

// ── Regime classification ──

#[test]
fn regime_coherent_bullish() {
    assert_eq!(
        Regime::classify(dec!(0.5), dec!(0.3)),
        Regime::CoherentBullish
    );
    assert_eq!(
        Regime::classify(dec!(0), dec!(0.1)),
        Regime::CoherentBullish
    );
}

#[test]
fn regime_coherent_bearish() {
    assert_eq!(
        Regime::classify(dec!(0.5), dec!(-0.3)),
        Regime::CoherentBearish
    );
    assert_eq!(
        Regime::classify(dec!(0), dec!(-0.1)),
        Regime::CoherentBearish
    );
}

#[test]
fn regime_coherent_neutral() {
    assert_eq!(
        Regime::classify(dec!(0.5), dec!(0)),
        Regime::CoherentNeutral
    );
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
    assert!(
        tension.coherence < Decimal::ZERO,
        "coherence should be negative: {}",
        tension.coherence
    );
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
fn readings_count_matches_dimension_count() {
    let dims = make_dims(dec!(0.1), dec!(0.2), dec!(0.3), dec!(0.4));
    let tension = make_tension(&dims);
    let narrative = compute_symbol_narrative(&tension, &dims);
    assert_eq!(narrative.readings.len(), Dimension::ALL.len());
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
    assert_eq!(
        narrative.readings[0].dimension,
        Dimension::CapitalFlowDirection
    );
}

// ── Pair partitioning ──

#[test]
fn partition_matches_dimension_count() {
    let dims = make_dims(dec!(0.4), dec!(-0.2), dec!(0.3), dec!(-0.1));
    let tension = make_tension(&dims);
    let narrative = compute_symbol_narrative(&tension, &dims);
    assert_eq!(
        narrative.agreements.len() + narrative.contradictions.len(),
        Dimension::ALL.len() * (Dimension::ALL.len() - 1) / 2,
    );
}

#[test]
fn all_positive_no_contradictions() {
    // Four positive dimensions plus four neutral ones. Zero-product pairs count as agreements.
    let dims = make_dims(dec!(0.5), dec!(0.3), dec!(0.4), dec!(0.6));
    let tension = make_tension(&dims);
    let narrative = compute_symbol_narrative(&tension, &dims);
    assert_eq!(narrative.contradictions.len(), 0);
    assert_eq!(narrative.agreements.len(), 28);
}

#[test]
fn split_signals_has_contradictions() {
    // Two positive, two negative, four neutral dimensions.
    let dims = make_dims(dec!(0.5), dec!(0.3), dec!(-0.4), dec!(-0.6));
    let tension = make_tension(&dims);
    let narrative = compute_symbol_narrative(&tension, &dims);
    assert_eq!(narrative.contradictions.len(), 4);
    assert_eq!(narrative.agreements.len(), 24);
}

// ── Snapshot integration ──

#[test]
fn snapshot_all_symbols() {
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
    let tension_snap = TensionSnapshot::compute(&dim_snap);
    let narrative_snap = NarrativeSnapshot::compute(&tension_snap, &dim_snap);
    assert_eq!(narrative_snap.narratives.len(), 2);
    assert!(narrative_snap
        .narratives
        .contains_key(&Symbol("700.HK".into())));
    assert!(narrative_snap
        .narratives
        .contains_key(&Symbol("9988.HK".into())));
}

#[test]
fn snapshot_skips_missing() {
    // Tension has both symbols, but dimensions only has one.
    let full_dims = DimensionSnapshot {
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
    let tension_snap = TensionSnapshot::compute(&full_dims);

    // Partial dimensions — only 700.HK
    let partial_dims = DimensionSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        dimensions: HashMap::from([(
            Symbol("700.HK".into()),
            make_dims(dec!(0.4), dec!(-0.2), dec!(-0.3), dec!(0.1)),
        )]),
    };

    let narrative_snap = NarrativeSnapshot::compute(&tension_snap, &partial_dims);
    assert_eq!(narrative_snap.narratives.len(), 1);
    assert!(narrative_snap
        .narratives
        .contains_key(&Symbol("700.HK".into())));
}

#[test]
fn snapshot_preserves_timestamp() {
    let ts = OffsetDateTime::UNIX_EPOCH;
    let dim_snap = DimensionSnapshot {
        timestamp: ts,
        dimensions: HashMap::from([(
            Symbol("700.HK".into()),
            make_dims(dec!(0.1), dec!(0.2), dec!(0.3), dec!(0.4)),
        )]),
    };
    let tension_snap = TensionSnapshot::compute(&dim_snap);
    let narrative_snap = NarrativeSnapshot::compute(&tension_snap, &dim_snap);
    assert_eq!(narrative_snap.timestamp, ts);
}
