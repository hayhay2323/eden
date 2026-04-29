use super::*;
use crate::us::pipeline::dimensions::UsSymbolDimensions;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum UsMarketRegimeBias {
    RiskOn,
    Neutral,
    RiskOff,
}

impl UsMarketRegimeBias {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RiskOn => "risk_on",
            Self::Neutral => "neutral",
            Self::RiskOff => "risk_off",
        }
    }
}

impl std::fmt::Display for UsMarketRegimeBias {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct UsMarketRegimeFilter {
    pub bias: UsMarketRegimeBias,
    pub confidence: Decimal,
    pub macro_return: Decimal,
    pub breadth_up: Decimal,
    pub breadth_down: Decimal,
    pub pre_market_sentiment: Decimal,
}

impl UsMarketRegimeFilter {
    pub fn neutral() -> Self {
        Self {
            bias: UsMarketRegimeBias::Neutral,
            confidence: Decimal::ZERO,
            macro_return: Decimal::ZERO,
            breadth_up: Decimal::ZERO,
            breadth_down: Decimal::ZERO,
            pre_market_sentiment: Decimal::ZERO,
        }
    }

    pub fn compute(
        all_dims: &HashMap<Symbol, UsSymbolDimensions>,
        macro_symbols: &[Symbol],
    ) -> Self {
        if all_dims.is_empty() {
            return Self::neutral();
        }

        let macro_momentums: Vec<Decimal> = macro_symbols
            .iter()
            .filter_map(|s| all_dims.get(s).map(|d| d.price_momentum))
            .collect();
        let macro_return = if macro_momentums.is_empty() {
            Decimal::ZERO
        } else {
            macro_momentums.iter().copied().sum::<Decimal>()
                / Decimal::from(macro_momentums.len() as i64)
        };

        let total = Decimal::from(all_dims.len() as i64);
        let up_count = all_dims
            .values()
            .filter(|d| d.price_momentum > Decimal::ZERO)
            .count();
        let down_count = all_dims
            .values()
            .filter(|d| d.price_momentum < Decimal::ZERO)
            .count();
        let breadth_up = Decimal::from(up_count as i64) / total;
        let breadth_down = Decimal::from(down_count as i64) / total;

        let pre_market_sentiment = all_dims
            .values()
            .map(|d| d.pre_post_market_anomaly)
            .sum::<Decimal>()
            / total;

        let risk_off_score = [
            scale_to_unit(breadth_down, Decimal::new(55, 2), Decimal::new(80, 2)),
            scale_to_unit(-macro_return, Decimal::new(10, 2), Decimal::new(50, 2)),
            scale_to_unit(
                -pre_market_sentiment,
                Decimal::new(5, 2),
                Decimal::new(30, 2),
            ),
        ]
        .iter()
        .copied()
        .sum::<Decimal>()
            / Decimal::from(3);

        let risk_on_score = [
            scale_to_unit(breadth_up, Decimal::new(55, 2), Decimal::new(80, 2)),
            scale_to_unit(macro_return, Decimal::new(10, 2), Decimal::new(50, 2)),
            scale_to_unit(
                pre_market_sentiment,
                Decimal::new(5, 2),
                Decimal::new(30, 2),
            ),
        ]
        .iter()
        .copied()
        .sum::<Decimal>()
            / Decimal::from(3);

        let min_score = Decimal::new(55, 2);
        let min_gap = Decimal::new(15, 2);
        let bias = if risk_off_score >= min_score && risk_off_score - risk_on_score >= min_gap {
            UsMarketRegimeBias::RiskOff
        } else if risk_on_score >= min_score && risk_on_score - risk_off_score >= min_gap {
            UsMarketRegimeBias::RiskOn
        } else {
            UsMarketRegimeBias::Neutral
        };
        let confidence = match bias {
            UsMarketRegimeBias::RiskOff => risk_off_score,
            UsMarketRegimeBias::RiskOn => risk_on_score,
            UsMarketRegimeBias::Neutral => risk_off_score.max(risk_on_score),
        };

        UsMarketRegimeFilter {
            bias,
            confidence,
            macro_return,
            breadth_up,
            breadth_down,
            pre_market_sentiment,
        }
    }

    pub fn blocks(&self, direction: UsOrderDirection) -> bool {
        matches!(
            (self.bias, direction),
            (UsMarketRegimeBias::RiskOff, UsOrderDirection::Buy)
                | (UsMarketRegimeBias::RiskOn, UsOrderDirection::Sell)
        )
    }
}

fn scale_to_unit(value: Decimal, floor: Decimal, ceiling: Decimal) -> Decimal {
    if ceiling <= floor {
        return Decimal::ZERO;
    }
    crate::math::clamp_unit_interval((value - floor) / (ceiling - floor))
}
