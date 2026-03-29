use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::math::clamp_unit_interval;
use crate::ontology::{AtomicPredicate, AtomicPredicateKind};

use super::{normalize_count, predicate, weighted_sum, PredicateInputs};

pub(super) fn regime_stability(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let regime_age = inputs
        .track
        .map(|track| normalize_count(track.age_ticks as usize, 12))
        .unwrap_or(Decimal::ZERO);
    let status_score = inputs
        .track
        .map(|track| match track.status.as_str() {
            "stable" => dec!(0.65),
            "strengthening" => dec!(0.55),
            "new" => dec!(0.20),
            _ => Decimal::ZERO,
        })
        .unwrap_or(Decimal::ZERO);
    let low_stress =
        (Decimal::ONE - clamp_unit_interval(inputs.stress.composite_stress)).max(Decimal::ZERO);
    let score = weighted_sum(&[
        (regime_age, dec!(0.40)),
        (status_score, dec!(0.35)),
        (low_stress, dec!(0.25)),
    ]);

    let mut evidence = Vec::new();
    if let Some(track) = inputs.track {
        evidence.push(format!(
            "track {} age {} status {}",
            track.title, track.age_ticks, track.status
        ));
    }
    evidence.push(format!(
        "stress {}",
        inputs.stress.composite_stress.round_dp(2)
    ));

    predicate(
        AtomicPredicateKind::RegimeStability,
        score,
        "市場體制在近期 tick 中保持一致，結構性模式尚未打破。",
        evidence,
    )
}

pub(super) fn consolidation_before_breakout(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let low_momentum = inputs
        .signal
        .map(|signal| {
            let m = signal.price_momentum.abs();
            if m < dec!(0.15) {
                dec!(0.8)
            } else if m < dec!(0.30) {
                dec!(0.4)
            } else {
                Decimal::ZERO
            }
        })
        .unwrap_or(Decimal::ZERO);
    let flow_building = inputs
        .pressure
        .map(|pressure| {
            let flow = clamp_unit_interval(pressure.capital_flow_pressure.abs());
            let duration = normalize_count(pressure.pressure_duration as usize, 6);
            weighted_sum(&[(flow, dec!(0.6)), (duration, dec!(0.4))])
        })
        .unwrap_or(Decimal::ZERO);
    let score = weighted_sum(&[(low_momentum, dec!(0.45)), (flow_building, dec!(0.55))]);

    let mut evidence = Vec::new();
    if let Some(signal) = inputs.signal {
        evidence.push(format!(
            "price momentum {}",
            signal.price_momentum.round_dp(2)
        ));
    }
    if let Some(pressure) = inputs.pressure {
        evidence.push(format!(
            "capital flow {} duration {}",
            pressure.capital_flow_pressure.round_dp(2),
            pressure.pressure_duration
        ));
    }

    predicate(
        AtomicPredicateKind::ConsolidationBeforeBreakout,
        score,
        "價格區間震盪但資金流正在單方向累積，可能正在吸籌或出貨。",
        evidence,
    )
}

pub(super) fn broker_replenish_active(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let symbol_events = inputs.events_for_symbol();
    let iceberg_score = symbol_events
        .iter()
        .filter(|event| event.kind == "IcebergDetected")
        .map(|event| clamp_unit_interval(event.magnitude.abs()))
        .max()
        .unwrap_or(Decimal::ZERO);
    let evidence: Vec<String> = symbol_events
        .iter()
        .filter(|event| event.kind == "IcebergDetected")
        .take(2)
        .map(|event| format!("iceberg {}", event.summary))
        .collect();
    predicate(
        AtomicPredicateKind::BrokerReplenishActive,
        iceberg_score,
        "經紀商在消失後迅速重新出現，暗示冰山訂單正在吸收流動性。",
        evidence,
    )
}

pub(super) fn broker_cluster_aligned(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let cluster_score = inputs
        .events
        .iter()
        .filter(|event| event.kind == "BrokerClusterFormation")
        .map(|event| clamp_unit_interval(event.magnitude.abs()))
        .max()
        .unwrap_or(Decimal::ZERO);
    let evidence: Vec<String> = inputs
        .events
        .iter()
        .filter(|event| event.kind == "BrokerClusterFormation")
        .take(2)
        .map(|event| format!("cluster {}", event.summary))
        .collect();
    predicate(
        AtomicPredicateKind::BrokerClusterAligned,
        cluster_score,
        "同一機構的多個經紀商同時出現在隊列中，表明協調部署。",
        evidence,
    )
}

pub(super) fn broker_concentration_risk(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let symbol_events = inputs.events_for_symbol();
    let flip_score = symbol_events
        .iter()
        .filter(|event| event.kind == "BrokerSideFlip")
        .map(|event| clamp_unit_interval(event.magnitude.abs()))
        .max()
        .unwrap_or(Decimal::ZERO);
    let iceberg_count = symbol_events
        .iter()
        .filter(|event| event.kind == "IcebergDetected")
        .count();
    let count_score = clamp_unit_interval(Decimal::from(iceberg_count.min(5) as i64) / dec!(5));
    let score = weighted_sum(&[(flip_score, dec!(0.5)), (count_score, dec!(0.5))]);
    let mut evidence: Vec<String> = Vec::new();
    if flip_score > Decimal::ZERO {
        evidence.push(format!("side flip score {}", flip_score.round_dp(2)));
    }
    if iceberg_count > 0 {
        evidence.push(format!("{} iceberg events", iceberg_count));
    }
    predicate(
        AtomicPredicateKind::BrokerConcentrationRisk,
        score,
        "經紀商行為集中在少數席位，帶有冰山和翻轉信號，形成集中風險。",
        evidence,
    )
}
