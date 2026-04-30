use serde::{Deserialize, Serialize};

use super::*;

/// Y-facing JSON output for L4 perception. Distinct from EdenPerception
/// (which lives in AgentSnapshot) so future schema evolution of the
/// internal representation can happen without breaking the on-disk
/// surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgentPerceptionReport {
    pub schema_version: u32,
    pub tick: u64,
    pub timestamp: String,
    pub market: LiveMarket,
    /// `None` when the snapshot did not include perception (e.g.
    /// pre-L4 build, no NDJSON streams produced yet). Surface as null
    /// in JSON so downstream clients can detect this state.
    pub perception: Option<EdenPerception>,
}

pub fn build_perception_report(snapshot: &AgentSnapshot) -> AgentPerceptionReport {
    AgentPerceptionReport {
        schema_version: 1,
        tick: snapshot.tick,
        timestamp: snapshot.timestamp.clone(),
        market: snapshot.market,
        perception: snapshot.perception.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_perception_report_from_snapshot_with_perception() {
        let perception = EdenPerception {
            schema_version: 1,
            market: LiveMarket::Hk,
            tick: 42,
            timestamp: "t".to_string(),
            emergent_clusters: vec![],
            sector_leaders: vec![],
            causal_chains: vec![],
            anomaly_alerts: vec![],
            regime: None,
            belief_kinetics: vec![],
            signature_replays: vec![],
            pre_market_movers: vec![],
            catalysts: vec![],
        };
        let snapshot = AgentSnapshot {
            tick: 42,
            timestamp: "ts".to_string(),
            market: LiveMarket::Hk,
            market_regime: LiveMarketRegime {
                bias: String::new(),
                confidence: rust_decimal::Decimal::ZERO,
                breadth_up: rust_decimal::Decimal::ZERO,
                breadth_down: rust_decimal::Decimal::ZERO,
                average_return: rust_decimal::Decimal::ZERO,
                directional_consensus: None,
                pre_market_sentiment: None,
            },
            stress: LiveStressSnapshot {
                composite_stress: rust_decimal::Decimal::ZERO,
                sector_synchrony: None,
                pressure_consensus: None,
                momentum_consensus: None,
                pressure_dispersion: None,
                volume_anomaly: None,
            },
            wake: AgentWakeState {
                should_speak: false,
                priority: rust_decimal::Decimal::ZERO,
                headline: None,
                summary: vec![],
                focus_symbols: vec![],
                reasons: vec![],
                suggested_tools: vec![],
            },
            world_state: None,
            backward_reasoning: None,
            perception: Some(perception.clone()),
            notices: vec![],
            active_structures: vec![],
            recent_transitions: vec![],
            investigation_selections: vec![],
            sector_flows: vec![],
            symbols: vec![],
            perception_states: vec![],
            events: vec![],
            cross_market_signals: vec![],
            raw_sources: vec![],
            context_priors: vec![],
            macro_event_candidates: vec![],
            macro_events: vec![],
            knowledge_links: vec![],
        };
        let report = build_perception_report(&snapshot);
        assert_eq!(report.schema_version, 1);
        assert_eq!(report.tick, 42);
        assert_eq!(report.timestamp, "ts");
        assert!(report.perception.is_some());
    }

    #[test]
    fn build_perception_report_from_snapshot_without_perception() {
        let snapshot = AgentSnapshot {
            tick: 0,
            timestamp: "ts".to_string(),
            market: LiveMarket::Hk,
            market_regime: LiveMarketRegime {
                bias: String::new(),
                confidence: rust_decimal::Decimal::ZERO,
                breadth_up: rust_decimal::Decimal::ZERO,
                breadth_down: rust_decimal::Decimal::ZERO,
                average_return: rust_decimal::Decimal::ZERO,
                directional_consensus: None,
                pre_market_sentiment: None,
            },
            stress: LiveStressSnapshot {
                composite_stress: rust_decimal::Decimal::ZERO,
                sector_synchrony: None,
                pressure_consensus: None,
                momentum_consensus: None,
                pressure_dispersion: None,
                volume_anomaly: None,
            },
            wake: AgentWakeState {
                should_speak: false,
                priority: rust_decimal::Decimal::ZERO,
                headline: None,
                summary: vec![],
                focus_symbols: vec![],
                reasons: vec![],
                suggested_tools: vec![],
            },
            world_state: None,
            backward_reasoning: None,
            perception: None,
            notices: vec![],
            active_structures: vec![],
            recent_transitions: vec![],
            investigation_selections: vec![],
            sector_flows: vec![],
            symbols: vec![],
            perception_states: vec![],
            events: vec![],
            cross_market_signals: vec![],
            raw_sources: vec![],
            context_priors: vec![],
            macro_event_candidates: vec![],
            macro_events: vec![],
            knowledge_links: vec![],
        };
        let report = build_perception_report(&snapshot);
        assert!(report.perception.is_none());
    }
}
