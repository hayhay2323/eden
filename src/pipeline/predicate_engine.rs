use std::collections::HashMap;

use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::live_snapshot::{
    LiveBackwardChain, LiveCausalLeader, LiveCrossMarketAnomaly, LiveCrossMarketSignal, LiveEvent,
    LiveHypothesisTrack, LiveMarketRegime, LivePressure, LiveSignal, LiveStressSnapshot,
    LiveTacticalCase,
};
use crate::math::clamp_unit_interval;
use crate::ontology::{
    AtomicPredicate, AtomicPredicateKind, HumanReviewContext, HumanReviewReason,
    HumanReviewReasonKind, HumanReviewVerdict,
};

pub struct PredicateInputs<'a> {
    pub tactical_case: &'a LiveTacticalCase,
    pub chain: Option<&'a LiveBackwardChain>,
    pub pressure: Option<&'a LivePressure>,
    pub signal: Option<&'a LiveSignal>,
    pub causal: Option<&'a LiveCausalLeader>,
    pub track: Option<&'a LiveHypothesisTrack>,
    pub stress: &'a LiveStressSnapshot,
    pub market_regime: &'a LiveMarketRegime,
    pub all_signals: &'a [LiveSignal],
    pub all_pressures: &'a [LivePressure],
    pub events: &'a [LiveEvent],
    pub cross_market_signals: &'a [LiveCrossMarketSignal],
    pub cross_market_anomalies: &'a [LiveCrossMarketAnomaly],
}

impl PredicateInputs<'_> {
    fn events_for_symbol(&self) -> Vec<&LiveEvent> {
        let symbol = self.tactical_case.symbol.trim();
        if symbol.is_empty() {
            return Vec::new();
        }

        let symbol_upper = symbol.to_uppercase();
        self.events
            .iter()
            .filter(|event| event.summary.to_uppercase().contains(&symbol_upper))
            .collect()
    }
}

pub fn derive_atomic_predicates(inputs: &PredicateInputs<'_>) -> Vec<AtomicPredicate> {
    let mut predicates = vec![
        signal_recurs(inputs),
        confidence_builds(inputs),
        pressure_persists(inputs),
        cross_scope_propagation(inputs),
        cross_market_link_active(inputs),
        source_concentrated(inputs),
        structural_degradation(inputs),
        stress_accelerating(inputs),
        price_reasoning_divergence(inputs),
        event_catalyst_active(inputs),
        liquidity_imbalance(inputs),
        mean_reversion_pressure(inputs),
        cross_market_dislocation(inputs),
        sector_rotation_pressure(inputs),
        leader_flip_detected(inputs),
        counterevidence_present(inputs),
    ];

    predicates.retain(|predicate| predicate.score > dec!(0.15));
    predicates.sort_by(|left, right| right.score.cmp(&left.score));
    predicates
}

pub fn augment_predicates_with_workflow(
    predicates: &[AtomicPredicate],
    workflow_state: &str,
    workflow_note: Option<&str>,
) -> Vec<AtomicPredicate> {
    let human_review = derive_human_review_context(workflow_state, workflow_note);
    let mut next = predicates
        .iter()
        .filter(|predicate| predicate.kind != AtomicPredicateKind::HumanRejected)
        .cloned()
        .collect::<Vec<_>>();

    if let Some(predicate) = human_review.as_ref().and_then(human_rejected) {
        next.push(predicate);
    }

    next.sort_by(|left, right| right.score.cmp(&left.score));
    next
}

pub fn derive_human_review_context(
    workflow_state: &str,
    workflow_note: Option<&str>,
) -> Option<HumanReviewContext> {
    let note = workflow_note
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string);
    if workflow_state == "suggest" && note.is_none() {
        return None;
    }

    let note_lower = note.as_deref().map(str::to_lowercase).unwrap_or_default();
    let mut reasons = Vec::new();
    classify_review_reason(
        &mut reasons,
        &note_lower,
        HumanReviewReasonKind::MechanismMismatch,
        &[
            "mechanism",
            "thesis",
            "narrative",
            "why",
            "premise",
            "mismatch",
            "機制",
            "敘事",
            "邏輯",
            "因果",
            "不成立",
        ],
        dec!(0.85),
    );
    classify_review_reason(
        &mut reasons,
        &note_lower,
        HumanReviewReasonKind::TimingMismatch,
        &[
            "timing",
            "too early",
            "too late",
            "wait",
            "later",
            "時機",
            "過早",
            "太早",
            "太晚",
            "先等",
        ],
        dec!(0.75),
    );
    classify_review_reason(
        &mut reasons,
        &note_lower,
        HumanReviewReasonKind::RiskTooHigh,
        &[
            "risk",
            "drawdown",
            "stop",
            "volatility",
            "sizing",
            "風險",
            "波動",
            "回撤",
            "倉位",
            "停損",
        ],
        dec!(0.80),
    );
    classify_review_reason(
        &mut reasons,
        &note_lower,
        HumanReviewReasonKind::EventRisk,
        &[
            "earnings", "fed", "cpi", "policy", "event", "news", "財報", "業績", "政策", "事件",
            "消息",
        ],
        dec!(0.75),
    );
    classify_review_reason(
        &mut reasons,
        &note_lower,
        HumanReviewReasonKind::ExecutionConstraint,
        &[
            "liquidity",
            "execution",
            "spread",
            "queue",
            "fill",
            "流動性",
            "成交",
            "排隊",
            "價差",
        ],
        dec!(0.70),
    );
    classify_review_reason(
        &mut reasons,
        &note_lower,
        HumanReviewReasonKind::EvidenceTooWeak,
        &[
            "unclear",
            "weak",
            "insufficient",
            "noisy",
            "uncertain",
            "不清楚",
            "太弱",
            "不足",
            "噪音",
            "不確定",
        ],
        dec!(0.70),
    );

    if reasons.is_empty() && workflow_state == "review" {
        reasons.push(review_reason(
            HumanReviewReasonKind::Unspecified,
            dec!(0.45),
        ));
    }

    let verdict = match workflow_state {
        "confirm" | "execute" => HumanReviewVerdict::Confirmed,
        "review" => HumanReviewVerdict::Rejected,
        _ if !reasons.is_empty() => HumanReviewVerdict::ReviewRequested,
        _ => HumanReviewVerdict::ReviewRequested,
    };

    let confidence = if reasons.is_empty() {
        match verdict {
            HumanReviewVerdict::Confirmed => dec!(0.55),
            HumanReviewVerdict::ReviewRequested => dec!(0.45),
            HumanReviewVerdict::Rejected => dec!(0.60),
        }
    } else {
        mean(
            &reasons
                .iter()
                .map(|reason| reason.confidence)
                .collect::<Vec<_>>(),
        )
    };

    Some(HumanReviewContext {
        verdict,
        verdict_label: verdict.label().to_string(),
        confidence,
        reasons,
        note,
    })
}

fn signal_recurs(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
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

fn confidence_builds(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let confidence_score = clamp_unit_interval(inputs.tactical_case.confidence);
    let gap_score = normalize_ratio(inputs.tactical_case.confidence_gap.abs(), dec!(0.25));
    let track_confidence = inputs
        .track
        .map(|track| clamp_unit_interval(track.confidence))
        .unwrap_or(confidence_score);
    let score = weighted_sum(&[
        (confidence_score, dec!(0.5)),
        (gap_score, dec!(0.2)),
        (track_confidence, dec!(0.3)),
    ]);

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

fn pressure_persists(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
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
    let accelerate_bonus = inputs
        .pressure
        .map(|pressure| {
            if pressure.accelerating {
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

fn cross_scope_propagation(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
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

fn cross_market_link_active(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
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

fn source_concentrated(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let concentration = inputs
        .chain
        .map(|chain| evidence_concentration(&chain.evidence))
        .unwrap_or_else(|| {
            if inputs.chain.is_some() {
                dec!(0.55)
            } else {
                Decimal::ZERO
            }
        });
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

fn structural_degradation(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
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

fn stress_accelerating(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let acceleration = inputs
        .pressure
        .map(|pressure| {
            if pressure.accelerating {
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
    let stress = clamp_unit_interval(inputs.stress.composite_stress);
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

fn price_reasoning_divergence(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
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

fn event_catalyst_active(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
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
    let snapshot_event_score = inputs
        .events_for_symbol()
        .into_iter()
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

    let mut evidence = inputs
        .events_for_symbol()
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

fn liquidity_imbalance(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
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

fn mean_reversion_pressure(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
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

fn cross_market_dislocation(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
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

fn sector_rotation_pressure(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
    let Some(case_sector) = case_sector(inputs) else {
        return predicate(
            AtomicPredicateKind::SectorRotationPressure,
            Decimal::ZERO,
            "板塊間的替代資金流正在形成，當前 case 更像輪動受益/受害者。",
            vec![],
        );
    };

    let by_sector = sector_pressure_map(inputs);
    let Some(case_pressure) = by_sector.get(case_sector.as_str()).copied() else {
        return predicate(
            AtomicPredicateKind::SectorRotationPressure,
            Decimal::ZERO,
            "板塊間的替代資金流正在形成，當前 case 更像輪動受益/受害者。",
            vec![],
        );
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
        return predicate(
            AtomicPredicateKind::SectorRotationPressure,
            Decimal::ZERO,
            "板塊間的替代資金流正在形成，當前 case 更像輪動受益/受害者。",
            vec![],
        );
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

fn leader_flip_detected(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
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

fn counterevidence_present(inputs: &PredicateInputs<'_>) -> AtomicPredicate {
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
        .map(|causal| normalize_count(causal.flips, 4) * dec!(0.45))
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

fn human_rejected(review: &HumanReviewContext) -> Option<AtomicPredicate> {
    let verdict_score = match review.verdict {
        HumanReviewVerdict::Rejected => dec!(0.55),
        HumanReviewVerdict::ReviewRequested => dec!(0.25),
        HumanReviewVerdict::Confirmed => Decimal::ZERO,
    };
    let reason_score = review
        .reasons
        .iter()
        .map(|reason| reason.confidence)
        .max()
        .unwrap_or(Decimal::ZERO)
        * dec!(0.45);
    let score = clamp_unit_interval(verdict_score + reason_score);

    if score <= dec!(0.15) {
        return None;
    }

    let mut evidence = vec![format!("human verdict {}", review.verdict_label)];
    for reason in review.reasons.iter().take(3) {
        evidence.push(format!(
            "reason {} ({})",
            reason.label,
            reason.confidence.round_dp(2)
        ));
    }
    if let Some(note) = review.note.as_deref() {
        if !note.is_empty() {
            evidence.push(format!("note {}", note));
        }
    }

    Some(predicate(
        AtomicPredicateKind::HumanRejected,
        score,
        "人工 workflow 的結構化判斷正在直接修正系統原先的結論。",
        evidence,
    ))
}

fn predicate(
    kind: AtomicPredicateKind,
    score: Decimal,
    summary: &str,
    evidence: Vec<String>,
) -> AtomicPredicate {
    AtomicPredicate {
        kind,
        label: kind.label().to_string(),
        law: kind.law(),
        score: clamp_unit_interval(score),
        summary: summary.to_string(),
        evidence,
    }
}

fn evidence_concentration(items: &[crate::live_snapshot::LiveEvidence]) -> Decimal {
    let total = items
        .iter()
        .fold(Decimal::ZERO, |acc, item| acc + item.weight.abs());
    if total <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    let peak = items
        .iter()
        .map(|item| item.weight.abs())
        .max()
        .unwrap_or(Decimal::ZERO);
    clamp_unit_interval(peak / total)
}

#[derive(Clone, Copy)]
struct SectorPressureStats {
    avg_pressure: Decimal,
    avg_duration: u64,
    count: usize,
}

fn case_sector(inputs: &PredicateInputs<'_>) -> Option<String> {
    inputs
        .signal
        .and_then(|signal| signal.sector.clone())
        .or_else(|| inputs.pressure.and_then(|pressure| pressure.sector.clone()))
        .map(|sector| sector.trim().to_string())
        .filter(|sector| !sector.is_empty())
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
            (count > 0).then_some((
                sector,
                SectorPressureStats {
                    avg_pressure: pressure_sum / Decimal::from(count as i64),
                    avg_duration: duration_sum / count as u64,
                    count,
                },
            ))
        })
        .collect()
}

fn weighted_sum(items: &[(Decimal, Decimal)]) -> Decimal {
    clamp_unit_interval(items.iter().fold(Decimal::ZERO, |acc, (value, weight)| {
        acc + clamp_unit_interval(*value) * *weight
    }))
}

fn mean(values: &[Decimal]) -> Decimal {
    if values.is_empty() {
        return Decimal::ZERO;
    }
    clamp_unit_interval(
        values.iter().fold(Decimal::ZERO, |acc, value| acc + *value)
            / Decimal::from(values.len() as i64),
    )
}

fn classify_review_reason(
    reasons: &mut Vec<HumanReviewReason>,
    note_lower: &str,
    kind: HumanReviewReasonKind,
    keywords: &[&str],
    confidence: Decimal,
) {
    if keywords.iter().any(|keyword| note_lower.contains(keyword)) {
        reasons.push(review_reason(kind, confidence));
    }
}

fn review_reason(kind: HumanReviewReasonKind, confidence: Decimal) -> HumanReviewReason {
    HumanReviewReason {
        kind,
        label: kind.label().to_string(),
        confidence,
    }
}

fn normalize_count(count: usize, max: usize) -> Decimal {
    if max == 0 {
        return Decimal::ZERO;
    }
    clamp_unit_interval(Decimal::from(count as i64) / Decimal::from(max as i64))
}

fn normalize_ratio(value: Decimal, max: Decimal) -> Decimal {
    if max <= Decimal::ZERO {
        return Decimal::ZERO;
    }
    clamp_unit_interval(value / max)
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;
    use crate::live_snapshot::{
        LiveBackwardChain, LiveCrossMarketAnomaly, LiveCrossMarketSignal, LiveEvent, LiveEvidence,
        LiveHypothesisTrack, LiveMarketRegime, LivePressure, LiveSignal, LiveStressSnapshot,
        LiveTacticalCase,
    };

    #[test]
    fn workflow_rejection_predicate_can_be_injected() {
        let predicates = augment_predicates_with_workflow(&[], "review", Some("reviewer reject"));
        assert!(predicates
            .iter()
            .any(|predicate| predicate.kind == AtomicPredicateKind::HumanRejected));
    }

    #[test]
    fn derive_predicates_returns_structural_signals() {
        let tactical_case = LiveTacticalCase {
            setup_id: "setup:test".into(),
            symbol: "700.HK".into(),
            title: "Test".into(),
            action: "enter".into(),
            confidence: dec!(0.74),
            confidence_gap: dec!(0.12),
            heuristic_edge: dec!(0.08),
            entry_rationale: "test".into(),
            family_label: Some("momentum".into()),
            counter_label: Some("mean_reversion".into()),
        };
        let chain = LiveBackwardChain {
            symbol: tactical_case.symbol.clone(),
            conclusion: "up".into(),
            primary_driver: "propagation".into(),
            confidence: dec!(0.7),
            evidence: vec![
                LiveEvidence {
                    source: "a".into(),
                    description: "a".into(),
                    weight: dec!(0.8),
                    direction: dec!(0.8),
                },
                LiveEvidence {
                    source: "b".into(),
                    description: "b".into(),
                    weight: dec!(0.2),
                    direction: dec!(0.1),
                },
            ],
        };
        let pressure = LivePressure {
            symbol: tactical_case.symbol.clone(),
            sector: Some("tech".into()),
            capital_flow_pressure: dec!(0.7),
            momentum: dec!(0.5),
            pressure_delta: dec!(0.4),
            pressure_duration: 6,
            accelerating: true,
        };
        let signal = LiveSignal {
            symbol: tactical_case.symbol.clone(),
            sector: Some("tech".into()),
            composite: dec!(0.65),
            mark_price: Some(dec!(380)),
            dimension_composite: Some(dec!(0.55)),
            capital_flow_direction: dec!(0.6),
            price_momentum: dec!(-0.2),
            volume_profile: dec!(0.3),
            pre_post_market_anomaly: dec!(0.6),
            valuation: dec!(0.1),
            cross_stock_correlation: Some(dec!(0.4)),
            sector_coherence: Some(dec!(0.45)),
            cross_market_propagation: Some(dec!(0.7)),
        };
        let track = LiveHypothesisTrack {
            symbol: tactical_case.symbol.clone(),
            title: "Tech bid".into(),
            status: "weakening".into(),
            age_ticks: 5,
            confidence: dec!(0.69),
        };
        let inputs = PredicateInputs {
            tactical_case: &tactical_case,
            chain: Some(&chain),
            pressure: Some(&pressure),
            signal: Some(&signal),
            causal: None,
            track: Some(&track),
            stress: &LiveStressSnapshot {
                composite_stress: dec!(0.72),
                sector_synchrony: Some(dec!(0.5)),
                pressure_consensus: Some(dec!(0.6)),
                momentum_consensus: Some(dec!(0.4)),
                pressure_dispersion: Some(dec!(0.45)),
                volume_anomaly: Some(dec!(0.2)),
            },
            market_regime: &LiveMarketRegime {
                bias: "neutral".into(),
                confidence: dec!(0.2),
                breadth_up: dec!(0.4),
                breadth_down: dec!(0.5),
                average_return: dec!(0.01),
                directional_consensus: None,
                pre_market_sentiment: None,
            },
            all_signals: &[
                signal.clone(),
                LiveSignal {
                    symbol: "9988.HK".into(),
                    sector: Some("Internet".into()),
                    composite: dec!(-0.55),
                    mark_price: Some(dec!(82)),
                    dimension_composite: Some(dec!(-0.45)),
                    capital_flow_direction: dec!(-0.6),
                    price_momentum: dec!(-0.4),
                    volume_profile: dec!(0.1),
                    pre_post_market_anomaly: dec!(0),
                    valuation: dec!(0.2),
                    cross_stock_correlation: None,
                    sector_coherence: Some(dec!(-0.5)),
                    cross_market_propagation: None,
                },
            ],
            all_pressures: &[
                pressure.clone(),
                LivePressure {
                    symbol: "9988.HK".into(),
                    sector: Some("Internet".into()),
                    capital_flow_pressure: dec!(-0.75),
                    momentum: dec!(-0.4),
                    pressure_delta: dec!(0.3),
                    pressure_duration: 9,
                    accelerating: false,
                },
            ],
            events: &[LiveEvent {
                kind: "PreMarketDislocation".into(),
                magnitude: dec!(0.8),
                summary: "700.HK pre-market dislocation".into(),
            }],
            cross_market_signals: &[LiveCrossMarketSignal {
                us_symbol: "TCEHY.US".into(),
                hk_symbol: tactical_case.symbol.clone(),
                propagation_confidence: dec!(0.66),
                time_since_hk_close_minutes: Some(120),
            }],
            cross_market_anomalies: &[LiveCrossMarketAnomaly {
                us_symbol: "TCEHY.US".into(),
                hk_symbol: tactical_case.symbol.clone(),
                expected_direction: dec!(0.5),
                actual_direction: dec!(-0.2),
                divergence: dec!(0.7),
            }],
        };

        let predicates = derive_atomic_predicates(&inputs);
        assert!(!predicates.is_empty());
        assert!(predicates
            .iter()
            .any(|item| item.kind == AtomicPredicateKind::PressurePersists));
        assert!(predicates
            .iter()
            .any(|item| item.kind == AtomicPredicateKind::CrossMarketLinkActive));
        assert!(predicates
            .iter()
            .any(|item| item.kind == AtomicPredicateKind::EventCatalystActive));
        assert!(predicates
            .iter()
            .any(|item| item.kind == AtomicPredicateKind::SectorRotationPressure));
    }

    #[test]
    fn human_review_context_is_structured() {
        let review = derive_human_review_context(
            "review",
            Some("reject thesis, timing too early, risk too high"),
        )
        .expect("human review");
        assert_eq!(review.verdict, HumanReviewVerdict::Rejected);
        assert!(review
            .reasons
            .iter()
            .any(|item| item.kind == HumanReviewReasonKind::MechanismMismatch));
        assert!(review
            .reasons
            .iter()
            .any(|item| item.kind == HumanReviewReasonKind::TimingMismatch));
    }
}
