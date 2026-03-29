use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::math::clamp_unit_interval;
use crate::ontology::{AtomicPredicate, AtomicPredicateKind};

use super::{case_sector, normalize_count, predicate, weighted_sum, PredicateInputs};

pub(super) fn cross_market_dislocation(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let divergence = inputs
        .cross_market_anomalies
        .iter()
        .map(|item| clamp_unit_interval(item.divergence.abs()))
        .max()
        .unwrap_or(Decimal::ZERO);
    let propagation = inputs
        .cross_market_signals
        .iter()
        .map(|item| clamp_unit_interval(item.propagation_confidence))
        .max()
        .unwrap_or(Decimal::ZERO);
    let direction_mismatch = inputs
        .cross_market_anomalies
        .iter()
        .map(|item| {
            if item.expected_direction * item.actual_direction < Decimal::ZERO {
                clamp_unit_interval(item.divergence.abs())
            } else {
                Decimal::ZERO
            }
        })
        .max()
        .unwrap_or(Decimal::ZERO);
    let score = weighted_sum(&[
        (divergence, dec!(0.55)),
        (propagation, dec!(0.25)),
        (direction_mismatch, dec!(0.20)),
    ]);

    let evidence = inputs
        .cross_market_anomalies
        .iter()
        .take(2)
        .map(|item| {
            format!(
                "{} / {} divergence {}",
                item.us_symbol,
                item.hk_symbol,
                item.divergence.round_dp(2)
            )
        })
        .collect::<Vec<_>>();

    predicate(
        AtomicPredicateKind::CrossMarketDislocation,
        score,
        "跨市場關係出現可交易失衡，價格更像在等待相對價值收斂。",
        evidence,
    )
}

pub(super) fn sector_rotation_pressure(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let zero_rotation = || {
        predicate(
            AtomicPredicateKind::SectorRotationPressure,
            Decimal::ZERO,
            "板塊間的替代資金流正在形成，當前 case 更像輪動受益/受害者。",
            vec![],
        )
    };
    let Some(case_sector) = case_sector(inputs) else {
        return zero_rotation();
    };

    let by_sector = sector_pressure_map(inputs);
    let Some(case_pressure) = by_sector.get(case_sector.as_str()).copied() else {
        return zero_rotation();
    };

    let opposite = by_sector
        .iter()
        .filter(|(sector, pressure)| {
            sector.as_str() != case_sector.as_str()
                && pressure.avg_pressure * case_pressure.avg_pressure < Decimal::ZERO
        })
        .max_by(|left, right| {
            left.1
                .avg_pressure
                .abs()
                .cmp(&right.1.avg_pressure.abs())
                .then_with(|| left.0.cmp(right.0))
        });

    let Some((opposite_sector, opposite_pressure)) = opposite else {
        return zero_rotation();
    };

    let spread = clamp_unit_interval(
        (case_pressure.avg_pressure - opposite_pressure.avg_pressure).abs() / dec!(2),
    );
    let durability = clamp_unit_interval(
        Decimal::from(
            case_pressure
                .avg_duration
                .min(opposite_pressure.avg_duration) as i64,
        ) / Decimal::from(24),
    );
    let peer_support =
        clamp_unit_interval(Decimal::from(case_pressure.count.min(4) as i64) / Decimal::from(4));
    let regime_alignment = if inputs.market_regime.bias.contains("risk") {
        dec!(0.10)
    } else {
        Decimal::ZERO
    };
    let score = clamp_unit_interval(
        weighted_sum(&[
            (spread, dec!(0.45)),
            (durability, dec!(0.25)),
            (peer_support, dec!(0.20)),
        ]) + regime_alignment,
    );

    let direction = if case_pressure.avg_pressure > Decimal::ZERO {
        format!("{} <- {}", case_sector, opposite_sector)
    } else {
        format!("{} -> {}", case_sector, opposite_sector)
    };
    let evidence = vec![
        format!("rotation {}", direction),
        format!(
            "sector pressure {} vs {}",
            case_pressure.avg_pressure.round_dp(2),
            opposite_pressure.avg_pressure.round_dp(2)
        ),
        format!(
            "sector duration {} vs {}",
            case_pressure.avg_duration, opposite_pressure.avg_duration
        ),
    ];

    predicate(
        AtomicPredicateKind::SectorRotationPressure,
        score,
        "板塊間的替代資金流正在形成，當前 case 更像輪動受益/受害者。",
        evidence,
    )
}

pub(super) fn leader_flip_detected(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let flip_score = inputs
        .causal
        .map(|causal| normalize_count(causal.flips, 4))
        .unwrap_or(Decimal::ZERO);
    let instability = inputs
        .causal
        .map(|causal| {
            if causal.flips == 0 {
                Decimal::ZERO
            } else if causal.leader_streak <= 2 {
                dec!(0.8)
            } else if causal.leader_streak <= 4 {
                dec!(0.45)
            } else {
                dec!(0.20)
            }
        })
        .unwrap_or(Decimal::ZERO);
    let score = weighted_sum(&[(flip_score, dec!(0.6)), (instability, dec!(0.4))]);

    let mut evidence = Vec::new();
    if let Some(causal) = inputs.causal {
        evidence.push(format!("leader {}", causal.current_leader));
        evidence.push(format!(
            "flips {} / streak {}",
            causal.flips, causal.leader_streak
        ));
    }

    predicate(
        AtomicPredicateKind::LeaderFlipDetected,
        score,
        "主導解釋最近發生切換，世界狀態尚未完全穩定。",
        evidence,
    )
}

pub(super) fn counterevidence_present(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let counter_label = if inputs.tactical_case.counter_label.is_some() {
        dec!(0.35)
    } else {
        Decimal::ZERO
    };
    let track_score = inputs
        .track
        .map(|track| match track.status.as_str() {
            "invalidated" => dec!(0.85),
            "weakening" => dec!(0.55),
            "new" => dec!(0.15),
            _ => Decimal::ZERO,
        })
        .unwrap_or(Decimal::ZERO);
    let flips = inputs
        .causal
        .map(|causal| normalize_count(causal.flips, 4))
        .unwrap_or(Decimal::ZERO);
    let score = weighted_sum(&[
        (counter_label, dec!(0.30)),
        (track_score, dec!(0.40)),
        (flips, dec!(0.30)),
    ]);

    let mut evidence = Vec::new();
    if let Some(counter) = &inputs.tactical_case.counter_label {
        evidence.push(format!("counter label {}", counter));
    }
    if let Some(track) = inputs.track {
        evidence.push(format!("hypothesis status {}", track.status));
    }

    predicate(
        AtomicPredicateKind::CounterevidencePresent,
        score,
        "存在足夠的反向證據，主要敘事需要與競爭解釋一起看。",
        evidence,
    )
}

#[derive(Clone, Copy)]
struct SectorPressureStats {
    avg_pressure: Decimal,
    avg_duration: u64,
    count: usize,
}

fn sector_pressure_map(inputs: &PredicateInputs<'_>) -> HashMap<String, SectorPressureStats> {
    let mut sums: HashMap<String, (Decimal, u64, usize)> = HashMap::new();

    for pressure in inputs.all_pressures {
        let Some(sector) = pressure
            .sector
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let entry = sums
            .entry(sector.to_string())
            .or_insert((Decimal::ZERO, 0, 0));
        entry.0 += pressure.capital_flow_pressure;
        entry.1 += pressure.pressure_duration;
        entry.2 += 1;
    }

    for signal in inputs.all_signals {
        let Some(sector) = signal
            .sector
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        else {
            continue;
        };
        let entry = sums
            .entry(sector.to_string())
            .or_insert((Decimal::ZERO, 0, 0));
        if entry.2 == 0 {
            entry.0 += signal.capital_flow_direction;
            entry.2 += 1;
        }
    }

    sums.into_iter()
        .filter_map(|(sector, (pressure_sum, duration_sum, count))| {
            let count_u64 = u64::try_from(count).ok()?;
            let avg_duration = duration_sum.checked_div(count_u64)?;
            Some((
                sector,
                SectorPressureStats {
                    avg_pressure: pressure_sum / Decimal::from(count as i64),
                    avg_duration,
                    count,
                },
            ))
        })
        .collect()
}
