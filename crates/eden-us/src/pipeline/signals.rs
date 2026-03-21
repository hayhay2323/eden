use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use eden::ontology::domain::{
    DerivedSignal, Event, Observation, ProvenanceMetadata, ProvenanceSource,
};
use eden::ontology::objects::Symbol;

use super::dimensions::{UsDimensionSnapshot, UsSymbolDimensions};

// ── Scope ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UsSignalScope {
    Market,
    Symbol(Symbol),
    Sector(String),
}

// ── Observation layer ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UsObservationRecord {
    Quote {
        symbol: Symbol,
        last_done: Decimal,
        prev_close: Decimal,
        open: Decimal,
        turnover: Decimal,
    },
    CapitalFlow {
        symbol: Symbol,
        net_inflow: Decimal,
    },
    CalcIndex {
        symbol: Symbol,
        volume_ratio: Option<Decimal>,
        pe_ttm_ratio: Option<Decimal>,
        pb_ratio: Option<Decimal>,
        dividend_ratio_ttm: Option<Decimal>,
    },
    Candlestick {
        symbol: Symbol,
        candle_count: usize,
        window_return: Decimal,
        body_bias: Decimal,
        volume_ratio: Decimal,
        range_ratio: Decimal,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsObservationSnapshot {
    pub timestamp: OffsetDateTime,
    pub observations: Vec<Observation<UsObservationRecord>>,
}

// ── Event layer ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UsEventKind {
    /// Pre-market price deviates > 1% from prev_close.
    PreMarketDislocation,
    /// Capital flow direction flips sign tick-over-tick.
    CapitalFlowReversal,
    /// Volume ratio > 3x normal.
    VolumeSpike,
    /// HK counterpart moved significantly but US hasn't followed.
    CrossMarketDivergence,
    /// Open gaps > 2% from prev_close.
    GapOpen,
    /// Sector ETF momentum diverges from constituent.
    SectorMomentumShift,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsEventRecord {
    pub scope: UsSignalScope,
    pub kind: UsEventKind,
    pub magnitude: Decimal,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsEventSnapshot {
    pub timestamp: OffsetDateTime,
    pub events: Vec<Event<UsEventRecord>>,
}

// ── Derived signal layer ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UsDerivedSignalKind {
    /// Weighted mean of 5 US dimensions.
    StructuralComposite,
    /// Pre/post market anomaly + volume conviction.
    PreMarketConviction,
    /// HK signal propagated with time decay.
    CrossMarketPropagation,
    /// PE/PB beyond peer median.
    ValuationExtreme,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsDerivedSignalRecord {
    pub scope: UsSignalScope,
    pub kind: UsDerivedSignalKind,
    pub strength: Decimal,
    pub summary: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsDerivedSignalSnapshot {
    pub timestamp: OffsetDateTime,
    pub signals: Vec<DerivedSignal<UsDerivedSignalRecord>>,
}

// ── Construction helpers ──

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
    let mut p = ProvenanceMetadata::new(source, observed_at).with_inputs(inputs);
    if let Some(c) = confidence {
        p = p.with_confidence(c.clamp(Decimal::ZERO, Decimal::ONE));
    }
    p
}

fn confidence_from_turnover(turnover: Decimal) -> Decimal {
    if turnover <= Decimal::ZERO {
        Decimal::ZERO
    } else {
        (turnover / Decimal::new(1_000_000, 0)).min(Decimal::ONE)
    }
}

// ── ObservationSnapshot ──

impl UsObservationSnapshot {
    pub fn from_raw(
        quotes: &[eden::ontology::links::QuoteObservation],
        capital_flows: &[eden::ontology::links::CapitalFlow],
        calc_indexes: &[eden::ontology::links::CalcIndexObservation],
        candlesticks: &[eden::ontology::links::CandlestickObservation],
        timestamp: OffsetDateTime,
    ) -> Self {
        let mut observations = Vec::new();

        for q in quotes {
            observations.push(Observation::new(
                UsObservationRecord::Quote {
                    symbol: q.symbol.clone(),
                    last_done: q.last_done,
                    prev_close: q.prev_close,
                    open: q.open,
                    turnover: q.turnover,
                },
                provenance(
                    ProvenanceSource::WebSocket,
                    q.timestamp,
                    Some(confidence_from_turnover(q.turnover)),
                    [format!("quote:{}", q.symbol)],
                ),
            ));
        }

        for cf in capital_flows {
            observations.push(Observation::new(
                UsObservationRecord::CapitalFlow {
                    symbol: cf.symbol.clone(),
                    net_inflow: cf.net_inflow,
                },
                provenance(
                    ProvenanceSource::Api,
                    timestamp,
                    Some(
                        (cf.net_inflow.abs() / Decimal::new(1_000_000, 0)).min(Decimal::ONE),
                    ),
                    [format!("capital_flow:{}", cf.symbol)],
                ),
            ));
        }

        for idx in calc_indexes {
            observations.push(Observation::new(
                UsObservationRecord::CalcIndex {
                    symbol: idx.symbol.clone(),
                    volume_ratio: idx.volume_ratio,
                    pe_ttm_ratio: idx.pe_ttm_ratio,
                    pb_ratio: idx.pb_ratio,
                    dividend_ratio_ttm: idx.dividend_ratio_ttm,
                },
                provenance(
                    ProvenanceSource::Api,
                    timestamp,
                    Some(
                        idx.volume_ratio
                            .unwrap_or(Decimal::ONE)
                            .min(Decimal::new(3, 0))
                            / Decimal::new(3, 0),
                    ),
                    [format!("calc_index:{}", idx.symbol)],
                ),
            ));
        }

        for c in candlesticks {
            observations.push(Observation::new(
                UsObservationRecord::Candlestick {
                    symbol: c.symbol.clone(),
                    candle_count: c.candle_count,
                    window_return: c.window_return,
                    body_bias: c.body_bias,
                    volume_ratio: c.volume_ratio,
                    range_ratio: c.range_ratio,
                },
                provenance(
                    ProvenanceSource::Computed,
                    timestamp,
                    Some(Decimal::from(c.candle_count.min(5) as i64) / Decimal::new(5, 0)),
                    [format!("candlestick:{}", c.symbol)],
                ),
            ));
        }

        Self {
            timestamp,
            observations,
        }
    }
}

// ── EventSnapshot ──

/// Previous tick's capital flow direction per symbol (for reversal detection).
pub type PreviousFlows = std::collections::HashMap<Symbol, Decimal>;

/// HK counterpart movement for cross-market divergence detection.
/// Maps US symbol -> HK counterpart's price change ratio since last close.
pub type HkCounterpartMoves = std::collections::HashMap<Symbol, Decimal>;

impl UsEventSnapshot {
    pub fn detect(
        quotes: &[eden::ontology::links::QuoteObservation],
        calc_indexes: &[eden::ontology::links::CalcIndexObservation],
        capital_flows: &[eden::ontology::links::CapitalFlow],
        previous_flows: &PreviousFlows,
        hk_moves: &HkCounterpartMoves,
        timestamp: OffsetDateTime,
    ) -> Self {
        let mut events = Vec::new();

        for q in quotes {
            if q.prev_close == Decimal::ZERO {
                continue;
            }

            // PreMarketDislocation: open deviates > 1% from prev_close
            let pre_market_gap = (q.open - q.prev_close) / q.prev_close;
            if pre_market_gap.abs() > Decimal::new(1, 2) {
                let magnitude = pre_market_gap.abs().min(Decimal::ONE);
                events.push(Event::new(
                    UsEventRecord {
                        scope: UsSignalScope::Symbol(q.symbol.clone()),
                        kind: UsEventKind::PreMarketDislocation,
                        magnitude,
                        summary: format!(
                            "{} pre-market gap {:+.2}%",
                            q.symbol,
                            pre_market_gap * Decimal::new(100, 0)
                        ),
                    },
                    provenance(
                        ProvenanceSource::Computed,
                        timestamp,
                        Some(magnitude),
                        [format!("quote:{}", q.symbol)],
                    ),
                ));
            }

            // GapOpen: open gaps > 2% from prev_close
            if pre_market_gap.abs() > Decimal::new(2, 2) {
                let magnitude = pre_market_gap.abs().min(Decimal::ONE);
                events.push(Event::new(
                    UsEventRecord {
                        scope: UsSignalScope::Symbol(q.symbol.clone()),
                        kind: UsEventKind::GapOpen,
                        magnitude,
                        summary: format!(
                            "{} gap open {:+.2}%",
                            q.symbol,
                            pre_market_gap * Decimal::new(100, 0)
                        ),
                    },
                    provenance(
                        ProvenanceSource::Computed,
                        timestamp,
                        Some(magnitude),
                        [format!("quote:{}", q.symbol)],
                    ),
                ));
            }

            // CrossMarketDivergence: HK counterpart moved but US hasn't
            if let Some(&hk_move) = hk_moves.get(&q.symbol) {
                let us_move = (q.last_done - q.prev_close) / q.prev_close;
                let divergence = (hk_move - us_move).abs();
                if divergence > Decimal::new(2, 2) {
                    let magnitude = divergence.min(Decimal::ONE);
                    events.push(Event::new(
                        UsEventRecord {
                            scope: UsSignalScope::Symbol(q.symbol.clone()),
                            kind: UsEventKind::CrossMarketDivergence,
                            magnitude,
                            summary: format!(
                                "{} HK moved {:+.2}% but US {:+.2}%",
                                q.symbol,
                                hk_move * Decimal::new(100, 0),
                                us_move * Decimal::new(100, 0)
                            ),
                        },
                        provenance(
                            ProvenanceSource::Computed,
                            timestamp,
                            Some(magnitude),
                            [
                                format!("quote:{}", q.symbol),
                                format!("hk_counterpart:{}", q.symbol),
                            ],
                        ),
                    ));
                }
            }
        }

        // VolumeSpike: volume_ratio > 3x
        for idx in calc_indexes {
            if let Some(vr) = idx.volume_ratio {
                if vr > Decimal::new(3, 0) {
                    let magnitude =
                        ((vr - Decimal::ONE) / Decimal::new(5, 0)).min(Decimal::ONE);
                    events.push(Event::new(
                        UsEventRecord {
                            scope: UsSignalScope::Symbol(idx.symbol.clone()),
                            kind: UsEventKind::VolumeSpike,
                            magnitude,
                            summary: format!("{} volume ratio {}x", idx.symbol, vr),
                        },
                        provenance(
                            ProvenanceSource::Computed,
                            timestamp,
                            Some(magnitude),
                            [format!("calc_index:{}", idx.symbol)],
                        ),
                    ));
                }
            }
        }

        // CapitalFlowReversal: flow direction flips sign
        for cf in capital_flows {
            if let Some(&prev) = previous_flows.get(&cf.symbol) {
                if prev != Decimal::ZERO
                    && cf.net_inflow != Decimal::ZERO
                    && prev.is_sign_positive() != cf.net_inflow.is_sign_positive()
                {
                    let magnitude = (cf.net_inflow - prev).abs()
                        / (prev.abs() + cf.net_inflow.abs())
                            .max(Decimal::ONE);
                    let magnitude = magnitude.min(Decimal::ONE);
                    events.push(Event::new(
                        UsEventRecord {
                            scope: UsSignalScope::Symbol(cf.symbol.clone()),
                            kind: UsEventKind::CapitalFlowReversal,
                            magnitude,
                            summary: format!(
                                "{} capital flow reversed from {:+} to {:+}",
                                cf.symbol, prev, cf.net_inflow
                            ),
                        },
                        provenance(
                            ProvenanceSource::Computed,
                            timestamp,
                            Some(magnitude),
                            [format!("capital_flow:{}", cf.symbol)],
                        ),
                    ));
                }
            }
        }

        Self { timestamp, events }
    }
}

// ── DerivedSignalSnapshot ──

impl UsDerivedSignalSnapshot {
    pub fn compute(
        dimensions: &UsDimensionSnapshot,
        hk_signals: &std::collections::HashMap<Symbol, Decimal>,
        timestamp: OffsetDateTime,
    ) -> Self {
        let mut signals = Vec::new();

        for (symbol, dims) in &dimensions.dimensions {
            // StructuralComposite: weighted mean of 5 dims
            let composite = average_us_dimensions(dims);
            if composite != Decimal::ZERO {
                signals.push(
                    DerivedSignal::new(
                        UsDerivedSignalRecord {
                            scope: UsSignalScope::Symbol(symbol.clone()),
                            kind: UsDerivedSignalKind::StructuralComposite,
                            strength: composite,
                            summary: "US structural composite".into(),
                        },
                        provenance(
                            ProvenanceSource::Computed,
                            timestamp,
                            Some(composite.abs()),
                            [
                                format!("dimension:capital_flow_direction:{}", symbol),
                                format!("dimension:price_momentum:{}", symbol),
                                format!("dimension:volume_profile:{}", symbol),
                                format!("dimension:pre_post_market_anomaly:{}", symbol),
                                format!("dimension:valuation:{}", symbol),
                            ],
                        ),
                    )
                    .with_derivation(vec![
                        format!("dimension:capital_flow_direction:{}", symbol),
                        format!("dimension:price_momentum:{}", symbol),
                        format!("dimension:volume_profile:{}", symbol),
                        format!("dimension:pre_post_market_anomaly:{}", symbol),
                        format!("dimension:valuation:{}", symbol),
                    ]),
                );
            }

            // PreMarketConviction: pre/post anomaly weighted by volume_profile
            let conviction = dims.pre_post_market_anomaly
                * ((Decimal::ONE + dims.volume_profile.abs()) / Decimal::TWO);
            if conviction != Decimal::ZERO {
                signals.push(
                    DerivedSignal::new(
                        UsDerivedSignalRecord {
                            scope: UsSignalScope::Symbol(symbol.clone()),
                            kind: UsDerivedSignalKind::PreMarketConviction,
                            strength: conviction,
                            summary: "pre-market conviction".into(),
                        },
                        provenance(
                            ProvenanceSource::Computed,
                            timestamp,
                            Some(conviction.abs()),
                            [
                                format!("dimension:pre_post_market_anomaly:{}", symbol),
                                format!("dimension:volume_profile:{}", symbol),
                            ],
                        ),
                    )
                    .with_derivation(vec![
                        format!("dimension:pre_post_market_anomaly:{}", symbol),
                        format!("dimension:volume_profile:{}", symbol),
                    ]),
                );
            }

            // CrossMarketPropagation: HK signal * decay factor
            if let Some(&hk_signal) = hk_signals.get(symbol) {
                // Decay factor: 0.7 (signal attenuates crossing markets)
                let propagated = hk_signal * Decimal::new(7, 1);
                if propagated != Decimal::ZERO {
                    signals.push(
                        DerivedSignal::new(
                            UsDerivedSignalRecord {
                                scope: UsSignalScope::Symbol(symbol.clone()),
                                kind: UsDerivedSignalKind::CrossMarketPropagation,
                                strength: propagated,
                                summary: format!("HK signal propagated (decay 0.7) for {}", symbol),
                            },
                            provenance(
                                ProvenanceSource::Computed,
                                timestamp,
                                Some(propagated.abs()),
                                [
                                    format!("hk_signal:{}", symbol),
                                    format!("cross_market:{}", symbol),
                                ],
                            ),
                        )
                        .with_derivation(vec![format!("hk_signal:{}", symbol)]),
                    );
                }
            }

            // ValuationExtreme: valuation dimension beyond peer median
            if dims.valuation.abs() >= Decimal::new(4, 1) {
                signals.push(
                    DerivedSignal::new(
                        UsDerivedSignalRecord {
                            scope: UsSignalScope::Symbol(symbol.clone()),
                            kind: UsDerivedSignalKind::ValuationExtreme,
                            strength: dims.valuation,
                            summary: format!(
                                "{} valuation {}",
                                symbol,
                                if dims.valuation > Decimal::ZERO {
                                    "cheap vs peers"
                                } else {
                                    "expensive vs peers"
                                }
                            ),
                        },
                        provenance(
                            ProvenanceSource::Computed,
                            timestamp,
                            Some(dims.valuation.abs()),
                            [format!("dimension:valuation:{}", symbol)],
                        ),
                    )
                    .with_derivation(vec![format!("dimension:valuation:{}", symbol)]),
                );
            }
        }

        Self { timestamp, signals }
    }
}

fn average_us_dimensions(dims: &UsSymbolDimensions) -> Decimal {
    let values = [
        dims.capital_flow_direction,
        dims.price_momentum,
        dims.volume_profile,
        dims.pre_post_market_anomaly,
        dims.valuation,
    ];
    values.iter().copied().sum::<Decimal>() / Decimal::from(values.len() as i64)
}

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use eden::ontology::links::{
        CalcIndexObservation, CandlestickObservation, CapitalFlow, MarketStatus, QuoteObservation,
    };
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn sym(s: &str) -> Symbol {
        Symbol(s.into())
    }

    fn make_quote(
        symbol: &str,
        last_done: Decimal,
        prev_close: Decimal,
        open: Decimal,
    ) -> QuoteObservation {
        QuoteObservation {
            symbol: sym(symbol),
            last_done,
            prev_close,
            open,
            high: last_done + dec!(1),
            low: prev_close - dec!(1),
            volume: 1_000_000,
            turnover: dec!(50000),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            market_status: MarketStatus::Normal,
        }
    }

    // ── ObservationSnapshot ──

    #[test]
    fn observation_snapshot_collects_all_sources() {
        let quotes = vec![make_quote("AAPL.US", dec!(180), dec!(178), dec!(179))];
        let flows = vec![CapitalFlow {
            symbol: sym("AAPL.US"),
            net_inflow: dec!(5000),
        }];
        let calc = vec![CalcIndexObservation {
            symbol: sym("AAPL.US"),
            turnover_rate: None,
            volume_ratio: Some(dec!(1.5)),
            pe_ttm_ratio: Some(dec!(28)),
            pb_ratio: Some(dec!(6)),
            dividend_ratio_ttm: Some(dec!(0.005)),
            amplitude: None,
            five_minutes_change_rate: None,
        }];
        let candles = vec![CandlestickObservation {
            symbol: sym("AAPL.US"),
            candle_count: 5,
            window_return: dec!(0.3),
            body_bias: dec!(0.4),
            volume_ratio: dec!(1.5),
            range_ratio: dec!(0.2),
        }];

        let snap =
            UsObservationSnapshot::from_raw(&quotes, &flows, &calc, &candles, OffsetDateTime::UNIX_EPOCH);
        // 1 quote + 1 flow + 1 calc + 1 candle = 4
        assert_eq!(snap.observations.len(), 4);
    }

    #[test]
    fn observation_provenance_carries_confidence() {
        let quotes = vec![make_quote("NVDA.US", dec!(120), dec!(100), dec!(105))];
        let snap =
            UsObservationSnapshot::from_raw(&quotes, &[], &[], &[], OffsetDateTime::UNIX_EPOCH);
        assert!(snap.observations[0].provenance.confidence.is_some());
    }

    // ── EventSnapshot ──

    #[test]
    fn event_detects_pre_market_dislocation() {
        // open=102, prev_close=100 => 2% gap > 1%
        let quotes = vec![make_quote("TSLA.US", dec!(103), dec!(100), dec!(102))];
        let snap = UsEventSnapshot::detect(
            &quotes,
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            OffsetDateTime::UNIX_EPOCH,
        );
        assert!(snap
            .events
            .iter()
            .any(|e| matches!(e.value.kind, UsEventKind::PreMarketDislocation)));
    }

    #[test]
    fn event_detects_gap_open() {
        // open=103, prev_close=100 => 3% gap > 2%
        let quotes = vec![make_quote("TSLA.US", dec!(104), dec!(100), dec!(103))];
        let snap = UsEventSnapshot::detect(
            &quotes,
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            OffsetDateTime::UNIX_EPOCH,
        );
        assert!(snap
            .events
            .iter()
            .any(|e| matches!(e.value.kind, UsEventKind::GapOpen)));
    }

    #[test]
    fn event_no_gap_open_below_threshold() {
        // open=101, prev_close=100 => 1% gap, below 2% threshold
        let quotes = vec![make_quote("TSLA.US", dec!(101), dec!(100), dec!(101))];
        let snap = UsEventSnapshot::detect(
            &quotes,
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            OffsetDateTime::UNIX_EPOCH,
        );
        assert!(!snap
            .events
            .iter()
            .any(|e| matches!(e.value.kind, UsEventKind::GapOpen)));
    }

    #[test]
    fn event_detects_volume_spike() {
        let calc = vec![CalcIndexObservation {
            symbol: sym("NVDA.US"),
            turnover_rate: None,
            volume_ratio: Some(dec!(4.5)),
            pe_ttm_ratio: None,
            pb_ratio: None,
            dividend_ratio_ttm: None,
            amplitude: None,
            five_minutes_change_rate: None,
        }];
        let snap = UsEventSnapshot::detect(
            &[],
            &calc,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            OffsetDateTime::UNIX_EPOCH,
        );
        assert!(snap
            .events
            .iter()
            .any(|e| matches!(e.value.kind, UsEventKind::VolumeSpike)));
    }

    #[test]
    fn event_no_volume_spike_below_3x() {
        let calc = vec![CalcIndexObservation {
            symbol: sym("NVDA.US"),
            turnover_rate: None,
            volume_ratio: Some(dec!(2.5)),
            pe_ttm_ratio: None,
            pb_ratio: None,
            dividend_ratio_ttm: None,
            amplitude: None,
            five_minutes_change_rate: None,
        }];
        let snap = UsEventSnapshot::detect(
            &[],
            &calc,
            &[],
            &HashMap::new(),
            &HashMap::new(),
            OffsetDateTime::UNIX_EPOCH,
        );
        assert!(!snap
            .events
            .iter()
            .any(|e| matches!(e.value.kind, UsEventKind::VolumeSpike)));
    }

    #[test]
    fn event_detects_capital_flow_reversal() {
        let mut prev = HashMap::new();
        prev.insert(sym("AAPL.US"), dec!(5000));
        let flows = vec![CapitalFlow {
            symbol: sym("AAPL.US"),
            net_inflow: dec!(-3000),
        }];
        let snap = UsEventSnapshot::detect(
            &[],
            &[],
            &flows,
            &prev,
            &HashMap::new(),
            OffsetDateTime::UNIX_EPOCH,
        );
        assert!(snap
            .events
            .iter()
            .any(|e| matches!(e.value.kind, UsEventKind::CapitalFlowReversal)));
    }

    #[test]
    fn event_no_reversal_same_direction() {
        let mut prev = HashMap::new();
        prev.insert(sym("AAPL.US"), dec!(5000));
        let flows = vec![CapitalFlow {
            symbol: sym("AAPL.US"),
            net_inflow: dec!(3000),
        }];
        let snap = UsEventSnapshot::detect(
            &[],
            &[],
            &flows,
            &prev,
            &HashMap::new(),
            OffsetDateTime::UNIX_EPOCH,
        );
        assert!(!snap
            .events
            .iter()
            .any(|e| matches!(e.value.kind, UsEventKind::CapitalFlowReversal)));
    }

    #[test]
    fn event_detects_cross_market_divergence() {
        // HK moved +5%, US only +1%
        let quotes = vec![make_quote("BABA.US", dec!(101), dec!(100), dec!(100))];
        let mut hk_moves = HashMap::new();
        hk_moves.insert(sym("BABA.US"), dec!(0.05));
        let snap = UsEventSnapshot::detect(
            &quotes,
            &[],
            &[],
            &HashMap::new(),
            &hk_moves,
            OffsetDateTime::UNIX_EPOCH,
        );
        assert!(snap
            .events
            .iter()
            .any(|e| matches!(e.value.kind, UsEventKind::CrossMarketDivergence)));
    }

    #[test]
    fn event_no_divergence_when_aligned() {
        let quotes = vec![make_quote("BABA.US", dec!(103), dec!(100), dec!(100))];
        let mut hk_moves = HashMap::new();
        hk_moves.insert(sym("BABA.US"), dec!(0.03));
        let snap = UsEventSnapshot::detect(
            &quotes,
            &[],
            &[],
            &HashMap::new(),
            &hk_moves,
            OffsetDateTime::UNIX_EPOCH,
        );
        assert!(!snap
            .events
            .iter()
            .any(|e| matches!(e.value.kind, UsEventKind::CrossMarketDivergence)));
    }

    #[test]
    fn event_skips_zero_prev_close() {
        let quotes = vec![make_quote("TSLA.US", dec!(100), dec!(0), dec!(100))];
        let snap = UsEventSnapshot::detect(
            &quotes,
            &[],
            &[],
            &HashMap::new(),
            &HashMap::new(),
            OffsetDateTime::UNIX_EPOCH,
        );
        assert!(snap.events.is_empty());
    }

    // ── DerivedSignalSnapshot ──

    #[test]
    fn derived_emits_structural_composite() {
        let dims = UsDimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::from([(
                sym("AAPL.US"),
                UsSymbolDimensions {
                    capital_flow_direction: dec!(0.3),
                    price_momentum: dec!(0.5),
                    volume_profile: dec!(0.4),
                    pre_post_market_anomaly: dec!(0.2),
                    valuation: dec!(0.1),
                },
            )]),
        };
        let snap =
            UsDerivedSignalSnapshot::compute(&dims, &HashMap::new(), OffsetDateTime::UNIX_EPOCH);
        assert!(snap
            .signals
            .iter()
            .any(|s| matches!(s.value.kind, UsDerivedSignalKind::StructuralComposite)));
    }

    #[test]
    fn derived_emits_pre_market_conviction() {
        let dims = UsDimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::from([(
                sym("TSLA.US"),
                UsSymbolDimensions {
                    capital_flow_direction: dec!(0),
                    price_momentum: dec!(0),
                    volume_profile: dec!(0.6),
                    pre_post_market_anomaly: dec!(0.8),
                    valuation: dec!(0),
                },
            )]),
        };
        let snap =
            UsDerivedSignalSnapshot::compute(&dims, &HashMap::new(), OffsetDateTime::UNIX_EPOCH);
        let conviction = snap
            .signals
            .iter()
            .find(|s| matches!(s.value.kind, UsDerivedSignalKind::PreMarketConviction));
        assert!(conviction.is_some());
        // 0.8 * (1 + 0.6) / 2 = 0.8 * 0.8 = 0.64
        assert_eq!(conviction.unwrap().value.strength, dec!(0.64));
    }

    #[test]
    fn derived_emits_cross_market_propagation() {
        let dims = UsDimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::from([(sym("BABA.US"), UsSymbolDimensions::default())]),
        };
        let mut hk_signals = HashMap::new();
        hk_signals.insert(sym("BABA.US"), dec!(0.6));
        let snap =
            UsDerivedSignalSnapshot::compute(&dims, &hk_signals, OffsetDateTime::UNIX_EPOCH);
        let prop = snap
            .signals
            .iter()
            .find(|s| matches!(s.value.kind, UsDerivedSignalKind::CrossMarketPropagation));
        assert!(prop.is_some());
        // 0.6 * 0.7 = 0.42
        assert_eq!(prop.unwrap().value.strength, dec!(0.42));
    }

    #[test]
    fn derived_emits_valuation_extreme() {
        let dims = UsDimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::from([(
                sym("MSFT.US"),
                UsSymbolDimensions {
                    capital_flow_direction: dec!(0),
                    price_momentum: dec!(0),
                    volume_profile: dec!(0),
                    pre_post_market_anomaly: dec!(0),
                    valuation: dec!(0.6),
                },
            )]),
        };
        let snap =
            UsDerivedSignalSnapshot::compute(&dims, &HashMap::new(), OffsetDateTime::UNIX_EPOCH);
        assert!(snap
            .signals
            .iter()
            .any(|s| matches!(s.value.kind, UsDerivedSignalKind::ValuationExtreme)));
    }

    #[test]
    fn derived_no_valuation_extreme_below_threshold() {
        let dims = UsDimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::from([(
                sym("MSFT.US"),
                UsSymbolDimensions {
                    capital_flow_direction: dec!(0),
                    price_momentum: dec!(0),
                    volume_profile: dec!(0),
                    pre_post_market_anomaly: dec!(0),
                    valuation: dec!(0.3),
                },
            )]),
        };
        let snap =
            UsDerivedSignalSnapshot::compute(&dims, &HashMap::new(), OffsetDateTime::UNIX_EPOCH);
        assert!(!snap
            .signals
            .iter()
            .any(|s| matches!(s.value.kind, UsDerivedSignalKind::ValuationExtreme)));
    }

    #[test]
    fn derived_skips_zero_composite() {
        let dims = UsDimensionSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            dimensions: HashMap::from([(sym("FLAT.US"), UsSymbolDimensions::default())]),
        };
        let snap =
            UsDerivedSignalSnapshot::compute(&dims, &HashMap::new(), OffsetDateTime::UNIX_EPOCH);
        assert!(!snap
            .signals
            .iter()
            .any(|s| matches!(s.value.kind, UsDerivedSignalKind::StructuralComposite)));
    }

    #[test]
    fn average_dimensions_correct() {
        let dims = UsSymbolDimensions {
            capital_flow_direction: dec!(0.2),
            price_momentum: dec!(0.4),
            volume_profile: dec!(0.6),
            pre_post_market_anomaly: dec!(0.3),
            valuation: dec!(0.5),
        };
        // (0.2 + 0.4 + 0.6 + 0.3 + 0.5) / 5 = 2.0 / 5 = 0.4
        assert_eq!(average_us_dimensions(&dims), dec!(0.4));
    }
}
