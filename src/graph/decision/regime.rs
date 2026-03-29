use std::collections::HashMap;

use rust_decimal::prelude::Signed;
use rust_decimal::Decimal;

use crate::external::polymarket::{PolymarketBias, PolymarketSnapshot};
use crate::math::clamp_unit_interval;
use crate::ontology::links::LinkSnapshot;
use crate::ontology::objects::Symbol;

use super::{ConvergenceScore, OrderDirection};

fn scale_to_unit(value: Decimal, floor: Decimal, ceiling: Decimal) -> Decimal {
    if ceiling <= floor {
        return Decimal::ZERO;
    }
    clamp_unit_interval((value - floor) / (ceiling - floor))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketRegimeBias {
    RiskOn,
    Neutral,
    RiskOff,
}

impl MarketRegimeBias {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::RiskOn => "risk_on",
            Self::Neutral => "neutral",
            Self::RiskOff => "risk_off",
        }
    }
}

impl std::fmt::Display for MarketRegimeBias {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone)]
pub struct MarketRegimeFilter {
    pub bias: MarketRegimeBias,
    pub confidence: Decimal,
    pub breadth_up: Decimal,
    pub breadth_down: Decimal,
    pub average_return: Decimal,
    pub leader_return: Option<Decimal>,
    pub directional_consensus: Decimal,
    pub external_bias: Option<MarketRegimeBias>,
    pub external_confidence: Option<Decimal>,
    pub external_driver: Option<String>,
}

impl MarketRegimeFilter {
    pub fn neutral() -> Self {
        Self {
            bias: MarketRegimeBias::Neutral,
            confidence: Decimal::ZERO,
            breadth_up: Decimal::ZERO,
            breadth_down: Decimal::ZERO,
            average_return: Decimal::ZERO,
            leader_return: None,
            directional_consensus: Decimal::ZERO,
            external_bias: None,
            external_confidence: None,
            external_driver: None,
        }
    }

    pub(super) fn compute(
        links: &LinkSnapshot,
        convergence_scores: &HashMap<Symbol, ConvergenceScore>,
    ) -> Self {
        const LEADER_SYMBOLS: &[&str] = &[
            "700.HK", "9988.HK", "3690.HK", "1810.HK", "388.HK", "5.HK", "939.HK", "883.HK",
        ];

        let returns = links
            .quotes
            .iter()
            .filter_map(|quote| {
                if quote.prev_close > Decimal::ZERO {
                    Some((
                        quote.symbol.clone(),
                        (quote.last_done - quote.prev_close) / quote.prev_close,
                    ))
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let total_returns = Decimal::from(returns.len() as i64);
        let breadth_up = if total_returns > Decimal::ZERO {
            Decimal::from(returns.iter().filter(|item| item.1 > Decimal::ZERO).count() as i64)
                / total_returns
        } else {
            Decimal::ZERO
        };
        let breadth_down = if total_returns > Decimal::ZERO {
            Decimal::from(returns.iter().filter(|item| item.1 < Decimal::ZERO).count() as i64)
                / total_returns
        } else {
            Decimal::ZERO
        };
        let average_return = if total_returns > Decimal::ZERO {
            returns.iter().map(|(_, value)| *value).sum::<Decimal>() / total_returns
        } else {
            Decimal::ZERO
        };

        let leader_returns = returns
            .iter()
            .filter_map(|(symbol, value)| {
                LEADER_SYMBOLS
                    .contains(&symbol.0.as_str())
                    .then_some(*value)
            })
            .collect::<Vec<_>>();
        let leader_return = if leader_returns.is_empty() {
            None
        } else {
            Some(
                leader_returns.iter().copied().sum::<Decimal>()
                    / Decimal::from(leader_returns.len() as i64),
            )
        };

        let directional_consensus = if convergence_scores.is_empty() {
            Decimal::ZERO
        } else {
            convergence_scores
                .values()
                .map(|score| {
                    score.composite.signum()
                        * clamp_unit_interval(score.composite.abs() / Decimal::new(4, 1))
                })
                .sum::<Decimal>()
                / Decimal::from(convergence_scores.len() as i64)
        };

        let leader_proxy = leader_return.unwrap_or(average_return);
        let risk_off_score = [
            scale_to_unit(breadth_down, Decimal::new(58, 2), Decimal::new(82, 2)),
            scale_to_unit(-average_return, Decimal::new(6, 3), Decimal::new(3, 2)),
            scale_to_unit(-leader_proxy, Decimal::new(12, 3), Decimal::new(5, 2)),
            scale_to_unit(
                -directional_consensus,
                Decimal::new(15, 2),
                Decimal::new(75, 2),
            ),
        ]
        .iter()
        .copied()
        .sum::<Decimal>()
            / Decimal::from(4);
        let risk_on_score = [
            scale_to_unit(breadth_up, Decimal::new(58, 2), Decimal::new(82, 2)),
            scale_to_unit(average_return, Decimal::new(6, 3), Decimal::new(3, 2)),
            scale_to_unit(leader_proxy, Decimal::new(12, 3), Decimal::new(5, 2)),
            scale_to_unit(
                directional_consensus,
                Decimal::new(15, 2),
                Decimal::new(75, 2),
            ),
        ]
        .iter()
        .copied()
        .sum::<Decimal>()
            / Decimal::from(4);

        let min_score = Decimal::new(60, 2);
        let min_gap = Decimal::new(15, 2);
        let bias = if risk_off_score >= min_score && risk_off_score - risk_on_score >= min_gap {
            MarketRegimeBias::RiskOff
        } else if risk_on_score >= min_score && risk_on_score - risk_off_score >= min_gap {
            MarketRegimeBias::RiskOn
        } else {
            MarketRegimeBias::Neutral
        };
        let confidence = match bias {
            MarketRegimeBias::RiskOff => risk_off_score,
            MarketRegimeBias::RiskOn => risk_on_score,
            MarketRegimeBias::Neutral => risk_off_score.max(risk_on_score),
        };

        Self {
            bias,
            confidence,
            breadth_up,
            breadth_down,
            average_return,
            leader_return,
            directional_consensus,
            external_bias: None,
            external_confidence: None,
            external_driver: None,
        }
    }

    pub fn apply_polymarket_snapshot(&mut self, snapshot: &PolymarketSnapshot) {
        let strongest = snapshot
            .priors
            .iter()
            .filter(|prior| prior.active && !prior.closed)
            .filter(|prior| matches!(prior.scope, crate::ontology::ReasoningScope::Market(_)))
            .filter(|prior| prior.is_material())
            .max_by(|a, b| a.probability.cmp(&b.probability));

        let Some(prior) = strongest else {
            self.external_bias = None;
            self.external_confidence = None;
            self.external_driver = None;
            return;
        };

        self.external_bias = match prior.bias {
            PolymarketBias::RiskOn => Some(MarketRegimeBias::RiskOn),
            PolymarketBias::RiskOff => Some(MarketRegimeBias::RiskOff),
            PolymarketBias::Neutral => Some(MarketRegimeBias::Neutral),
        };
        self.external_confidence = Some(prior.probability);
        self.external_driver = Some(format!(
            "polymarket {}={} on {}",
            prior.selected_outcome,
            prior.probability.round_dp(3),
            prior.label
        ));
    }

    fn effective_blocking_bias(&self) -> Option<MarketRegimeBias> {
        let local_bias = (!matches!(self.bias, MarketRegimeBias::Neutral)).then_some(self.bias);
        let external_bias = self
            .external_bias
            .filter(|bias| !matches!(bias, MarketRegimeBias::Neutral));
        let external_confidence = self.external_confidence.unwrap_or(Decimal::ZERO);

        match (local_bias, external_bias) {
            (Some(local), Some(external)) if local == external => Some(local),
            (Some(_local), Some(external))
                if external_confidence >= Decimal::new(75, 2)
                    && self.confidence < Decimal::new(70, 2) =>
            {
                Some(external)
            }
            (Some(local), _) => Some(local),
            (None, Some(external)) if external_confidence >= Decimal::new(65, 2) => Some(external),
            _ => None,
        }
    }

    pub fn blocks(&self, direction: OrderDirection) -> bool {
        matches!(
            (self.effective_blocking_bias(), direction),
            (Some(MarketRegimeBias::RiskOff), OrderDirection::Buy)
                | (Some(MarketRegimeBias::RiskOn), OrderDirection::Sell)
        )
    }

    pub fn gate_reason(&self, direction: OrderDirection) -> Option<String> {
        if !self.blocks(direction) {
            return None;
        }

        let blocked_side = match direction {
            OrderDirection::Buy => "long",
            OrderDirection::Sell => "short",
        };
        let blocking_bias = self.effective_blocking_bias().unwrap_or(self.bias);
        let leader_fragment = self
            .leader_return
            .map(|value| {
                format!(
                    " leader_return={:+.2}%",
                    (value * Decimal::from(100)).round_dp(2)
                )
            })
            .unwrap_or_default();
        let external_fragment = self
            .external_driver
            .as_ref()
            .map(|driver| {
                format!(
                    " external={} ext_conf={:.0}%",
                    driver,
                    (self.external_confidence.unwrap_or(Decimal::ZERO) * Decimal::from(100))
                        .round_dp(0)
                )
            })
            .unwrap_or_default();

        Some(format!(
            "market regime {} blocks {} entries (breadth_down={:.0}% breadth_up={:.0}% avg_return={:+.2}% consensus={:+.2}{}{} conf={:.0}%)",
            blocking_bias,
            blocked_side,
            (self.breadth_down * Decimal::from(100)).round_dp(0),
            (self.breadth_up * Decimal::from(100)).round_dp(0),
            (self.average_return * Decimal::from(100)).round_dp(2),
            self.directional_consensus.round_dp(2),
            leader_fragment,
            external_fragment,
            (self.confidence * Decimal::from(100)).round_dp(0),
        ))
    }
}
