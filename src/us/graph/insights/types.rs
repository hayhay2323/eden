use super::*;

#[derive(Debug, Clone)]
pub struct UsStockPressure {
    pub symbol: Symbol,
    pub capital_flow_pressure: Decimal,
    pub volume_intensity: Decimal,
    pub momentum: Decimal,
    pub pressure_delta: Decimal,
    pub pressure_duration: u64,
    pub accelerating: bool,
}

#[derive(Debug, Clone)]
pub struct UsSectorRotation {
    pub sector_a: SectorId,
    pub sector_b: SectorId,
    pub spread: Decimal,
    pub spread_delta: Decimal,
    pub widening: bool,
}

#[derive(Debug, Clone)]
pub struct UsStockCluster {
    pub members: Vec<Symbol>,
    pub directional_alignment: Decimal,
    pub stability: Decimal,
    pub age: u64,
}

#[derive(Debug, Clone)]
pub struct UsMarketStressIndex {
    pub pressure_dispersion: Decimal,
    pub momentum_consensus: Decimal,
    pub volume_anomaly: Decimal,
    pub composite_stress: Decimal,
}

#[derive(Debug, Clone)]
pub struct UsCrossMarketAnomaly {
    pub us_symbol: Symbol,
    pub hk_symbol: Symbol,
    pub expected_direction: Decimal,
    pub actual_direction: Decimal,
    pub divergence: Decimal,
}

#[derive(Debug, Clone)]
pub struct UsPropagationSense {
    pub source_symbol: Symbol,
    pub target_symbol: Symbol,
    pub channel: String,
    pub propagation_strength: Decimal,
    pub target_momentum: Decimal,
    pub lag_gap: Decimal,
}

#[derive(Debug, Clone)]
pub struct UsGraphInsights {
    pub pressures: Vec<UsStockPressure>,
    pub rotations: Vec<UsSectorRotation>,
    pub clusters: Vec<UsStockCluster>,
    pub stress: UsMarketStressIndex,
    pub cross_market_anomalies: Vec<UsCrossMarketAnomaly>,
}

impl UsGraphInsights {
    pub fn compute(
        graph: &UsGraph,
        dims: &UsDimensionSnapshot,
        cross_market: &[CrossMarketSignal],
        prev: Option<&UsGraphInsights>,
        _tick: u64,
    ) -> Self {
        let pressures = super::pressure::compute_pressures(graph, dims, prev);
        let rotations = super::rotation::compute_rotations(graph, dims, prev);
        let clusters = super::cluster::compute_clusters(graph, dims, prev);
        let stress = super::stress::compute_stress_index(&pressures, dims);
        let cross_market_anomalies =
            super::anomaly::compute_cross_market_anomalies(graph, dims, cross_market);

        UsGraphInsights {
            pressures,
            rotations,
            clusters,
            stress,
            cross_market_anomalies,
        }
    }
}
