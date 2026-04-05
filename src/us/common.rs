use rust_decimal::Decimal;

use crate::us::pipeline::dimensions::UsSymbolDimensions;

pub const SIGNAL_RESOLUTION_LAG: u64 = 50;

/// Equal-weight composite across the five US dimensions.
pub fn dimension_composite(dims: &UsSymbolDimensions) -> Decimal {
    let values = [
        dims.capital_flow_direction,
        dims.price_momentum,
        dims.volume_profile,
        dims.pre_post_market_anomaly,
        dims.valuation,
    ];
    values.iter().copied().sum::<Decimal>() / Decimal::from(values.len() as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn dimension_composite_is_equal_weight_average() {
        let dims = UsSymbolDimensions {
            capital_flow_direction: dec!(1.0),
            price_momentum: dec!(0.5),
            volume_profile: Decimal::ZERO,
            pre_post_market_anomaly: dec!(-0.5),
            valuation: dec!(0.5),
            multi_horizon_momentum: Decimal::ZERO,
        };

        assert_eq!(dimension_composite(&dims), dec!(0.3));
    }
}
