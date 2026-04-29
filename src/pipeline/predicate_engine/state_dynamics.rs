use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::math::clamp_unit_interval;
use crate::ontology::{AtomicPredicate, AtomicPredicateKind};

use super::{normalize_count, normalize_ratio, predicate, weighted_sum, PredicateInputs};

pub(super) fn signal_recurs(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let age_score = inputs
        .track
        .map(|track| normalize_count(track.age_ticks as usize, 6))
        .unwrap_or(Decimal::ZERO);
    let track_confidence = inputs
        .track
        .map(|track| clamp_unit_interval(track.confidence))
        .unwrap_or(Decimal::ZERO);
    let score = weighted_sum(&[(age_score, dec!(0.6)), (track_confidence, dec!(0.4))]);

    let mut evidence = Vec::new();
    if let Some(track) = inputs.track {
        evidence.push(format!("{} 已持續 {} ticks", track.title, track.age_ticks));
        evidence.push(format!(
            "目前 track confidence {}",
            track.confidence.round_dp(2)
        ));
    }

    predicate(
        AtomicPredicateKind::SignalRecurs,
        score,
        "同一方向的訊號在連續 tick 反覆出現，較不像單點噪音。",
        evidence,
    )
}

pub(super) fn confidence_builds(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let confidence_score = clamp_unit_interval(inputs.tactical_case.confidence);
    let gap_score = normalize_ratio(inputs.tactical_case.confidence_gap.abs(), dec!(0.25));
    let score = if let Some(track) = inputs.track {
        let tc = clamp_unit_interval(track.confidence);
        weighted_sum(&[
            (confidence_score, dec!(0.5)),
            (gap_score, dec!(0.2)),
            (tc, dec!(0.3)),
        ])
    } else {
        // No track yet — redistribute weight to avoid double-counting confidence_score
        weighted_sum(&[(confidence_score, dec!(0.65)), (gap_score, dec!(0.35))])
    };

    predicate(
        AtomicPredicateKind::ConfidenceBuilds,
        score,
        "case 的信心與結構邊際正在形成，而不是只有單點高波動。",
        vec![
            format!("confidence {}", inputs.tactical_case.confidence.round_dp(2)),
            format!(
                "confidence gap {}",
                inputs.tactical_case.confidence_gap.round_dp(2)
            ),
        ],
    )
}

pub(super) fn pressure_persists(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let duration = inputs
        .pressure
        .map(|pressure| normalize_count(pressure.pressure_duration as usize, 8))
        .unwrap_or(Decimal::ZERO);
    let intensity = inputs
        .pressure
        .map(|pressure| clamp_unit_interval(pressure.capital_flow_pressure.abs()))
        .unwrap_or(Decimal::ZERO);
    let delta = inputs
        .pressure
        .map(|pressure| clamp_unit_interval(pressure.pressure_delta.abs()))
        .unwrap_or(Decimal::ZERO);
    // Only grant the acceleration bonus when pressure direction aligns with the
    // case's implied direction — acceleration opposing the case is not persistence,
    // it's a counter-signal (handled by stress_accelerating instead).
    let accelerate_bonus = inputs
        .pressure
        .map(|pressure| {
            let direction_aligned = pressure.capital_flow_pressure.is_sign_positive()
                == inputs.tactical_case.confidence.is_sign_positive();
            if pressure.accelerating && direction_aligned {
                dec!(0.15)
            } else {
                Decimal::ZERO
            }
        })
        .unwrap_or(Decimal::ZERO);
    let score = clamp_unit_interval(
        weighted_sum(&[
            (duration, dec!(0.35)),
            (intensity, dec!(0.35)),
            (delta, dec!(0.15)),
        ]) + accelerate_bonus,
    );

    let mut evidence = Vec::new();
    if let Some(pressure) = inputs.pressure {
        evidence.push(format!("pressure duration {}", pressure.pressure_duration));
        evidence.push(format!(
            "capital flow pressure {} / delta {}",
            pressure.capital_flow_pressure.round_dp(2),
            pressure.pressure_delta.round_dp(2)
        ));
        if pressure.accelerating {
            evidence.push("pressure 正在加速".into());
        }
    }

    predicate(
        AtomicPredicateKind::PressurePersists,
        score,
        "資金壓力正在持續並累積，而不是瞬時尖峰。",
        evidence,
    )
}

pub(super) fn structural_degradation(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let stress = clamp_unit_interval(inputs.stress.composite_stress);
    let dispersion = inputs
        .stress
        .pressure_dispersion
        .map(|value| clamp_unit_interval(value.abs()))
        .unwrap_or(Decimal::ZERO);
    let track_score = inputs
        .track
        .map(|track| match track.status.as_str() {
            "invalidated" => dec!(0.95),
            "weakening" => dec!(0.75),
            "new" => dec!(0.35),
            "strengthening" => dec!(0.20),
            _ => dec!(0.10),
        })
        .unwrap_or(Decimal::ZERO);
    let score = weighted_sum(&[
        (stress, dec!(0.45)),
        (dispersion, dec!(0.2)),
        (track_score, dec!(0.35)),
    ]);

    predicate(
        AtomicPredicateKind::StructuralDegradation,
        score,
        "結構退化正在發生，價格之外的系統狀態已先開始惡化。",
        vec![
            format!(
                "composite stress {}",
                inputs.stress.composite_stress.round_dp(2)
            ),
            format!(
                "pressure dispersion {}",
                inputs
                    .stress
                    .pressure_dispersion
                    .unwrap_or(Decimal::ZERO)
                    .round_dp(2)
            ),
        ],
    )
}

pub(super) fn stress_accelerating(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    // Only treat acceleration as stress-related when composite stress is already
    // elevated (> 0.30). This avoids co-activation with pressure_persists when
    // pressure is accelerating in a low-stress environment (which is directional
    // reinforcement, not structural fragility).
    let stress = clamp_unit_interval(inputs.stress.composite_stress);
    let stress_elevated = stress > dec!(0.30);
    let acceleration = inputs
        .pressure
        .map(|pressure| {
            if pressure.accelerating && stress_elevated {
                dec!(0.8)
            } else {
                Decimal::ZERO
            }
        })
        .unwrap_or(Decimal::ZERO);
    let delta = inputs
        .pressure
        .map(|pressure| clamp_unit_interval(pressure.pressure_delta.abs()))
        .unwrap_or(Decimal::ZERO);
    let score = weighted_sum(&[
        (acceleration, dec!(0.45)),
        (delta, dec!(0.3)),
        (stress, dec!(0.25)),
    ]);

    predicate(
        AtomicPredicateKind::StressAccelerating,
        score,
        "壓力不只是存在，而是在加速接近臨界點。",
        vec![format!(
            "stress {}",
            inputs.stress.composite_stress.round_dp(2)
        )],
    )
}
