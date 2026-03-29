use super::*;

pub(super) fn provenance<I, S>(
    source: ProvenanceSource,
    observed_at: OffsetDateTime,
    confidence: Option<Decimal>,
    inputs: I,
) -> ProvenanceMetadata
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let mut provenance = ProvenanceMetadata::new(source, observed_at).with_inputs(inputs);
    if let Some(confidence) = confidence {
        provenance = provenance.with_confidence(confidence.clamp(Decimal::ZERO, Decimal::ONE));
    }
    provenance
}

pub(super) fn confidence_from_turnover(turnover: Decimal) -> Decimal {
    if turnover <= Decimal::ZERO {
        Decimal::ZERO
    } else {
        (turnover / Decimal::new(1_000_000, 0)).min(Decimal::ONE)
    }
}

pub(super) fn confidence_from_magnitude(value: Decimal) -> Decimal {
    let magnitude = value.abs();
    if magnitude == Decimal::ZERO {
        Decimal::ZERO
    } else {
        (magnitude / Decimal::new(1_000_000, 0)).min(Decimal::ONE)
    }
}

pub(super) fn average_dimension_strength(dims: &SymbolDimensions) -> Decimal {
    let values = [
        dims.order_book_pressure,
        dims.capital_flow_direction,
        dims.capital_size_divergence,
        dims.institutional_direction,
        dims.depth_structure_imbalance,
        dims.valuation_support,
        dims.activity_momentum,
        dims.candlestick_conviction,
    ];
    values.iter().copied().sum::<Decimal>() / Decimal::from(values.len() as i64)
}
