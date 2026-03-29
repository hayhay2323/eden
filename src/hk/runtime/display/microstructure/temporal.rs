use super::*;

pub(crate) fn display_hk_temporal_debug(
    tick: u64,
    decision: &DecisionSnapshot,
    graph_node_delta: &crate::graph::temporal::GraphNodeTemporalDelta,
    broker_delta: &crate::graph::temporal::BrokerTemporalDelta,
) {
    if tick % 5 != 0 {
        return;
    }

    let mut enriched: Vec<_> = decision
        .convergence_scores
        .values()
        .filter(|s| s.temporal_weight.is_some())
        .collect();
    enriched.sort_by(|a, b| {
        b.composite
            .abs()
            .partial_cmp(&a.composite.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    if !enriched.is_empty() {
        eprintln!("\n── Temporal Enrichment (top 5) ──");
        for s in enriched.iter().take(5) {
            eprintln!("  {}  composite={:.4}  tw={:.3}  stability={:.3}  age={:.1}  new_frac={:.3}  micro={:?}  spread={:?}",
                s.symbol,
                s.composite,
                s.temporal_weight.unwrap_or_default(),
                s.edge_stability.unwrap_or_default(),
                s.institutional_edge_age.unwrap_or_default(),
                s.new_edge_fraction.unwrap_or_default(),
                s.microstructure_confirmation,
                s.component_spread,
            );
        }
    } else {
        eprintln!("\n── Temporal Enrichment: no edges with temporal data yet (tick {tick}) ──");
    }

    if !graph_node_delta.transitions.is_empty() {
        eprintln!("── Node Transitions ({}) ──", graph_node_delta.transitions.len());
        for t in graph_node_delta.transitions.iter().take(10) {
            match &t.kind {
                crate::graph::temporal::GraphNodeTransitionKind::RegimeChanged => {
                    eprintln!(
                        "  {:?} {} {} → {}",
                        t.kind,
                        t.label,
                        t.previous_regime.as_deref().unwrap_or("?"),
                        t.new_regime.as_deref().unwrap_or("?")
                    );
                }
                _ => {
                    eprintln!("  {:?} {}", t.kind, t.label);
                }
            }
        }
    }
    eprintln!("  Active nodes: {}", graph_node_delta.active_node_count);

    let replenish_count = broker_delta
        .transitions
        .iter()
        .filter(|t| {
            matches!(
                t.kind,
                crate::graph::temporal::BrokerTransitionKind::Replenished
            )
        })
        .count();
    let side_flip_count = broker_delta
        .transitions
        .iter()
        .filter(|t| {
            matches!(
                t.kind,
                crate::graph::temporal::BrokerTransitionKind::SideFlipped
            )
        })
        .count();
    eprintln!(
        "── Broker Perception: {} active | {} transitions | {} replenish (iceberg) | {} side flips ──",
        broker_delta.active_broker_count,
        broker_delta.transitions.len(),
        replenish_count,
        side_flip_count
    );
    if replenish_count > 0 {
        for t in broker_delta
            .transitions
            .iter()
            .filter(|t| {
                matches!(
                    t.kind,
                    crate::graph::temporal::BrokerTransitionKind::Replenished
                )
            })
            .take(5)
        {
            eprintln!(
                "  ICE B{} {} {:?} pos {} conf={} count={} depth={}",
                t.broker_symbol_id.broker_id.0,
                t.broker_symbol_id.symbol,
                t.side,
                t.position,
                t.iceberg_confidence
                    .unwrap_or_default()
                    .round_dp(2),
                t.replenish_count.unwrap_or(0),
                t.depth_recovery_ratio
                    .unwrap_or_default()
                    .round_dp(2),
            );
        }
    }
}
