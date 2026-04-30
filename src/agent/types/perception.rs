use serde::{Deserialize, Serialize};

use crate::live_snapshot::LiveMarket;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EdenPerception {
    pub schema_version: u32,
    pub market: LiveMarket,
    pub tick: u64,
    pub timestamp: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub emergent_clusters: Vec<EmergentCluster>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sector_leaders: Vec<SymbolContrast>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub causal_chains: Vec<LeadLagEdge>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub anomaly_alerts: Vec<SurpriseAlert>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub regime: Option<RegimePerception>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EmergentCluster {
    pub sector: String,
    pub total_members: u32,
    pub sync_member_count: u32,
    pub sync_ratio: String,
    pub sync_pct: f64,
    pub strongest_member: String,
    pub strongest_activation: f64,
    pub mean_activation_intent: f64,
    pub mean_activation_pressure: f64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub members: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SymbolContrast {
    pub symbol: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sector: Option<String>,
    pub center_activation: f64,
    pub sector_mean: f64,
    pub vs_sector_contrast: f64,
    pub node_kind: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub persistence_ticks: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct LeadLagEdge {
    pub leader: String,
    pub follower: String,
    pub lag_ticks: i32,
    pub correlation: f64,
    pub n_samples: usize,
    pub direction: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SurpriseAlert {
    pub symbol: String,
    pub channel: String,
    pub observed: f64,
    pub expected: f64,
    pub squared_error: f64,
    pub total_surprise: f64,
    pub floor: f64,
    pub deviation_kind: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegimePerception {
    pub bucket: String,
    pub historical_visits: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_tick: Option<u64>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub forward_outcomes: Vec<RegimeForward>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RegimeForward {
    pub horizon_ticks: u32,
    pub n_samples: u32,
    pub mean_stress_delta: f64,
    pub mean_synchrony_delta: f64,
    pub mean_bull_bias_delta: f64,
}

/// Filter thresholds for surfacing perception signals. Defaults set per
/// design spec 2026-04-30-perception-report-design.md.
#[derive(Debug, Clone, Copy)]
pub struct PerceptionFilterConfig {
    pub min_cluster_sync_pct: f64,
    pub min_leader_contrast: f64,
    pub max_leaders: usize,
    pub min_chain_correlation: f64,
    pub min_chain_samples: usize,
    pub max_chains: usize,
    pub min_anomaly_surprise_ratio: f64,
    pub max_anomalies: usize,
}

impl Default for PerceptionFilterConfig {
    fn default() -> Self {
        Self {
            min_cluster_sync_pct: 0.7,
            min_leader_contrast: 3.0,
            max_leaders: 20,
            min_chain_correlation: 0.5,
            min_chain_samples: 10,
            max_chains: 30,
            min_anomaly_surprise_ratio: 1.5,
            max_anomalies: 15,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perception_default_filter_config() {
        let cfg = PerceptionFilterConfig::default();
        assert!((cfg.min_cluster_sync_pct - 0.7).abs() < 1e-9);
        assert!((cfg.min_leader_contrast - 3.0).abs() < 1e-9);
        assert_eq!(cfg.max_leaders, 20);
        assert!((cfg.min_chain_correlation - 0.5).abs() < 1e-9);
        assert_eq!(cfg.min_chain_samples, 10);
        assert_eq!(cfg.max_chains, 30);
        assert!((cfg.min_anomaly_surprise_ratio - 1.5).abs() < 1e-9);
        assert_eq!(cfg.max_anomalies, 15);
    }

    #[test]
    fn perception_serde_roundtrip_empty() {
        let original = EdenPerception {
            schema_version: 1,
            market: LiveMarket::Hk,
            tick: 42,
            timestamp: "2026-04-30T09:00:00Z".to_string(),
            emergent_clusters: vec![],
            sector_leaders: vec![],
            causal_chains: vec![],
            anomaly_alerts: vec![],
            regime: None,
        };
        let json = serde_json::to_string(&original).expect("serialise");
        let recovered: EdenPerception = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(original, recovered);
    }

    #[test]
    fn perception_serde_roundtrip_full() {
        let original = EdenPerception {
            schema_version: 1,
            market: LiveMarket::Hk,
            tick: 42,
            timestamp: "2026-04-30T09:00:00Z".to_string(),
            emergent_clusters: vec![EmergentCluster {
                sector: "semiconductor".to_string(),
                total_members: 9,
                sync_member_count: 9,
                sync_ratio: "9/9".to_string(),
                sync_pct: 1.0,
                strongest_member: "6809.HK".to_string(),
                strongest_activation: 0.79,
                mean_activation_intent: 0.51,
                mean_activation_pressure: 0.71,
                members: vec!["6809.HK".to_string(), "981.HK".to_string()],
            }],
            sector_leaders: vec![SymbolContrast {
                symbol: "6869.HK".to_string(),
                sector: Some("semiconductor".to_string()),
                center_activation: 13.68,
                sector_mean: 5.85,
                vs_sector_contrast: 7.82,
                node_kind: "Role".to_string(),
                persistence_ticks: Some(23),
            }],
            causal_chains: vec![LeadLagEdge {
                leader: "6883.HK".to_string(),
                follower: "2477.HK".to_string(),
                lag_ticks: 3,
                correlation: 0.89,
                n_samples: 17,
                direction: "from_leads".to_string(),
            }],
            anomaly_alerts: vec![SurpriseAlert {
                symbol: "1800.HK".to_string(),
                channel: "PressureStructure".to_string(),
                observed: 0.68,
                expected: 1.88,
                squared_error: 1.45,
                total_surprise: 1.46,
                floor: 1.22,
                deviation_kind: "below_expected".to_string(),
            }],
            regime: Some(RegimePerception {
                bucket: "stress=4|sync=4|bias=2|act=3|turn=3".to_string(),
                historical_visits: 188,
                last_seen_tick: Some(29),
                forward_outcomes: vec![RegimeForward {
                    horizon_ticks: 30,
                    n_samples: 89,
                    mean_stress_delta: -0.048,
                    mean_synchrony_delta: -0.0001,
                    mean_bull_bias_delta: 0.0,
                }],
            }),
        };
        let json = serde_json::to_string(&original).expect("serialise");
        let recovered: EdenPerception = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(original, recovered);
    }
}
