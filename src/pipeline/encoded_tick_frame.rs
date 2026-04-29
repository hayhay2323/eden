//! V7-0 encoded tick frame.
//!
//! This is Eden's first explicit "encode once, decode many" contract.
//! The frame is still ontology-native: it preserves channels, provenance,
//! uncertainty, and graph state instead of collapsing the market into an
//! opaque vector. Downstream stages can use this as their shared perception
//! surface while the runtime migrates away from direct raw reads.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::core::market::{MarketDataCapability, MarketRegistry};
use crate::core::runtime_artifacts::{RuntimeArtifactKind, RuntimeArtifactStore};
use crate::ontology::objects::Symbol;
use crate::pipeline::loopy_bp::{
    GraphEdge, NodePrior, N_STATES, STATE_BEAR, STATE_BULL, STATE_NEUTRAL,
};
use crate::pipeline::pressure::{NodePressure, PressureChannel, PressureField, TimeScale};
use crate::pipeline::symbol_sub_kg::{NodeFreshness, NodeProvenanceSource, SubKgRegistry};

pub const ENCODED_TICK_FRAME_VERSION: &str = "v7-0";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodedTickFrame {
    pub version: String,
    pub market: String,
    pub tick: u64,
    pub ts: DateTime<Utc>,
    pub symbols: Vec<EncodedSymbolFrame>,
    pub master_edges: Vec<EncodedMasterEdge>,
    pub counts: EncodedTickCounts,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct EncodedTickCounts {
    pub symbols: usize,
    pub pressure_channels: usize,
    pub subkg_nodes: usize,
    pub subkg_edges: usize,
    pub master_edges: usize,
    pub bp_posteriors: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncodedSymbolFrame {
    pub symbol: String,
    pub pressure: Vec<EncodedPressureChannel>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pressure_summary: Option<EncodedPressureSummary>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subkg: Option<EncodedSubKgFrame>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bp: Option<EncodedBpFrame>,
    pub provenance: Vec<EncodedProvenance>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EncodedPressureChannel {
    pub scale: TimeScale,
    pub channel: PressureChannel,
    pub local: f64,
    pub propagated: f64,
    pub net: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EncodedPressureSummary {
    pub composite: f64,
    pub convergence: f64,
    pub conflict: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EncodedSubKgFrame {
    pub nodes: Vec<EncodedSubKgNode>,
    pub edges: Vec<EncodedSubKgEdge>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EncodedSubKgNode {
    pub id: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub value: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub aux: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub label: Option<String>,
    pub last_seen_tick: u64,
    pub age_ticks: u64,
    pub freshness: NodeFreshness,
    pub provenance_source: NodeProvenanceSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_capability: Option<MarketDataCapability>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EncodedSubKgEdge {
    pub from: String,
    pub to: String,
    pub kind: String,
    pub weight: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EncodedBpFrame {
    pub observed_prior: bool,
    pub p_bull: f64,
    pub p_bear: f64,
    pub p_neutral: f64,
    pub entropy: f64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct EncodedMasterEdge {
    pub from: String,
    pub to: String,
    pub weight: f64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EncodedProvenance {
    PressureField,
    SubKg,
    LoopyBp,
}

impl EncodedTickFrame {
    pub fn new(market: impl Into<String>, tick: u64, ts: DateTime<Utc>) -> Self {
        Self {
            version: ENCODED_TICK_FRAME_VERSION.to_string(),
            market: market.into(),
            tick,
            ts,
            symbols: Vec::new(),
            master_edges: Vec::new(),
            counts: EncodedTickCounts::default(),
        }
    }

    /// Encode the current pressure field into the shared frame. This is
    /// intentionally a constructor, not a side-effecting decoder hook:
    /// the pressure surface becomes data consumed through the same frame
    /// as graph and BP state.
    pub fn from_pressure_field(
        market: impl Into<String>,
        tick: u64,
        ts: DateTime<Utc>,
        pressure_field: &PressureField,
    ) -> Self {
        let mut frame = Self::new(market, tick, ts);
        frame.attach_pressure_field(pressure_field);
        frame
    }

    pub fn attach_pressure_field(&mut self, pressure_field: &PressureField) {
        for (scale, layer) in &pressure_field.layers {
            for (symbol, pressure) in &layer.pressures {
                self.attach_pressure(symbol, *scale, pressure);
            }
        }
        self.normalize();
    }

    pub fn attach_pressure(
        &mut self,
        symbol: &Symbol,
        scale: TimeScale,
        pressure: &NodePressure,
    ) {
        let sym = self.symbol_mut(&symbol.0);
        insert_provenance(sym, EncodedProvenance::PressureField);
        for (channel, channel_pressure) in &pressure.channels {
            sym.pressure.push(EncodedPressureChannel {
                scale,
                channel: *channel,
                local: decimal_to_f64(channel_pressure.local),
                propagated: decimal_to_f64(channel_pressure.propagated),
                net: decimal_to_f64(channel_pressure.net()),
            });
        }
        if scale == TimeScale::Tick {
            sym.pressure_summary = Some(EncodedPressureSummary {
                composite: decimal_to_f64(pressure.composite),
                convergence: decimal_to_f64(pressure.convergence),
                conflict: decimal_to_f64(pressure.conflict),
            });
        }
    }

    /// Attach the visible sub-KG substrate to the same symbol frame.
    /// This preserves ontology transparency while making sub-KG state a
    /// consumer of the encoded tick frame contract.
    pub fn attach_subkg_registry(&mut self, registry: &SubKgRegistry) {
        for (symbol, kg) in &registry.graphs {
            let sym = self.symbol_mut(symbol);
            insert_provenance(sym, EncodedProvenance::SubKg);
            let mut nodes = kg
                .nodes
                .iter()
                .map(|(id, activation)| EncodedSubKgNode {
                    id: id.to_serde_key(),
                    kind: format!("{:?}", activation.kind),
                    value: activation.value.and_then(|v| v.to_f64()),
                    aux: activation.aux.and_then(|v| v.to_f64()),
                    label: activation.label.clone(),
                    last_seen_tick: activation.last_seen_tick,
                    age_ticks: activation.age_ticks,
                    freshness: activation.freshness,
                    provenance_source: activation.provenance_source,
                    market_capability: activation.market_capability,
                })
                .collect::<Vec<_>>();
            nodes.sort_by(|a, b| a.id.cmp(&b.id));

            let mut edges = kg
                .edges
                .iter()
                .map(|edge| EncodedSubKgEdge {
                    from: edge.from.to_serde_key(),
                    to: edge.to.to_serde_key(),
                    kind: format!("{:?}", edge.kind),
                    weight: decimal_to_f64(edge.weight),
                })
                .collect::<Vec<_>>();
            edges.sort_by(|a, b| {
                a.from
                    .cmp(&b.from)
                    .then_with(|| a.to.cmp(&b.to))
                    .then_with(|| a.kind.cmp(&b.kind))
            });

            sym.subkg = Some(EncodedSubKgFrame { nodes, edges });
        }
        self.normalize();
    }

    /// Attach BP priors, posteriors, and sparse master graph edges.
    /// This creates the first common frame that can feed wake, visual
    /// export, and later decoders from one representation.
    pub fn attach_bp_state(
        &mut self,
        priors: &HashMap<String, NodePrior>,
        beliefs: &HashMap<String, [f64; N_STATES]>,
        master_edges: &[GraphEdge],
    ) {
        for (symbol, posterior) in beliefs {
            let prior = priors.get(symbol).cloned().unwrap_or_default();
            let sym = self.symbol_mut(symbol);
            insert_provenance(sym, EncodedProvenance::LoopyBp);
            sym.bp = Some(EncodedBpFrame {
                observed_prior: prior.observed,
                p_bull: posterior[STATE_BULL],
                p_bear: posterior[STATE_BEAR],
                p_neutral: posterior[STATE_NEUTRAL],
                entropy: categorical_entropy(posterior),
            });
        }
        self.master_edges = master_edges
            .iter()
            .map(|edge| EncodedMasterEdge {
                from: edge.from.clone(),
                to: edge.to.clone(),
                weight: edge.weight,
            })
            .collect();
        self.normalize();
    }

    pub fn symbol(&self, symbol: &str) -> Option<&EncodedSymbolFrame> {
        self.symbols.iter().find(|s| s.symbol == symbol)
    }

    fn symbol_mut(&mut self, symbol: &str) -> &mut EncodedSymbolFrame {
        if let Some(idx) = self.symbols.iter().position(|s| s.symbol == symbol) {
            return &mut self.symbols[idx];
        }
        self.symbols.push(EncodedSymbolFrame {
            symbol: symbol.to_string(),
            pressure: Vec::new(),
            pressure_summary: None,
            subkg: None,
            bp: None,
            provenance: Vec::new(),
        });
        self.symbols.last_mut().expect("just pushed symbol")
    }

    fn normalize(&mut self) {
        for symbol in &mut self.symbols {
            symbol.pressure.sort_by(|a, b| {
                time_scale_rank(a.scale)
                    .cmp(&time_scale_rank(b.scale))
                    .then_with(|| {
                        pressure_channel_rank(a.channel).cmp(&pressure_channel_rank(b.channel))
                    })
            });
            symbol.provenance.sort_by_key(|p| format!("{:?}", p));
            symbol.provenance.dedup();
        }
        self.symbols.sort_by(|a, b| a.symbol.cmp(&b.symbol));
        self.master_edges.sort_by(|a, b| {
            a.from
                .cmp(&b.from)
                .then_with(|| a.to.cmp(&b.to))
                .then_with(|| a.weight.total_cmp(&b.weight))
        });
        self.recompute_counts();
    }

    fn recompute_counts(&mut self) {
        self.counts.symbols = self.symbols.len();
        self.counts.pressure_channels = self.symbols.iter().map(|s| s.pressure.len()).sum();
        self.counts.subkg_nodes = self
            .symbols
            .iter()
            .filter_map(|s| s.subkg.as_ref())
            .map(|g| g.nodes.len())
            .sum();
        self.counts.subkg_edges = self
            .symbols
            .iter()
            .filter_map(|s| s.subkg.as_ref())
            .map(|g| g.edges.len())
            .sum();
        self.counts.master_edges = self.master_edges.len();
        self.counts.bp_posteriors = self.symbols.iter().filter(|s| s.bp.is_some()).count();
    }
}

impl EncodedSymbolFrame {
    pub fn pressure_by_scale(&self) -> HashMap<TimeScale, Vec<&EncodedPressureChannel>> {
        let mut out: HashMap<TimeScale, Vec<&EncodedPressureChannel>> = HashMap::new();
        for channel in &self.pressure {
            out.entry(channel.scale).or_default().push(channel);
        }
        out
    }
}

pub fn write_frame(market: &str, frame: &EncodedTickFrame) -> std::io::Result<usize> {
    let market = MarketRegistry::by_slug(market).ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("unknown market for encoded tick frame: {market}"),
        )
    })?;
    RuntimeArtifactStore::default().append_json_line(
        RuntimeArtifactKind::EncodedTickFrame,
        market,
        frame,
    )?;
    Ok(1)
}

fn insert_provenance(symbol: &mut EncodedSymbolFrame, provenance: EncodedProvenance) {
    if !symbol.provenance.contains(&provenance) {
        symbol.provenance.push(provenance);
    }
}

fn decimal_to_f64(value: Decimal) -> f64 {
    value.to_f64().unwrap_or(0.0)
}

fn time_scale_rank(scale: TimeScale) -> u8 {
    match scale {
        TimeScale::Tick => 0,
        TimeScale::Minute => 1,
        TimeScale::Hour => 2,
        TimeScale::Day => 3,
    }
}

fn pressure_channel_rank(channel: PressureChannel) -> u8 {
    match channel {
        PressureChannel::OrderBook => 0,
        PressureChannel::CapitalFlow => 1,
        PressureChannel::Institutional => 2,
        PressureChannel::Momentum => 3,
        PressureChannel::Volume => 4,
        PressureChannel::Structure => 5,
    }
}

fn categorical_entropy<const N: usize>(posterior: &[f64; N]) -> f64 {
    posterior
        .iter()
        .copied()
        .filter(|p| *p > 0.0 && p.is_finite())
        .map(|p| -p * p.ln())
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    use crate::pipeline::pressure::{ChannelPressure, PressureChannel, ScaledPressure};
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    #[test]
    fn pressure_field_encodes_once_per_tick() {
        let mut field = PressureField::new(OffsetDateTime::UNIX_EPOCH);
        let mut pressure = NodePressure::default();
        pressure.channels.insert(
            PressureChannel::OrderBook,
            ChannelPressure {
                local: dec!(0.30),
                propagated: dec!(0.20),
            },
        );
        pressure.composite = dec!(0.50);
        pressure.convergence = dec!(0.75);
        pressure.conflict = dec!(0.10);

        let mut layer = ScaledPressure::default();
        layer
            .pressures
            .insert(Symbol("AAPL.US".to_string()), pressure);
        field.layers.insert(TimeScale::Tick, layer);

        let frame = EncodedTickFrame::from_pressure_field("us", 42, Utc::now(), &field);
        let symbol = frame.symbol("AAPL.US").expect("symbol frame");

        assert_eq!(frame.version, ENCODED_TICK_FRAME_VERSION);
        assert_eq!(frame.counts.symbols, 1);
        assert_eq!(frame.counts.pressure_channels, 1);
        assert_eq!(symbol.provenance, vec![EncodedProvenance::PressureField]);
        assert_eq!(symbol.pressure[0].scale, TimeScale::Tick);
        assert_eq!(symbol.pressure[0].channel, PressureChannel::OrderBook);
        assert!((symbol.pressure[0].net - 0.50).abs() < 1e-9);
        assert_eq!(
            symbol.pressure_summary.as_ref().map(|s| s.composite),
            Some(0.50)
        );
    }

    #[test]
    fn bp_entropy_is_encoded_with_posterior() {
        let mut frame = EncodedTickFrame::new("us", 7, Utc::now());
        let mut beliefs = HashMap::new();
        beliefs.insert("MSFT.US".to_string(), [0.70, 0.20, 0.10]);

        frame.attach_bp_state(&HashMap::new(), &beliefs, &[]);
        let bp = frame.symbol("MSFT.US").and_then(|s| s.bp.as_ref()).unwrap();

        assert!((bp.p_bull - 0.70).abs() < 1e-9);
        assert!(bp.entropy > 0.0);
        assert_eq!(frame.counts.bp_posteriors, 1);
    }
}
