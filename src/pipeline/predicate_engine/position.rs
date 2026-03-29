use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::math::clamp_unit_interval;
use crate::ontology::{AtomicPredicate, AtomicPredicateKind};

use super::{
    active_positions_for_symbol, case_direction, case_sector, direction_label,
    directions_align, directions_conflict, normalize_count, predicate, stage_label,
    weighted_sum, PredicateInputs,
};

pub(super) fn position_conflict(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let desired = case_direction(inputs);
    let conflicts = active_positions_for_symbol(inputs)
        .into_iter()
        .filter(|position| directions_conflict(desired, position.direction))
        .collect::<Vec<_>>();
    let score = conflicts
        .iter()
        .map(|position| {
            weighted_sum(&[
                (clamp_unit_interval(position.current_confidence), dec!(0.45)),
                (clamp_unit_interval(position.entry_confidence), dec!(0.20)),
                (normalize_count(position.age_ticks as usize, 24), dec!(0.20)),
                (
                    if position.exit_forming {
                        Decimal::ZERO
                    } else {
                        dec!(0.15)
                    },
                    Decimal::ONE,
                ),
            ])
        })
        .max()
        .unwrap_or(Decimal::ZERO);

    let evidence = conflicts
        .iter()
        .take(3)
        .map(|position| {
            format!(
                "{} {} stage={} age={} pnl={}",
                position.workflow_id,
                direction_label(position.direction),
                stage_label(position),
                position.age_ticks,
                position.pnl.unwrap_or(Decimal::ZERO).round_dp(2)
            )
        })
        .collect::<Vec<_>>();

    predicate(
        AtomicPredicateKind::PositionConflict,
        score,
        "現有倉位方向與新 case 相衝，新增行動更像在對沖或互相打架。",
        evidence,
    )
}

pub(super) fn position_reinforcement(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let desired = case_direction(inputs);
    let reinforcements = active_positions_for_symbol(inputs)
        .into_iter()
        .filter(|position| directions_align(desired, position.direction))
        .collect::<Vec<_>>();
    let score = reinforcements
        .iter()
        .map(|position| {
            let pnl_support = position
                .pnl
                .map(|pnl| clamp_unit_interval(pnl.max(Decimal::ZERO)))
                .unwrap_or(Decimal::ZERO);
            weighted_sum(&[
                (clamp_unit_interval(position.current_confidence), dec!(0.40)),
                (normalize_count(position.age_ticks as usize, 24), dec!(0.20)),
                (pnl_support, dec!(0.20)),
                (
                    if position.exit_forming {
                        Decimal::ZERO
                    } else {
                        dec!(0.20)
                    },
                    Decimal::ONE,
                ),
            ])
        })
        .max()
        .unwrap_or(Decimal::ZERO);

    let evidence = reinforcements
        .iter()
        .take(3)
        .map(|position| {
            format!(
                "{} {} stage={} age={} current_conf={}",
                position.workflow_id,
                direction_label(position.direction),
                stage_label(position),
                position.age_ticks,
                position.current_confidence.round_dp(2)
            )
        })
        .collect::<Vec<_>>();

    predicate(
        AtomicPredicateKind::PositionReinforcement,
        score,
        "現有倉位已在同方向運行，新的 case 更像加碼/續抱而非全新論點。",
        evidence,
    )
}

pub(super) fn concentration_risk(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let Some(case_sector) = case_sector(inputs) else {
        return predicate(
            AtomicPredicateKind::ConcentrationRisk,
            Decimal::ZERO,
            "同板塊倉位過多時，新增 case 可能只是把風險堆得更集中。",
            vec![],
        );
    };

    let same_sector = inputs
        .active_positions
        .iter()
        .filter(|position| {
            position
                .sector
                .as_deref()
                .map(str::trim)
                .filter(|sector| !sector.is_empty())
                .map(|sector| sector == case_sector)
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    let desired = case_direction(inputs);
    let aligned = same_sector
        .iter()
        .filter(|position| directions_align(desired, position.direction))
        .count();
    let same_sector_count = same_sector.len();
    let aligned_share = if same_sector_count == 0 {
        Decimal::ZERO
    } else {
        Decimal::from(aligned as i64) / Decimal::from(same_sector_count as i64)
    };
    let book_share = if inputs.active_positions.is_empty() {
        Decimal::ZERO
    } else {
        Decimal::from(same_sector_count as i64)
            / Decimal::from(inputs.active_positions.len() as i64)
    };
    let score = weighted_sum(&[
        (normalize_count(same_sector_count, 4), dec!(0.45)),
        (clamp_unit_interval(aligned_share), dec!(0.35)),
        (clamp_unit_interval(book_share), dec!(0.20)),
    ]);
    let evidence = vec![
        format!("sector {}", case_sector),
        format!("same-sector positions {}", same_sector_count),
        format!("aligned share {}", aligned_share.round_dp(2)),
    ];

    predicate(
        AtomicPredicateKind::ConcentrationRisk,
        score,
        "同板塊倉位過多時，新增 case 可能只是把風險堆得更集中。",
        evidence,
    )
}

pub(super) fn exit_condition_forming(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let positions = active_positions_for_symbol(inputs);
    let score = positions
        .iter()
        .map(|position| {
            let degradation = position.degradation_score.unwrap_or(Decimal::ZERO);
            clamp_unit_interval(if position.exit_forming {
                degradation.max(dec!(0.85))
            } else {
                degradation
            })
        })
        .max()
        .unwrap_or(Decimal::ZERO);
    let evidence = positions
        .iter()
        .filter(|position| {
            position.exit_forming
                || position.degradation_score.unwrap_or(Decimal::ZERO) > Decimal::ZERO
        })
        .take(3)
        .map(|position| {
            format!(
                "{} exit_forming={} degradation={}",
                position.workflow_id,
                position.exit_forming,
                position
                    .degradation_score
                    .unwrap_or(Decimal::ZERO)
                    .round_dp(2)
            )
        })
        .collect::<Vec<_>>();

    predicate(
        AtomicPredicateKind::ExitConditionForming,
        score,
        "既有倉位已接近出場條件，新 case 的可持續性需要打折看待。",
        evidence,
    )
}
