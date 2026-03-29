use super::*;

pub(super) fn compute_stress_index(
    pressures: &[UsStockPressure],
    dims: &UsDimensionSnapshot,
) -> UsMarketStressIndex {
    let pressure_vals: Vec<Decimal> = pressures.iter().map(|p| p.capital_flow_pressure).collect();
    let pressure_dispersion = std_dev(&pressure_vals);

    let momentum_vals: Vec<Decimal> = dims.dimensions.values().map(|d| d.price_momentum).collect();
    let positive = momentum_vals.iter().filter(|&&m| m > Decimal::ZERO).count();
    let negative = momentum_vals.iter().filter(|&&m| m < Decimal::ZERO).count();
    let majority = positive.max(negative);
    let momentum_consensus = if momentum_vals.is_empty() {
        Decimal::ZERO
    } else {
        Decimal::from(majority as i64) / Decimal::from(momentum_vals.len() as i64)
    };

    let vol_abs: Vec<Decimal> = dims
        .dimensions
        .values()
        .map(|d| d.volume_profile.abs())
        .collect();
    let vol_median = median_decimal(vol_abs);
    let total_stocks = dims.dimensions.len();
    let above_median_count = dims
        .dimensions
        .values()
        .filter(|d| d.volume_profile.abs() > vol_median)
        .count();
    let volume_anomaly = if total_stocks == 0 {
        Decimal::ZERO
    } else {
        Decimal::from(above_median_count as i64) / Decimal::from(total_stocks as i64)
    };

    let dispersion_clamped = pressure_dispersion.clamp(Decimal::ZERO, Decimal::ONE);
    let composite_stress = average([
        dispersion_clamped * Decimal::new(4, 1),
        momentum_consensus * Decimal::new(4, 1),
        volume_anomaly * Decimal::new(2, 1),
    ]);

    UsMarketStressIndex {
        pressure_dispersion,
        momentum_consensus,
        volume_anomaly,
        composite_stress,
    }
}
