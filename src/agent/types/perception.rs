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
    /// Symbols whose BP belief (p_bull − p_bear) is changing fastest.
    /// Sorted by abs(velocity) descending; positive = belief turning
    /// bullish, negative = belief turning bearish. This is the
    /// time-derivative signal that fixed-state perception lacks —
    /// "where is eden's mind moving, not where it currently sits".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub belief_kinetics: Vec<BeliefKinetic>,
    /// Pattern memory replay: when current sub-graph signature matches
    /// past instances, what was the average forward-belief change at
    /// horizons +5 / +30 ticks? Surfaces "this state has happened
    /// before, here's what followed".
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub signature_replays: Vec<crate::pipeline::signature_replay::SignatureReplay>,
    /// Symbols moving in pre-market / post-market sessions. Catalyst
    /// signals that show up before regular-session perception begins.
    /// Populated when live snapshot has fresh `pre_market_quote` data
    /// from Longport — currently a skeleton field; reader returns
    /// empty until snapshot-level ingestion is wired (P2 follow-up).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pre_market_movers: Vec<PreMarketMove>,
    /// External catalysts known for current trading window: scheduled
    /// earnings, macro events (Fed/CPI/FOMC), policy gates, news
    /// flagged by external feeds. Lets Y see "NXPI has earnings tomorrow
    /// after-close" alongside the perception data.
    /// Skeleton field — reader returns empty until external feeds wired.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub catalysts: Vec<Catalyst>,
    /// Energy Vortices: regions of the graph where sensory flux (power)
    /// and phase coherence (alignment) are both high. Realizes the 'Y'
    /// archetype: finding truth in the electrical activity of the bus.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sensory_vortices: Vec<SensoryFlux>,
    /// Thematic Vortices: aggregated energy centers across sectors or
    /// themes. Projected from individual symbol fluxes onto the
    /// ontological hierarchy.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub thematic_vortices: Vec<ThematicVortex>,
    /// FP6 (synaptic vitality): the SensoryGainLedger snapshot — the
    /// learned trust weights eden currently applies to each sensory
    /// channel when forming BP priors. Updated by the closed-loop in
    /// `active_probe::evaluate_due` and persisted across sessions.
    /// Y can read these to discount or amplify perception fields based
    /// on which channels have been historically reliable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sensory_gain: Vec<ChannelGain>,
}

/// Y-facing view of one sensory channel's currently-learned trust
/// weight. Mirrors `crate::perception::SensoryGainSnapshot` but lives
/// in agent types to keep the agent boundary self-contained.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ChannelGain {
    pub channel_name: String,
    /// Current learned weight, clamped to `[0.1, 2.0]` by the
    /// active_probe calibrator.
    pub current_gain: f64,
    /// Most recent realized accuracy (probe truth vs forecast). 0.5 is
    /// neutral / pre-learning seed value.
    pub recent_accuracy: f64,
    /// Tick at which `current_gain` was last calibrated. 0 means seed
    /// default — the channel has never been touched by a probe.
    pub last_calibrated: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ThematicVortex {
    pub theme_id: String,
    pub theme_name: String,
    /// Total aggregated energy (flux) across all active members.
    pub total_energy: f64,
    /// Overall coherence of the theme.
    pub collective_coherence: f64,
    pub active_member_count: u32,
    /// Top contributing symbol in this theme.
    pub leader_symbol: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SensoryFlux {
    pub symbol: String,
    /// Sum of absolute magnitudes across all channels.
    pub flux_magnitude: f64,
    /// 1.0 = all channels agree; 0.0 = complete disagreement.
    pub coherence_ratio: f64,
    /// Channels currently resonant in this vortex.
    pub active_channels: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Catalyst {
    /// Symbol affected. None = market-wide event (e.g. FOMC, CPI).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub symbol: Option<String>,
    /// Catalyst kind: "earnings" | "fomc" | "cpi" | "policy" | "news" | "split" | other.
    pub kind: String,
    /// Human-readable description.
    pub description: String,
    /// When the catalyst is expected (ISO 8601). Past = already
    /// happened, Y should down-weight; future = upcoming, Y should
    /// pre-position.
    pub scheduled_at: String,
    /// Source: "earnings_calendar" | "manual" | "news_feed" | etc.
    pub source: String,
    /// Importance/severity 1-5 (5 = market-moving).
    pub importance: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PreMarketMove {
    pub symbol: String,
    pub session: String, // "pre_market" | "post_market" | "overnight"
    pub last_done: f64,
    pub prev_close: f64,
    pub change_pct: f64,
    pub volume: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BeliefKinetic {
    pub symbol: String,
    /// Current (p_bull - p_bear), ∈ [-1, +1].
    pub belief_now: f64,
    /// Δ(p_bull - p_bear) over last tick.
    pub velocity: f64,
    /// Δvelocity over last tick (acceleration).
    pub acceleration: f64,
    /// How many ticks of consecutive same-sign velocity (momentum streak).
    pub streak_ticks: u32,
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
    /// Minimum abs(velocity) to surface a symbol's belief kinetics.
    /// belief is in [-1,+1], so 0.05 = 5% per-tick change is meaningful.
    pub min_kinetic_velocity: f64,
    pub max_kinetics: usize,
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
            min_kinetic_velocity: 0.05,
            max_kinetics: 15,
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
            belief_kinetics: vec![],
            signature_replays: vec![],
            pre_market_movers: vec![],
            catalysts: vec![],
            sensory_vortices: vec![],
            thematic_vortices: vec![],
            sensory_gain: vec![],
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
            belief_kinetics: vec![BeliefKinetic {
                symbol: "NXPI.US".to_string(),
                belief_now: 0.42,
                velocity: 0.08,
                acceleration: 0.03,
                streak_ticks: 2,
            }],
            signature_replays: vec![crate::pipeline::signature_replay::SignatureReplay {
                symbol: "NXPI.US".to_string(),
                signature_hash: "deadbeef".to_string(),
                historical_visits: 3,
                mean_forward_belief_5tick: 0.05,
                n_5tick: 3,
                mean_forward_belief_30tick: 0.12,
                n_30tick: 2,
            }],
            pre_market_movers: vec![PreMarketMove {
                symbol: "QCOM.US".to_string(),
                session: "post_market".to_string(),
                last_done: 179.74,
                prev_close: 156.0,
                change_pct: 0.152,
                volume: 5_782_670,
            }],
            catalysts: vec![Catalyst {
                symbol: Some("NXPI.US".to_string()),
                kind: "earnings".to_string(),
                description: "Q1 earnings after-close".to_string(),
                scheduled_at: "2026-05-01T20:00:00Z".to_string(),
                source: "earnings_calendar".to_string(),
                importance: 4,
            }],
            sensory_vortices: vec![],
            thematic_vortices: vec![],
            sensory_gain: vec![ChannelGain {
                channel_name: "CapitalFlow".to_string(),
                current_gain: 1.4,
                recent_accuracy: 0.78,
                last_calibrated: 41,
            }],
        };
        let json = serde_json::to_string(&original).expect("serialise");
        let recovered: EdenPerception = serde_json::from_str(&json).expect("deserialise");
        assert_eq!(original, recovered);
    }
}
