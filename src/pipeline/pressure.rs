//! Pressure Field Engine
//!
//! Data flows in → creates pressure at graph nodes → propagates along edges → vortices emerge.
//! No templates. No predefined patterns. The topology determines what matters.

#[path = "pressure/bridge.rs"]
pub mod bridge;

use std::collections::HashMap;

use petgraph::visit::EdgeRef;
use petgraph::Direction as GraphDirection;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::graph::edge_learning::{EdgeKey, EdgeLearningLedger};
use crate::graph::graph::{BrainGraph, EdgeKind, NodeKind};
use crate::ontology::objects::Symbol;
use crate::pipeline::dimensions::SymbolDimensions;
use crate::us::graph::graph::{UsEdgeKind, UsGraph, UsNodeKind};
use crate::us::pipeline::dimensions::UsSymbolDimensions;

// ── Pressure Channels ──
// Each channel represents an independent information stream.
// Vortices form where multiple independent channels converge.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PressureChannel {
    OrderBook,
    CapitalFlow,
    Institutional,
    Momentum,
    Volume,
    Structure,
}

impl PressureChannel {
    pub const ALL: &[PressureChannel] = &[
        PressureChannel::OrderBook,
        PressureChannel::CapitalFlow,
        PressureChannel::Institutional,
        PressureChannel::Momentum,
        PressureChannel::Volume,
        PressureChannel::Structure,
    ];
}

// ── Per-channel pressure at a node ──

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ChannelPressure {
    /// Pressure from the node's own signals.
    pub local: Decimal,
    /// Pressure received through graph edges from neighbors.
    pub propagated: Decimal,
}

impl ChannelPressure {
    pub fn net(&self) -> Decimal {
        self.local + self.propagated
    }
}

// ── Aggregate pressure at a node ──

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct NodePressure {
    pub channels: HashMap<PressureChannel, ChannelPressure>,
    /// Net pressure across all channels (signed: + bullish, - bearish).
    pub composite: Decimal,
    /// How many channels agree in direction [0, 1].
    pub convergence: Decimal,
    /// How much channels disagree [0, 1].
    pub conflict: Decimal,
}

// ── Vortex: emergent convergence point ──

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PressureVortex {
    pub symbol: Symbol,
    /// Overall strength = |composite| * convergence.
    pub strength: Decimal,
    /// Channel agreement [0, 1].
    pub coherence: Decimal,
    /// Net direction across all contributing channels.
    pub direction: Decimal,
    /// Which channels contribute meaningfully.
    pub active_channels: Vec<PressureChannel>,
    /// Number of active channels.
    pub channel_count: usize,
}

// ── Multi-scale accumulator ──
// Each scale accumulates pressure with a different decay rate.
// Tick = instant snapshot, Minute = short-term trend, Hour = medium, Day = background.

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum TimeScale {
    Tick,
    Minute,
    Hour,
    Day,
}

impl TimeScale {
    /// Exponential decay factor per tick.
    /// Tick: no decay (replaced each tick).
    /// Minute: ~60-tick half-life.
    /// Hour: ~720-tick half-life.
    /// Day: ~5000-tick half-life.
    fn decay_factor(&self) -> Decimal {
        match self {
            TimeScale::Tick => Decimal::ZERO,
            TimeScale::Minute => Decimal::new(9885, 4),  // 0.9885 → half-life ~60 ticks
            TimeScale::Hour => Decimal::new(9990, 4),    // 0.9990 → half-life ~693 ticks
            TimeScale::Day => Decimal::new(9999, 4),     // 0.9999 → half-life ~6931 ticks
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct ScaledPressure {
    pub pressures: HashMap<Symbol, NodePressure>,
}

// ── The Pressure Field ──

#[derive(Debug, Clone)]
pub struct PressureField {
    pub timestamp: OffsetDateTime,
    /// Pressure at each time scale.
    pub layers: HashMap<TimeScale, ScaledPressure>,
    /// Detected vortices (multi-channel convergence points).
    pub vortices: Vec<PressureVortex>,
}

impl PressureField {
    pub fn new(timestamp: OffsetDateTime) -> Self {
        let mut layers = HashMap::new();
        layers.insert(TimeScale::Tick, ScaledPressure::default());
        layers.insert(TimeScale::Minute, ScaledPressure::default());
        layers.insert(TimeScale::Hour, ScaledPressure::default());
        layers.insert(TimeScale::Day, ScaledPressure::default());
        Self {
            timestamp,
            layers,
            vortices: Vec::new(),
        }
    }

    /// Core tick: inject local pressure, propagate, decay, detect vortices.
    pub fn tick(
        &mut self,
        timestamp: OffsetDateTime,
        dimensions: &HashMap<Symbol, SymbolDimensions>,
        brain: &BrainGraph,
        edge_ledger: &EdgeLearningLedger,
    ) {
        self.timestamp = timestamp;

        // 1. Compute instant (tick-scale) pressure from raw dimensions.
        let tick_pressure = compute_local_pressure(dimensions);

        // 2. Propagate tick pressure through graph edges.
        let propagated = propagate_pressure(&tick_pressure, brain, edge_ledger);

        // 3. Merge local + propagated into full tick snapshot.
        let tick_snapshot = merge_pressure(tick_pressure, propagated);

        // 4. Replace tick layer, accumulate into longer scales.
        self.layers.insert(TimeScale::Tick, tick_snapshot.clone());
        for scale in &[TimeScale::Minute, TimeScale::Hour, TimeScale::Day] {
            let decay = scale.decay_factor();
            let layer = self.layers.entry(*scale).or_default();
            accumulate_into(layer, &tick_snapshot, decay);
        }

        // 5. Detect vortices from the minute-scale layer (smoothed, not noisy tick).
        let minute_layer = self.layers.get(&TimeScale::Minute).unwrap();
        self.vortices = detect_vortices(minute_layer);
    }

    /// US tick: same engine, different data sources and graph topology.
    pub fn tick_us(
        &mut self,
        timestamp: OffsetDateTime,
        dimensions: &HashMap<Symbol, UsSymbolDimensions>,
        graph: &UsGraph,
    ) {
        self.timestamp = timestamp;

        let tick_pressure = compute_us_local_pressure(dimensions);
        let propagated = propagate_us_pressure(&tick_pressure, graph);
        let tick_snapshot = merge_pressure(tick_pressure, propagated);

        self.layers.insert(TimeScale::Tick, tick_snapshot.clone());
        for scale in &[TimeScale::Minute, TimeScale::Hour, TimeScale::Day] {
            let decay = scale.decay_factor();
            let layer = self.layers.entry(*scale).or_default();
            accumulate_into(layer, &tick_snapshot, decay);
        }

        let minute_layer = self.layers.get(&TimeScale::Minute).unwrap();
        self.vortices = detect_vortices(minute_layer);
    }

    pub fn node_pressure(&self, symbol: &Symbol, scale: TimeScale) -> Option<&NodePressure> {
        self.layers.get(&scale)?.pressures.get(symbol)
    }
}

// ── Step 1: Create local pressure from dimensions ──

fn compute_local_pressure(
    dimensions: &HashMap<Symbol, SymbolDimensions>,
) -> ScaledPressure {
    let mut pressures = HashMap::new();

    for (symbol, dims) in dimensions {
        let mut channels = HashMap::new();

        channels.insert(PressureChannel::OrderBook, ChannelPressure {
            local: dims.order_book_pressure,
            propagated: Decimal::ZERO,
        });
        channels.insert(PressureChannel::CapitalFlow, ChannelPressure {
            local: dims.capital_flow_direction,
            propagated: Decimal::ZERO,
        });
        channels.insert(PressureChannel::Institutional, ChannelPressure {
            local: dims.institutional_direction,
            propagated: Decimal::ZERO,
        });
        channels.insert(PressureChannel::Momentum, ChannelPressure {
            local: dims.activity_momentum,
            propagated: Decimal::ZERO,
        });
        channels.insert(PressureChannel::Volume, ChannelPressure {
            local: dims.capital_size_divergence,
            propagated: Decimal::ZERO,
        });
        channels.insert(PressureChannel::Structure, ChannelPressure {
            local: dims.depth_structure_imbalance,
            propagated: Decimal::ZERO,
        });

        let node = compute_node_aggregate(&channels);
        pressures.insert(symbol.clone(), node);
    }

    ScaledPressure { pressures }
}

// ── Step 2: Propagate pressure along graph edges ──

fn propagate_pressure(
    local: &ScaledPressure,
    brain: &BrainGraph,
    edge_ledger: &EdgeLearningLedger,
) -> HashMap<Symbol, HashMap<PressureChannel, Decimal>> {
    let mut received: HashMap<Symbol, HashMap<PressureChannel, Decimal>> = HashMap::new();

    // For each stock node, look at incoming edges and absorb pressure from neighbors.
    for (symbol, &node_idx) in &brain.stock_nodes {
        let mut channel_acc: HashMap<PressureChannel, (Decimal, Decimal)> = HashMap::new(); // (weighted_sum, weight_total)

        for edge in brain.graph.edges_directed(node_idx, GraphDirection::Incoming) {
            match edge.weight() {
                EdgeKind::InstitutionToStock(e) => {
                    // Institution → Stock: institutional channel propagates direction.
                    let source_node = &brain.graph[edge.source()];
                    let inst_id = match source_node {
                        NodeKind::Institution(inst) => &inst.institution_id,
                        _ => continue,
                    };
                    let edge_key = EdgeKey::InstitutionToStock {
                        institution_id: inst_id.clone(),
                        symbol: symbol.clone(),
                    };
                    let multiplier = edge_ledger.weight_multiplier(&edge_key);
                    let weight = Decimal::from(e.seat_count as i64) * multiplier;
                    let direction = e.direction;

                    let acc = channel_acc.entry(PressureChannel::Institutional).or_default();
                    acc.0 += direction * weight;
                    acc.1 += weight;
                }
                EdgeKind::StockToStock(e) => {
                    // Stock → Stock: propagate all channels from neighbor, weighted by similarity.
                    let source_node = &brain.graph[edge.source()];
                    let neighbor_symbol = match source_node {
                        NodeKind::Stock(s) => &s.symbol,
                        _ => continue,
                    };
                    let (a, b) = if symbol.0 <= neighbor_symbol.0 {
                        (symbol.clone(), neighbor_symbol.clone())
                    } else {
                        (neighbor_symbol.clone(), symbol.clone())
                    };
                    let edge_key = EdgeKey::StockToStock { a, b };
                    let multiplier = edge_ledger.weight_multiplier(&edge_key);
                    let weight = e.similarity * multiplier;
                    if weight <= Decimal::ZERO {
                        continue;
                    }

                    // Propagate each channel from the neighbor.
                    if let Some(neighbor_pressure) = local.pressures.get(neighbor_symbol) {
                        for (channel, cp) in &neighbor_pressure.channels {
                            let acc = channel_acc.entry(*channel).or_default();
                            acc.0 += cp.local * weight;
                            acc.1 += weight;
                        }
                    }
                }
                EdgeKind::StockToSector(e) => {
                    // Sector → Stock (reverse direction): sector coherence propagates.
                    let source_node = &brain.graph[edge.source()];
                    let sector_id = match source_node {
                        NodeKind::Sector(s) => &s.sector_id,
                        _ => continue,
                    };
                    let edge_key = EdgeKey::StockToSector {
                        symbol: symbol.clone(),
                        sector_id: sector_id.clone(),
                    };
                    let multiplier = edge_ledger.weight_multiplier(&edge_key);
                    let weight = e.weight * multiplier;

                    // Sector mean direction propagates as momentum channel.
                    if let NodeKind::Sector(sector) = source_node {
                        let acc = channel_acc.entry(PressureChannel::Momentum).or_default();
                        acc.0 += sector.mean_direction * weight;
                        acc.1 += weight;
                    }
                }
                _ => {}
            }
        }

        // Convert weighted sums to propagated pressure values.
        let mut propagated_channels = HashMap::new();
        for (channel, (weighted_sum, weight_total)) in channel_acc {
            if weight_total > Decimal::ZERO {
                propagated_channels.insert(channel, weighted_sum / weight_total);
            }
        }
        if !propagated_channels.is_empty() {
            received.insert(symbol.clone(), propagated_channels);
        }
    }

    received
}

// ── Step 3: Merge local + propagated ──

fn merge_pressure(
    mut local: ScaledPressure,
    propagated: HashMap<Symbol, HashMap<PressureChannel, Decimal>>,
) -> ScaledPressure {
    for (symbol, prop_channels) in propagated {
        let node = local.pressures.entry(symbol).or_default();
        for (channel, prop_value) in prop_channels {
            let cp = node.channels.entry(channel).or_default();
            cp.propagated = prop_value;
        }
        // Recompute aggregate after adding propagated.
        let aggregate = compute_node_aggregate(&node.channels);
        node.composite = aggregate.composite;
        node.convergence = aggregate.convergence;
        node.conflict = aggregate.conflict;
    }

    // Also recompute for nodes with only local (no propagated).
    for node in local.pressures.values_mut() {
        let aggregate = compute_node_aggregate(&node.channels);
        node.composite = aggregate.composite;
        node.convergence = aggregate.convergence;
        node.conflict = aggregate.conflict;
    }

    local
}

// ── Step 4: Accumulate into longer-scale layers with decay ──

fn accumulate_into(layer: &mut ScaledPressure, tick: &ScaledPressure, decay: Decimal) {
    // Decay existing pressure.
    for node in layer.pressures.values_mut() {
        for cp in node.channels.values_mut() {
            cp.local *= decay;
            cp.propagated *= decay;
        }
        node.composite *= decay;
    }

    // Add tick pressure (scaled down so accumulation is additive).
    let contribution = Decimal::ONE - decay; // complement of decay
    for (symbol, tick_node) in &tick.pressures {
        let node = layer.pressures.entry(symbol.clone()).or_default();
        for (channel, tick_cp) in &tick_node.channels {
            let cp = node.channels.entry(*channel).or_default();
            cp.local += tick_cp.local * contribution;
            cp.propagated += tick_cp.propagated * contribution;
        }
        // Recompute aggregate.
        let aggregate = compute_node_aggregate(&node.channels);
        node.composite = aggregate.composite;
        node.convergence = aggregate.convergence;
        node.conflict = aggregate.conflict;
    }

    // Prune nodes with negligible pressure.
    layer.pressures.retain(|_, node| {
        node.composite.abs() > Decimal::new(1, 4) // > 0.0001
    });
}

// ── Step 5: Detect vortices ──

fn detect_vortices(layer: &ScaledPressure) -> Vec<PressureVortex> {
    let mut vortices = Vec::new();

    for (symbol, node) in &layer.pressures {
        let mut active_channels = Vec::new();
        let mut direction_sum = Decimal::ZERO;
        let mut magnitude_sum = Decimal::ZERO;
        let mut same_direction_count = 0u32;
        let mut total_active = 0u32;

        let dominant_sign = if node.composite >= Decimal::ZERO {
            Decimal::ONE
        } else {
            Decimal::NEGATIVE_ONE
        };

        for channel in PressureChannel::ALL {
            if let Some(cp) = node.channels.get(channel) {
                let net = cp.net();
                if net.abs() < Decimal::new(1, 3) {
                    continue; // below noise floor
                }
                active_channels.push(*channel);
                total_active += 1;
                direction_sum += net;
                magnitude_sum += net.abs();

                if net * dominant_sign > Decimal::ZERO {
                    same_direction_count += 1;
                }
            }
        }

        // A vortex requires at least 3 independent channels agreeing.
        if total_active < 3 || same_direction_count < 3 {
            continue;
        }

        let coherence = if total_active > 0 {
            Decimal::from(same_direction_count as i64) / Decimal::from(total_active as i64)
        } else {
            Decimal::ZERO
        };

        // Strength = magnitude * coherence. Strong and aligned = vortex.
        let avg_magnitude = magnitude_sum / Decimal::from(total_active as i64);
        let strength = (avg_magnitude * coherence).round_dp(4);

        if strength < Decimal::new(5, 3) {
            continue; // too weak
        }

        let direction = if total_active > 0 {
            (direction_sum / Decimal::from(total_active as i64)).round_dp(4)
        } else {
            Decimal::ZERO
        };

        vortices.push(PressureVortex {
            symbol: symbol.clone(),
            strength,
            coherence: coherence.round_dp(4),
            direction,
            active_channels,
            channel_count: total_active as usize,
        });
    }

    // Sort by strength descending, apply relative threshold + hard cap.
    vortices.sort_by(|a, b| b.strength.cmp(&a.strength));

    // Drop anything below 20% of the top vortex's strength.
    if let Some(top) = vortices.first() {
        let floor = top.strength * Decimal::new(2, 1);
        vortices.retain(|v| v.strength >= floor);
    }

    const MAX_VORTICES: usize = 15;
    vortices.truncate(MAX_VORTICES);
    vortices
}

// ── Helpers ──

fn compute_node_aggregate(channels: &HashMap<PressureChannel, ChannelPressure>) -> NodePressure {
    if channels.is_empty() {
        return NodePressure::default();
    }

    let mut direction_sum = Decimal::ZERO;
    let mut magnitude_sum = Decimal::ZERO;
    let mut active_count = 0u32;
    let mut positive_count = 0u32;
    let mut negative_count = 0u32;

    for cp in channels.values() {
        let net = cp.net();
        if net.abs() < Decimal::new(1, 4) {
            continue; // negligible
        }
        active_count += 1;
        direction_sum += net;
        magnitude_sum += net.abs();
        if net > Decimal::ZERO {
            positive_count += 1;
        } else {
            negative_count += 1;
        }
    }

    let composite = if active_count > 0 {
        (direction_sum / Decimal::from(active_count as i64)).round_dp(4)
    } else {
        Decimal::ZERO
    };

    // Convergence: fraction of channels that agree with the majority direction.
    let majority = std::cmp::max(positive_count, negative_count);
    let convergence = if active_count > 0 {
        (Decimal::from(majority as i64) / Decimal::from(active_count as i64)).round_dp(4)
    } else {
        Decimal::ZERO
    };

    // Conflict: fraction of channels that disagree with majority.
    let minority = std::cmp::min(positive_count, negative_count);
    let conflict = if active_count > 0 {
        (Decimal::from(minority as i64) / Decimal::from(active_count as i64)).round_dp(4)
    } else {
        Decimal::ZERO
    };

    NodePressure {
        channels: channels.clone(),
        composite,
        convergence,
        conflict,
    }
}

// ── US-specific pressure sources ──

fn compute_us_local_pressure(
    dimensions: &HashMap<Symbol, UsSymbolDimensions>,
) -> ScaledPressure {
    let mut pressures = HashMap::new();

    for (symbol, dims) in dimensions {
        let mut channels = HashMap::new();

        // US has fewer channels — map available dimensions.
        // CapitalFlow and Momentum are the primary US signals.
        channels.insert(PressureChannel::CapitalFlow, ChannelPressure {
            local: dims.capital_flow_direction,
            propagated: Decimal::ZERO,
        });
        channels.insert(PressureChannel::Momentum, ChannelPressure {
            local: dims.price_momentum,
            propagated: Decimal::ZERO,
        });
        channels.insert(PressureChannel::Volume, ChannelPressure {
            local: dims.volume_profile,
            propagated: Decimal::ZERO,
        });
        // Pre/post market anomaly → Structure channel (structural dislocation).
        channels.insert(PressureChannel::Structure, ChannelPressure {
            local: dims.pre_post_market_anomaly,
            propagated: Decimal::ZERO,
        });

        let node = compute_node_aggregate(&channels);
        pressures.insert(symbol.clone(), node);
    }

    ScaledPressure { pressures }
}

fn propagate_us_pressure(
    local: &ScaledPressure,
    graph: &UsGraph,
) -> HashMap<Symbol, HashMap<PressureChannel, Decimal>> {
    let mut received: HashMap<Symbol, HashMap<PressureChannel, Decimal>> = HashMap::new();

    for (symbol, &node_idx) in &graph.stock_nodes {
        let mut channel_acc: HashMap<PressureChannel, (Decimal, Decimal)> = HashMap::new();

        for edge in graph.graph.edges_directed(node_idx, GraphDirection::Incoming) {
            match edge.weight() {
                UsEdgeKind::StockToStock(e) => {
                    let source_node = &graph.graph[edge.source()];
                    let neighbor_symbol = match source_node {
                        UsNodeKind::Stock(s) => &s.symbol,
                        _ => continue,
                    };
                    let weight = e.similarity;
                    if weight <= Decimal::ZERO {
                        continue;
                    }
                    if let Some(neighbor_pressure) = local.pressures.get(neighbor_symbol) {
                        for (channel, cp) in &neighbor_pressure.channels {
                            let acc = channel_acc.entry(*channel).or_default();
                            acc.0 += cp.local * weight;
                            acc.1 += weight;
                        }
                    }
                }
                UsEdgeKind::StockToSector(_) => {
                    let source_node = &graph.graph[edge.source()];
                    if let UsNodeKind::Sector(sector) = source_node {
                        let acc = channel_acc.entry(PressureChannel::Momentum).or_default();
                        acc.0 += sector.mean_direction;
                        acc.1 += Decimal::ONE;
                    }
                }
                UsEdgeKind::CrossMarket(e) => {
                    // Cross-market edge: HK signal propagates as CapitalFlow channel.
                    let weight = e.propagation_strength * e.confidence;
                    if weight > Decimal::ZERO {
                        let acc = channel_acc.entry(PressureChannel::CapitalFlow).or_default();
                        acc.0 += e.direction * weight;
                        acc.1 += weight;
                    }
                }
            }
        }

        let mut propagated_channels = HashMap::new();
        for (channel, (weighted_sum, weight_total)) in channel_acc {
            if weight_total > Decimal::ZERO {
                propagated_channels.insert(channel, weighted_sum / weight_total);
            }
        }
        if !propagated_channels.is_empty() {
            received.insert(symbol.clone(), propagated_channels);
        }
    }

    received
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_dims(
        order_book: Decimal,
        capital_flow: Decimal,
        institutional: Decimal,
        momentum: Decimal,
        volume: Decimal,
        structure: Decimal,
    ) -> SymbolDimensions {
        SymbolDimensions {
            order_book_pressure: order_book,
            capital_flow_direction: capital_flow,
            institutional_direction: institutional,
            activity_momentum: momentum,
            capital_size_divergence: volume,
            depth_structure_imbalance: structure,
            valuation_support: Decimal::ZERO,
            candlestick_conviction: Decimal::ZERO,
            multi_horizon_momentum: Decimal::ZERO,
        }
    }

    #[test]
    fn local_pressure_from_dimensions() {
        let mut dimensions = HashMap::new();
        let sym = Symbol("0700.HK".into());
        dimensions.insert(
            sym.clone(),
            make_dims(dec!(0.5), dec!(0.3), dec!(0.7), dec!(0.4), dec!(0.2), dec!(0.6)),
        );

        let local = compute_local_pressure(&dimensions);
        let node = local.pressures.get(&sym).unwrap();

        assert_eq!(node.channels[&PressureChannel::OrderBook].local, dec!(0.5));
        assert_eq!(node.channels[&PressureChannel::CapitalFlow].local, dec!(0.3));
        assert_eq!(node.channels[&PressureChannel::Institutional].local, dec!(0.7));
        assert!(node.composite > Decimal::ZERO);
        assert!(node.convergence > Decimal::ZERO);
    }

    #[test]
    fn vortex_detection_requires_3_channels() {
        let mut pressures = HashMap::new();
        let sym = Symbol("AAPL".into());

        // Only 2 active channels → no vortex.
        let mut channels_2 = HashMap::new();
        channels_2.insert(PressureChannel::OrderBook, ChannelPressure { local: dec!(0.5), propagated: dec!(0.1) });
        channels_2.insert(PressureChannel::CapitalFlow, ChannelPressure { local: dec!(0.4), propagated: dec!(0.1) });
        pressures.insert(sym.clone(), compute_node_aggregate(&channels_2));
        pressures.get_mut(&sym).unwrap().channels = channels_2;
        let layer = ScaledPressure { pressures: pressures.clone() };
        assert!(detect_vortices(&layer).is_empty());

        // 4 aligned channels → vortex.
        let mut channels_4 = HashMap::new();
        channels_4.insert(PressureChannel::OrderBook, ChannelPressure { local: dec!(0.5), propagated: dec!(0.1) });
        channels_4.insert(PressureChannel::CapitalFlow, ChannelPressure { local: dec!(0.4), propagated: dec!(0.1) });
        channels_4.insert(PressureChannel::Institutional, ChannelPressure { local: dec!(0.6), propagated: dec!(0.2) });
        channels_4.insert(PressureChannel::Momentum, ChannelPressure { local: dec!(0.3), propagated: dec!(0.1) });
        let node = compute_node_aggregate(&channels_4);
        let mut full_node = node;
        full_node.channels = channels_4;
        pressures.insert(sym.clone(), full_node);
        let layer = ScaledPressure { pressures };
        let vortices = detect_vortices(&layer);
        assert_eq!(vortices.len(), 1);
        assert_eq!(vortices[0].symbol, sym);
        assert!(vortices[0].strength > Decimal::ZERO);
        assert!(vortices[0].coherence >= dec!(0.75));
    }

    #[test]
    fn conflicting_channels_reduce_coherence() {
        let mut channels = HashMap::new();
        channels.insert(PressureChannel::OrderBook, ChannelPressure { local: dec!(0.5), propagated: dec!(0.0) });
        channels.insert(PressureChannel::CapitalFlow, ChannelPressure { local: dec!(-0.4), propagated: dec!(0.0) });
        channels.insert(PressureChannel::Institutional, ChannelPressure { local: dec!(0.3), propagated: dec!(0.0) });
        channels.insert(PressureChannel::Momentum, ChannelPressure { local: dec!(-0.2), propagated: dec!(0.0) });

        let node = compute_node_aggregate(&channels);
        assert!(node.conflict > Decimal::ZERO);
        // 2 positive, 2 negative → convergence = 0.5, conflict = 0.5
        assert_eq!(node.convergence, dec!(0.5));
        assert_eq!(node.conflict, dec!(0.5));
    }

    #[test]
    fn decay_accumulation() {
        let mut layer = ScaledPressure::default();
        let sym = Symbol("TEST".into());

        let mut tick_channels = HashMap::new();
        tick_channels.insert(PressureChannel::OrderBook, ChannelPressure { local: dec!(1.0), propagated: dec!(0.0) });
        let node = compute_node_aggregate(&tick_channels);
        let mut full_node = node;
        full_node.channels = tick_channels;
        let tick = ScaledPressure {
            pressures: HashMap::from([(sym.clone(), full_node)]),
        };

        let decay = dec!(0.9);
        // First accumulation.
        accumulate_into(&mut layer, &tick, decay);
        let p1 = layer.pressures[&sym].channels[&PressureChannel::OrderBook].local;

        // Second accumulation: previous decays + new contribution.
        accumulate_into(&mut layer, &tick, decay);
        let p2 = layer.pressures[&sym].channels[&PressureChannel::OrderBook].local;

        // Pressure should increase but not double (decay + contribution).
        assert!(p2 > p1);
        assert!(p2 < p1 * Decimal::TWO);
    }
}
