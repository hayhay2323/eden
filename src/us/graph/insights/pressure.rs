use super::*;

pub(super) fn compute_pressures(
    graph: &UsGraph,
    dims: &UsDimensionSnapshot,
    prev: Option<&UsGraphInsights>,
) -> Vec<UsStockPressure> {
    let prev_map: HashMap<&Symbol, &UsStockPressure> = prev
        .map(|p| p.pressures.iter().map(|sp| (&sp.symbol, sp)).collect())
        .unwrap_or_default();

    let mut results = Vec::new();

    for (symbol, &node_idx) in &graph.stock_nodes {
        let stock_dims = match dims.dimensions.get(symbol) {
            Some(d) => d,
            None => continue,
        };

        if !matches!(graph.graph[node_idx], UsNodeKind::Stock(_)) {
            continue;
        }

        let capital_flow_pressure = stock_dims.capital_flow_direction;
        let volume_intensity = stock_dims.volume_profile;
        let momentum = stock_dims.price_momentum;

        let (pressure_delta, pressure_duration, accelerating) =
            if let Some(prev_p) = prev_map.get(symbol) {
                let delta = capital_flow_pressure - prev_p.capital_flow_pressure;
                let prev_delta = prev_p.pressure_delta;

                let duration = if capital_flow_pressure == Decimal::ZERO {
                    0
                } else if (capital_flow_pressure > Decimal::ZERO
                    && prev_p.capital_flow_pressure > Decimal::ZERO)
                    || (capital_flow_pressure < Decimal::ZERO
                        && prev_p.capital_flow_pressure < Decimal::ZERO)
                {
                    prev_p.pressure_duration + 1
                } else {
                    1
                };
                let accelerating = delta.abs() > prev_delta.abs();
                (delta, duration, accelerating)
            } else {
                (Decimal::ZERO, 1, false)
            };

        results.push(UsStockPressure {
            symbol: symbol.clone(),
            capital_flow_pressure,
            volume_intensity,
            momentum,
            pressure_delta,
            pressure_duration,
            accelerating,
        });
    }

    results.sort_by(|a, b| {
        b.capital_flow_pressure
            .abs()
            .cmp(&a.capital_flow_pressure.abs())
    });
    results
}
