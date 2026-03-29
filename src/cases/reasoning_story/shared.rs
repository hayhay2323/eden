use super::*;
#[cfg(feature = "persistence")]
use crate::persistence::tactical_setup::TacticalSetupRecord;

#[cfg(feature = "persistence")]
pub(super) fn current_assessment_snapshot(detail: &CaseDetail) -> CaseReasoningAssessmentSnapshot {
    let recorded_at = detail
        .workflow
        .as_ref()
        .map(|workflow| workflow.timestamp)
        .or_else(|| summary_recorded_at(&detail.summary));
    assessment_snapshot_from_summary(&detail.summary, recorded_at)
}

#[cfg(feature = "persistence")]
pub(super) fn assessment_snapshot_from_summary(
    summary: &CaseSummary,
    recorded_at: Option<OffsetDateTime>,
) -> CaseReasoningAssessmentSnapshot {
    let recorded_at = recorded_at
        .or_else(|| summary_recorded_at(summary))
        .unwrap_or(OffsetDateTime::UNIX_EPOCH);

    CaseReasoningAssessmentSnapshot {
        assessment_id: format!("{}:current", summary.setup_id),
        setup_id: summary.setup_id.clone(),
        market: match summary.market {
            LiveMarket::Hk => "hk".into(),
            LiveMarket::Us => "us".into(),
        },
        symbol: summary.symbol.clone(),
        title: summary.title.clone(),
        recommended_action: summary.recommended_action.clone(),
        source: "current".into(),
        recorded_at,
        workflow_state: summary.workflow_state.clone(),
        market_regime_bias: Some(summary.market_regime_bias.clone()),
        market_regime_confidence: Some(summary.market_regime_confidence),
        market_breadth_delta: Some(summary.market_breadth_delta),
        market_average_return: Some(summary.market_average_return),
        market_directional_consensus: summary.market_directional_consensus,
        owner: summary.owner.clone(),
        reviewer: summary.reviewer.clone(),
        actor: summary.workflow_actor.clone(),
        note: summary.workflow_note.clone(),
        sector: summary.sector.clone(),
        primary_mechanism_kind: summary
            .reasoning_profile
            .primary_mechanism
            .as_ref()
            .map(|item| item.label.clone()),
        primary_mechanism_score: summary
            .reasoning_profile
            .primary_mechanism
            .as_ref()
            .map(|item| item.score),
        law_kinds: summary
            .reasoning_profile
            .laws
            .iter()
            .map(|item| item.label.clone())
            .collect(),
        predicate_kinds: summary
            .reasoning_profile
            .predicates
            .iter()
            .map(|item| item.label.clone())
            .collect(),
        composite_state_kinds: summary
            .reasoning_profile
            .composite_states
            .iter()
            .map(|item| item.label.clone())
            .collect(),
        competing_mechanism_kinds: summary
            .reasoning_profile
            .competing_mechanisms
            .iter()
            .map(|item| item.label.clone())
            .collect(),
        invalidation_rules: summary.invalidation_rules.clone(),
        reasoning_profile: summary.reasoning_profile.clone(),
    }
}

#[cfg(feature = "persistence")]
pub(super) fn summary_recorded_at(summary: &CaseSummary) -> Option<OffsetDateTime> {
    OffsetDateTime::parse(
        &summary.updated_at,
        &time::format_description::well_known::Rfc3339,
    )
    .ok()
}

#[cfg(feature = "persistence")]
pub(super) fn regime_bucket(summary: &CaseSummary) -> String {
    let confidence_bucket = if summary.market_regime_confidence >= Decimal::new(65, 2) {
        "high"
    } else if summary.market_regime_confidence >= Decimal::new(35, 2) {
        "medium"
    } else {
        "low"
    };
    format!("{}:{confidence_bucket}", summary.market_regime_bias)
}

#[cfg(feature = "persistence")]
pub(super) fn snapshot_matches_current(
    existing: &CaseReasoningAssessmentSnapshot,
    current: &CaseReasoningAssessmentSnapshot,
) -> bool {
    existing.workflow_state == current.workflow_state
        && existing.market_regime_bias == current.market_regime_bias
        && existing.primary_mechanism_kind == current.primary_mechanism_kind
        && existing.note == current.note
}

#[cfg(feature = "persistence")]
pub(super) fn mechanism_factor_map(
    snapshot: &CaseReasoningAssessmentSnapshot,
) -> HashMap<String, (String, Decimal)> {
    snapshot
        .reasoning_profile
        .primary_mechanism
        .as_ref()
        .map(|mechanism| {
            mechanism
                .factors
                .iter()
                .map(|factor| {
                    (
                        factor.key.clone(),
                        (factor.label.clone(), factor.contribution),
                    )
                })
                .collect::<HashMap<_, _>>()
        })
        .unwrap_or_default()
}

#[cfg(feature = "persistence")]
pub(super) fn state_score_map(snapshot: &CaseReasoningAssessmentSnapshot) -> HashMap<String, Decimal> {
    snapshot
        .reasoning_profile
        .composite_states
        .iter()
        .map(|state| (state.label.clone(), state.score))
        .collect()
}

#[cfg(feature = "persistence")]
pub(super) fn factor_delta_strings(
    primary: &HashMap<String, (String, Decimal)>,
    reference: &HashMap<String, (String, Decimal)>,
    require_negative: bool,
) -> Vec<String> {
    let mut deltas = primary
        .iter()
        .filter_map(|(key, (label, value))| {
            let other = reference
                .get(key)
                .map(|item| item.1)
                .unwrap_or(Decimal::ZERO);
            let delta = *value - other;
            if require_negative {
                (delta > Decimal::new(8, 2))
                    .then_some(format!("{label} faded {:+}", -delta.round_dp(2)))
            } else {
                (delta > Decimal::new(8, 2))
                    .then_some(format!("{label} rose {:+}", delta.round_dp(2)))
            }
        })
        .collect::<Vec<_>>();
    deltas.sort();
    deltas.truncate(3);
    deltas
}

#[cfg(feature = "persistence")]
pub(super) fn regime_delta_strings(
    from_states: &HashMap<String, Decimal>,
    to_states: &HashMap<String, Decimal>,
) -> Vec<String> {
    let regime_keys = [
        "Event Catalyst",
        "Cross-market Dislocation",
        "Substitution Flow",
        "Cross-scope Contagion",
        "Structural Fragility",
    ];
    let mut items = regime_keys
        .iter()
        .filter_map(|key| {
            let delta = to_states.get(*key).copied().unwrap_or(Decimal::ZERO)
                - from_states.get(*key).copied().unwrap_or(Decimal::ZERO);
            (delta > Decimal::new(8, 2))
                .then_some(format!("{key} strengthened {:+}", delta.round_dp(2)))
        })
        .collect::<Vec<_>>();
    items.truncate(3);
    items
}

#[cfg(feature = "persistence")]
pub(super) fn regime_metric_delta_strings(
    from: &CaseReasoningAssessmentSnapshot,
    to: &CaseReasoningAssessmentSnapshot,
) -> Vec<String> {
    let mut items = Vec::new();
    if let Some(delta) = decimal_delta(from.market_regime_confidence, to.market_regime_confidence) {
        if delta.abs() >= Decimal::new(10, 2) {
            items.push(format!("regime confidence {:+}", delta.round_dp(2)));
        }
    }
    if let Some(delta) = decimal_delta(from.market_breadth_delta, to.market_breadth_delta) {
        if delta.abs() >= Decimal::new(10, 2) {
            items.push(format!("breadth delta {:+}", delta.round_dp(2)));
        }
    }
    if let Some(delta) = decimal_delta(from.market_average_return, to.market_average_return) {
        if delta.abs() >= Decimal::new(2, 2) {
            items.push(format!("avg return {:+}", delta.round_dp(2)));
        }
    }
    items.truncate(3);
    items
}

#[cfg(feature = "persistence")]
pub(super) fn regime_shift_score(
    from_states: &HashMap<String, Decimal>,
    to_states: &HashMap<String, Decimal>,
    market_regime_changed: bool,
) -> Decimal {
    let regime_keys = [
        "Event Catalyst",
        "Cross-market Dislocation",
        "Substitution Flow",
        "Cross-scope Contagion",
        "Structural Fragility",
    ];
    let mut total = regime_keys.iter().fold(Decimal::ZERO, |acc, key| {
        let delta = to_states.get(*key).copied().unwrap_or(Decimal::ZERO)
            - from_states.get(*key).copied().unwrap_or(Decimal::ZERO);
        if delta > Decimal::ZERO {
            acc + delta
        } else {
            acc
        }
    });
    if market_regime_changed {
        total += Decimal::new(18, 2);
    }
    clamp_unit_interval(total)
}

#[cfg(feature = "persistence")]
pub(super) fn regime_metric_shift_score(
    from: &CaseReasoningAssessmentSnapshot,
    to: &CaseReasoningAssessmentSnapshot,
) -> Decimal {
    let mut total = Decimal::ZERO;
    if let Some(delta) = decimal_delta(from.market_regime_confidence, to.market_regime_confidence) {
        total += delta.abs();
    }
    if let Some(delta) = decimal_delta(from.market_breadth_delta, to.market_breadth_delta) {
        total += delta.abs() / Decimal::from(2);
    }
    if let Some(delta) = decimal_delta(from.market_average_return, to.market_average_return) {
        total += delta.abs() * Decimal::from(4);
    }
    clamp_unit_interval(total)
}

#[cfg(feature = "persistence")]
pub(super) fn factor_decay_score(
    from_factors: &HashMap<String, (String, Decimal)>,
    to_factors: &HashMap<String, (String, Decimal)>,
) -> Decimal {
    clamp_unit_interval(
        from_factors
            .iter()
            .fold(Decimal::ZERO, |acc, (key, (_, value))| {
                let next = to_factors
                    .get(key)
                    .map(|item| item.1)
                    .unwrap_or(Decimal::ZERO);
                if *value > next {
                    acc + (*value - next)
                } else {
                    acc
                }
            }),
    )
}

#[cfg(feature = "persistence")]
pub(super) fn classify_transition(
    from_mechanism: Option<&str>,
    to_mechanism: Option<&str>,
    regime_score: Decimal,
    decay_score: Decimal,
    review_score: Decimal,
) -> String {
    let regime_shift = Decimal::new(22, 2);
    let decay_shift = Decimal::new(20, 2);
    let mild = Decimal::new(12, 2);

    if review_score >= Decimal::new(18, 2) && regime_score < mild && decay_score < mild {
        return "review_override".into();
    }
    if regime_score >= regime_shift && decay_score < Decimal::new(16, 2) {
        return "regime_shift".into();
    }
    if decay_score >= decay_shift && regime_score < Decimal::new(15, 2) {
        return "mechanism_decay".into();
    }
    if regime_score >= mild && decay_score >= mild {
        return "mixed".into();
    }
    if from_mechanism != to_mechanism {
        return "mixed".into();
    }
    "stable".into()
}

#[cfg(feature = "persistence")]
pub(super) fn transition_summary(
    from_mechanism: Option<&str>,
    to_mechanism: Option<&str>,
    classification: &str,
    regime_hint: Option<String>,
    decay_hint: Option<String>,
    review_hint: Option<String>,
) -> String {
    let from = from_mechanism.unwrap_or("Unknown");
    let to = to_mechanism.unwrap_or("Unknown");
    match classification {
        "regime_shift" => format!(
            "{from} -> {to}，主因偏向環境切換：{}。",
            regime_hint.unwrap_or_else(|| "regime-sensitive states strengthened".into())
        ),
        "mechanism_decay" => format!(
            "{from} -> {to}，更像原機制先衰減：{}。",
            decay_hint.unwrap_or_else(|| "old primary factors faded".into())
        ),
        "review_override" => format!(
            "{from} -> {to}，主要由人類校準推動：{}。",
            review_hint.unwrap_or_else(|| "review reasons overrode the prior thesis".into())
        ),
        "mixed" => format!(
            "{from} -> {to}，同時有環境切換與原機制衰減。{}{}",
            regime_hint
                .map(|item| format!("環境面：{item}。"))
                .unwrap_or_default(),
            decay_hint
                .map(|item| format!("結構面：{item}。"))
                .unwrap_or_default()
        ),
        _ => format!("{to} 仍為主機制，近期沒有足夠證據顯示結構性切換。"),
    }
}

#[cfg(feature = "persistence")]
pub(super) fn decimal_delta(from: Option<Decimal>, to: Option<Decimal>) -> Option<Decimal> {
    let from = from?;
    let to = to?;
    Some(to - from)
}

impl CaseReasoningAssessmentSnapshot {
    #[cfg(feature = "persistence")]
    pub(in crate::cases) fn from_record(record: CaseReasoningAssessmentRecord) -> Self {
        Self {
            assessment_id: record.assessment_id,
            setup_id: record.setup_id,
            market: record.market,
            symbol: record.symbol,
            title: record.title,
            sector: record.sector,
            recommended_action: record.recommended_action,
            source: record.source,
            recorded_at: record.recorded_at,
            workflow_state: record.workflow_state,
            market_regime_bias: record.market_regime_bias,
            market_regime_confidence: record.market_regime_confidence,
            market_breadth_delta: record.market_breadth_delta,
            market_average_return: record.market_average_return,
            market_directional_consensus: record.market_directional_consensus,
            owner: record.owner,
            reviewer: record.reviewer,
            actor: record.actor,
            note: record.note,
            primary_mechanism_kind: record.primary_mechanism_kind,
            primary_mechanism_score: record.primary_mechanism_score,
            law_kinds: record.law_kinds,
            predicate_kinds: record.predicate_kinds,
            composite_state_kinds: record.composite_state_kinds,
            competing_mechanism_kinds: record.competing_mechanism_kinds,
            invalidation_rules: record.invalidation_rules,
            reasoning_profile: record.reasoning_profile,
        }
    }
}

#[cfg(feature = "persistence")]
pub(in crate::cases) fn record_invalidation_rules(setup: &TacticalSetupRecord) -> Vec<String> {
    ordered_unique(
        setup
            .risk_notes
            .iter()
            .filter(|note| {
                !note.starts_with("family=")
                    && !note.starts_with("local_support=")
                    && !note.starts_with("policy_gate:")
                    && !note.starts_with("policy_transition:")
                    && !note.starts_with("estimated execution cost=")
                    && !note.starts_with("convergence_score=")
                    && !note.starts_with("effective_confidence=")
                    && !note.starts_with("hypothesis_margin=")
                    && !note.starts_with("external_")
                    && !note.starts_with("lineage_prior=")
            })
            .cloned()
            .collect::<Vec<_>>(),
    )
}
