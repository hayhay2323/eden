use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::math::clamp_unit_interval;
use crate::ontology::{
    AtomicPredicate, AtomicPredicateKind, HumanReviewContext, HumanReviewReason,
    HumanReviewReasonKind, HumanReviewVerdict,
};

use super::{mean, predicate};

pub(crate) fn derive_human_review_context(
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
            HumanReviewVerdict::Modified => dec!(0.50),
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

pub(super) fn human_rejected(review: &HumanReviewContext) -> Option<AtomicPredicate> {
    let verdict_score = match review.verdict {
        HumanReviewVerdict::Rejected => dec!(0.55),
        HumanReviewVerdict::Modified => dec!(0.35),
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
