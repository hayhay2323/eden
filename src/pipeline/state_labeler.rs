use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SymbolStateLabel {
    Continuation,
    TurningPoint,
    LowInformation,
}

impl SymbolStateLabel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Continuation => "continuation",
            Self::TurningPoint => "turning_point",
            Self::LowInformation => "low_information",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum StateLabelHorizon {
    Fast,
    Mid,
    Late,
}

impl StateLabelHorizon {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Fast => "fast",
            Self::Mid => "mid",
            Self::Late => "late",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct StateLabel {
    pub label: SymbolStateLabel,
    pub confidence: Decimal,
    pub horizon: StateLabelHorizon,
    pub ticks_to_resolution: u64,
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OutcomeLabelInput {
    pub entry_tick: u64,
    pub resolved_tick: u64,
    pub net_return: Decimal,
    pub max_favorable_excursion: Decimal,
    pub max_adverse_excursion: Decimal,
    pub followed_through: bool,
    pub invalidated: bool,
    pub structure_retained: bool,
}

pub fn label_outcome(input: OutcomeLabelInput) -> StateLabel {
    let ticks_to_resolution = input.resolved_tick.saturating_sub(input.entry_tick);
    let horizon = horizon_bucket(ticks_to_resolution);
    let meaningful_excursion = input
        .max_favorable_excursion
        .max(abs_decimal(input.max_adverse_excursion));

    if input.followed_through && input.structure_retained && input.net_return > Decimal::ZERO {
        let evidence = input.max_favorable_excursion.max(input.net_return);
        let confidence = clamp_decimal(
            dec!(0.55) + normalize_against(evidence, dec!(0.02)) * dec!(0.35),
            dec!(0.55),
            dec!(0.99),
        );
        return StateLabel {
            label: SymbolStateLabel::Continuation,
            confidence,
            horizon,
            ticks_to_resolution,
            reason_codes: vec![
                "followed_through".into(),
                "structure_retained".into(),
                "positive_net_return".into(),
            ],
        };
    }

    if input.invalidated
        || (input.followed_through && !input.structure_retained)
        || (input.net_return <= Decimal::ZERO && meaningful_excursion >= dec!(0.003))
    {
        let adverse = abs_decimal(input.max_adverse_excursion).max(abs_decimal(input.net_return));
        let confidence = clamp_decimal(
            dec!(0.55) + normalize_against(adverse, dec!(0.02)) * dec!(0.30),
            dec!(0.55),
            dec!(0.95),
        );
        let mut reason_codes = Vec::new();
        if input.invalidated {
            reason_codes.push("invalidated".into());
        }
        if input.followed_through && !input.structure_retained {
            reason_codes.push("whipsaw_after_follow_through".into());
        }
        if input.net_return <= Decimal::ZERO {
            reason_codes.push("negative_net_return".into());
        }
        if reason_codes.is_empty() {
            reason_codes.push("adverse_excursion_dominant".into());
        }
        return StateLabel {
            label: SymbolStateLabel::TurningPoint,
            confidence,
            horizon,
            ticks_to_resolution,
            reason_codes,
        };
    }

    let confidence = clamp_decimal(
        dec!(0.35) + normalize_against(meaningful_excursion, dec!(0.01)) * dec!(0.20),
        dec!(0.35),
        dec!(0.70),
    );
    StateLabel {
        label: SymbolStateLabel::LowInformation,
        confidence,
        horizon,
        ticks_to_resolution,
        reason_codes: vec![
            "no_follow_through".into(),
            "no_invalidation".into(),
            "weak_excursion".into(),
        ],
    }
}

fn horizon_bucket(ticks_to_resolution: u64) -> StateLabelHorizon {
    match ticks_to_resolution {
        0..=5 => StateLabelHorizon::Fast,
        6..=20 => StateLabelHorizon::Mid,
        _ => StateLabelHorizon::Late,
    }
}

fn normalize_against(value: Decimal, scale: Decimal) -> Decimal {
    if scale <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    clamp_decimal(value / scale, Decimal::ZERO, Decimal::ONE)
}

fn abs_decimal(value: Decimal) -> Decimal {
    if value < Decimal::ZERO {
        -value
    } else {
        value
    }
}

fn clamp_decimal(value: Decimal, lower: Decimal, upper: Decimal) -> Decimal {
    value.max(lower).min(upper)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn retained_positive_move_is_continuation() {
        let label = label_outcome(OutcomeLabelInput {
            entry_tick: 10,
            resolved_tick: 14,
            net_return: dec!(0.012),
            max_favorable_excursion: dec!(0.018),
            max_adverse_excursion: dec!(-0.002),
            followed_through: true,
            invalidated: false,
            structure_retained: true,
        });
        assert_eq!(label.label, SymbolStateLabel::Continuation);
        assert_eq!(label.horizon, StateLabelHorizon::Fast);
        assert!(label.confidence >= dec!(0.55));
    }

    #[test]
    fn invalidated_case_is_turning_point() {
        let label = label_outcome(OutcomeLabelInput {
            entry_tick: 10,
            resolved_tick: 22,
            net_return: dec!(-0.009),
            max_favorable_excursion: dec!(0.002),
            max_adverse_excursion: dec!(-0.011),
            followed_through: false,
            invalidated: true,
            structure_retained: false,
        });
        assert_eq!(label.label, SymbolStateLabel::TurningPoint);
        assert_eq!(label.horizon, StateLabelHorizon::Mid);
        assert!(label.reason_codes.iter().any(|item| item == "invalidated"));
    }

    #[test]
    fn weak_path_is_low_information() {
        let label = label_outcome(OutcomeLabelInput {
            entry_tick: 10,
            resolved_tick: 35,
            net_return: dec!(0.0004),
            max_favorable_excursion: dec!(0.0015),
            max_adverse_excursion: dec!(-0.0010),
            followed_through: false,
            invalidated: false,
            structure_retained: false,
        });
        assert_eq!(label.label, SymbolStateLabel::LowInformation);
        assert_eq!(label.horizon, StateLabelHorizon::Late);
    }
}
