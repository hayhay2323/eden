use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::math::clamp_unit_interval;
use crate::ontology::{AtomicPredicate, AtomicPredicateKind};

use super::{evidence_concentration, normalize_count, predicate, weighted_sum, PredicateInputs};

pub(super) fn cross_scope_propagation(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let chain_depth = inputs
        .chain
        .map(|chain| normalize_count(chain.evidence.len(), 4))
        .unwrap_or(Decimal::ZERO);
    let signal_propagation = inputs
        .signal
        .and_then(|signal| signal.cross_market_propagation)
        .map(clamp_unit_interval)
        .unwrap_or(Decimal::ZERO);
    let linked_nodes = normalize_count(inputs.cross_market_signals.len(), 3);
    let score = weighted_sum(&[
        (chain_depth, dec!(0.4)),
        (signal_propagation, dec!(0.35)),
        (linked_nodes, dec!(0.25)),
    ]);

    let mut evidence = Vec::new();
    if let Some(chain) = inputs.chain {
        evidence.push(format!("backward chain evidence {}", chain.evidence.len()));
        evidence.push(format!("primary driver {}", chain.primary_driver));
    }
    if !inputs.cross_market_signals.is_empty() {
        evidence.push(format!(
            "cross-market links {}",
            inputs.cross_market_signals.len()
        ));
    }

    predicate(
        AtomicPredicateKind::CrossScopePropagation,
        score,
        "影響開始跨作用域傳播，case 不再只是局部局勢。",
        evidence,
    )
}

pub(super) fn cross_market_link_active(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let link_score = inputs
        .cross_market_signals
        .iter()
        .map(|item| clamp_unit_interval(item.propagation_confidence))
        .max()
        .unwrap_or(Decimal::ZERO);
    let signal_score = inputs
        .signal
        .and_then(|signal| signal.cross_market_propagation)
        .map(clamp_unit_interval)
        .unwrap_or(Decimal::ZERO);
    let anomaly_score = inputs
        .cross_market_anomalies
        .iter()
        .map(|item| clamp_unit_interval(item.divergence.abs()))
        .max()
        .unwrap_or(Decimal::ZERO);
    let score = weighted_sum(&[
        (link_score, dec!(0.45)),
        (signal_score, dec!(0.35)),
        (anomaly_score, dec!(0.20)),
    ]);

    predicate(
        AtomicPredicateKind::CrossMarketLinkActive,
        score,
        "跨市場 linkage 已經參與這個 case 的生成，而不是純本地波動。",
        vec![
            format!("cross-market signals {}", inputs.cross_market_signals.len()),
            format!(
                "cross-market anomalies {}",
                inputs.cross_market_anomalies.len()
            ),
        ],
    )
}

pub(super) fn source_concentrated(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let concentration = inputs
        .chain
        .map(|chain| {
            let c = evidence_concentration(&chain.evidence);
            // A chain with no evidence items has no information about
            // concentration — default to zero to avoid spurious activation.
            if chain.evidence.is_empty() {
                Decimal::ZERO
            } else {
                c
            }
        })
        .unwrap_or(Decimal::ZERO);
    let leader = inputs
        .causal
        .map(|causal| normalize_count(causal.leader_streak as usize, 6))
        .unwrap_or(Decimal::ZERO);
    let score = weighted_sum(&[(concentration, dec!(0.7)), (leader, dec!(0.3))]);

    let mut evidence = Vec::new();
    if let Some(chain) = inputs.chain {
        evidence.push(format!("primary driver {}", chain.primary_driver));
    }
    if let Some(causal) = inputs.causal {
        evidence.push(format!(
            "leader {} / streak {}",
            causal.current_leader, causal.leader_streak
        ));
    }

    predicate(
        AtomicPredicateKind::SourceConcentrated,
        score,
        "驅動源頭較集中，結構更像由少數主導因子推動。",
        evidence,
    )
}

pub(super) fn price_reasoning_divergence(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let anomaly_dominance = inputs
        .signal
        .map(|signal| {
            let anomaly = signal.pre_post_market_anomaly.abs();
            let momentum = signal.price_momentum.abs();
            if anomaly <= momentum {
                Decimal::ZERO
            } else {
                clamp_unit_interval(anomaly / (momentum + dec!(0.05)))
            }
        })
        .unwrap_or(Decimal::ZERO);
    let action_mismatch = inputs
        .signal
        .map(|signal| {
            let momentum = signal.price_momentum;
            match inputs.tactical_case.action.as_str() {
                "enter" if momentum < Decimal::ZERO => clamp_unit_interval(momentum.abs()),
                "review" | "watch" if momentum > dec!(0.25) => clamp_unit_interval(momentum.abs()),
                _ => Decimal::ZERO,
            }
        })
        .unwrap_or(Decimal::ZERO);
    let anomaly_score = inputs
        .cross_market_anomalies
        .iter()
        .map(|item| clamp_unit_interval(item.divergence.abs()))
        .max()
        .unwrap_or(Decimal::ZERO);
    let score = weighted_sum(&[
        (anomaly_dominance, dec!(0.4)),
        (action_mismatch, dec!(0.25)),
        (anomaly_score, dec!(0.35)),
    ]);

    predicate(
        AtomicPredicateKind::PriceReasoningDivergence,
        score,
        "價格表現與推理主線出現背離，敘事與市場反應未完全一致。",
        vec![
            format!("action {}", inputs.tactical_case.action),
            format!(
                "cross-market anomalies {}",
                inputs.cross_market_anomalies.len()
            ),
        ],
    )
}

pub(super) fn event_catalyst_active(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let event_score = inputs
        .chain
        .map(|chain| {
            chain
                .evidence
                .iter()
                .filter(|item| {
                    let source = item.source.to_lowercase();
                    source.contains("event") || source.contains("pre_market")
                })
                .map(|item| clamp_unit_interval(item.weight.abs()))
                .max()
                .unwrap_or(Decimal::ZERO)
        })
        .unwrap_or(Decimal::ZERO);
    let symbol_events = inputs.events_for_symbol();
    let snapshot_event_score = symbol_events
        .iter()
        .map(|event| clamp_unit_interval(event.magnitude.abs()))
        .max()
        .unwrap_or(Decimal::ZERO);
    let anomaly_score = inputs
        .signal
        .map(|signal| clamp_unit_interval(signal.pre_post_market_anomaly.abs()))
        .unwrap_or(Decimal::ZERO);
    let score = weighted_sum(&[
        (snapshot_event_score, dec!(0.45)),
        (anomaly_score, dec!(0.35)),
        (event_score, dec!(0.20)),
    ]);

    let mut evidence = symbol_events
        .into_iter()
        .take(2)
        .map(|event| format!("event {} {}", event.kind, event.summary))
        .collect::<Vec<_>>();
    if let Some(signal) = inputs.signal {
        if signal.pre_post_market_anomaly.abs() > dec!(0.20) {
            evidence.push(format!(
                "pre/post anomaly {}",
                signal.pre_post_market_anomaly.round_dp(2)
            ));
        }
    }

    predicate(
        AtomicPredicateKind::EventCatalystActive,
        score,
        "事件或盤前異常正直接推動當前 case，而不是純結構延續。",
        evidence,
    )
}

pub(super) fn liquidity_imbalance(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let pressure = inputs
        .pressure
        .map(|pressure| clamp_unit_interval(pressure.capital_flow_pressure.abs()))
        .unwrap_or(Decimal::ZERO);
    let duration = inputs
        .pressure
        .map(|pressure| normalize_count(pressure.pressure_duration as usize, 12))
        .unwrap_or(Decimal::ZERO);
    let absorption_gap = match (inputs.pressure, inputs.signal) {
        (Some(pressure), Some(signal)) => {
            let flow = pressure.capital_flow_pressure.abs();
            let price = signal.price_momentum.abs();
            if flow <= price {
                Decimal::ZERO
            } else {
                clamp_unit_interval((flow - price) / (flow + dec!(0.10)))
            }
        }
        _ => Decimal::ZERO,
    };
    let concentration = inputs
        .chain
        .map(|chain| evidence_concentration(&chain.evidence))
        .unwrap_or(Decimal::ZERO);
    let score = weighted_sum(&[
        (absorption_gap, dec!(0.45)),
        (pressure, dec!(0.25)),
        (duration, dec!(0.15)),
        (concentration, dec!(0.15)),
    ]);

    let mut evidence = Vec::new();
    if let Some(pressure) = inputs.pressure {
        evidence.push(format!(
            "capital flow pressure {} / duration {}",
            pressure.capital_flow_pressure.round_dp(2),
            pressure.pressure_duration
        ));
    }
    if let Some(signal) = inputs.signal {
        evidence.push(format!(
            "price momentum {}",
            signal.price_momentum.round_dp(2)
        ));
    }

    predicate(
        AtomicPredicateKind::LiquidityImbalance,
        score,
        "資金力量存在，但價格推進被吸收或卡住，出現流動性失衡跡象。",
        evidence,
    )
}

pub(super) fn mean_reversion_pressure(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let valuation = inputs
        .signal
        .map(|signal| clamp_unit_interval(signal.valuation.abs()))
        .unwrap_or(Decimal::ZERO);
    let stretch = inputs
        .signal
        .map(|signal| {
            clamp_unit_interval(
                signal
                    .pre_post_market_anomaly
                    .abs()
                    .max(signal.price_momentum.abs()),
            )
        })
        .unwrap_or(Decimal::ZERO);
    let weak_flow_support = match (inputs.signal, inputs.pressure) {
        (Some(signal), Some(pressure)) => {
            let stretch = signal
                .pre_post_market_anomaly
                .abs()
                .max(signal.price_momentum.abs());
            let flow = pressure.capital_flow_pressure.abs();
            if stretch <= flow {
                Decimal::ZERO
            } else {
                clamp_unit_interval((stretch - flow) / (stretch + dec!(0.10)))
            }
        }
        (Some(signal), None) => clamp_unit_interval(
            signal
                .pre_post_market_anomaly
                .abs()
                .max(signal.price_momentum.abs()),
        ),
        _ => Decimal::ZERO,
    };
    let score = weighted_sum(&[
        (stretch, dec!(0.45)),
        (weak_flow_support, dec!(0.30)),
        (valuation, dec!(0.25)),
    ]);

    let mut evidence = Vec::new();
    if let Some(signal) = inputs.signal {
        evidence.push(format!("valuation {}", signal.valuation.round_dp(2)));
        evidence.push(format!(
            "stretch {}",
            signal
                .pre_post_market_anomaly
                .abs()
                .max(signal.price_momentum.abs())
                .round_dp(2)
        ));
    }

    predicate(
        AtomicPredicateKind::MeanReversionPressure,
        score,
        "價格伸展已高於流量支撐，均值回歸的機械修正壓力正在累積。",
        evidence,
    )
}
