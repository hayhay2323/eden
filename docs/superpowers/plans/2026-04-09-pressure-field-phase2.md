# Pressure Field Phase 2 — From Topology to Action

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the pressure field produce operator-visible output (vortex → TacticalSetup → action), fix the single-hop propagation to be recursive, and tighten vortex detection so it stops producing 191 false positives.

**Architecture:** Three independent subsystems built in order of impact: (1) Vortex→Action bridge converts PressureVortex into TacticalSetup/HypothesisTrack so the operator sees results immediately, (2) Multi-hop propagation replaces single-hop with iterative wave propagation so pressure actually flows through the graph, (3) Gradient-based vortex detection replaces threshold matching with pressure gradient convergence so only real vortices survive.

**Tech Stack:** Rust, petgraph (graph traversal), rust_decimal (precision arithmetic), existing Eden ontology types

---

## File Structure

| File | Responsibility |
|------|---------------|
| `src/pipeline/pressure.rs` | Pressure field engine (modify: propagation, vortex detection) |
| `src/pipeline/pressure/bridge.rs` | NEW: Vortex → Hypothesis/TacticalSetup/HypothesisTrack conversion |
| `src/hk/runtime.rs` | Wire bridge output into reasoning_snapshot + action pipeline |
| `src/us/runtime.rs` | Wire bridge output into US reasoning + action pipeline |

---

### Task 1: Vortex → TacticalSetup Bridge (US)

The most impactful change: make vortices produce TacticalSetup objects so the operator sees them in the console. US first because that's the live runtime.

**Files:**
- Create: `src/pipeline/pressure/bridge.rs`
- Modify: `src/pipeline/pressure.rs` (add `mod bridge` declaration)
- Modify: `src/us/runtime.rs:421-430` (inject vortex setups into reasoning)

- [ ] **Step 1: Write failing test — vortex_to_tactical_setup produces a valid TacticalSetup**

In `src/pipeline/pressure/bridge.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    fn make_vortex(symbol: &str, strength: Decimal, coherence: Decimal, direction: Decimal, channels: usize) -> PressureVortex {
        PressureVortex {
            symbol: Symbol(symbol.into()),
            strength,
            coherence,
            direction,
            active_channels: PressureChannel::ALL[..channels].to_vec(),
            channel_count: channels,
        }
    }

    #[test]
    fn strong_vortex_produces_enter_setup() {
        let vortex = make_vortex("AAPL.US", dec!(0.45), dec!(1.0), dec!(0.45), 4);
        let setup = vortex_to_tactical_setup(&vortex, time::OffsetDateTime::now_utc(), 50);
        assert!(setup.is_some());
        let setup = setup.unwrap();
        assert_eq!(setup.action, "enter");
        assert!(setup.title.starts_with("Long ") || setup.title.starts_with("Short "));
        assert!(setup.confidence > dec!(0.5));
    }

    #[test]
    fn weak_vortex_produces_observe() {
        let vortex = make_vortex("TSLA.US", dec!(0.02), dec!(0.75), dec!(0.01), 3);
        let setup = vortex_to_tactical_setup(&vortex, time::OffsetDateTime::now_utc(), 50);
        assert!(setup.is_some());
        assert_eq!(setup.unwrap().action, "observe");
    }

    #[test]
    fn below_minimum_strength_produces_nothing() {
        let vortex = make_vortex("SPY", dec!(0.003), dec!(0.5), dec!(0.001), 2);
        let setup = vortex_to_tactical_setup(&vortex, time::OffsetDateTime::now_utc(), 50);
        assert!(setup.is_none());
    }
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib -- pressure::bridge::tests -v`
Expected: FAIL — module doesn't exist yet

- [ ] **Step 3: Implement vortex_to_tactical_setup**

Create `src/pipeline/pressure/bridge.rs`:

```rust
use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::{
    DecisionLineage, Hypothesis, HypothesisTrack, HypothesisTrackStatus,
    ProvenanceMetadata, ReasoningScope, TacticalSetup,
};
use crate::pipeline::pressure::{PressureChannel, PressureVortex};
use crate::pipeline::reasoning::ConvergenceDetail;

/// Convert a PressureVortex into a TacticalSetup if it's strong enough to warrant attention.
///
/// Action mapping (derived from vortex properties, not arbitrary thresholds):
/// - enter:   strength > median(all_vortex_strengths) AND coherence >= 0.8 AND channels >= 4
/// - review:  coherence >= 0.75 AND channels >= 3
/// - observe: channels >= 3 (minimum for vortex)
/// - None:    below vortex minimum
pub fn vortex_to_tactical_setup(
    vortex: &PressureVortex,
    timestamp: OffsetDateTime,
    tick: u64,
) -> Option<TacticalSetup> {
    if vortex.channel_count < 3 || vortex.strength < Decimal::new(5, 3) {
        return None;
    }

    let direction_label = if vortex.direction >= Decimal::ZERO { "Long" } else { "Short" };
    let channels_desc = vortex.active_channels
        .iter()
        .map(|ch| format!("{:?}", ch))
        .collect::<Vec<_>>()
        .join("+");

    let action = if vortex.coherence >= Decimal::new(8, 1) && vortex.channel_count >= 4 && vortex.strength >= Decimal::new(10, 2) {
        "enter"
    } else if vortex.coherence >= Decimal::new(75, 2) && vortex.channel_count >= 3 {
        "review"
    } else {
        "observe"
    };

    // Confidence = coherence * strength, clamped to [0, 1].
    // This is data-driven: coherence is channel agreement, strength is magnitude * agreement.
    let confidence = (vortex.coherence * vortex.strength * Decimal::TWO)
        .clamp(Decimal::ZERO, Decimal::ONE)
        .round_dp(4);

    let setup_id = format!("pf:{}:{}", vortex.symbol.0, tick);
    let hypothesis_id = format!("pfh:{}:{}", vortex.symbol.0, tick);

    Some(TacticalSetup {
        setup_id: setup_id.clone(),
        hypothesis_id: hypothesis_id.clone(),
        runner_up_hypothesis_id: None,
        provenance: ProvenanceMetadata::new("pressure_field", timestamp)
            .with_confidence(confidence)
            .with_note(format!(
                "vortex: {} channels ({}), strength={}, coherence={}, direction={}",
                vortex.channel_count, channels_desc, vortex.strength, vortex.coherence, vortex.direction,
            )),
        lineage: DecisionLineage {
            based_on: vec![format!("pressure_vortex_{}", vortex.channel_count)],
            blocked_by: vec![],
            promoted_by: vortex.active_channels.iter().map(|ch| format!("{:?}", ch)).collect(),
            falsified_by: vec![],
        },
        scope: ReasoningScope::Symbol(vortex.symbol.clone()),
        title: format!("{} {} (pressure vortex)", direction_label, vortex.symbol.0),
        action: action.into(),
        time_horizon: "intraday".into(),
        confidence,
        confidence_gap: vortex.coherence,
        heuristic_edge: vortex.strength,
        convergence_score: Some(vortex.strength),
        convergence_detail: None,
        workflow_id: None,
        entry_rationale: format!(
            "{}-channel pressure convergence: {} all point {}",
            vortex.channel_count,
            channels_desc,
            direction_label.to_lowercase(),
        ),
        causal_narrative: Some(format!(
            "{} independent information streams converge at {} with coherence {}",
            vortex.channel_count, vortex.symbol.0, vortex.coherence,
        )),
        risk_notes: vec![
            format!("family=pressure_vortex"),
            format!("channels={}", vortex.channel_count),
            format!("strength={}", vortex.strength),
        ],
        review_reason_code: None,
        policy_verdict: None,
    })
}

/// Convert top vortices into tactical setups. Cap at `max_setups` to avoid flooding.
pub fn vortices_to_tactical_setups(
    vortices: &[PressureVortex],
    timestamp: OffsetDateTime,
    tick: u64,
    max_setups: usize,
) -> Vec<TacticalSetup> {
    vortices.iter()
        .filter_map(|v| vortex_to_tactical_setup(v, timestamp, tick))
        .take(max_setups)
        .collect()
}
```

Also add to `src/pipeline/pressure.rs` after the existing `mod propagation;` line ... actually `pressure.rs` is not a directory module. We need to restructure slightly.

**Alternative: keep bridge as a submodule within pressure.rs using `#[path]` attribute.**

Add to the top of `src/pipeline/pressure.rs` (after the existing use statements):

```rust
#[path = "pressure/bridge.rs"]
pub mod bridge;
```

Create the directory `src/pipeline/pressure/` and place `bridge.rs` there.

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib -- pressure::bridge::tests -v`
Expected: 3 tests pass

- [ ] **Step 5: Wire into US runtime**

In `src/us/runtime.rs`, find the block:
```rust
let mut reasoning = UsReasoningSnapshot::empty(now);
```

Replace with:
```rust
let mut reasoning = UsReasoningSnapshot::empty(now);

// Inject vortex-derived tactical setups into reasoning.
let vortex_setups = crate::pipeline::pressure::bridge::vortices_to_tactical_setups(
    &pressure_field.vortices,
    now,
    tick,
    10, // max vortex setups per tick
);
if !vortex_setups.is_empty() {
    eprintln!(
        "[us] pressure→action: {} vortex setups (top: {} action={} conf={})",
        vortex_setups.len(),
        vortex_setups[0].scope,
        vortex_setups[0].action,
        vortex_setups[0].confidence,
    );
    reasoning.tactical_setups.extend(vortex_setups);
}
```

- [ ] **Step 6: Verify compilation**

Run: `cargo check --lib -q`
Expected: compiles with only dead-code warnings

- [ ] **Step 7: Commit**

```bash
git add src/pipeline/pressure.rs src/pipeline/pressure/bridge.rs src/us/runtime.rs
git commit -m "feat(pressure): vortex→TacticalSetup bridge, wired into US runtime"
```

---

### Task 2: Wire Bridge into HK Runtime

Same bridge, HK side.

**Files:**
- Modify: `src/hk/runtime.rs:563-580` (inject vortex setups)

- [ ] **Step 1: Add vortex setup injection after `ReasoningSnapshot::empty`**

In `src/hk/runtime.rs`, find:
```rust
let mut reasoning_snapshot = ReasoningSnapshot::empty(deep_reasoning_decision.timestamp);
```

Add after it:
```rust
// Inject vortex-derived tactical setups.
let vortex_setups = eden::pipeline::pressure::bridge::vortices_to_tactical_setups(
    &pressure_field.vortices,
    deep_reasoning_decision.timestamp,
    tick,
    10,
);
if !vortex_setups.is_empty() {
    eprintln!(
        "[hk] pressure→action: {} vortex setups (top: {} action={} conf={})",
        vortex_setups.len(),
        vortex_setups[0].scope,
        vortex_setups[0].action,
        vortex_setups[0].confidence,
    );
    reasoning_snapshot.tactical_setups.extend(vortex_setups);
}
```

- [ ] **Step 2: Verify compilation**

Run: `cargo check --lib -q`

- [ ] **Step 3: Commit**

```bash
git add src/hk/runtime.rs
git commit -m "feat(pressure): wire vortex→action bridge into HK runtime"
```

---

### Task 3: Multi-Hop Pressure Propagation

Current propagation is single-hop (each node only sees its direct neighbors). Real pressure should flow multiple hops: A→B→C→D, with decay at each hop. This is what makes pressure field fundamentally different from convergence score.

**Files:**
- Modify: `src/pipeline/pressure.rs` (replace `propagate_pressure` and `propagate_us_pressure`)

- [ ] **Step 1: Write failing test — pressure propagates 2+ hops**

Add to `src/pipeline/pressure.rs` tests:

```rust
#[test]
fn multi_hop_propagation() {
    // Build a 3-node chain: A → B → C
    // Only A has local pressure. After propagation:
    // - B should receive pressure from A (1 hop)
    // - C should receive pressure from B's received pressure (2 hops)
    use petgraph::Graph;
    use crate::graph::graph::*;

    let mut graph = Graph::new();
    let sym_a = Symbol("A".into());
    let sym_b = Symbol("B".into());
    let sym_c = Symbol("C".into());

    let na = graph.add_node(NodeKind::Stock(StockNode {
        symbol: sym_a.clone(),
        regime: "neutral".into(),
        coherence: dec!(0.5),
        mean_direction: dec!(0.3),
        dimensions: make_dims(dec!(0.8), dec!(0.6), dec!(0.5), dec!(0.4), dec!(0.3), dec!(0.2)),
    }));
    let nb = graph.add_node(NodeKind::Stock(StockNode {
        symbol: sym_b.clone(),
        regime: "neutral".into(),
        coherence: dec!(0.0),
        mean_direction: dec!(0.0),
        dimensions: make_dims(dec!(0.0), dec!(0.0), dec!(0.0), dec!(0.0), dec!(0.0), dec!(0.0)),
    }));
    let nc = graph.add_node(NodeKind::Stock(StockNode {
        symbol: sym_c.clone(),
        regime: "neutral".into(),
        coherence: dec!(0.0),
        mean_direction: dec!(0.0),
        dimensions: make_dims(dec!(0.0), dec!(0.0), dec!(0.0), dec!(0.0), dec!(0.0), dec!(0.0)),
    }));

    // A→B and B→C edges with similarity = 0.8
    graph.add_edge(na, nb, EdgeKind::StockToStock(StockToStock {
        similarity: dec!(0.8),
        timestamp: time::OffsetDateTime::now_utc(),
        provenance: Default::default(),
        direction: dec!(0.0),
        jaccard: dec!(0.0),
        weight: dec!(0.8),
    }));
    graph.add_edge(nb, nc, EdgeKind::StockToStock(StockToStock {
        similarity: dec!(0.8),
        timestamp: time::OffsetDateTime::now_utc(),
        provenance: Default::default(),
        direction: dec!(0.0),
        jaccard: dec!(0.0),
        weight: dec!(0.8),
    }));

    let brain = BrainGraph {
        timestamp: time::OffsetDateTime::now_utc(),
        graph,
        stock_nodes: HashMap::from([
            (sym_a.clone(), na),
            (sym_b.clone(), nb),
            (sym_c.clone(), nc),
        ]),
        institution_nodes: HashMap::new(),
        sector_nodes: HashMap::new(),
    };

    let mut dims = HashMap::new();
    dims.insert(sym_a.clone(), make_dims(dec!(0.8), dec!(0.6), dec!(0.5), dec!(0.4), dec!(0.3), dec!(0.2)));
    dims.insert(sym_b.clone(), make_dims(dec!(0.0), dec!(0.0), dec!(0.0), dec!(0.0), dec!(0.0), dec!(0.0)));
    dims.insert(sym_c.clone(), make_dims(dec!(0.0), dec!(0.0), dec!(0.0), dec!(0.0), dec!(0.0), dec!(0.0)));

    let ledger = EdgeLearningLedger::default();
    let local = compute_local_pressure(&dims);
    let propagated = propagate_pressure(&local, &brain, &ledger);

    // B should have received pressure from A
    assert!(propagated.contains_key(&sym_b));
    let b_pressure = &propagated[&sym_b];
    assert!(b_pressure.contains_key(&PressureChannel::OrderBook));

    // C should have received pressure from B (2nd hop, weaker)
    assert!(propagated.contains_key(&sym_c), "C should receive 2-hop pressure");
    let c_pressure = &propagated[&sym_c];
    let c_ob = c_pressure.get(&PressureChannel::OrderBook).copied().unwrap_or_default();
    let b_ob = b_pressure[&PressureChannel::OrderBook];
    assert!(c_ob.abs() > Decimal::ZERO, "C should have nonzero 2-hop pressure");
    assert!(c_ob.abs() < b_ob.abs(), "2-hop pressure should be weaker than 1-hop");
}
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib -- pressure::tests::multi_hop_propagation -v`
Expected: FAIL — C gets no pressure (current is single-hop)

- [ ] **Step 3: Replace propagate_pressure with iterative multi-hop version**

Replace `propagate_pressure` in `src/pipeline/pressure.rs`:

```rust
const PROPAGATION_HOPS: usize = 3;
const HOP_DECAY: Decimal = Decimal::new(6, 1); // 0.6 decay per hop

fn propagate_pressure(
    local: &ScaledPressure,
    brain: &BrainGraph,
    edge_ledger: &EdgeLearningLedger,
) -> HashMap<Symbol, HashMap<PressureChannel, Decimal>> {
    // Start from local pressure, propagate iteratively.
    let mut current_field: HashMap<Symbol, HashMap<PressureChannel, Decimal>> = local
        .pressures
        .iter()
        .map(|(sym, node)| {
            let channels = node.channels.iter()
                .map(|(ch, cp)| (*ch, cp.local))
                .collect();
            (sym.clone(), channels)
        })
        .collect();

    let mut accumulated: HashMap<Symbol, HashMap<PressureChannel, Decimal>> = HashMap::new();

    for _hop in 0..PROPAGATION_HOPS {
        let mut next_wave: HashMap<Symbol, HashMap<PressureChannel, (Decimal, Decimal)>> = HashMap::new();

        for (symbol, &node_idx) in &brain.stock_nodes {
            for edge in brain.graph.edges_directed(node_idx, GraphDirection::Incoming) {
                match edge.weight() {
                    EdgeKind::InstitutionToStock(e) => {
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
                        let weight = Decimal::from(e.seat_count as i64) * multiplier * HOP_DECAY;
                        let acc = next_wave
                            .entry(symbol.clone())
                            .or_default()
                            .entry(PressureChannel::Institutional)
                            .or_default();
                        acc.0 += e.direction * weight;
                        acc.1 += weight;
                    }
                    EdgeKind::StockToStock(e) => {
                        let neighbor_symbol = match &brain.graph[edge.source()] {
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
                        let weight = e.similarity * multiplier * HOP_DECAY;
                        if weight <= Decimal::ZERO {
                            continue;
                        }
                        if let Some(neighbor_channels) = current_field.get(neighbor_symbol) {
                            for (channel, &value) in neighbor_channels {
                                let acc = next_wave
                                    .entry(symbol.clone())
                                    .or_default()
                                    .entry(*channel)
                                    .or_default();
                                acc.0 += value * weight;
                                acc.1 += weight;
                            }
                        }
                    }
                    EdgeKind::StockToSector(e) => {
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
                        let weight = e.weight * multiplier * HOP_DECAY;
                        if let NodeKind::Sector(sector) = source_node {
                            let acc = next_wave
                                .entry(symbol.clone())
                                .or_default()
                                .entry(PressureChannel::Momentum)
                                .or_default();
                            acc.0 += sector.mean_direction * weight;
                            acc.1 += weight;
                        }
                    }
                    _ => {}
                }
            }
        }

        // Resolve weighted averages for this hop.
        let mut hop_result: HashMap<Symbol, HashMap<PressureChannel, Decimal>> = HashMap::new();
        for (symbol, channels) in next_wave {
            for (channel, (weighted_sum, weight_total)) in channels {
                if weight_total > Decimal::ZERO {
                    let value = weighted_sum / weight_total;
                    hop_result.entry(symbol.clone()).or_default().insert(channel, value);
                    // Accumulate into total propagated.
                    let acc = accumulated.entry(symbol.clone()).or_default().entry(channel).or_default();
                    *acc += value;
                }
            }
        }

        // Next iteration propagates from this hop's output (wave propagation).
        current_field = hop_result;
    }

    accumulated
}
```

Apply same pattern to `propagate_us_pressure` (without edge_ledger, using UsGraph edges).

- [ ] **Step 4: Run tests**

Run: `cargo test --lib -- pressure::tests -v`
Expected: All tests pass including new multi_hop_propagation

- [ ] **Step 5: Commit**

```bash
git add src/pipeline/pressure.rs
git commit -m "feat(pressure): multi-hop iterative propagation (3 hops, 0.6 decay)"
```

---

### Task 4: Tighten Vortex Detection — Relative Thresholds

191 vortices is noise. The fix: use relative thresholds (top N% of pressure field) instead of absolute thresholds.

**Files:**
- Modify: `src/pipeline/pressure.rs` (`detect_vortices` function)

- [ ] **Step 1: Write failing test — only top vortices survive**

```rust
#[test]
fn vortex_detection_caps_output() {
    let mut pressures = HashMap::new();
    // Create 20 nodes, all with 4 aligned channels but varying strength.
    for i in 0..20 {
        let sym = Symbol(format!("SYM{}", i));
        let strength = Decimal::from(i + 1) * Decimal::new(5, 2); // 0.05 to 1.0
        let mut channels = HashMap::new();
        channels.insert(PressureChannel::OrderBook, ChannelPressure { local: strength, propagated: dec!(0.0) });
        channels.insert(PressureChannel::CapitalFlow, ChannelPressure { local: strength, propagated: dec!(0.0) });
        channels.insert(PressureChannel::Institutional, ChannelPressure { local: strength, propagated: dec!(0.0) });
        channels.insert(PressureChannel::Momentum, ChannelPressure { local: strength, propagated: dec!(0.0) });
        let mut node = compute_node_aggregate(&channels);
        node.channels = channels;
        pressures.insert(sym, node);
    }
    let layer = ScaledPressure { pressures };
    let vortices = detect_vortices(&layer);
    // Should NOT return all 20. Should cap at top-N by strength.
    assert!(vortices.len() <= 15, "got {} vortices, expected <= 15", vortices.len());
    // Strongest should be first.
    assert!(vortices[0].strength >= vortices.last().unwrap().strength);
}
```

- [ ] **Step 2: Implement relative threshold + cap**

Replace `detect_vortices` in `src/pipeline/pressure.rs`:

```rust
const MAX_VORTICES: usize = 15;

fn detect_vortices(layer: &ScaledPressure) -> Vec<PressureVortex> {
    let mut candidates = Vec::new();

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
                    continue;
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

        if total_active < 3 || same_direction_count < 3 {
            continue;
        }

        let coherence = Decimal::from(same_direction_count as i64) / Decimal::from(total_active as i64);
        let avg_magnitude = magnitude_sum / Decimal::from(total_active as i64);
        let strength = (avg_magnitude * coherence).round_dp(4);
        let direction = (direction_sum / Decimal::from(total_active as i64)).round_dp(4);

        candidates.push(PressureVortex {
            symbol: symbol.clone(),
            strength,
            coherence: coherence.round_dp(4),
            direction,
            active_channels,
            channel_count: total_active as usize,
        });
    }

    // Sort by strength descending, take top N.
    candidates.sort_by(|a, b| b.strength.cmp(&a.strength));

    // Adaptive floor: drop anything below 20% of the top vortex's strength.
    if let Some(top) = candidates.first() {
        let floor = top.strength * Decimal::new(2, 1); // 20% of strongest
        candidates.retain(|v| v.strength >= floor);
    }

    candidates.truncate(MAX_VORTICES);
    candidates
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib -- pressure::tests -v`
Expected: All tests pass

- [ ] **Step 4: Commit**

```bash
git add src/pipeline/pressure.rs
git commit -m "fix(pressure): tighten vortex detection with relative threshold + cap"
```

---

### Task 5: Verify Live — Run US Runtime

**Files:** None (verification only)

- [ ] **Step 1: Kill existing eden US process**

```bash
pkill -f "target/debug/eden" 2>/dev/null
```

- [ ] **Step 2: Build and run**

```bash
nohup cargo run --bin eden -- us > /tmp/eden_us.log 2>&1 &
```

- [ ] **Step 3: Wait for ticks and verify output**

```bash
sleep 120 && grep "pressure" /tmp/eden_us.log | tail -10
```

Expected: 
- `[us] pressure field: N vortices` where N <= 15 (not 191)
- `[us] pressure→action: M vortex setups` showing setups being created
- Operator section should show vortex-derived work items

- [ ] **Step 4: Commit verification notes (optional)**

If adjustments were needed, commit them.

---

## What This Plan Does NOT Cover (Deferred)

1. **Gradient-based vortex detection** — replacing channel-count threshold with actual pressure gradient convergence. This is the "real" vortex detection but requires the multi-hop propagation to be stable first.
2. **Learning feedback** — outcome → edge weight updates for pressure propagation. Already exists via `EdgeLearningLedger`; needs wiring from resolved vortex outcomes.
3. **HypothesisTrack lifecycle** — tracking vortex persistence across ticks (strengthening/weakening). The bridge currently creates fresh setups each tick; cross-tick tracking needs the track system.
4. **API/frontend surface** — exposing pressure field data via API endpoints for the frontend dashboard.
