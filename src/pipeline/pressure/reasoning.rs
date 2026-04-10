//! Ontological Reasoning on top of the Pressure Field.
//!
//! The pressure field SEES. This module UNDERSTANDS.
//!
//! Four reasoning primitives, all derived from pressure field structure:
//! 1. Attribution — WHY is there tension? (which channels drive it)
//! 2. Absence — WHO should be reacting but isn't? (neighbor pressure comparison)
//! 3. Competition — WHICH explanation is better? (multi-channel vs single-channel)
//! 4. Lifecycle — IS the anomaly growing, peaking, or dying? (acceleration tracking)

use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::ontology::objects::{SectorId, Symbol};

use super::{
    NodePressure, PressureChannel, PressureField, PressureVortex, TimeScale,
};

// ═══════════════════════════════════════════════════════════════════
// 1. ATTRIBUTION — Why does this node have tension?
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TensionDriver {
    /// Volume + Momentum but not CapitalFlow → someone is buying/selling
    /// through order flow, not large block trades.
    TradeFlowDriven,
    /// CapitalFlow dominant → institutional-level money movement.
    CapitalFlowDriven,
    /// OrderBook + Structure → microstructure shift (depth/spread change).
    MicrostructureDriven,
    /// Institutional channel dominant → broker/institution positioning.
    InstitutionalDriven,
    /// Multiple channels (3+) contribute → broad structural shift.
    BroadStructural,
    /// Single channel dominates → narrow signal, lower confidence.
    SingleChannel { channel: PressureChannel },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TensionAttribution {
    pub symbol: Symbol,
    pub driver: TensionDriver,
    /// Which channels contribute to the tension, sorted by magnitude.
    pub contributing_channels: Vec<(PressureChannel, Decimal)>,
    /// Channels that are ABSENT (near zero despite tension elsewhere).
    pub silent_channels: Vec<PressureChannel>,
    /// Human-readable explanation.
    pub narrative: String,
}

pub fn attribute_tension(vortex: &PressureVortex, node: &NodePressure) -> TensionAttribution {
    let mut channel_magnitudes: Vec<(PressureChannel, Decimal)> = PressureChannel::ALL
        .iter()
        .filter_map(|ch| {
            let net = node.channels.get(ch)?.net();
            if net.abs() >= Decimal::new(1, 3) {
                Some((*ch, net))
            } else {
                None
            }
        })
        .collect();
    channel_magnitudes.sort_by(|a, b| b.1.abs().cmp(&a.1.abs()));

    let silent_channels: Vec<PressureChannel> = PressureChannel::ALL
        .iter()
        .filter(|ch| {
            node.channels
                .get(ch)
                .map(|cp| cp.net().abs() < Decimal::new(1, 3))
                .unwrap_or(true)
        })
        .copied()
        .collect();

    let active_count = channel_magnitudes.len();
    let has_volume = channel_magnitudes
        .iter()
        .any(|(ch, _)| *ch == PressureChannel::Volume);
    let has_momentum = channel_magnitudes
        .iter()
        .any(|(ch, _)| *ch == PressureChannel::Momentum);
    let has_capital = channel_magnitudes
        .iter()
        .any(|(ch, _)| *ch == PressureChannel::CapitalFlow);
    let has_institutional = channel_magnitudes
        .iter()
        .any(|(ch, _)| *ch == PressureChannel::Institutional);
    let has_orderbook = channel_magnitudes
        .iter()
        .any(|(ch, _)| *ch == PressureChannel::OrderBook);
    let has_structure = channel_magnitudes
        .iter()
        .any(|(ch, _)| *ch == PressureChannel::Structure);

    let driver = if active_count >= 3 {
        TensionDriver::BroadStructural
    } else if has_volume && has_momentum && !has_capital {
        TensionDriver::TradeFlowDriven
    } else if has_capital {
        TensionDriver::CapitalFlowDriven
    } else if has_institutional {
        TensionDriver::InstitutionalDriven
    } else if has_orderbook || has_structure {
        TensionDriver::MicrostructureDriven
    } else if active_count == 1 {
        TensionDriver::SingleChannel {
            channel: channel_magnitudes[0].0,
        }
    } else {
        TensionDriver::BroadStructural
    };

    let channel_names: Vec<String> = channel_magnitudes
        .iter()
        .map(|(ch, mag)| format!("{:?}({:.3})", ch, mag))
        .collect();
    let silent_names: Vec<String> = silent_channels.iter().map(|ch| format!("{:?}", ch)).collect();

    let narrative = match &driver {
        TensionDriver::TradeFlowDriven => format!(
            "{} tension driven by trade flow (Volume+Momentum) without capital flow backing. Channels: {}. Silent: {}.",
            vortex.symbol.0, channel_names.join(", "), silent_names.join(", ")
        ),
        TensionDriver::CapitalFlowDriven => format!(
            "{} tension driven by capital flow — institutional money is moving. Channels: {}. Silent: {}.",
            vortex.symbol.0, channel_names.join(", "), silent_names.join(", ")
        ),
        TensionDriver::InstitutionalDriven => format!(
            "{} tension driven by institutional positioning. Channels: {}. Silent: {}.",
            vortex.symbol.0, channel_names.join(", "), silent_names.join(", ")
        ),
        TensionDriver::MicrostructureDriven => format!(
            "{} tension from microstructure shift (order book / depth). Channels: {}.",
            vortex.symbol.0, channel_names.join(", ")
        ),
        TensionDriver::BroadStructural => format!(
            "{} broad structural tension across {} channels: {}.",
            vortex.symbol.0, active_count, channel_names.join(", ")
        ),
        TensionDriver::SingleChannel { channel } => format!(
            "{} narrow tension from single channel {:?}. Low confidence.",
            vortex.symbol.0, channel
        ),
    };

    TensionAttribution {
        symbol: vortex.symbol.clone(),
        driver,
        contributing_channels: channel_magnitudes,
        silent_channels,
        narrative,
    }
}

// ═══════════════════════════════════════════════════════════════════
// 2. ABSENCE — Who should be reacting but isn't?
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropagationAbsence {
    pub source_symbol: Symbol,
    pub source_tension: Decimal,
    /// Neighbors that SHOULD have tension (connected via graph) but don't.
    pub silent_neighbors: Vec<Symbol>,
    /// Neighbors that DO have tension.
    pub active_neighbors: Vec<(Symbol, Decimal)>,
    /// Is this an isolated anomaly? (no neighbors reacting)
    pub is_isolated: bool,
    /// Human-readable.
    pub narrative: String,
}

pub fn detect_absence(
    vortex: &PressureVortex,
    field: &PressureField,
    sector_members: &HashMap<SectorId, Vec<Symbol>>,
    symbol_sector: &HashMap<Symbol, SectorId>,
) -> PropagationAbsence {
    let mut silent = Vec::new();
    let mut active = Vec::new();

    // Find peers: same sector symbols.
    let peers: Vec<&Symbol> = symbol_sector
        .get(&vortex.symbol)
        .and_then(|sid| sector_members.get(sid))
        .map(|members| members.iter().filter(|s| **s != vortex.symbol).collect())
        .unwrap_or_default();

    let minute_layer = field.layers.get(&TimeScale::Minute);

    for peer in &peers {
        let peer_tension = minute_layer
            .and_then(|layer| layer.pressures.get(*peer))
            .map(|node| node.composite.abs())
            .unwrap_or(Decimal::ZERO);

        if peer_tension < Decimal::new(1, 2) {
            silent.push((*peer).clone());
        } else {
            active.push(((*peer).clone(), peer_tension));
        }
    }

    let is_isolated = active.is_empty() && !silent.is_empty();

    let narrative = if is_isolated {
        format!(
            "{} has tension {:.3} but {} sector peers show no reaction. Isolated anomaly — company-specific event.",
            vortex.symbol.0, vortex.tension, silent.len()
        )
    } else if active.is_empty() && silent.is_empty() {
        format!(
            "{} has tension {:.3} with no known sector peers to compare.",
            vortex.symbol.0, vortex.tension
        )
    } else {
        let active_names: Vec<String> = active
            .iter()
            .take(3)
            .map(|(s, t)| format!("{}({:.3})", s.0, t))
            .collect();
        format!(
            "{} tension {:.3}. {} peers reacting: {}. {} peers silent. {}",
            vortex.symbol.0,
            vortex.tension,
            active.len(),
            active_names.join(", "),
            silent.len(),
            if active.len() >= 2 {
                "Sector-wide movement."
            } else {
                "Mostly isolated."
            }
        )
    };

    PropagationAbsence {
        source_symbol: vortex.symbol.clone(),
        source_tension: vortex.tension,
        silent_neighbors: silent,
        active_neighbors: active,
        is_isolated,
        narrative,
    }
}

// ═══════════════════════════════════════════════════════════════════
// 3. COMPETITION — Which explanation is more credible?
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompetingExplanation {
    pub label: String,
    pub confidence: Decimal,
    pub basis: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompetitionResult {
    pub symbol: Symbol,
    pub winner: CompetingExplanation,
    pub runner_up: Option<CompetingExplanation>,
    pub narrative: String,
}

pub fn compete_explanations(
    attribution: &TensionAttribution,
    absence: &PropagationAbsence,
) -> CompetitionResult {
    // Build candidate explanations from attribution + absence.
    let mut explanations: Vec<CompetingExplanation> = Vec::new();

    // Explanation from attribution driver.
    let driver_confidence = Decimal::from(attribution.contributing_channels.len() as i64)
        * Decimal::new(2, 1); // 0.2 per channel
    let driver_confidence = driver_confidence.min(Decimal::ONE);
    explanations.push(CompetingExplanation {
        label: format!("{:?}", attribution.driver),
        confidence: driver_confidence,
        basis: format!(
            "{} channels active, {} silent",
            attribution.contributing_channels.len(),
            attribution.silent_channels.len()
        ),
    });

    // If isolated: add "company-specific" explanation with bonus confidence.
    if absence.is_isolated {
        explanations.push(CompetingExplanation {
            label: "CompanySpecific".into(),
            confidence: (driver_confidence + Decimal::new(2, 1)).min(Decimal::ONE),
            basis: format!(
                "Isolated anomaly: {} sector peers silent",
                absence.silent_neighbors.len()
            ),
        });
    }

    // If peers are also active: add "sector-wide" explanation.
    if absence.active_neighbors.len() >= 2 {
        let peer_strength: Decimal = absence
            .active_neighbors
            .iter()
            .map(|(_, t)| *t)
            .sum::<Decimal>()
            / Decimal::from(absence.active_neighbors.len() as i64);
        explanations.push(CompetingExplanation {
            label: "SectorWide".into(),
            confidence: (peer_strength * Decimal::TWO).min(Decimal::ONE),
            basis: format!(
                "{} peers also active, mean tension {:.3}",
                absence.active_neighbors.len(),
                peer_strength
            ),
        });
    }

    // Sort by confidence, pick winner + runner-up.
    explanations.sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());

    let winner = explanations.remove(0);
    let runner_up = explanations.into_iter().next();

    let narrative = match &runner_up {
        Some(ru) => format!(
            "{}: best explanation is {} (conf={:.2}) over {} (conf={:.2}). {}",
            attribution.symbol.0, winner.label, winner.confidence, ru.label, ru.confidence, winner.basis
        ),
        None => format!(
            "{}: {} (conf={:.2}). {}",
            attribution.symbol.0, winner.label, winner.confidence, winner.basis
        ),
    };

    CompetitionResult {
        symbol: attribution.symbol.clone(),
        winner,
        runner_up,
        narrative,
    }
}

// ═══════════════════════════════════════════════════════════════════
// 4. LIFECYCLE — Is the anomaly growing, peaking, or dying?
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AnomalyPhase {
    /// Tension is increasing (velocity > 0, acceleration >= 0).
    Growing,
    /// Tension is still positive but decelerating (velocity > 0, acceleration < 0).
    Peaking,
    /// Tension is decreasing (velocity <= 0).
    Fading,
    /// First observation, no history yet.
    New,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnomalyLifecycle {
    pub symbol: Symbol,
    pub phase: AnomalyPhase,
    pub tension: Decimal,
    pub velocity: Decimal,
    pub acceleration: Decimal,
    pub ticks_alive: u64,
    pub peak_tension: Decimal,
    pub narrative: String,
}

/// Tracks tension history per symbol for velocity/acceleration computation.
#[derive(Debug, Clone, Default)]
pub struct LifecycleTracker {
    history: HashMap<Symbol, Vec<(u64, Decimal)>>, // (tick, tension)
    peaks: HashMap<Symbol, Decimal>,
}

impl LifecycleTracker {
    /// Record current tension for a symbol. Call every tick for active vortices.
    pub fn record(&mut self, symbol: &Symbol, tick: u64, tension: Decimal) {
        let entry = self.history.entry(symbol.clone()).or_default();
        entry.push((tick, tension));
        // Keep last 60 data points.
        if entry.len() > 60 {
            entry.remove(0);
        }
        let peak = self.peaks.entry(symbol.clone()).or_insert(Decimal::ZERO);
        if tension > *peak {
            *peak = tension;
        }
    }

    /// Remove symbols that haven't been seen recently.
    pub fn decay(&mut self, current_tick: u64) {
        let cutoff = current_tick.saturating_sub(30);
        self.history.retain(|_, entries| {
            entries.last().map(|(t, _)| *t >= cutoff).unwrap_or(false)
        });
        let active: std::collections::HashSet<Symbol> = self.history.keys().cloned().collect();
        self.peaks.retain(|s, _| active.contains(s));
    }

    /// Compute lifecycle for a symbol.
    pub fn lifecycle(&self, symbol: &Symbol) -> AnomalyLifecycle {
        let empty_narrative = AnomalyLifecycle {
            symbol: symbol.clone(),
            phase: AnomalyPhase::New,
            tension: Decimal::ZERO,
            velocity: Decimal::ZERO,
            acceleration: Decimal::ZERO,
            ticks_alive: 0,
            peak_tension: Decimal::ZERO,
            narrative: format!("{} — first observation", symbol.0),
        };

        let entries = match self.history.get(symbol) {
            Some(e) if e.len() >= 2 => e,
            _ => return empty_narrative,
        };

        let current = entries.last().unwrap().1;
        let prev = entries[entries.len() - 2].1;
        let velocity = current - prev;

        let acceleration = if entries.len() >= 3 {
            let prev_prev = entries[entries.len() - 3].1;
            let prev_velocity = prev - prev_prev;
            velocity - prev_velocity
        } else {
            Decimal::ZERO
        };

        let ticks_alive = entries.len() as u64;
        let peak = self.peaks.get(symbol).copied().unwrap_or(current);

        let phase = if velocity > Decimal::new(1, 3) && acceleration >= Decimal::ZERO {
            AnomalyPhase::Growing
        } else if velocity > Decimal::ZERO && acceleration < Decimal::ZERO {
            AnomalyPhase::Peaking
        } else {
            AnomalyPhase::Fading
        };

        let narrative = match phase {
            AnomalyPhase::Growing => format!(
                "{} tension GROWING: {:.3} (vel={:+.3} acc={:+.3}). Alive {} ticks. Hold.",
                symbol.0, current, velocity, acceleration, ticks_alive
            ),
            AnomalyPhase::Peaking => format!(
                "{} tension PEAKING: {:.3} (vel={:+.3} acc={:+.3}). Peak was {:.3}. Prepare to exit.",
                symbol.0, current, velocity, acceleration, peak
            ),
            AnomalyPhase::Fading => format!(
                "{} tension FADING: {:.3} (vel={:+.3}). Peak was {:.3}. Exit signal.",
                symbol.0, current, velocity, peak
            ),
            AnomalyPhase::New => format!("{} — first observation", symbol.0),
        };

        AnomalyLifecycle {
            symbol: symbol.clone(),
            phase,
            tension: current,
            velocity,
            acceleration,
            ticks_alive,
            peak_tension: peak,
            narrative,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════
// COMBINED: Full reasoning output for a vortex
// ═══════════════════════════════════════════════════════════════════

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VortexInsight {
    pub symbol: Symbol,
    pub attribution: TensionAttribution,
    pub absence: PropagationAbsence,
    pub competition: CompetitionResult,
    pub lifecycle: AnomalyLifecycle,
    /// One-sentence summary for Claude Code / operator.
    pub summary: String,
}

pub fn reason_about_vortex(
    vortex: &PressureVortex,
    field: &PressureField,
    lifecycle_tracker: &LifecycleTracker,
    sector_members: &HashMap<SectorId, Vec<Symbol>>,
    symbol_sector: &HashMap<Symbol, SectorId>,
) -> Option<VortexInsight> {
    let minute_layer = field.layers.get(&TimeScale::Minute)?;
    let node = minute_layer.pressures.get(&vortex.symbol)?;

    let attribution = attribute_tension(vortex, node);
    let absence = detect_absence(vortex, field, sector_members, symbol_sector);
    let competition = compete_explanations(&attribution, &absence);
    let lifecycle = lifecycle_tracker.lifecycle(&vortex.symbol);

    let summary = format!(
        "{} | {} | {} | {} | conf={:.2}",
        vortex.symbol.0,
        match lifecycle.phase {
            AnomalyPhase::Growing => "GROWING",
            AnomalyPhase::Peaking => "PEAKING",
            AnomalyPhase::Fading => "FADING",
            AnomalyPhase::New => "NEW",
        },
        competition.winner.label,
        if absence.is_isolated {
            "isolated"
        } else {
            "sector-linked"
        },
        competition.winner.confidence,
    );

    Some(VortexInsight {
        symbol: vortex.symbol.clone(),
        attribution,
        absence,
        competition,
        lifecycle,
        summary,
    })
}
