use super::*;

pub(crate) fn stabilize_cross_market_signals(
    signals: Vec<crate::bridges::hk_to_us::CrossMarketSignal>,
    dims: &UsDimensionSnapshot,
) -> Vec<crate::bridges::hk_to_us::CrossMarketSignal> {
    signals
        .into_iter()
        .map(|mut signal| {
            let Some(target_dims) = dims.dimensions.get(&signal.us_symbol) else {
                return signal;
            };

            let anchor_confidence =
                (signal.hk_composite.abs() * Decimal::new(35, 2)).min(Decimal::ONE);
            let us_response = target_dims
                .price_momentum
                .abs()
                .max(target_dims.capital_flow_direction.abs())
                .max(target_dims.volume_profile.abs());
            let signal_direction = signal.propagation_confidence.signum();
            let us_direction = if target_dims.price_momentum != Decimal::ZERO {
                target_dims.price_momentum.signum()
            } else {
                target_dims.capital_flow_direction.signum()
            };

            let should_hold_anchor = us_response < Decimal::new(15, 2)
                || us_direction == Decimal::ZERO
                || us_direction != signal_direction;

            if should_hold_anchor {
                let magnitude = signal.propagation_confidence.abs().max(anchor_confidence);
                signal.propagation_confidence = signal_direction * magnitude;
            }

            signal
        })
        .collect()
}

pub(crate) fn us_sector_name(store: &Arc<ObjectStore>, symbol: &Symbol) -> Option<String> {
    store.sector_name_for_symbol(symbol).map(str::to_string)
}
