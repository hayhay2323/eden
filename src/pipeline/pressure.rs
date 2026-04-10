//! Pressure Field Engine
//!
//! Data flows in → creates pressure at graph nodes → propagates along edges → vortices emerge.
//! No templates. No predefined patterns. The topology determines what matters.

#[path = "pressure/bridge.rs"]
pub mod bridge;
#[path = "pressure/reasoning.rs"]
pub mod reasoning;

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

// ── Vortex: emergent tension point ──
//
// A vortex is NOT "all channels agree" (that's the past).
// A vortex is "channels DISAGREE across time scales" (that's the future).
//
// Example: option pressure says bearish (hour layer) but stock price says
// bullish (tick layer). This tension means someone is positioning ahead
// of a move that hasn't happened yet. The tension itself is the signal.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PressureVortex {
    pub symbol: Symbol,
    /// Tension strength: how much do the time layers disagree?
    /// High tension = something is building up.
    pub tension: Decimal,
    /// Cross-channel conflict: how much do different channels disagree?
    /// High conflict at hour/day layer = structural divergence.
    pub cross_channel_conflict: Decimal,
    /// Tick-vs-hour divergence: is the short-term moving against the long-term?
    /// Positive = tick is above hour (potential reversal incoming).
    /// Negative = tick is below hour (potential bounce incoming).
    pub temporal_divergence: Decimal,
    /// Hour-layer composite (the "background truth").
    pub hour_direction: Decimal,
    /// Tick-layer composite (the "surface noise").
    pub tick_direction: Decimal,
    /// Which channels have the most tension.
    pub tense_channels: Vec<PressureChannel>,
    /// Number of channels with material tension.
    pub tense_channel_count: usize,
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

#[derive(Debug, Clone)]
struct EdgeAttribution {
    edge_key: EdgeKey,
    net_pressure: Decimal,
    magnitude: Decimal,
}

#[derive(Debug, Clone, Default)]
struct PropagationResult {
    propagated: HashMap<Symbol, HashMap<PressureChannel, Decimal>>,
    edge_attributions: HashMap<Symbol, Vec<EdgeAttribution>>,
}

// ── The Pressure Field ──

/// Anomaly at a node: how much current pressure deviates from baseline.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PressureAnomaly {
    /// Deviation of composite from baseline (positive = unusually strong).
    pub composite_deviation: Decimal,
    /// Deviation of convergence from baseline.
    pub convergence_deviation: Decimal,
    /// Deviation of conflict from baseline.
    pub conflict_deviation: Decimal,
    /// Per-channel deviation from baseline.
    pub channel_deviations: HashMap<PressureChannel, Decimal>,
}

#[derive(Debug, Clone)]
pub struct PressureField {
    pub timestamp: OffsetDateTime,
    /// Pressure at each time scale.
    pub layers: HashMap<TimeScale, ScaledPressure>,
    /// Long-term baseline: what "normal" looks like for each node.
    /// Updated with very slow EMA (decay ~0.9999) so it drifts with market structure.
    pub baseline: ScaledPressure,
    /// Anomalies: how much the current minute-layer deviates from baseline.
    pub anomalies: HashMap<Symbol, PressureAnomaly>,
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
            baseline: ScaledPressure::default(),
            anomalies: HashMap::new(),
            vortices: Vec::new(),
        }
    }

    /// Core tick: inject local pressure, propagate, decay, detect vortices.
    pub fn tick(
        &mut self,
        timestamp: OffsetDateTime,
        dimensions: &HashMap<Symbol, SymbolDimensions>,
        brain: &BrainGraph,
        edge_ledger: &mut EdgeLearningLedger,
    ) {
        self.timestamp = timestamp;
        let hour_before = snapshot_layer_composites(self.layers.get(&TimeScale::Hour));

        // 1. Compute instant (tick-scale) pressure from raw dimensions.
        let tick_pressure = compute_local_pressure(dimensions);

        // 2. Propagate tick pressure through graph edges.
        let propagated = propagate_pressure(&tick_pressure, brain, edge_ledger);

        // 3. Merge local + propagated into full tick snapshot.
        let tick_snapshot = merge_pressure(tick_pressure, &propagated.propagated);

        // 4. Replace tick layer, accumulate into longer scales.
        self.layers.insert(TimeScale::Tick, tick_snapshot.clone());
        for scale in &[TimeScale::Minute, TimeScale::Hour, TimeScale::Day] {
            let decay = scale.decay_factor();
            let layer = self.layers.entry(*scale).or_default();
            accumulate_into(layer, &tick_snapshot, decay);
        }

        // 5. Update baseline (very slow EMA — adapts to market structure drift).
        accumulate_into(&mut self.baseline, &tick_snapshot, Decimal::new(9999, 4));

        // 6. Compute anomalies: how much minute-layer deviates from baseline.
        let minute_layer = self.layers.get(&TimeScale::Minute).unwrap();
        self.anomalies = compute_anomalies(minute_layer, &self.baseline);

        // 7. Detect tension vortices: where tick and hour layers disagree.
        let tick_layer = self.layers.get(&TimeScale::Tick).unwrap();
        let hour_layer = self.layers.get(&TimeScale::Hour).unwrap();
        self.vortices = detect_vortices(tick_layer, hour_layer);

        let hour_after = snapshot_layer_composites(Some(hour_layer));
        let hour_deltas = compute_hour_deltas(&hour_before, &hour_after, &propagated);
        self.learn_from_hour_deltas(hour_deltas, &propagated, edge_ledger);
    }

    /// US tick: same engine, different data sources and graph topology.
    pub fn tick_us(
        &mut self,
        timestamp: OffsetDateTime,
        dimensions: &HashMap<Symbol, UsSymbolDimensions>,
        graph: &UsGraph,
        edge_ledger: &mut EdgeLearningLedger,
    ) {
        self.timestamp = timestamp;
        let hour_before = snapshot_layer_composites(self.layers.get(&TimeScale::Hour));

        let tick_pressure = compute_us_local_pressure(dimensions);
        let propagated = propagate_us_pressure(&tick_pressure, graph, edge_ledger);
        let tick_snapshot = merge_pressure(tick_pressure, &propagated.propagated);

        self.layers.insert(TimeScale::Tick, tick_snapshot.clone());
        for scale in &[TimeScale::Minute, TimeScale::Hour, TimeScale::Day] {
            let decay = scale.decay_factor();
            let layer = self.layers.entry(*scale).or_default();
            accumulate_into(layer, &tick_snapshot, decay);
        }

        accumulate_into(&mut self.baseline, &tick_snapshot, Decimal::new(9999, 4));

        let minute_layer = self.layers.get(&TimeScale::Minute).unwrap();
        self.anomalies = compute_anomalies(minute_layer, &self.baseline);

        let tick_layer = self.layers.get(&TimeScale::Tick).unwrap();
        let hour_layer = self.layers.get(&TimeScale::Hour).unwrap();
        self.vortices = detect_vortices(tick_layer, hour_layer);

        let hour_after = snapshot_layer_composites(Some(hour_layer));
        let hour_deltas = compute_hour_deltas(&hour_before, &hour_after, &propagated);
        self.learn_from_hour_deltas(hour_deltas, &propagated, edge_ledger);
    }

    pub fn node_pressure(&self, symbol: &Symbol, scale: TimeScale) -> Option<&NodePressure> {
        self.layers.get(&scale)?.pressures.get(symbol)
    }

    pub fn node_anomaly(&self, symbol: &Symbol) -> Option<&PressureAnomaly> {
        self.anomalies.get(symbol)
    }

    /// Top anomalies sorted by absolute composite deviation.
    pub fn top_anomalies(&self, limit: usize) -> Vec<(&Symbol, &PressureAnomaly)> {
        let mut ranked: Vec<_> = self.anomalies.iter().collect();
        ranked.sort_by(|a, b| {
            b.1.composite_deviation
                .abs()
                .cmp(&a.1.composite_deviation.abs())
        });
        ranked.truncate(limit);
        ranked
    }

    fn learn_from_hour_deltas(
        &self,
        hour_deltas: HashMap<Symbol, Decimal>,
        propagated_pressures: &PropagationResult,
        edge_ledger: &mut EdgeLearningLedger,
    ) {
        let threshold = Decimal::new(1, 4);
        let noise_debit_scale = Decimal::new(1, 1);

        for (symbol, edge_attributions) in &propagated_pressures.edge_attributions {
            if edge_attributions.is_empty() {
                continue;
            }

            let total_magnitude: Decimal = edge_attributions.iter().map(|edge| edge.magnitude).sum();
            if total_magnitude <= Decimal::ZERO {
                continue;
            }

            let hour_delta = hour_deltas.get(symbol).copied().unwrap_or(Decimal::ZERO);
            let meaningful_move = hour_delta.abs() >= threshold;

            for edge in edge_attributions {
                if edge.magnitude <= Decimal::ZERO {
                    continue;
                }

                let share = edge.magnitude / total_magnitude;
                let credit = if meaningful_move {
                    let alignment = directional_alignment(hour_delta, edge.net_pressure);
                    if alignment == Decimal::ZERO {
                        continue;
                    }
                    hour_delta.abs() * share * alignment
                } else {
                    -(hour_delta.abs().max(threshold) * noise_debit_scale * share)
                };

                update_edge_credit(edge_ledger, &edge.edge_key, credit, self.timestamp);
            }
        }
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
) -> PropagationResult {
    let mut result = PropagationResult::default();

    // For each stock node, look at incoming edges and absorb pressure from neighbors.
    for (symbol, &node_idx) in &brain.stock_nodes {
        let mut channel_acc: HashMap<PressureChannel, (Decimal, Decimal)> = HashMap::new(); // (weighted_sum, weight_total)
        let mut edge_acc: HashMap<EdgeKey, HashMap<PressureChannel, Decimal>> = HashMap::new();

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
                    let edge_channel_acc = edge_acc.entry(edge_key).or_default();
                    *edge_channel_acc
                        .entry(PressureChannel::Institutional)
                        .or_default() += direction * weight;
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
                            let weighted = cp.local * weight;
                            acc.0 += weighted;
                            acc.1 += weight;
                            let edge_channel_acc = edge_acc.entry(edge_key.clone()).or_default();
                            *edge_channel_acc.entry(*channel).or_default() += weighted;
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
                        let weighted = sector.mean_direction * weight;
                        acc.0 += weighted;
                        acc.1 += weight;
                        let edge_channel_acc = edge_acc.entry(edge_key).or_default();
                        *edge_channel_acc.entry(PressureChannel::Momentum).or_default() += weighted;
                    }
                }
                _ => {}
            }
        }

        // Convert weighted sums to propagated pressure values.
        let mut propagated_channels = HashMap::new();
        for (channel, (weighted_sum, weight_total)) in &channel_acc {
            if *weight_total > Decimal::ZERO {
                propagated_channels.insert(*channel, *weighted_sum / *weight_total);
            }
        }
        if !propagated_channels.is_empty() {
            result.propagated.insert(symbol.clone(), propagated_channels);
        }

        let edge_attributions = normalize_edge_attributions(&channel_acc, edge_acc);
        if !edge_attributions.is_empty() {
            result
                .edge_attributions
                .insert(symbol.clone(), edge_attributions);
        }
    }

    result
}

// ── Step 3: Merge local + propagated ──

fn merge_pressure(
    mut local: ScaledPressure,
    propagated: &HashMap<Symbol, HashMap<PressureChannel, Decimal>>,
) -> ScaledPressure {
    for (symbol, prop_channels) in propagated {
        let node = local.pressures.entry(symbol.clone()).or_default();
        for (channel, prop_value) in prop_channels {
            let cp = node.channels.entry(*channel).or_default();
            cp.propagated = *prop_value;
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

// ── Step 5b: Compute anomalies vs baseline ──

fn compute_anomalies(
    current: &ScaledPressure,
    baseline: &ScaledPressure,
) -> HashMap<Symbol, PressureAnomaly> {
    let mut anomalies = HashMap::new();

    for (symbol, node) in &current.pressures {
        let baseline_node = baseline.pressures.get(symbol);
        let base_composite = baseline_node.map(|n| n.composite).unwrap_or(Decimal::ZERO);
        let base_convergence = baseline_node.map(|n| n.convergence).unwrap_or(Decimal::ZERO);
        let base_conflict = baseline_node.map(|n| n.conflict).unwrap_or(Decimal::ZERO);

        let composite_dev = node.composite - base_composite;
        let convergence_dev = node.convergence - base_convergence;
        let conflict_dev = node.conflict - base_conflict;

        // Only record if any deviation is material
        if composite_dev.abs() < Decimal::new(1, 3)
            && convergence_dev.abs() < Decimal::new(1, 3)
            && conflict_dev.abs() < Decimal::new(1, 3)
        {
            continue;
        }

        let mut channel_devs = HashMap::new();
        for channel in PressureChannel::ALL {
            let current_net = node
                .channels
                .get(channel)
                .map(|c| c.net())
                .unwrap_or(Decimal::ZERO);
            let base_net = baseline_node
                .and_then(|n| n.channels.get(channel))
                .map(|c| c.net())
                .unwrap_or(Decimal::ZERO);
            let dev = current_net - base_net;
            if dev.abs() >= Decimal::new(1, 3) {
                channel_devs.insert(*channel, dev);
            }
        }

        anomalies.insert(
            symbol.clone(),
            PressureAnomaly {
                composite_deviation: composite_dev,
                convergence_deviation: convergence_dev,
                conflict_deviation: conflict_dev,
                channel_deviations: channel_devs,
            },
        );
    }

    anomalies
}

// ── Step 6: Detect vortices (tension-based) ──
//
// A vortex is NOT where channels agree (that's the past — already priced in).
// A vortex is where TIME SCALES DISAGREE — the short-term says one thing,
// the long-term says another. This tension means something hasn't resolved yet.
//
// Analogy: if the hour layer shows negative pressure (institutions selling)
// but the tick layer shows positive (price bouncing), that's tension.
// The bounce is temporary; the selling pressure will win eventually.
// OR the selling is exhausted and the bounce is real.
// Either way: tension = something is about to happen.

fn detect_vortices(
    tick_layer: &ScaledPressure,
    hour_layer: &ScaledPressure,
) -> Vec<PressureVortex> {
    let mut vortices = Vec::new();

    // Iterate all symbols in the hour layer (the "background truth").
    for (symbol, hour_node) in &hour_layer.pressures {
        let tick_node = tick_layer.pressures.get(symbol);

        let hour_composite = hour_node.composite;
        let tick_composite = tick_node.map(|n| n.composite).unwrap_or(Decimal::ZERO);

        // Temporal divergence: tick vs hour. Opposite signs = maximum tension.
        let temporal_div = tick_composite - hour_composite;

        // Cross-channel conflict at hour level: channels disagree in direction.
        let hour_conflict = hour_node.conflict;

        // Per-channel tension: which channels diverge most between tick and hour?
        let mut tense_channels = Vec::new();
        let mut max_channel_tension = Decimal::ZERO;
        for channel in PressureChannel::ALL {
            let hour_net = hour_node
                .channels
                .get(channel)
                .map(|c| c.net())
                .unwrap_or(Decimal::ZERO);
            let tick_net = tick_node
                .and_then(|n| n.channels.get(channel))
                .map(|c| c.net())
                .unwrap_or(Decimal::ZERO);
            let channel_tension = (tick_net - hour_net).abs();
            if channel_tension > Decimal::new(5, 3) {
                tense_channels.push(*channel);
                if channel_tension > max_channel_tension {
                    max_channel_tension = channel_tension;
                }
            }
        }

        // Tension = temporal divergence magnitude + cross-channel conflict + channel tension
        let tension = (temporal_div.abs() + hour_conflict + max_channel_tension)
            .round_dp(4);

        // Need at least some tension and at least 1 tense channel
        if tension < Decimal::new(1, 2) || tense_channels.is_empty() {
            continue;
        }

        vortices.push(PressureVortex {
            symbol: symbol.clone(),
            tension,
            cross_channel_conflict: hour_conflict.round_dp(4),
            temporal_divergence: temporal_div.round_dp(4),
            hour_direction: hour_composite.round_dp(4),
            tick_direction: tick_composite.round_dp(4),
            tense_channels,
            tense_channel_count: 0, // set below
        });
    }

    // Set channel count and sort by tension
    for v in &mut vortices {
        v.tense_channel_count = v.tense_channels.len();
    }
    vortices.sort_by(|a, b| b.tension.cmp(&a.tension));

    // Keep top N
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
    edge_ledger: &EdgeLearningLedger,
) -> PropagationResult {
    let mut result = PropagationResult::default();

    for (symbol, &node_idx) in &graph.stock_nodes {
        let mut channel_acc: HashMap<PressureChannel, (Decimal, Decimal)> = HashMap::new();
        let mut edge_acc: HashMap<EdgeKey, HashMap<PressureChannel, Decimal>> = HashMap::new();

        for edge in graph.graph.edges_directed(node_idx, GraphDirection::Incoming) {
            match edge.weight() {
                UsEdgeKind::StockToStock(e) => {
                    let source_node = &graph.graph[edge.source()];
                    let neighbor_symbol = match source_node {
                        UsNodeKind::Stock(s) => &s.symbol,
                        _ => continue,
                    };
                    let (a, b) = if symbol.0 <= neighbor_symbol.0 {
                        (symbol.clone(), neighbor_symbol.clone())
                    } else {
                        (neighbor_symbol.clone(), symbol.clone())
                    };
                    let edge_key = EdgeKey::StockToStock { a, b };
                    let weight = e.similarity * edge_ledger.weight_multiplier(&edge_key);
                    if weight <= Decimal::ZERO {
                        continue;
                    }
                    if let Some(neighbor_pressure) = local.pressures.get(neighbor_symbol) {
                        for (channel, cp) in &neighbor_pressure.channels {
                            let acc = channel_acc.entry(*channel).or_default();
                            let weighted = cp.local * weight;
                            acc.0 += weighted;
                            acc.1 += weight;
                            let edge_channel_acc = edge_acc.entry(edge_key.clone()).or_default();
                            *edge_channel_acc.entry(*channel).or_default() += weighted;
                        }
                    }
                }
                UsEdgeKind::StockToSector(_) => {
                    let source_node = &graph.graph[edge.source()];
                    if let UsNodeKind::Sector(sector) = source_node {
                        let edge_key = EdgeKey::StockToSector {
                            symbol: symbol.clone(),
                            sector_id: sector.sector_id.clone(),
                        };
                        let weight = edge_ledger.weight_multiplier(&edge_key);
                        let acc = channel_acc.entry(PressureChannel::Momentum).or_default();
                        let weighted = sector.mean_direction * weight;
                        acc.0 += weighted;
                        acc.1 += weight;
                        let edge_channel_acc = edge_acc.entry(edge_key).or_default();
                        *edge_channel_acc.entry(PressureChannel::Momentum).or_default() += weighted;
                    }
                }
                UsEdgeKind::CrossMarket(e) => {
                    // Cross-market edge: HK signal propagates as CapitalFlow channel.
                    let (a, b) = if e.us_symbol.0 <= e.hk_symbol.0 {
                        (e.us_symbol.clone(), e.hk_symbol.clone())
                    } else {
                        (e.hk_symbol.clone(), e.us_symbol.clone())
                    };
                    let edge_key = EdgeKey::StockToStock { a, b };
                    let weight =
                        e.propagation_strength * e.confidence * edge_ledger.weight_multiplier(&edge_key);
                    if weight > Decimal::ZERO {
                        let acc = channel_acc.entry(PressureChannel::CapitalFlow).or_default();
                        let weighted = e.direction * weight;
                        acc.0 += weighted;
                        acc.1 += weight;
                        let edge_channel_acc = edge_acc.entry(edge_key).or_default();
                        *edge_channel_acc.entry(PressureChannel::CapitalFlow).or_default() += weighted;
                    }
                }
            }
        }

        let mut propagated_channels = HashMap::new();
        for (channel, (weighted_sum, weight_total)) in &channel_acc {
            if *weight_total > Decimal::ZERO {
                propagated_channels.insert(*channel, *weighted_sum / *weight_total);
            }
        }
        if !propagated_channels.is_empty() {
            result.propagated.insert(symbol.clone(), propagated_channels);
        }

        let edge_attributions = normalize_edge_attributions(&channel_acc, edge_acc);
        if !edge_attributions.is_empty() {
            result
                .edge_attributions
                .insert(symbol.clone(), edge_attributions);
        }
    }

    result
}

fn snapshot_layer_composites(layer: Option<&ScaledPressure>) -> HashMap<Symbol, Decimal> {
    layer
        .map(|layer| {
            layer
                .pressures
                .iter()
                .map(|(symbol, node)| (symbol.clone(), node.composite))
                .collect()
        })
        .unwrap_or_default()
}

fn compute_hour_deltas(
    hour_before: &HashMap<Symbol, Decimal>,
    hour_after: &HashMap<Symbol, Decimal>,
    propagated: &PropagationResult,
) -> HashMap<Symbol, Decimal> {
    let mut hour_deltas = HashMap::new();
    for symbol in hour_before
        .keys()
        .chain(hour_after.keys())
        .chain(propagated.propagated.keys())
    {
        hour_deltas.entry(symbol.clone()).or_insert_with(|| {
            hour_after.get(symbol).copied().unwrap_or(Decimal::ZERO)
                - hour_before.get(symbol).copied().unwrap_or(Decimal::ZERO)
        });
    }
    hour_deltas
}

fn normalize_edge_attributions(
    channel_acc: &HashMap<PressureChannel, (Decimal, Decimal)>,
    edge_acc: HashMap<EdgeKey, HashMap<PressureChannel, Decimal>>,
) -> Vec<EdgeAttribution> {
    let mut attributions = Vec::new();

    for (edge_key, channel_contributions) in edge_acc {
        let mut net_pressure = Decimal::ZERO;
        let mut magnitude = Decimal::ZERO;

        for (channel, weighted_contribution) in channel_contributions {
            let Some((_, weight_total)) = channel_acc.get(&channel) else {
                continue;
            };
            if *weight_total <= Decimal::ZERO {
                continue;
            }

            let normalized = weighted_contribution / *weight_total;
            net_pressure += normalized;
            magnitude += normalized.abs();
        }

        if magnitude > Decimal::ZERO {
            attributions.push(EdgeAttribution {
                edge_key,
                net_pressure,
                magnitude,
            });
        }
    }

    attributions
}

fn directional_alignment(hour_delta: Decimal, propagated_pressure: Decimal) -> Decimal {
    if hour_delta > Decimal::ZERO && propagated_pressure > Decimal::ZERO {
        Decimal::ONE
    } else if hour_delta < Decimal::ZERO && propagated_pressure < Decimal::ZERO {
        Decimal::ONE
    } else if propagated_pressure == Decimal::ZERO {
        Decimal::ZERO
    } else {
        -Decimal::ONE
    }
}

fn update_edge_credit(
    edge_ledger: &mut EdgeLearningLedger,
    edge_key: &EdgeKey,
    credit: Decimal,
    now: OffsetDateTime,
) {
    let entry = edge_ledger.entry_mut_or_insert(edge_key, now);
    entry.total_credit += credit;
    entry.sample_count += 1;
    entry.mean_credit = entry.total_credit / Decimal::from(entry.sample_count);
    entry.last_updated = now;
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

    // Old convergence-based vortex tests removed.
    // Tension-based vortex detection needs tick vs hour layers — tested via integration.

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
