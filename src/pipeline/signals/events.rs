use super::*;

fn attribution_inputs(driver: &str, scope: &str) -> Vec<String> {
    vec![
        format!("attr:driver={}", driver),
        format!("attr:scope={}", scope),
    ]
}

impl EventSnapshot {
    pub fn detect(
        history: &TickHistory,
        current_tick_number: u64,
        links: &LinkSnapshot,
        dimensions: &DimensionSnapshot,
        insights: &GraphInsights,
        decision: &DecisionSnapshot,
    ) -> Self {
        let mut events = Vec::new();

        let order_book_candidates: Vec<_> = links
            .order_books
            .iter()
            .filter_map(|order_book| {
                let imbalance = (Decimal::from(order_book.total_bid_volume)
                    - Decimal::from(order_book.total_ask_volume))
                .abs();
                let total =
                    Decimal::from(order_book.total_bid_volume + order_book.total_ask_volume);
                (total > Decimal::ZERO).then(|| (order_book, imbalance / total))
            })
            .collect();
        let order_book_cutoff =
            strict_positive_median_cutoff(order_book_candidates.iter().map(|(_, ratio)| *ratio));
        for (order_book, ratio) in order_book_candidates {
            if !exceeds_cutoff(ratio, order_book_cutoff) {
                continue;
            }
            events.push(Event::new(
                MarketEventRecord {
                    scope: SignalScope::Symbol(order_book.symbol.clone()),
                    kind: MarketEventKind::OrderBookDislocation,
                    magnitude: ratio,
                    summary: format!("{} book imbalance widened", order_book.symbol),
                },
                {
                    let mut p = provenance(
                        ProvenanceSource::Computed,
                        links.timestamp,
                        Some(ratio),
                        [format!("order_book:{}", order_book.symbol)],
                    );

                    p
                },
            ));
        }

        let volume_candidates: Vec<_> = links
            .calc_indexes
            .iter()
            .filter_map(|calc| {
                let volume_ratio = calc.volume_ratio?;
                let magnitude = volume_dislocation_magnitude(volume_ratio)?;
                Some((calc, volume_ratio, magnitude))
            })
            .collect();
        let volume_cutoff =
            strict_positive_median_cutoff(volume_candidates.iter().map(|(_, _, mag)| *mag));
        for (calc, volume_ratio, magnitude) in volume_candidates {
            if !exceeds_cutoff(magnitude, volume_cutoff) {
                continue;
            }
            events.push(Event::new(
                MarketEventRecord {
                    scope: SignalScope::Symbol(calc.symbol.clone()),
                    kind: MarketEventKind::VolumeDislocation,
                    magnitude,
                    summary: format!("{} volume ratio elevated to {}", calc.symbol, volume_ratio),
                },
                {
                    let mut p = provenance(
                        ProvenanceSource::Computed,
                        links.timestamp,
                        Some(magnitude),
                        [format!("calc_index:{}", calc.symbol)],
                    );

                    p
                },
            ));
        }

        let breakout_candidates: Vec<_> = dimensions
            .dimensions
            .iter()
            .map(|(symbol, dims)| (symbol, dims.candlestick_conviction.abs()))
            .filter(|(_, magnitude)| *magnitude > Decimal::ZERO)
            .collect();
        let breakout_cutoff =
            strict_positive_median_cutoff(breakout_candidates.iter().map(|(_, mag)| *mag));
        for (symbol, magnitude) in breakout_candidates {
            if !exceeds_cutoff(magnitude, breakout_cutoff) {
                continue;
            }
            events.push(Event::new(
                MarketEventRecord {
                    scope: SignalScope::Symbol(symbol.clone()),
                    kind: MarketEventKind::CandlestickBreakout,
                    magnitude,
                    summary: format!("{} candle conviction confirms short-term move", symbol),
                },
                {
                    let mut p = provenance(
                        ProvenanceSource::Computed,
                        links.timestamp,
                        Some(magnitude),
                        [
                            format!("dimension:candlestick_conviction:{}", symbol),
                            format!("dimension:activity_momentum:{}", symbol),
                        ],
                    );

                    p
                },
            ));
        }

        let pressure_candidates: Vec<_> = insights
            .pressures
            .iter()
            .map(|pressure| (pressure, pressure.net_pressure.abs()))
            .filter(|(_, magnitude)| *magnitude > Decimal::ZERO)
            .collect();
        let pressure_cutoff =
            strict_positive_median_cutoff(pressure_candidates.iter().map(|(_, mag)| *mag));
        for (pressure, magnitude) in pressure_candidates {
            if !exceeds_cutoff(magnitude, pressure_cutoff) {
                continue;
            }
            events.push(Event::new(
                MarketEventRecord {
                    scope: SignalScope::Symbol(pressure.symbol.clone()),
                    kind: MarketEventKind::SmartMoneyPressure,
                    magnitude,
                    summary: format!("{} smart-money pressure remains elevated", pressure.symbol),
                },
                {
                    let mut p = provenance(
                        ProvenanceSource::Computed,
                        links.timestamp,
                        Some(magnitude),
                        [format!("graph_pressure:{}", pressure.symbol)],
                    );

                    p
                },
            ));
        }

        if exceeds_cutoff(
            insights.stress.composite_stress,
            historical_market_stress_cutoff(history),
        ) {
            events.push(Event::new(
                MarketEventRecord {
                    scope: SignalScope::Market,
                    kind: MarketEventKind::MarketStressElevated,
                    magnitude: insights.stress.composite_stress,
                    summary: "market stress composite elevated".into(),
                },
                {
                    let mut p = provenance(
                        ProvenanceSource::Computed,
                        links.timestamp,
                        Some(insights.stress.composite_stress),
                        ["graph_stress".to_string()],
                    );

                    p
                },
            ));
        }

        let latest_history = previous_history_tick(history, current_tick_number);
        for (symbol, convergence) in &decision.convergence_scores {
            if let Some(previous) = latest_history.and_then(|tick| tick.signals.get(symbol)) {
                let composite_delta = convergence.composite - previous.composite;
                if composite_delta.abs() >= Decimal::new(15, 2) {
                    events.push(Event::new(
                        MarketEventRecord {
                            scope: SignalScope::Symbol(symbol.clone()),
                            kind: MarketEventKind::CompositeAcceleration,
                            magnitude: composite_delta.abs(),
                            summary: format!(
                                "{} composite moved by {:+} since previous tick",
                                symbol,
                                composite_delta.round_dp(3)
                            ),
                        },
                        {
                            let mut p = provenance(
                                ProvenanceSource::Computed,
                                links.timestamp,
                                Some(composite_delta.abs()),
                                [
                                    format!("history:previous_tick:{}", symbol),
                                    format!("convergence:{}", symbol),
                                ],
                            );

                            p
                        },
                    ));
                }

                let prev_inst = previous.institutional_alignment;
                let curr_inst = convergence.institutional_alignment;
                if prev_inst != Decimal::ZERO
                    && curr_inst != Decimal::ZERO
                    && prev_inst.signum() != curr_inst.signum()
                {
                    events.push(Event::new(
                        MarketEventRecord {
                            scope: SignalScope::Symbol(symbol.clone()),
                            kind: MarketEventKind::InstitutionalFlip,
                            magnitude: (curr_inst - prev_inst).abs(),
                            summary: format!(
                                "{} institutional alignment flipped from {:+} to {:+}",
                                symbol,
                                prev_inst.round_dp(2),
                                curr_inst.round_dp(2)
                            ),
                        },
                        {
                            let mut p = provenance(
                                ProvenanceSource::Computed,
                                links.timestamp,
                                Some((curr_inst - prev_inst).abs()),
                                [
                                    format!("history:previous_tick:{}", symbol),
                                    format!("institutional_alignment:{}", symbol),
                                ],
                            );

                            p
                        },
                    ));
                }
            }
        }

        if let Some(previous_market_stress) = latest_history.and_then(previous_market_stress) {
            let stress_delta = insights.stress.composite_stress - previous_market_stress;
            if stress_delta.abs() >= Decimal::new(20, 2) {
                events.push(Event::new(
                    MarketEventRecord {
                        scope: SignalScope::Market,
                        kind: MarketEventKind::StressRegimeShift,
                        magnitude: stress_delta.abs(),
                        summary: format!("market stress shifted by {:+}", stress_delta.round_dp(3)),
                    },
                    {
                        let mut p = provenance(
                            ProvenanceSource::Computed,
                            links.timestamp,
                            Some(stress_delta.abs()),
                            [
                                "history:market_stress".to_string(),
                                "graph_stress".to_string(),
                            ],
                        );
    
                        p
                    },
                ));
            }
        }

        for suggestion in &decision.order_suggestions {
            if suggestion.requires_confirmation {
                events.push(Event::new(
                    MarketEventRecord {
                        scope: SignalScope::Symbol(suggestion.symbol.clone()),
                        kind: MarketEventKind::ManualReviewRequired,
                        magnitude: suggestion.convergence.composite.abs(),
                        summary: format!("{} order suggestion requires review", suggestion.symbol),
                    },
                    {
                        let mut p = provenance(
                            ProvenanceSource::Computed,
                            links.timestamp,
                            Some(suggestion.convergence.composite.abs()),
                            [
                                format!("decision:{}", suggestion.symbol),
                                format!("convergence:{}", suggestion.symbol),
                            ],
                        );
    
                        p
                    },
                ));
            }
        }

        for shared in insights.shared_holders.iter().take(5) {
            events.push(Event::new(
                MarketEventRecord {
                    scope: SignalScope::Market,
                    kind: MarketEventKind::SharedHolderAnomaly,
                    magnitude: shared.jaccard,
                    summary: format!(
                        "{} and {} share unusual holder overlap",
                        shared.symbol_a, shared.symbol_b
                    ),
                },
                {
                    let mut p = provenance(
                        ProvenanceSource::Computed,
                        links.timestamp,
                        Some(shared.jaccard),
                        [
                            format!("shared_holder:{}", shared.symbol_a),
                            format!("shared_holder:{}", shared.symbol_b),
                        ],
                    );
                    p
                },
            ));
        }

        // Inject attribution provenance from the static EVENT_ATTRIBUTION table
        // instead of hardcoding per call site.
        for event in &mut events {
            let attr = super::types::attribution_inputs_for_kind(&event.value.kind);
            if !attr.is_empty() {
                // Remove any existing attr: entries (from inline calls) and replace with table lookup
                event
                    .provenance
                    .inputs
                    .retain(|i| !i.starts_with("attr:"));
                event.provenance.inputs.extend(attr.iter().map(|s| s.to_string()));
            }
        }

        Self {
            timestamp: links.timestamp,
            events,
        }
    }
}

fn previous_history_tick(
    history: &TickHistory,
    current_tick_number: u64,
) -> Option<&crate::temporal::record::TickRecord> {
    history
        .latest_n(history.len())
        .into_iter()
        .rev()
        .find(|tick| tick.tick_number < current_tick_number)
}

fn strict_positive_median_cutoff<I>(values: I) -> Option<Decimal>
where
    I: IntoIterator<Item = Decimal>,
{
    median(
        values
            .into_iter()
            .filter(|value| *value > Decimal::ZERO)
            .collect(),
    )
}

fn exceeds_cutoff(value: Decimal, cutoff: Option<Decimal>) -> bool {
    cutoff.map(|cutoff| value > cutoff).unwrap_or(false)
}

fn volume_dislocation_magnitude(volume_ratio: Decimal) -> Option<Decimal> {
    (volume_ratio > Decimal::ONE)
        .then_some(normalized_ratio(volume_ratio, Decimal::ONE))
        .filter(|magnitude| *magnitude > Decimal::ZERO)
}

fn historical_market_stress_cutoff(history: &TickHistory) -> Option<Decimal> {
    strict_positive_median_cutoff(
        history
            .latest_n(history.len())
            .into_iter()
            .filter_map(previous_market_stress),
    )
}

fn previous_market_stress(tick: &crate::temporal::record::TickRecord) -> Option<Decimal> {
    tick.derived_signals.iter().find_map(|signal| {
        if matches!(signal.value.scope, SignalScope::Market)
            && matches!(signal.value.kind, DerivedSignalKind::MarketStress)
        {
            Some(signal.value.strength)
        } else {
            None
        }
    })
}

pub fn enrich_attribution_with_evidence(
    snapshot: &mut EventSnapshot,
    cross_stock_presences: &[crate::ontology::links::CrossStockPresence],
    _macro_events: &[crate::ontology::knowledge::AgentMacroEvent],
) {
    let cross_stock_symbols: std::collections::HashSet<crate::ontology::objects::Symbol> =
        cross_stock_presences
            .iter()
            .flat_map(|p| p.symbols.iter().cloned())
            .collect();

    for event in &mut snapshot.events {
        let event_symbol = match &event.value.scope {
            SignalScope::Symbol(s) => Some(s),
            _ => None,
        };
        let Some(symbol) = event_symbol else {
            continue;
        };
        if !cross_stock_symbols.contains(symbol) {
            continue;
        }
        // Symbol appears in cross-stock presence → upgrade from local to sector
        let has_local_scope = event
            .provenance
            .inputs
            .iter()
            .any(|i| i == "attr:scope=local");
        if has_local_scope {
            event.provenance.inputs.retain(|i| {
                !i.starts_with("attr:scope=") && !i.starts_with("attr:driver=")
            });
            event
                .provenance
                .inputs
                .extend(super::types::attribution_inputs_for_kind(
                    &MarketEventKind::SharedHolderAnomaly,
                ).iter().map(|s| s.to_string()));
        }
    }
}

pub fn detect_propagation_absences(
    snapshot: &mut EventSnapshot,
    dimensions: &crate::pipeline::dimensions::DimensionSnapshot,
    sector_map: &std::collections::HashMap<
        crate::ontology::objects::Symbol,
        crate::ontology::objects::SectorId,
    >,
) {
    use std::collections::HashMap;

    // Group existing event symbols by sector
    let mut sectors_with_events: HashMap<
        crate::ontology::objects::SectorId,
        Vec<crate::ontology::objects::Symbol>,
    > = HashMap::new();
    for event in &snapshot.events {
        if let SignalScope::Symbol(symbol) = &event.value.scope {
            if let Some(sector) = sector_map.get(symbol) {
                sectors_with_events
                    .entry(sector.clone())
                    .or_default()
                    .push(symbol.clone());
            }
        }
    }

    // For each sector with events, check if peers are silent
    for (sector, _event_symbols) in &sectors_with_events {
        let sector_symbols: Vec<&crate::ontology::objects::Symbol> = sector_map
            .iter()
            .filter(|(_, s)| *s == sector)
            .map(|(sym, _)| sym)
            .collect();
        if sector_symbols.len() < 2 {
            continue;
        }
        let silent_count = sector_symbols
            .iter()
            .filter(|sym| {
                dimensions
                    .dimensions
                    .get(*sym)
                    .map(|d| d.activity_momentum == Decimal::ZERO)
                    .unwrap_or(true)
            })
            .count();
        let silent_ratio =
            Decimal::from(silent_count as i64) / Decimal::from(sector_symbols.len() as i64);
        if silent_ratio > Decimal::new(5, 1) {
            snapshot.events.push(Event::new(
                MarketEventRecord {
                    scope: SignalScope::Sector(sector.clone()),
                    kind: MarketEventKind::PropagationAbsence,
                    magnitude: silent_ratio,
                    summary: format!(
                        "sector {} has events but {:.0}% of peers are silent",
                        sector,
                        silent_ratio * Decimal::from(100)
                    ),
                },
                provenance(
                    ProvenanceSource::Computed,
                    snapshot.timestamp,
                    Some(silent_ratio),
                    [format!("sector_absence:{}", sector)],
                ),
            ));
        }
    }
}

pub fn catalyst_events_from_macro_events(
    macro_events: &[crate::ontology::knowledge::AgentMacroEvent],
    timestamp: OffsetDateTime,
) -> Vec<Event<MarketEventRecord>> {
    macro_events
        .iter()
        .filter(|e| is_thematic_catalyst(&e.event_type))
        .map(|e| {
            let magnitude = e.confidence.clamp(Decimal::ZERO, Decimal::ONE);
            Event::new(
                MarketEventRecord {
                    scope: SignalScope::Market,
                    kind: MarketEventKind::CatalystActivation,
                    magnitude,
                    summary: e.headline.clone(),
                },
                {
                    let mut p = provenance(
                        ProvenanceSource::Computed,
                        timestamp,
                        Some(magnitude),
                        [format!("macro_event:{}", e.event_id)],
                    );
                    p
                },
            )
        })
        .collect()
}

fn is_thematic_catalyst(event_type: &str) -> bool {
    matches!(
        event_type,
        "thematic_catalyst"
            | "sector_catalyst"
            | "policy_catalyst"
            | "earnings_catalyst"
            | "macro_catalyst"
    )
}

pub fn broker_events_from_delta(
    delta: &crate::graph::temporal::BrokerTemporalDelta,
    timestamp: OffsetDateTime,
) -> Vec<Event<MarketEventRecord>> {
    use crate::graph::temporal::BrokerTransitionKind;

    let mut events = Vec::new();
    let mut institution_appeared: std::collections::HashMap<Option<i32>, Vec<String>> =
        std::collections::HashMap::new();

    for t in &delta.transitions {
        let symbol_str = t.broker_symbol_id.symbol.to_string();
        let broker_id = t.broker_symbol_id.broker_id.0;
        let inst_id = t.institution_id.map(|i| i.0);

        match &t.kind {
            BrokerTransitionKind::Replenished => {
                let magnitude = t.iceberg_confidence.unwrap_or(Decimal::new(5, 2));
                let replenish_count = t.replenish_count.unwrap_or(1);
                let interval = t
                    .replenish_interval
                    .map(|value| format!("Δ{}t", value))
                    .unwrap_or_else(|| "Δ--".into());
                let position_consistency = t
                    .replenish_position_consistency
                    .map(|value| format!("pos_consistency={}", value.round_dp(2)))
                    .unwrap_or_else(|| "pos_consistency=--".into());
                let depth_recovery = t
                    .depth_recovery_ratio
                    .map(|value| format!("depth_recovery={}", value.round_dp(2)))
                    .unwrap_or_else(|| "depth_recovery=--".into());
                events.push(Event::new(
                    MarketEventRecord {
                        scope: SignalScope::Symbol(t.broker_symbol_id.symbol.clone()),
                        kind: MarketEventKind::IcebergDetected,
                        magnitude,
                        summary: format!(
                            "B{} replenished in {} ({:?} pos {}) count={} {} {} {}",
                            broker_id,
                            symbol_str,
                            t.side,
                            t.position,
                            replenish_count,
                            interval,
                            position_consistency,
                            depth_recovery,
                        ),
                    },
                    {
                        let mut p = ProvenanceMetadata::new(ProvenanceSource::Computed, timestamp);
                        p.inputs = attribution_inputs("company_specific", "local");
                        p
                    },
                ));
            }
            BrokerTransitionKind::SideFlipped => {
                events.push(Event::new(
                    MarketEventRecord {
                        scope: SignalScope::Symbol(t.broker_symbol_id.symbol.clone()),
                        kind: MarketEventKind::BrokerSideFlip,
                        magnitude: Decimal::new(3, 1),
                        summary: format!("B{} flipped side in {}", broker_id, symbol_str),
                    },
                    {
                        let mut p = ProvenanceMetadata::new(ProvenanceSource::Computed, timestamp);
                        p.inputs = attribution_inputs("company_specific", "local");
                        p
                    },
                ));
            }
            BrokerTransitionKind::Appeared => {
                if let Some(iid) = inst_id {
                    institution_appeared
                        .entry(Some(iid))
                        .or_default()
                        .push(symbol_str.clone());
                }
            }
            _ => {}
        }
    }

    for (inst_id, symbols) in &institution_appeared {
        if symbols.len() >= 3 {
            if let Some(iid) = inst_id {
                events.push(Event::new(
                    MarketEventRecord {
                        scope: SignalScope::Institution(InstitutionId(*iid)),
                        kind: MarketEventKind::BrokerClusterFormation,
                        magnitude: Decimal::from(symbols.len() as i64) / Decimal::from(10),
                        summary: format!(
                            "I{} deployed {} brokers simultaneously",
                            iid,
                            symbols.len()
                        ),
                    },
                    {
                        let mut p = ProvenanceMetadata::new(ProvenanceSource::Computed, timestamp);
                        p.inputs = attribution_inputs("company_specific", "local");
                        p
                    },
                ));
            }
        }
    }

    // Inject attribution from table (same pattern as detect)
    for event in &mut events {
        let attr = super::types::attribution_inputs_for_kind(&event.value.kind);
        if !attr.is_empty() {
            event
                .provenance
                .inputs
                .retain(|i| !i.starts_with("attr:"));
            event.provenance.inputs.extend(attr.iter().map(|s| s.to_string()));
        }
    }

    events
}
