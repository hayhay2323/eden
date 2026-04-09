use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use time::OffsetDateTime;

use crate::ontology::domain::{
    DerivedSignal, Event, Observation, ProvenanceMetadata, ProvenanceSource,
};
use crate::ontology::objects::{SectorId, Symbol};
use crate::us::common::dimension_composite;

use super::dimensions::UsDimensionSnapshot;

// ── Scope ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum UsSignalScope {
    Market,
    Symbol(Symbol),
    Sector(SectorId),
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
        ytd_change_rate: Option<Decimal>,
        five_day_change_rate: Option<Decimal>,
        ten_day_change_rate: Option<Decimal>,
        half_year_change_rate: Option<Decimal>,
        total_market_value: Option<Decimal>,
        change_rate: Option<Decimal>,
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
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
    /// A promoted macro catalyst is still active for this scope.
    CatalystActivation,
    /// Expected sector propagation did not occur — peers are silent.
    PropagationAbsence,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum UsEventDriverKind {
    CompanySpecific,
    SectorWide,
    MacroWide,
    CrossMarket,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum UsPropagationScope {
    Local,
    Sector,
    Market,
    CrossMarket,
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
        quotes: &[crate::ontology::links::QuoteObservation],
        capital_flows: &[crate::ontology::links::CapitalFlow],
        calc_indexes: &[crate::ontology::links::CalcIndexObservation],
        candlesticks: &[crate::ontology::links::CandlestickObservation],
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
                    net_inflow: cf.net_inflow.as_yuan(),
                },
                provenance(
                    ProvenanceSource::Api,
                    timestamp,
                    Some(
                        (cf.net_inflow.as_yuan().abs() / Decimal::new(1_000_000, 0))
                            .min(Decimal::ONE),
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
                    ytd_change_rate: idx.ytd_change_rate,
                    five_day_change_rate: idx.five_day_change_rate,
                    ten_day_change_rate: idx.ten_day_change_rate,
                    half_year_change_rate: idx.half_year_change_rate,
                    total_market_value: idx.total_market_value,
                    change_rate: idx.change_rate,
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

/// HK counterpart move proxy for cross-market divergence detection.
/// Maps US symbol -> HK counterpart's latest directional move estimate.
pub type HkCounterpartMoves = std::collections::HashMap<Symbol, Decimal>;

impl UsEventSnapshot {
    pub fn detect(
        quotes: &[crate::ontology::links::QuoteObservation],
        calc_indexes: &[crate::ontology::links::CalcIndexObservation],
        capital_flows: &[crate::ontology::links::CapitalFlow],
        previous_flows: &PreviousFlows,
        hk_moves: &HkCounterpartMoves,
        timestamp: OffsetDateTime,
    ) -> Self {
        let mut events = Vec::new();

        for q in quotes {
            if q.prev_close == Decimal::ZERO || q.open == Decimal::ZERO {
                continue;
            }

            // PreMarketDislocation: open deviates > 1% from prev_close
            let pre_market_gap = (q.open - q.prev_close) / q.prev_close;
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
            } else if pre_market_gap.abs() > Decimal::new(1, 2) {
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

            // CrossMarketDivergence: HK counterpart moved but US hasn't
            if let Some(&hk_move) = hk_moves.get(&q.symbol) {
                let us_anchor = if q.last_done != q.prev_close {
                    q.last_done
                } else if q.open != q.prev_close {
                    q.open
                } else {
                    continue;
                };
                let us_move = (us_anchor - q.prev_close) / q.prev_close;
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
                    let magnitude = ((vr - Decimal::ONE) / Decimal::new(5, 0)).min(Decimal::ONE);
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
                let current = cf.net_inflow.as_yuan();
                if prev != Decimal::ZERO
                    && current != Decimal::ZERO
                    && prev.is_sign_positive() != current.is_sign_positive()
                {
                    let magnitude =
                        (current - prev).abs() / (prev.abs() + current.abs()).max(Decimal::ONE);
                    let magnitude = magnitude.min(Decimal::ONE);
                    events.push(Event::new(
                        UsEventRecord {
                            scope: UsSignalScope::Symbol(cf.symbol.clone()),
                            kind: UsEventKind::CapitalFlowReversal,
                            magnitude,
                            summary: format!(
                                "{} capital flow reversed from {:+} to {:+}",
                                cf.symbol, prev, current
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

        apply_us_event_attribution(&mut events, hk_moves);
        propagate_symbol_events_to_sector(&mut events, timestamp);
        apply_us_event_attribution(&mut events, hk_moves);

        Self { timestamp, events }
    }
}

fn apply_us_event_attribution(events: &mut [Event<UsEventRecord>], hk_moves: &HkCounterpartMoves) {
    let mut corroborated_sector_counts: HashMap<SectorId, usize> = HashMap::new();
    for event in events.iter() {
        let UsSignalScope::Symbol(symbol) = &event.value.scope else {
            continue;
        };
        if !matches!(
            event.value.kind,
            UsEventKind::GapOpen
                | UsEventKind::PreMarketDislocation
                | UsEventKind::VolumeSpike
                | UsEventKind::CapitalFlowReversal
        ) {
            continue;
        }
        if event.value.magnitude < Decimal::new(5, 2) {
            continue;
        }
        let Some(sector) = crate::us::watchlist::us_symbol_sector(&symbol.0) else {
            continue;
        };
        *corroborated_sector_counts
            .entry(SectorId(sector.to_string()))
            .or_insert(0) += 1;
    }

    for event in events.iter_mut() {
        match &event.value.scope {
            UsSignalScope::Symbol(symbol) => match event.value.kind {
                UsEventKind::CrossMarketDivergence => set_us_event_attribution(
                    event,
                    UsEventDriverKind::CrossMarket,
                    UsPropagationScope::CrossMarket,
                    "hk_counterpart_divergence",
                    event.value.magnitude,
                ),
                UsEventKind::GapOpen
                | UsEventKind::PreMarketDislocation
                | UsEventKind::VolumeSpike
                | UsEventKind::CapitalFlowReversal => {
                    let Some(sector_str) = crate::us::watchlist::us_symbol_sector(&symbol.0) else {
                        set_us_event_attribution(
                            event,
                            UsEventDriverKind::CompanySpecific,
                            UsPropagationScope::Local,
                            "isolated_symbol_move",
                            event.value.magnitude,
                        );
                        continue;
                    };
                    let sector = SectorId(sector_str.to_string());
                    let hk_move = hk_moves.get(symbol).copied().unwrap_or(Decimal::ZERO);
                    if hk_move.abs() >= Decimal::new(2, 2) {
                        set_us_event_attribution(
                            event,
                            UsEventDriverKind::CrossMarket,
                            UsPropagationScope::CrossMarket,
                            "hk_counterpart_move",
                            event.value.magnitude.max(hk_move.abs()),
                        );
                    } else if corroborated_sector_counts
                        .get(&sector)
                        .copied()
                        .unwrap_or(0)
                        >= 2
                    {
                        set_us_event_attribution(
                            event,
                            UsEventDriverKind::SectorWide,
                            UsPropagationScope::Sector,
                            "multi_symbol_sector_move",
                            event.value.magnitude,
                        );
                    } else if event.value.magnitude >= Decimal::new(8, 2) {
                        set_us_event_attribution(
                            event,
                            UsEventDriverKind::CompanySpecific,
                            UsPropagationScope::Local,
                            "isolated_large_gap",
                            event.value.magnitude,
                        );
                    } else {
                        set_us_event_attribution(
                            event,
                            UsEventDriverKind::CompanySpecific,
                            UsPropagationScope::Local,
                            "isolated_symbol_move",
                            event.value.magnitude,
                        );
                    }
                }
                UsEventKind::CatalystActivation => set_us_event_attribution(
                    event,
                    UsEventDriverKind::CompanySpecific,
                    UsPropagationScope::Local,
                    "symbol_catalyst",
                    event.value.magnitude,
                ),
                _ => {}
            },
            UsSignalScope::Sector(_) => match event.value.kind {
                UsEventKind::SectorMomentumShift | UsEventKind::CatalystActivation => {
                    set_us_event_attribution(
                        event,
                        UsEventDriverKind::SectorWide,
                        UsPropagationScope::Sector,
                        "sector_confirmation",
                        event.value.magnitude,
                    )
                }
                _ => {}
            },
            UsSignalScope::Market => {
                if matches!(event.value.kind, UsEventKind::CatalystActivation) {
                    set_us_event_attribution(
                        event,
                        UsEventDriverKind::MacroWide,
                        UsPropagationScope::Market,
                        "macro_catalyst",
                        event.value.magnitude,
                    );
                }
            }
        }
    }
}

fn propagate_symbol_events_to_sector(
    events: &mut Vec<Event<UsEventRecord>>,
    timestamp: OffsetDateTime,
) {
    const SIGNIFICANT_KINDS: &[UsEventKind] = &[
        UsEventKind::GapOpen,
        UsEventKind::VolumeSpike,
        UsEventKind::PreMarketDislocation,
        UsEventKind::CapitalFlowReversal,
    ];
    let mut sector_magnitudes: std::collections::HashMap<SectorId, (Decimal, String)> =
        std::collections::HashMap::new();

    for ev in events.iter() {
        if !SIGNIFICANT_KINDS.contains(&ev.value.kind) {
            continue;
        }
        let symbol = match &ev.value.scope {
            UsSignalScope::Symbol(s) => s,
            _ => continue,
        };
        let Some(propagation_scope) = event_propagation_scope(ev) else {
            continue;
        };
        let threshold = match propagation_scope {
            UsPropagationScope::Sector => Decimal::new(5, 2),
            UsPropagationScope::Market | UsPropagationScope::CrossMarket => Decimal::new(3, 2),
            UsPropagationScope::Local => Decimal::new(15, 2),
        };
        if ev.value.magnitude < threshold {
            continue;
        }
        if matches!(propagation_scope, UsPropagationScope::Local) {
            continue;
        }
        let Some(sector_str) = crate::us::watchlist::us_symbol_sector(&symbol.0) else {
            continue;
        };
        let sector = SectorId(sector_str.to_string());
        let entry = sector_magnitudes
            .entry(sector)
            .or_insert((Decimal::ZERO, ev.value.summary.clone()));
        if ev.value.magnitude > entry.0 {
            *entry = (ev.value.magnitude, ev.value.summary.clone());
        }
    }

    for (sector, (magnitude, trigger_summary)) in sector_magnitudes {
        events.push(Event::new(
            UsEventRecord {
                scope: UsSignalScope::Sector(sector.clone()),
                kind: UsEventKind::SectorMomentumShift,
                magnitude: (magnitude * Decimal::new(80, 2)).min(Decimal::ONE),
                summary: format!(
                    "sector {} under pressure from constituent: {}",
                    sector.0, trigger_summary
                ),
            },
            provenance(
                ProvenanceSource::Computed,
                timestamp,
                Some(magnitude),
                [format!("sector_propagation:{}", sector.0)],
            ),
        ));
    }
}

const ATTR_DRIVER_PREFIX: &str = "attr:driver=";
const ATTR_SCOPE_PREFIX: &str = "attr:scope=";
const ATTR_CONFIDENCE_PREFIX: &str = "attr:confidence=";
const ATTR_LABEL_PREFIX: &str = "attr:label=";

fn set_us_event_attribution(
    event: &mut Event<UsEventRecord>,
    driver: UsEventDriverKind,
    propagation_scope: UsPropagationScope,
    label: &str,
    confidence: Decimal,
) {
    event.provenance.inputs.retain(|input| {
        !input.starts_with(ATTR_DRIVER_PREFIX)
            && !input.starts_with(ATTR_SCOPE_PREFIX)
            && !input.starts_with(ATTR_CONFIDENCE_PREFIX)
            && !input.starts_with(ATTR_LABEL_PREFIX)
    });
    event
        .provenance
        .inputs
        .push(format!("{ATTR_DRIVER_PREFIX}{}", driver_slug(driver)));
    event.provenance.inputs.push(format!(
        "{ATTR_SCOPE_PREFIX}{}",
        propagation_scope_slug(propagation_scope)
    ));
    event.provenance.inputs.push(format!(
        "{ATTR_CONFIDENCE_PREFIX}{}",
        confidence.round_dp(4)
    ));
    event
        .provenance
        .inputs
        .push(format!("{ATTR_LABEL_PREFIX}{label}"));
}

fn driver_slug(driver: UsEventDriverKind) -> &'static str {
    match driver {
        UsEventDriverKind::CompanySpecific => "company_specific",
        UsEventDriverKind::SectorWide => "sector_wide",
        UsEventDriverKind::MacroWide => "macro_wide",
        UsEventDriverKind::CrossMarket => "cross_market",
    }
}

fn propagation_scope_slug(scope: UsPropagationScope) -> &'static str {
    match scope {
        UsPropagationScope::Local => "local",
        UsPropagationScope::Sector => "sector",
        UsPropagationScope::Market => "market",
        UsPropagationScope::CrossMarket => "cross_market",
    }
}

pub(crate) fn enrich_us_attribution_with_evidence(
    event_snapshot: &mut UsEventSnapshot,
    macro_events: &[crate::ontology::AgentMacroEvent],
) {
    use std::collections::HashSet;

    let macro_affected_sectors: HashSet<String> = macro_events
        .iter()
        .flat_map(|e| e.impact.affected_sectors.iter().cloned())
        .collect();
    let macro_affected_symbols: HashSet<String> = macro_events
        .iter()
        .flat_map(|e| e.impact.affected_symbols.iter().cloned())
        .collect();

    for event in event_snapshot.events.iter_mut() {
        let current_scope = event_propagation_scope(event);
        if matches!(
            current_scope,
            Some(UsPropagationScope::Market | UsPropagationScope::CrossMarket)
        ) {
            continue;
        }

        let symbol = match &event.value.scope {
            UsSignalScope::Symbol(s) => s,
            _ => continue,
        };

        if macro_affected_symbols.contains(&symbol.0) {
            set_us_event_attribution(
                event,
                UsEventDriverKind::MacroWide,
                UsPropagationScope::Market,
                "macro_event_targets_symbol",
                event.value.magnitude,
            );
            continue;
        }

        if let Some(sector_str) = crate::us::watchlist::us_symbol_sector(&symbol.0) {
            let sector_lower = sector_str.to_ascii_lowercase();
            if macro_affected_sectors
                .iter()
                .any(|s| s.to_ascii_lowercase() == sector_lower)
            {
                let current = current_scope.unwrap_or(UsPropagationScope::Local);
                if current < UsPropagationScope::Sector {
                    set_us_event_attribution(
                        event,
                        UsEventDriverKind::MacroWide,
                        UsPropagationScope::Market,
                        "macro_event_covers_sector",
                        event.value.magnitude,
                    );
                }
            }
        }
    }
}

pub(crate) fn detect_us_propagation_absences(
    event_snapshot: &mut UsEventSnapshot,
    timestamp: OffsetDateTime,
) {
    use std::collections::{HashMap, HashSet};

    let mut sector_active: HashMap<SectorId, HashSet<Symbol>> = HashMap::new();
    let mut sector_all: HashMap<SectorId, HashSet<Symbol>> = HashMap::new();

    for event in &event_snapshot.events {
        let UsSignalScope::Symbol(symbol) = &event.value.scope else {
            continue;
        };
        let Some(sector_str) = crate::us::watchlist::us_symbol_sector(&symbol.0) else {
            continue;
        };
        let sector = SectorId(sector_str.to_string());
        sector_all
            .entry(sector.clone())
            .or_default()
            .insert(symbol.clone());

        let scope = event_propagation_scope(event).unwrap_or(UsPropagationScope::Local);
        if scope >= UsPropagationScope::Sector && event.value.magnitude >= Decimal::new(4, 2) {
            sector_active
                .entry(sector)
                .or_default()
                .insert(symbol.clone());
        }
    }

    let mut new_events = Vec::new();
    for (sector, active_symbols) in &sector_active {
        if active_symbols.len() < 2 {
            continue;
        }
        let all_symbols = match sector_all.get(sector) {
            Some(all) => all,
            None => continue,
        };
        let silent: Vec<_> = all_symbols.difference(active_symbols).collect();
        let silent_ratio =
            Decimal::from(silent.len() as u64) / Decimal::from(all_symbols.len().max(1) as u64);
        if silent_ratio < Decimal::new(30, 2) {
            continue;
        }
        let magnitude = (silent_ratio * Decimal::new(50, 2)).min(Decimal::ONE);
        let mut ev = Event {
            value: UsEventRecord {
                scope: UsSignalScope::Sector(sector.clone()),
                kind: UsEventKind::PropagationAbsence,
                magnitude,
                summary: format!(
                    "sector {} has {} active symbols but {}/{} peers silent",
                    sector.0,
                    active_symbols.len(),
                    silent.len(),
                    all_symbols.len(),
                ),
            },
            provenance: ProvenanceMetadata::new(ProvenanceSource::Computed, timestamp),
        };
        set_us_event_attribution(
            &mut ev,
            UsEventDriverKind::SectorWide,
            UsPropagationScope::Sector,
            "propagation_absence",
            magnitude,
        );
        new_events.push(ev);
    }
    event_snapshot.events.extend(new_events);
}

pub(crate) fn event_propagation_scope(event: &Event<UsEventRecord>) -> Option<UsPropagationScope> {
    event.provenance.inputs.iter().find_map(|input| {
        input
            .strip_prefix(ATTR_SCOPE_PREFIX)
            .and_then(|value| match value {
                "local" => Some(UsPropagationScope::Local),
                "sector" => Some(UsPropagationScope::Sector),
                "market" => Some(UsPropagationScope::Market),
                "cross_market" => Some(UsPropagationScope::CrossMarket),
                _ => None,
            })
    })
}

fn is_thematic_catalyst(event_type: &str) -> bool {
    matches!(
        event_type,
        "geopolitical_policy"
            | "rates_macro"
            | "commodity_logistics"
            | "sector_rotation"
            | "informed_flow"
            | "cross_market_propagation"
            | "tech_sector"
            | "healthcare"
            | "earnings_surprise"
    )
}

pub fn catalyst_events_from_macro_events(
    macro_events: &[crate::ontology::AgentMacroEvent],
    timestamp: OffsetDateTime,
) -> Vec<Event<UsEventRecord>> {
    let mut events = Vec::new();

    for event in macro_events
        .iter()
        .filter(|e| is_thematic_catalyst(&e.event_type))
    {
        let magnitude = event.confidence.clamp(Decimal::ZERO, Decimal::ONE);
        let summary = format!("{} catalyst remains active", event.headline);
        let inputs = [format!("macro_event:{}", event.event_id)];

        for sector in &event.impact.affected_sectors {
            events.push(Event::new(
                UsEventRecord {
                    scope: UsSignalScope::Sector(SectorId(sector.clone())),
                    kind: UsEventKind::CatalystActivation,
                    magnitude,
                    summary: format!(
                        "{} catalyst remains active for sector {}",
                        event.headline, sector
                    ),
                },
                provenance(
                    ProvenanceSource::Computed,
                    timestamp,
                    Some(magnitude),
                    inputs.clone(),
                ),
            ));
        }

        for symbol in &event.impact.affected_symbols {
            if !symbol.to_ascii_uppercase().ends_with(".US") {
                continue;
            }
            events.push(Event::new(
                UsEventRecord {
                    scope: UsSignalScope::Symbol(Symbol(symbol.clone())),
                    kind: UsEventKind::CatalystActivation,
                    magnitude,
                    summary: format!("{} catalyst remains active for {}", event.headline, symbol),
                },
                provenance(
                    ProvenanceSource::Computed,
                    timestamp,
                    Some(magnitude),
                    inputs.clone(),
                ),
            ));
        }

        if event.impact.affected_symbols.is_empty() && event.impact.affected_sectors.is_empty() {
            events.push(Event::new(
                UsEventRecord {
                    scope: UsSignalScope::Market,
                    kind: UsEventKind::CatalystActivation,
                    magnitude,
                    summary: summary.clone(),
                },
                provenance(
                    ProvenanceSource::Computed,
                    timestamp,
                    Some(magnitude),
                    inputs.clone(),
                ),
            ));
        }
    }

    events
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
            let composite = dimension_composite(dims);
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

            // CrossMarketPropagation: caller already supplies the decayed HK signal.
            if let Some(&hk_signal) = hk_signals.get(symbol) {
                let propagated = hk_signal;
                if propagated != Decimal::ZERO {
                    signals.push(
                        DerivedSignal::new(
                            UsDerivedSignalRecord {
                                scope: UsSignalScope::Symbol(symbol.clone()),
                                kind: UsDerivedSignalKind::CrossMarketPropagation,
                                strength: propagated,
                                summary: format!("HK signal propagated for {}", symbol),
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

// ── Tests ──

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::links::{
        CalcIndexObservation, CandlestickObservation, CapitalFlow, MarketStatus, QuoteObservation,
    };
    use crate::us::pipeline::dimensions::UsSymbolDimensions;
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
            pre_market: None,
            post_market: None,
        }
    }

    // ── ObservationSnapshot ──

    #[test]
    fn observation_snapshot_collects_all_sources() {
        let quotes = vec![make_quote("AAPL.US", dec!(180), dec!(178), dec!(179))];
        let flows = vec![CapitalFlow {
            symbol: sym("AAPL.US"),
            net_inflow: crate::ontology::links::YuanAmount::from_yuan(dec!(5000)),
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
            ytd_change_rate: None,
            five_day_change_rate: None,
            ten_day_change_rate: None,
            half_year_change_rate: None,
            total_market_value: None,
            capital_flow: None,
            change_rate: None,
        }];
        let candles = vec![CandlestickObservation {
            symbol: sym("AAPL.US"),
            candle_count: 5,
            window_return: dec!(0.3),
            body_bias: dec!(0.4),
            volume_ratio: dec!(1.5),
            range_ratio: dec!(0.2),
        }];

        let snap = UsObservationSnapshot::from_raw(
            &quotes,
            &flows,
            &calc,
            &candles,
            OffsetDateTime::UNIX_EPOCH,
        );
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
        assert_eq!(
            snap.events
                .iter()
                .filter(|e| matches!(e.value.kind, UsEventKind::PreMarketDislocation))
                .count(),
            0
        );
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
    fn isolated_gap_open_does_not_force_sector_propagation() {
        let quotes = vec![make_quote("NKE.US", dec!(112), dec!(100), dec!(111))];
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
            .any(|e| matches!(e.value.kind, UsEventKind::SectorMomentumShift)));
    }

    #[test]
    fn corroborated_sector_moves_emit_sector_propagation() {
        let quotes = vec![
            make_quote("NKE.US", dec!(112), dec!(100), dec!(111)),
            make_quote("LULU.US", dec!(111), dec!(100), dec!(109)),
        ];
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
            .any(|e| matches!(e.value.kind, UsEventKind::SectorMomentumShift)));
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
            ytd_change_rate: None,
            five_day_change_rate: None,
            ten_day_change_rate: None,
            half_year_change_rate: None,
            total_market_value: None,
            capital_flow: None,
            change_rate: None,
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
            ytd_change_rate: None,
            five_day_change_rate: None,
            ten_day_change_rate: None,
            half_year_change_rate: None,
            total_market_value: None,
            capital_flow: None,
            change_rate: None,
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
            net_inflow: crate::ontology::links::YuanAmount::from_yuan(dec!(-3000)),
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
            net_inflow: crate::ontology::links::YuanAmount::from_yuan(dec!(3000)),
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
    fn event_skips_cross_market_divergence_when_us_has_not_moved() {
        let quotes = vec![make_quote("BABA.US", dec!(100), dec!(100), dec!(100))];
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
        assert!(!snap
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
                    multi_horizon_momentum: Decimal::ZERO,
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
                    multi_horizon_momentum: Decimal::ZERO,
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
        let snap = UsDerivedSignalSnapshot::compute(&dims, &hk_signals, OffsetDateTime::UNIX_EPOCH);
        let prop = snap
            .signals
            .iter()
            .find(|s| matches!(s.value.kind, UsDerivedSignalKind::CrossMarketPropagation));
        assert!(prop.is_some());
        assert_eq!(prop.unwrap().value.strength, dec!(0.6));
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
                    multi_horizon_momentum: Decimal::ZERO,
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
                    multi_horizon_momentum: Decimal::ZERO,
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
            multi_horizon_momentum: Decimal::ZERO,
        };
        // (0.2 + 0.4 + 0.6 + 0.3 + 0.5) / 5 = 2.0 / 5 = 0.4
        assert_eq!(dimension_composite(&dims), dec!(0.4));
    }
}

/// Extract sectors where propagation absence was detected this tick.
pub(crate) fn us_propagation_absence_sectors(
    events: &UsEventSnapshot,
) -> Vec<SectorId> {
    events
        .events
        .iter()
        .filter(|ev| ev.value.kind == UsEventKind::PropagationAbsence)
        .filter_map(|ev| match &ev.value.scope {
            UsSignalScope::Sector(sector_id) => Some(sector_id.clone()),
            _ => None,
        })
        .collect()
}
