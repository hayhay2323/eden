use super::*;

impl DerivedSignalSnapshot {
    pub fn compute(
        dimensions: &DimensionSnapshot,
        insights: &GraphInsights,
        decision: &DecisionSnapshot,
        events: &EventSnapshot,
    ) -> Self {
        let mut signals = Vec::new();
        let mut event_counts: HashMap<String, usize> = HashMap::new();
        for event in &events.events {
            *event_counts
                .entry(format!("{:?}", event.value.kind))
                .or_default() += 1;
        }

        for (symbol, dims) in &dimensions.dimensions {
            let structural_composite = average_dimension_strength(dims);
            push_symbol_signal(
                &mut signals,
                symbol,
                DerivedSignalKind::StructuralComposite,
                structural_composite,
                "aggregate structure".into(),
                dimensions.timestamp,
                [
                    format!("dimension:order_book_pressure:{}", symbol),
                    format!("dimension:capital_flow_direction:{}", symbol),
                    format!("dimension:capital_size_divergence:{}", symbol),
                    format!("dimension:institutional_direction:{}", symbol),
                    format!("dimension:depth_structure_imbalance:{}", symbol),
                    format!("dimension:valuation_support:{}", symbol),
                    format!("dimension:activity_momentum:{}", symbol),
                    format!("dimension:candlestick_conviction:{}", symbol),
                ],
            );
            push_symbol_signal(
                &mut signals,
                symbol,
                DerivedSignalKind::ValuationSupport,
                dims.valuation_support,
                "valuation support".into(),
                dimensions.timestamp,
                [format!("dimension:valuation_support:{}", symbol)],
            );
            push_symbol_signal(
                &mut signals,
                symbol,
                DerivedSignalKind::ActivityMomentum,
                dims.activity_momentum,
                "activity momentum".into(),
                dimensions.timestamp,
                [format!("dimension:activity_momentum:{}", symbol)],
            );
            push_symbol_signal(
                &mut signals,
                symbol,
                DerivedSignalKind::CandlestickConviction,
                dims.candlestick_conviction,
                "candlestick conviction".into(),
                dimensions.timestamp,
                [format!("dimension:candlestick_conviction:{}", symbol)],
            );
        }

        for (symbol, convergence) in &decision.convergence_scores {
            push_symbol_signal(
                &mut signals,
                symbol,
                DerivedSignalKind::Convergence,
                convergence.composite,
                "decision convergence".into(),
                decision.timestamp,
                [format!("convergence:{}", symbol)],
            );
        }

        for pressure in &insights.pressures {
            push_symbol_signal(
                &mut signals,
                &pressure.symbol,
                DerivedSignalKind::SmartMoneyPressure,
                pressure.net_pressure,
                "institutional pressure".into(),
                decision.timestamp,
                [format!("graph_pressure:{}", pressure.symbol)],
            );
        }

        let market_strength = insights.stress.composite_stress;
        signals.push(
            DerivedSignal::new(
                DerivedSignalRecord {
                    scope: SignalScope::Market,
                    kind: DerivedSignalKind::MarketStress,
                    strength: market_strength,
                    summary: format!(
                        "market stress with {} notable event kinds",
                        event_counts.len()
                    ),
                },
                provenance(
                    ProvenanceSource::Computed,
                    decision.timestamp,
                    Some(market_strength),
                    ["graph_stress".to_string(), "event_snapshot".to_string()],
                ),
            )
            .with_derivation(vec!["graph_stress", "event_snapshot"]),
        );

        Self {
            timestamp: decision.timestamp,
            signals,
        }
    }
}

fn push_symbol_signal<I, S>(
    signals: &mut Vec<DerivedSignal<DerivedSignalRecord>>,
    symbol: &Symbol,
    kind: DerivedSignalKind,
    strength: Decimal,
    summary: String,
    observed_at: OffsetDateTime,
    inputs: I,
) where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    if strength == Decimal::ZERO {
        return;
    }
    let input_vec = inputs.into_iter().map(Into::into).collect::<Vec<_>>();
    signals.push(
        DerivedSignal::new(
            DerivedSignalRecord {
                scope: SignalScope::Symbol(symbol.clone()),
                kind,
                strength,
                summary,
            },
            provenance(
                ProvenanceSource::Computed,
                observed_at,
                Some(strength.abs()),
                input_vec.clone(),
            ),
        )
        .with_derivation(input_vec),
    );
}
