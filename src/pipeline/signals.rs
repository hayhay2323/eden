use std::collections::HashMap;

use rust_decimal::prelude::Signed;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::graph::decision::DecisionSnapshot;
use crate::graph::insights::GraphInsights;
use crate::math::{median, normalized_ratio};
use crate::ontology::domain::{
    DerivedSignal, Event, Observation, ProvenanceMetadata, ProvenanceSource,
};
use crate::ontology::links::LinkSnapshot;
use crate::ontology::objects::Symbol;
use crate::temporal::buffer::TickHistory;

use super::dimensions::{DimensionSnapshot, SymbolDimensions};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SignalScope {
    Market,
    Symbol(Symbol),
    Institution(String),
    Sector(String),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ObservationRecord {
    Quote {
        symbol: Symbol,
        last_done: Decimal,
        turnover: Decimal,
        market_status: String,
    },
    OrderBook {
        symbol: Symbol,
        total_bid_volume: i64,
        total_ask_volume: i64,
        spread: Option<Decimal>,
    },
    CapitalFlow {
        symbol: Symbol,
        net_inflow: Decimal,
    },
    CapitalBreakdown {
        symbol: Symbol,
        large_net: Decimal,
        medium_net: Decimal,
        small_net: Decimal,
    },
    CalcIndex {
        symbol: Symbol,
        turnover_rate: Option<Decimal>,
        volume_ratio: Option<Decimal>,
        pe_ttm_ratio: Option<Decimal>,
        pb_ratio: Option<Decimal>,
        dividend_ratio_ttm: Option<Decimal>,
        amplitude: Option<Decimal>,
        five_minutes_change_rate: Option<Decimal>,
    },
    Candlestick {
        symbol: Symbol,
        candle_count: usize,
        window_return: Decimal,
        body_bias: Decimal,
        volume_ratio: Decimal,
        range_ratio: Decimal,
    },
    InstitutionActivity {
        symbol: Symbol,
        institution_id: String,
        seat_count: usize,
    },
    TradeActivity {
        symbol: Symbol,
        trade_count: usize,
        total_volume: i64,
        buy_volume: i64,
        sell_volume: i64,
        vwap: Decimal,
    },
    MarketTemperature {
        temperature: Decimal,
        valuation: Decimal,
        sentiment: Decimal,
        description: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MarketEventKind {
    OrderBookDislocation,
    VolumeDislocation,
    SmartMoneyPressure,
    CandlestickBreakout,
    CompositeAcceleration,
    InstitutionalFlip,
    MarketStressElevated,
    StressRegimeShift,
    ManualReviewRequired,
    SharedHolderAnomaly,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketEventRecord {
    pub scope: SignalScope,
    pub kind: MarketEventKind,
    pub magnitude: Decimal,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DerivedSignalKind {
    StructuralComposite,
    Convergence,
    ValuationSupport,
    ActivityMomentum,
    CandlestickConviction,
    SmartMoneyPressure,
    MarketStress,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivedSignalRecord {
    pub scope: SignalScope,
    pub kind: DerivedSignalKind,
    pub strength: Decimal,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationSnapshot {
    pub timestamp: OffsetDateTime,
    pub observations: Vec<Observation<ObservationRecord>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventSnapshot {
    pub timestamp: OffsetDateTime,
    pub events: Vec<Event<MarketEventRecord>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DerivedSignalSnapshot {
    pub timestamp: OffsetDateTime,
    pub signals: Vec<DerivedSignal<DerivedSignalRecord>>,
}

impl ObservationSnapshot {
    pub fn from_links(links: &LinkSnapshot) -> Self {
        let mut observations = Vec::new();

        for quote in &links.quotes {
            observations.push(Observation::new(
                ObservationRecord::Quote {
                    symbol: quote.symbol.clone(),
                    last_done: quote.last_done,
                    turnover: quote.turnover,
                    market_status: format!("{:?}", quote.market_status),
                },
                provenance(
                    ProvenanceSource::WebSocket,
                    quote.timestamp,
                    Some(confidence_from_turnover(quote.turnover)),
                    [format!("quote:{}", quote.symbol)],
                ),
            ));
        }

        for order_book in &links.order_books {
            observations.push(Observation::new(
                ObservationRecord::OrderBook {
                    symbol: order_book.symbol.clone(),
                    total_bid_volume: order_book.total_bid_volume,
                    total_ask_volume: order_book.total_ask_volume,
                    spread: order_book.spread,
                },
                provenance(
                    ProvenanceSource::WebSocket,
                    links.timestamp,
                    Some(Decimal::ONE),
                    [format!("depth:{}", order_book.symbol)],
                ),
            ));
        }

        for capital_flow in &links.capital_flows {
            observations.push(Observation::new(
                ObservationRecord::CapitalFlow {
                    symbol: capital_flow.symbol.clone(),
                    net_inflow: capital_flow.net_inflow.as_yuan(),
                },
                provenance(
                    ProvenanceSource::Api,
                    links.timestamp,
                    Some(confidence_from_magnitude(capital_flow.net_inflow.as_yuan())),
                    [format!("capital_flow:{}", capital_flow.symbol)],
                ),
            ));
        }

        for breakdown in &links.capital_breakdowns {
            observations.push(Observation::new(
                ObservationRecord::CapitalBreakdown {
                    symbol: breakdown.symbol.clone(),
                    large_net: breakdown.large_net,
                    medium_net: breakdown.medium_net,
                    small_net: breakdown.small_net,
                },
                provenance(
                    ProvenanceSource::Api,
                    links.timestamp,
                    Some(confidence_from_magnitude(breakdown.large_net)),
                    [format!("capital_breakdown:{}", breakdown.symbol)],
                ),
            ));
        }

        for calc in &links.calc_indexes {
            observations.push(Observation::new(
                ObservationRecord::CalcIndex {
                    symbol: calc.symbol.clone(),
                    turnover_rate: calc.turnover_rate,
                    volume_ratio: calc.volume_ratio,
                    pe_ttm_ratio: calc.pe_ttm_ratio,
                    pb_ratio: calc.pb_ratio,
                    dividend_ratio_ttm: calc.dividend_ratio_ttm,
                    amplitude: calc.amplitude,
                    five_minutes_change_rate: calc.five_minutes_change_rate,
                },
                provenance(
                    ProvenanceSource::Api,
                    links.timestamp,
                    Some(
                        calc.volume_ratio
                            .unwrap_or(Decimal::ONE)
                            .min(Decimal::new(3, 0))
                            / Decimal::new(3, 0),
                    ),
                    [format!("calc_index:{}", calc.symbol)],
                ),
            ));
        }

        for candle in &links.candlesticks {
            observations.push(Observation::new(
                ObservationRecord::Candlestick {
                    symbol: candle.symbol.clone(),
                    candle_count: candle.candle_count,
                    window_return: candle.window_return,
                    body_bias: candle.body_bias,
                    volume_ratio: candle.volume_ratio,
                    range_ratio: candle.range_ratio,
                },
                provenance(
                    ProvenanceSource::Computed,
                    links.timestamp,
                    Some(Decimal::from(candle.candle_count.min(5) as i64) / Decimal::new(5, 0)),
                    [format!("candlestick:{}", candle.symbol)],
                ),
            ));
        }

        for activity in &links.institution_activities {
            observations.push(Observation::new(
                ObservationRecord::InstitutionActivity {
                    symbol: activity.symbol.clone(),
                    institution_id: activity.institution_id.to_string(),
                    seat_count: activity.seat_count,
                },
                provenance(
                    ProvenanceSource::Computed,
                    links.timestamp,
                    Some(Decimal::ONE),
                    [
                        format!("broker_queue:{}", activity.symbol),
                        format!("institution:{}", activity.institution_id),
                    ],
                ),
            ));
        }

        for trade in &links.trade_activities {
            observations.push(Observation::new(
                ObservationRecord::TradeActivity {
                    symbol: trade.symbol.clone(),
                    trade_count: trade.trade_count,
                    total_volume: trade.total_volume,
                    buy_volume: trade.buy_volume,
                    sell_volume: trade.sell_volume,
                    vwap: trade.vwap,
                },
                provenance(
                    ProvenanceSource::WebSocket,
                    links.timestamp,
                    Some(Decimal::ONE),
                    [format!("trade_activity:{}", trade.symbol)],
                ),
            ));
        }

        if let Some(temp) = &links.market_temperature {
            observations.push(Observation::new(
                ObservationRecord::MarketTemperature {
                    temperature: temp.temperature,
                    valuation: temp.valuation,
                    sentiment: temp.sentiment,
                    description: temp.description.clone(),
                },
                provenance(
                    ProvenanceSource::Api,
                    temp.timestamp,
                    Some(Decimal::ONE),
                    ["market_temperature:HK".to_string()],
                ),
            ));
        }

        Self {
            timestamp: links.timestamp,
            observations,
        }
    }
}

impl EventSnapshot {
    pub fn detect(
        history: &TickHistory,
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
                (total > Decimal::ZERO).then_some((order_book, imbalance / total))
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
                provenance(
                    ProvenanceSource::Computed,
                    links.timestamp,
                    Some(ratio),
                    [format!("order_book:{}", order_book.symbol)],
                ),
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
                provenance(
                    ProvenanceSource::Computed,
                    links.timestamp,
                    Some(magnitude),
                    [format!("calc_index:{}", calc.symbol)],
                ),
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
                provenance(
                    ProvenanceSource::Computed,
                    links.timestamp,
                    Some(magnitude),
                    [
                        format!("dimension:candlestick_conviction:{}", symbol),
                        format!("dimension:activity_momentum:{}", symbol),
                    ],
                ),
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
                provenance(
                    ProvenanceSource::Computed,
                    links.timestamp,
                    Some(magnitude),
                    [format!("graph_pressure:{}", pressure.symbol)],
                ),
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
                provenance(
                    ProvenanceSource::Computed,
                    links.timestamp,
                    Some(insights.stress.composite_stress),
                    ["graph_stress".to_string()],
                ),
            ));
        }

        let latest_history = history.latest();
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
                        provenance(
                            ProvenanceSource::Computed,
                            links.timestamp,
                            Some(composite_delta.abs()),
                            [
                                format!("history:previous_tick:{}", symbol),
                                format!("convergence:{}", symbol),
                            ],
                        ),
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
                        provenance(
                            ProvenanceSource::Computed,
                            links.timestamp,
                            Some((curr_inst - prev_inst).abs()),
                            [
                                format!("history:previous_tick:{}", symbol),
                                format!("institutional_alignment:{}", symbol),
                            ],
                        ),
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
                    provenance(
                        ProvenanceSource::Computed,
                        links.timestamp,
                        Some(stress_delta.abs()),
                        [
                            "history:market_stress".to_string(),
                            "graph_stress".to_string(),
                        ],
                    ),
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
                    provenance(
                        ProvenanceSource::Computed,
                        links.timestamp,
                        Some(suggestion.convergence.composite.abs()),
                        [
                            format!("decision:{}", suggestion.symbol),
                            format!("convergence:{}", suggestion.symbol),
                        ],
                    ),
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
                provenance(
                    ProvenanceSource::Computed,
                    links.timestamp,
                    Some(shared.jaccard),
                    [
                        format!("shared_holder:{}", shared.symbol_a),
                        format!("shared_holder:{}", shared.symbol_b),
                    ],
                ),
            ));
        }

        Self {
            timestamp: links.timestamp,
            events,
        }
    }
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

fn provenance<I, S>(
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

fn confidence_from_turnover(turnover: Decimal) -> Decimal {
    if turnover <= Decimal::ZERO {
        Decimal::ZERO
    } else {
        (turnover / Decimal::new(1_000_000, 0)).min(Decimal::ONE)
    }
}

fn confidence_from_magnitude(value: Decimal) -> Decimal {
    let magnitude = value.abs();
    if magnitude == Decimal::ZERO {
        Decimal::ZERO
    } else {
        (magnitude / Decimal::new(1_000_000, 0)).min(Decimal::ONE)
    }
}

fn average_dimension_strength(dims: &SymbolDimensions) -> Decimal {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::decision::{ConvergenceScore, MarketRegimeFilter};
    use crate::graph::insights::MarketStressIndex;
    use crate::graph::insights::StockPressure;
    use crate::ontology::links::{
        CalcIndexObservation, DepthLevel, DepthProfile, LinkSnapshot, MarketStatus,
        MarketTemperatureObservation, OrderBookObservation, QuoteObservation,
    };
    use crate::ontology::world::{BackwardReasoningSnapshot, WorldStateSnapshot};
    use crate::temporal::record::{SymbolSignals, TickRecord};
    use rust_decimal_macros::dec;

    fn sym(value: &str) -> Symbol {
        Symbol(value.into())
    }

    fn empty_history() -> TickHistory {
        TickHistory::new(10)
    }

    fn history_tick(
        symbol: &str,
        composite: Decimal,
        institutional_alignment: Decimal,
        market_stress: Decimal,
    ) -> TickRecord {
        let mut signals = HashMap::new();
        signals.insert(
            sym(symbol),
            SymbolSignals {
                mark_price: None,
                composite,
                institutional_alignment,
                sector_coherence: None,
                cross_stock_correlation: Decimal::ZERO,
                order_book_pressure: Decimal::ZERO,
                capital_flow_direction: Decimal::ZERO,
                capital_size_divergence: Decimal::ZERO,
                institutional_direction: Decimal::ZERO,
                depth_structure_imbalance: Decimal::ZERO,
                bid_top3_ratio: Decimal::ZERO,
                ask_top3_ratio: Decimal::ZERO,
                bid_best_ratio: Decimal::ZERO,
                ask_best_ratio: Decimal::ZERO,
                spread: None,
                trade_count: 0,
                trade_volume: 0,
                buy_volume: 0,
                sell_volume: 0,
                vwap: None,
                convergence_score: None,
                composite_degradation: None,
                institution_retention: None,
            },
        );
        TickRecord {
            tick_number: 1,
            timestamp: OffsetDateTime::UNIX_EPOCH,
            signals,
            observations: vec![],
            events: vec![],
            derived_signals: vec![DerivedSignal::new(
                DerivedSignalRecord {
                    scope: SignalScope::Market,
                    kind: DerivedSignalKind::MarketStress,
                    strength: market_stress,
                    summary: "prev stress".into(),
                },
                ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH),
            )],
            action_workflows: vec![],
            polymarket_priors: vec![],
            hypotheses: vec![],
            propagation_paths: vec![],
            tactical_setups: vec![],
            hypothesis_tracks: vec![],
            case_clusters: vec![],
            world_state: WorldStateSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                entities: vec![],
            },
            backward_reasoning: BackwardReasoningSnapshot {
                timestamp: OffsetDateTime::UNIX_EPOCH,
                investigations: vec![],
            },
        }
    }

    fn order_book(
        symbol: &str,
        total_bid_volume: i64,
        total_ask_volume: i64,
    ) -> OrderBookObservation {
        OrderBookObservation {
            symbol: sym(symbol),
            ask_levels: vec![DepthLevel {
                position: 1,
                price: Some(dec!(10.1)),
                volume: total_ask_volume,
                order_num: 1,
            }],
            bid_levels: vec![DepthLevel {
                position: 1,
                price: Some(dec!(10.0)),
                volume: total_bid_volume,
                order_num: 1,
            }],
            total_ask_volume,
            total_bid_volume,
            total_ask_orders: 1,
            total_bid_orders: 1,
            spread: Some(dec!(0.1)),
            ask_level_count: 1,
            bid_level_count: 1,
            bid_profile: DepthProfile::empty(),
            ask_profile: DepthProfile::empty(),
        }
    }

    #[test]
    fn provenance_includes_confidence_and_inputs() {
        let provenance = provenance(
            ProvenanceSource::Computed,
            OffsetDateTime::UNIX_EPOCH,
            Some(dec!(0.6)),
            ["a", "b"],
        );
        assert_eq!(provenance.confidence, Some(dec!(0.6)));
        assert_eq!(provenance.inputs.len(), 2);
    }

    #[test]
    fn observation_snapshot_collects_market_temperature() {
        let links = LinkSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            broker_queues: vec![],
            calc_indexes: vec![],
            candlesticks: vec![],
            institution_activities: vec![],
            cross_stock_presences: vec![],
            capital_flows: vec![],
            capital_breakdowns: vec![],
            market_temperature: Some(MarketTemperatureObservation {
                temperature: dec!(70),
                valuation: dec!(60),
                sentiment: dec!(80),
                description: "warm".into(),
                timestamp: OffsetDateTime::UNIX_EPOCH,
            }),
            order_books: vec![],
            quotes: vec![QuoteObservation {
                symbol: sym("700.HK"),
                last_done: dec!(350),
                prev_close: dec!(348),
                open: dec!(349),
                high: dec!(352),
                low: dec!(347),
                volume: 100,
                turnover: dec!(35000),
                timestamp: OffsetDateTime::UNIX_EPOCH,
                market_status: MarketStatus::Normal,
            }],
            trade_activities: vec![],
        };

        let snapshot = ObservationSnapshot::from_links(&links);
        assert!(snapshot.observations.len() >= 2);
    }

    #[test]
    fn derived_signal_snapshot_emits_market_stress() {
        let dimensions = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::from([(
                sym("700.HK"),
                SymbolDimensions {
                    valuation_support: dec!(0.4),
                    activity_momentum: dec!(0.5),
                    candlestick_conviction: dec!(0.6),
                    ..Default::default()
                },
            )]),
        };
        let insights = GraphInsights {
            pressures: vec![StockPressure {
                symbol: sym("700.HK"),
                net_pressure: dec!(0.7),
                institution_count: 1,
                buy_inst_count: 1,
                sell_inst_count: 0,
                pressure_delta: dec!(0.1),
                pressure_duration: 1,
                accelerating: true,
            }],
            rotations: vec![],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: MarketStressIndex {
                sector_synchrony: dec!(0.4),
                pressure_consensus: dec!(0.5),
                conflict_intensity_mean: dec!(0.2),
                market_temperature_stress: dec!(0.8),
                composite_stress: dec!(0.6),
            },
            institution_stock_counts: HashMap::new(),
        };
        let decision = DecisionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            convergence_scores: HashMap::from([(
                sym("700.HK"),
                ConvergenceScore {
                    symbol: sym("700.HK"),
                    institutional_alignment: dec!(0.4),
                    sector_coherence: Some(dec!(0.3)),
                    cross_stock_correlation: dec!(0.2),
                    composite: dec!(0.5),
                },
            )]),
            market_regime: MarketRegimeFilter::neutral(),
            order_suggestions: vec![],
            degradations: HashMap::new(),
        };
        let events = EventSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            events: vec![],
        };

        let snapshot = DerivedSignalSnapshot::compute(&dimensions, &insights, &decision, &events);
        assert!(snapshot
            .signals
            .iter()
            .any(|signal| matches!(signal.value.kind, DerivedSignalKind::MarketStress)));
    }

    #[test]
    fn event_snapshot_detects_temporal_transitions() {
        let mut history = empty_history();
        history.push(history_tick("700.HK", dec!(0.1), dec!(-0.4), dec!(0.1)));

        let links = LinkSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            broker_queues: vec![],
            calc_indexes: vec![],
            candlesticks: vec![],
            institution_activities: vec![],
            cross_stock_presences: vec![],
            capital_flows: vec![],
            capital_breakdowns: vec![],
            market_temperature: None,
            order_books: vec![],
            quotes: vec![],
            trade_activities: vec![],
        };
        let dimensions = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::new(),
        };
        let insights = GraphInsights {
            pressures: vec![],
            rotations: vec![],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: MarketStressIndex {
                sector_synchrony: dec!(0.1),
                pressure_consensus: dec!(0.2),
                conflict_intensity_mean: dec!(0.1),
                market_temperature_stress: dec!(0.3),
                composite_stress: dec!(0.4),
            },
            institution_stock_counts: HashMap::new(),
        };
        let decision = DecisionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            convergence_scores: HashMap::from([(
                sym("700.HK"),
                ConvergenceScore {
                    symbol: sym("700.HK"),
                    institutional_alignment: dec!(0.5),
                    sector_coherence: Some(dec!(0.2)),
                    cross_stock_correlation: dec!(0.1),
                    composite: dec!(0.35),
                },
            )]),
            market_regime: MarketRegimeFilter::neutral(),
            order_suggestions: vec![],
            degradations: HashMap::new(),
        };

        let snapshot = EventSnapshot::detect(&history, &links, &dimensions, &insights, &decision);
        assert!(snapshot
            .events
            .iter()
            .any(|event| matches!(event.value.kind, MarketEventKind::CompositeAcceleration)));
        assert!(snapshot
            .events
            .iter()
            .any(|event| matches!(event.value.kind, MarketEventKind::InstitutionalFlip)));
        assert!(snapshot
            .events
            .iter()
            .any(|event| matches!(event.value.kind, MarketEventKind::StressRegimeShift)));
    }

    #[test]
    fn event_snapshot_uses_sample_derived_cutoffs() {
        let mut history = empty_history();
        history.push(history_tick(
            "700.HK",
            Decimal::ZERO,
            Decimal::ZERO,
            dec!(0.2),
        ));
        history.push(history_tick(
            "700.HK",
            Decimal::ZERO,
            Decimal::ZERO,
            dec!(0.4),
        ));
        history.push(history_tick(
            "700.HK",
            Decimal::ZERO,
            Decimal::ZERO,
            dec!(0.6),
        ));

        let links = LinkSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            broker_queues: vec![],
            calc_indexes: vec![
                CalcIndexObservation {
                    symbol: sym("A.HK"),
                    turnover_rate: None,
                    volume_ratio: Some(dec!(1.2)),
                    pe_ttm_ratio: None,
                    pb_ratio: None,
                    dividend_ratio_ttm: None,
                    amplitude: None,
                    five_minutes_change_rate: None,
                },
                CalcIndexObservation {
                    symbol: sym("B.HK"),
                    turnover_rate: None,
                    volume_ratio: Some(dec!(1.5)),
                    pe_ttm_ratio: None,
                    pb_ratio: None,
                    dividend_ratio_ttm: None,
                    amplitude: None,
                    five_minutes_change_rate: None,
                },
                CalcIndexObservation {
                    symbol: sym("C.HK"),
                    turnover_rate: None,
                    volume_ratio: Some(dec!(4)),
                    pe_ttm_ratio: None,
                    pb_ratio: None,
                    dividend_ratio_ttm: None,
                    amplitude: None,
                    five_minutes_change_rate: None,
                },
            ],
            candlesticks: vec![],
            institution_activities: vec![],
            cross_stock_presences: vec![],
            capital_flows: vec![],
            capital_breakdowns: vec![],
            market_temperature: None,
            order_books: vec![
                order_book("A.HK", 55, 45),
                order_book("B.HK", 65, 35),
                order_book("C.HK", 95, 5),
            ],
            quotes: vec![],
            trade_activities: vec![],
        };
        let dimensions = DimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::from([
                (
                    sym("A.HK"),
                    SymbolDimensions {
                        candlestick_conviction: dec!(0.2),
                        ..Default::default()
                    },
                ),
                (
                    sym("B.HK"),
                    SymbolDimensions {
                        candlestick_conviction: dec!(0.4),
                        ..Default::default()
                    },
                ),
                (
                    sym("C.HK"),
                    SymbolDimensions {
                        candlestick_conviction: dec!(0.8),
                        ..Default::default()
                    },
                ),
            ]),
        };
        let insights = GraphInsights {
            pressures: vec![
                StockPressure {
                    symbol: sym("A.HK"),
                    net_pressure: dec!(0.1),
                    institution_count: 1,
                    buy_inst_count: 1,
                    sell_inst_count: 0,
                    pressure_delta: Decimal::ZERO,
                    pressure_duration: 1,
                    accelerating: false,
                },
                StockPressure {
                    symbol: sym("B.HK"),
                    net_pressure: dec!(0.4),
                    institution_count: 1,
                    buy_inst_count: 1,
                    sell_inst_count: 0,
                    pressure_delta: Decimal::ZERO,
                    pressure_duration: 1,
                    accelerating: false,
                },
                StockPressure {
                    symbol: sym("C.HK"),
                    net_pressure: dec!(0.7),
                    institution_count: 1,
                    buy_inst_count: 1,
                    sell_inst_count: 0,
                    pressure_delta: Decimal::ZERO,
                    pressure_duration: 1,
                    accelerating: false,
                },
            ],
            rotations: vec![],
            clusters: vec![],
            conflicts: vec![],
            inst_rotations: vec![],
            inst_exoduses: vec![],
            shared_holders: vec![],
            stress: MarketStressIndex {
                sector_synchrony: dec!(0.3),
                pressure_consensus: dec!(0.4),
                conflict_intensity_mean: dec!(0.2),
                market_temperature_stress: dec!(0.5),
                composite_stress: dec!(0.7),
            },
            institution_stock_counts: HashMap::new(),
        };
        let decision = DecisionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            convergence_scores: HashMap::new(),
            market_regime: MarketRegimeFilter::neutral(),
            order_suggestions: vec![],
            degradations: HashMap::new(),
        };

        let snapshot = EventSnapshot::detect(&history, &links, &dimensions, &insights, &decision);

        let order_book_events: Vec<_> = snapshot
            .events
            .iter()
            .filter(|event| matches!(event.value.kind, MarketEventKind::OrderBookDislocation))
            .collect();
        assert_eq!(order_book_events.len(), 1);
        assert!(matches!(
            &order_book_events[0].value.scope,
            SignalScope::Symbol(symbol) if symbol == &sym("C.HK")
        ));

        let volume_events: Vec<_> = snapshot
            .events
            .iter()
            .filter(|event| matches!(event.value.kind, MarketEventKind::VolumeDislocation))
            .collect();
        assert_eq!(volume_events.len(), 1);
        assert!(matches!(
            &volume_events[0].value.scope,
            SignalScope::Symbol(symbol) if symbol == &sym("C.HK")
        ));

        let breakout_events: Vec<_> = snapshot
            .events
            .iter()
            .filter(|event| matches!(event.value.kind, MarketEventKind::CandlestickBreakout))
            .collect();
        assert_eq!(breakout_events.len(), 1);
        assert!(matches!(
            &breakout_events[0].value.scope,
            SignalScope::Symbol(symbol) if symbol == &sym("C.HK")
        ));

        let pressure_events: Vec<_> = snapshot
            .events
            .iter()
            .filter(|event| matches!(event.value.kind, MarketEventKind::SmartMoneyPressure))
            .collect();
        assert_eq!(pressure_events.len(), 1);
        assert!(matches!(
            &pressure_events[0].value.scope,
            SignalScope::Symbol(symbol) if symbol == &sym("C.HK")
        ));

        assert!(snapshot
            .events
            .iter()
            .any(|event| matches!(event.value.kind, MarketEventKind::MarketStressElevated)));
    }
}
