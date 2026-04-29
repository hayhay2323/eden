//! Vortex → TacticalSetup bridge.
//!
//! Converts tension-based vortices into TacticalSetup objects so the existing
//! action pipeline (workflows, console display, persistence) can surface them.
//!
//! A tension vortex means: tick and hour layers disagree. The setup predicts
//! that the hour direction will eventually win (mean reversion of surface noise).

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
use crate::ontology::horizon::{
    CaseHorizon, HorizonBucket, HorizonExpiry, SecondaryHorizon, SessionPhase, Urgency,
};
use crate::ontology::reasoning::{
    enrich_tactical_setup_with_ontology_projection, DecisionLineage, IntentOpportunityBias,
    IntentOpportunityWindow, ReasoningScope, TacticalDirection, TacticalSetup,
};
use crate::pipeline::pressure::reasoning::{AnomalyPhase, TensionDriver, VortexInsight};
use crate::pipeline::pressure::{PressureChannel, PressureVortex};

/// Select a single primary `CaseHorizon` from an intent's `opportunities`
/// profile. See Wave 2 Task 10 of the Horizon plan for the rule.
pub fn select_case_horizon(opportunities: &[IntentOpportunityWindow]) -> CaseHorizon {
    if opportunities.is_empty() {
        return CaseHorizon::new(
            HorizonBucket::Session,
            Urgency::Normal,
            SessionPhase::Midday,
            HorizonExpiry::UntilSessionClose,
            vec![],
        );
    }

    // Sort: bias rank → confidence desc → urgency desc → bucket policy.
    let mut ranked: Vec<&IntentOpportunityWindow> = opportunities.iter().collect();
    ranked.sort_by(|a, b| {
        bias_rank(a.bias)
            .cmp(&bias_rank(b.bias))
            .then_with(|| b.confidence.cmp(&a.confidence))
            .then_with(|| urgency_rank(a.urgency).cmp(&urgency_rank(b.urgency)))
            .then_with(|| bucket_tiebreak(a.bias, a.bucket).cmp(&bucket_tiebreak(b.bias, b.bucket)))
    });

    let primary_window = ranked[0];
    let primary_bucket = primary_window.bucket;

    let secondary: Vec<SecondaryHorizon> = ranked[1..]
        .iter()
        .map(|w| SecondaryHorizon {
            bucket: w.bucket,
            confidence: w.confidence,
        })
        .collect();

    let expiry = match primary_bucket {
        HorizonBucket::Tick50 | HorizonBucket::Fast5m | HorizonBucket::Mid30m => {
            HorizonExpiry::UntilNextBucket
        }
        HorizonBucket::Session => HorizonExpiry::UntilSessionClose,
        HorizonBucket::MultiSession => HorizonExpiry::None,
    };

    CaseHorizon::new(
        primary_bucket,
        primary_window.urgency,
        SessionPhase::Midday,
        expiry,
        secondary,
    )
}

fn bias_rank(bias: IntentOpportunityBias) -> u8 {
    match bias {
        IntentOpportunityBias::Enter | IntentOpportunityBias::Exit => 0,
        IntentOpportunityBias::Hold => 1,
        IntentOpportunityBias::Watch => 2,
    }
}

fn urgency_rank(u: Urgency) -> u8 {
    match u {
        Urgency::Immediate => 0,
        Urgency::Normal => 1,
        Urgency::Relaxed => 2,
    }
}

fn bucket_tiebreak(bias: IntentOpportunityBias, bucket: HorizonBucket) -> u8 {
    let short_first = matches!(
        bias,
        IntentOpportunityBias::Enter | IntentOpportunityBias::Exit
    );
    let order = match bucket {
        HorizonBucket::Tick50 => 0u8,
        HorizonBucket::Fast5m => 1,
        HorizonBucket::Mid30m => 2,
        HorizonBucket::Session => 3,
        HorizonBucket::MultiSession => 4,
    };
    if short_first {
        order
    } else {
        4 - order
    }
}

fn channel_name(ch: &PressureChannel) -> &'static str {
    match ch {
        PressureChannel::OrderBook => "order_book",
        PressureChannel::CapitalFlow => "capital_flow",
        PressureChannel::Institutional => "institutional",
        PressureChannel::Momentum => "momentum",
        PressureChannel::Volume => "volume",
        PressureChannel::Structure => "structure",
    }
}

fn pressure_direction_label(hour_direction: Decimal) -> &'static str {
    if hour_direction >= Decimal::ZERO {
        "Long"
    } else {
        "Short"
    }
}

fn pressure_direction_slug(hour_direction: Decimal) -> &'static str {
    if hour_direction >= Decimal::ZERO {
        "long"
    } else {
        "short"
    }
}

fn tactical_direction_from_hour(hour_direction: Decimal) -> Option<TacticalDirection> {
    if hour_direction > Decimal::ZERO {
        Some(TacticalDirection::Long)
    } else if hour_direction < Decimal::ZERO {
        Some(TacticalDirection::Short)
    } else {
        None
    }
}

fn horizon_bucket_slug(bucket: HorizonBucket) -> &'static str {
    match bucket {
        HorizonBucket::Tick50 => "tick50",
        HorizonBucket::Fast5m => "fast5m",
        HorizonBucket::Mid30m => "mid30m",
        HorizonBucket::Session => "session",
        HorizonBucket::MultiSession => "multi_session",
    }
}

fn pressure_setup_id(symbol: &str, hour_direction: Decimal, horizon: &CaseHorizon) -> String {
    format!(
        "pf:{symbol}:{}:{}",
        pressure_direction_slug(hour_direction),
        horizon_bucket_slug(horizon.primary)
    )
}

fn pressure_hypothesis_id(symbol: &str, hour_direction: Decimal, family: &str) -> String {
    format!(
        "pfh:{symbol}:{}:{family}",
        pressure_direction_slug(hour_direction)
    )
}

/// Convert a tension vortex into an optional TacticalSetup.
pub fn vortex_to_tactical_setup(
    vortex: &PressureVortex,
    timestamp: OffsetDateTime,
    _tick: u64,
) -> Option<TacticalSetup> {
    // Skip when no channel is tense (structural — no signal to carry).
    // Drop the previous `tension < 0.01` algorithmic noise floor too:
    // if a vortex has any tense channel at all, emit the setup at
    // base confidence and let downstream percentile promotion + BP
    // posterior decide actionability. Keeping a hardcoded numeric
    // floor here was a business threshold; structural emptiness is not.
    if vortex.tense_channel_count == 0 {
        return None;
    }

    // V2: always emit Observe. Action upgrade (Observe → Review →
    // Enter) lives in `apply_percentile_action_promotion`, which
    // ranks setups by their post-BP confidence within the current
    // tick — purely data-driven, no hardcoded tension threshold.
    let action = "observe";

    // First-principles base confidence: vortex.tension is already in
    // [0, 1] from compute_tension. Drop the previous "× 2 then clamp"
    // magic multiplier which made confidence saturate to 1.0 for any
    // tension >= 0.5 (= the well-known base=1.0 saturation that
    // collapsed setup ranking to noise).
    let confidence = vortex.tension.clamp(Decimal::ZERO, Decimal::ONE);

    // Direction: hour layer is the "background truth".
    let direction_label = pressure_direction_label(vortex.hour_direction);

    let symbol_str = &vortex.symbol.0;
    let channel_list: Vec<&str> = vortex.tense_channels.iter().map(channel_name).collect();
    let horizon = crate::ontology::reasoning::default_case_horizon();

    let entry_rationale = format!(
        "Tension: tick says {:.3} but hour says {:.3}. {} channels diverge: {}",
        vortex.tick_direction,
        vortex.hour_direction,
        vortex.tense_channel_count,
        channel_list.join(", ")
    );

    let mut setup = TacticalSetup {
        setup_id: pressure_setup_id(symbol_str, vortex.hour_direction, &horizon),
        hypothesis_id: pressure_hypothesis_id(symbol_str, vortex.hour_direction, "tension_vortex"),
        runner_up_hypothesis_id: None,
        provenance: ProvenanceMetadata::new(ProvenanceSource::Computed, timestamp),
        lineage: DecisionLineage {
            based_on: channel_list.iter().map(|c| c.to_string()).collect(),
            blocked_by: Vec::new(),
            promoted_by: Vec::new(),
            falsified_by: Vec::new(),
        },
        scope: ReasoningScope::Symbol(vortex.symbol.clone()),
        title: format!("{} {} (tension vortex)", direction_label, symbol_str),
        action: action.to_string().into(),
        direction: tactical_direction_from_hour(vortex.hour_direction),
        horizon,
        confidence,
        confidence_gap: Decimal::ZERO,
        heuristic_edge: vortex.tension,
        convergence_score: Some(vortex.cross_channel_conflict),
        convergence_detail: None,
        workflow_id: None,
        entry_rationale,
        causal_narrative: Some(format!(
            "Temporal tension: hour layer ({:.3}) vs tick layer ({:.3}). Divergence={:.3}. {} channels under stress.",
            vortex.hour_direction,
            vortex.tick_direction,
            vortex.temporal_divergence,
            vortex.tense_channel_count,
        )),
        risk_notes: Vec::new(),
        review_reason_code: None,
        policy_verdict: None,
    };
    enrich_tactical_setup_with_ontology_projection(&mut setup, None);
    Some(setup)
}

/// Convert a vortex WITH reasoning insight into a TacticalSetup.
/// Uses lifecycle phase to decide actionability; tension thresholds
/// removed in V2 (action upgrade comes from post-BP percentile rank,
/// not hardcoded numeric thresholds).
pub fn insight_to_tactical_setup(
    insight: &VortexInsight,
    vortex: &PressureVortex,
    timestamp: OffsetDateTime,
    _tick: u64,
) -> Option<TacticalSetup> {
    // Structural filter: no tense channel = no signal.
    if vortex.tense_channel_count == 0 {
        return None;
    }

    // Lifecycle phase is the one structural filter that stays —
    // Fading anomalies don't deserve a setup at all (their signal is
    // collapsing). Everything else emits Observe and lets the
    // percentile promoter decide.
    if matches!(insight.lifecycle.phase, AnomalyPhase::Fading) {
        return None;
    }
    let action = "observe";

    // First-principles base confidence: vortex.tension only. Magic
    // boost rules deleted (+0.1 for isolated, +peer_ratio×0.1, +
    // competition×0.1, +0.05 for growing) — they were a-priori weights
    // I never validated empirically. If those signals predict outcome,
    // outcome_history modulation will surface the relationship via
    // hit-rate; a-priori upmod just collapses the distribution.
    let confidence = vortex.tension.clamp(Decimal::ZERO, Decimal::ONE);
    let _ = insight; // boost-channel inputs no longer consumed

    let direction_label = pressure_direction_label(vortex.hour_direction);

    let symbol_str = &vortex.symbol.0;
    let channel_list: Vec<&str> = vortex.tense_channels.iter().map(channel_name).collect();
    let driver_family = insight.evidence.driver_class.as_str();
    let horizon = select_case_horizon(&derive_opportunities_from_insight(insight));

    // Rich narrative from reasoning insight.
    let entry_rationale = format!(
        "{} | {} | {} | vel={:.3} acc={:.3} | channels: {}",
        driver_family,
        if insight.absence.is_isolated {
            "isolated"
        } else {
            "sector-linked"
        },
        match insight.lifecycle.phase {
            AnomalyPhase::Growing => "GROWING",
            AnomalyPhase::Peaking => "PEAKING",
            AnomalyPhase::Fading => "FADING",
            AnomalyPhase::New => "NEW",
        },
        insight.lifecycle.velocity,
        insight.lifecycle.acceleration,
        channel_list.join(", "),
    );

    let mut setup = TacticalSetup {
        setup_id: pressure_setup_id(symbol_str, vortex.hour_direction, &horizon),
        hypothesis_id: pressure_hypothesis_id(symbol_str, vortex.hour_direction, driver_family),
        runner_up_hypothesis_id: None,
        provenance: ProvenanceMetadata::new(ProvenanceSource::Computed, timestamp),
        lineage: DecisionLineage {
            based_on: channel_list.iter().map(|c| c.to_string()).collect(),
            blocked_by: Vec::new(),
            promoted_by: Vec::new(),
            falsified_by: Vec::new(),
        },
        scope: ReasoningScope::Symbol(vortex.symbol.clone()),
        title: format!("{} {} ({} vortex)", direction_label, symbol_str, action),
        action: action.to_string().into(),
        direction: tactical_direction_from_hour(vortex.hour_direction),
        horizon,
        confidence,
        confidence_gap: Decimal::ZERO,
        heuristic_edge: vortex.tension,
        convergence_score: Some(vortex.cross_channel_conflict),
        convergence_detail: None,
        workflow_id: None,
        entry_rationale,
        causal_narrative: Some(insight.summary.clone()),
        risk_notes: vec![
            format!("phase={:?}", insight.lifecycle.phase),
            format!("driver={}", insight.attribution.driver_label()),
            format!("driver_class={}", insight.evidence.driver_class.as_str()),
            format!(
                "peer_confirmation_ratio={}",
                insight.evidence.peer_confirmation_ratio.round_dp(4)
            ),
            format!("peer_active_count={}", insight.evidence.peer_active_count),
            format!("peer_silent_count={}", insight.evidence.peer_silent_count),
            format!(
                "isolation_score={}",
                insight.evidence.isolation_score.round_dp(4)
            ),
            format!("is_isolated={}", insight.absence.is_isolated),
            format!(
                "competition_margin={}",
                insight.evidence.competition_margin.round_dp(4)
            ),
            format!(
                "driver_confidence={}",
                insight.evidence.driver_confidence.round_dp(4)
            ),
            format!("absence_summary={}", insight.absence.narrative),
            format!("competition_summary={}", insight.competition.narrative),
            format!("competition_winner={}", insight.competition.winner.label),
            format!(
                "competition_runner_up={}",
                insight
                    .competition
                    .runner_up
                    .as_ref()
                    .map(|item| item.label.clone())
                    .unwrap_or_default()
            ),
            format!("velocity={}", insight.evidence.velocity.round_dp(4)),
            format!("acceleration={}", insight.evidence.acceleration.round_dp(4)),
        ],
        review_reason_code: None,
        policy_verdict: None,
    };
    enrich_tactical_setup_with_ontology_projection(&mut setup, None);
    Some(setup)
}

/// Batch-convert vortices with insights into tactical setups.
pub fn insights_to_tactical_setups(
    insights: &[(VortexInsight, PressureVortex)],
    timestamp: OffsetDateTime,
    tick: u64,
    max_setups: usize,
) -> Vec<TacticalSetup> {
    insights
        .iter()
        .filter_map(|(insight, vortex)| insight_to_tactical_setup(insight, vortex, timestamp, tick))
        .take(max_setups)
        .collect()
}

/// Derive a profile of `IntentOpportunityWindow`s from a vortex insight.
///
/// This is the bridge between "what the pressure field sees" (attribution,
/// absence, competition, lifecycle) and "what trading windows are viable"
/// (a profile for `select_case_horizon` to rank).
///
/// Rules are locked — no heuristic inference. See Wave 3 Task 13a of the plan.
pub fn derive_opportunities_from_insight(insight: &VortexInsight) -> Vec<IntentOpportunityWindow> {
    use rust_decimal_macros::dec;

    // 1. Bias from lifecycle phase
    let bias = match insight.lifecycle.phase {
        AnomalyPhase::Growing => IntentOpportunityBias::Enter,
        AnomalyPhase::Peaking => IntentOpportunityBias::Hold,
        AnomalyPhase::Fading => IntentOpportunityBias::Exit,
        AnomalyPhase::New => IntentOpportunityBias::Watch,
    };

    // 2. Primary + secondary buckets from driver
    let (primary_bucket, secondary_buckets): (HorizonBucket, Vec<HorizonBucket>) = match &insight
        .attribution
        .driver
    {
        TensionDriver::MicrostructureDriven => (HorizonBucket::Fast5m, vec![HorizonBucket::Mid30m]),
        TensionDriver::TradeFlowDriven => (
            HorizonBucket::Fast5m,
            vec![HorizonBucket::Mid30m, HorizonBucket::Session],
        ),
        TensionDriver::CapitalFlowDriven => (HorizonBucket::Mid30m, vec![HorizonBucket::Session]),
        TensionDriver::InstitutionalDriven => (
            HorizonBucket::Session,
            vec![HorizonBucket::Mid30m, HorizonBucket::MultiSession],
        ),
        TensionDriver::BroadStructural => (HorizonBucket::Session, vec![HorizonBucket::Mid30m]),
        TensionDriver::SingleChannel { .. } => (HorizonBucket::Fast5m, vec![]),
    };

    // 3. Confidence + alignment from competition
    let winner_conf = insight
        .competition
        .winner
        .confidence
        .max(Decimal::ZERO)
        .min(Decimal::ONE);
    let runner_up_conf = insight
        .competition
        .runner_up
        .as_ref()
        .map(|r| r.confidence.max(Decimal::ZERO).min(Decimal::ONE));

    let scaled_confidence = match runner_up_conf {
        Some(r) => (winner_conf - r * dec!(0.3))
            .max(Decimal::ZERO)
            .min(Decimal::ONE),
        None => winner_conf,
    };
    let alignment = match runner_up_conf {
        Some(r) if winner_conf > Decimal::ZERO => (Decimal::ONE - r / winner_conf)
            .max(Decimal::ZERO)
            .min(Decimal::ONE),
        _ => Decimal::ONE,
    };

    // 4. Urgency for primary (secondaries default to Normal)
    let conflict_ratio = runner_up_conf
        .map(|r| {
            if winner_conf > Decimal::ZERO {
                r / winner_conf
            } else {
                Decimal::ZERO
            }
        })
        .unwrap_or(Decimal::ZERO);
    let primary_urgency = compute_primary_urgency(
        insight.lifecycle.phase,
        primary_bucket,
        insight.absence.is_isolated,
        conflict_ratio,
    );

    // 5. Rationale string
    let driver_label = format!("{:?}", insight.attribution.driver);
    let absence_label = if insight.absence.is_isolated {
        "isolated"
    } else {
        "sector-linked"
    };
    let phase_label = format!("{:?}", insight.lifecycle.phase);
    let rationale = format!("{phase_label}/{driver_label}/{absence_label}");

    // 6. Build the profile
    let mut windows = Vec::with_capacity(1 + secondary_buckets.len());
    windows.push(IntentOpportunityWindow::new(
        primary_bucket,
        primary_urgency,
        bias,
        scaled_confidence,
        alignment,
        rationale.clone(),
    ));
    for bucket in secondary_buckets {
        windows.push(IntentOpportunityWindow::new(
            bucket,
            Urgency::Normal,
            bias,
            scaled_confidence,
            alignment,
            rationale.clone(),
        ));
    }
    windows
}

fn compute_primary_urgency(
    phase: AnomalyPhase,
    bucket: HorizonBucket,
    isolated: bool,
    conflict_ratio: Decimal,
) -> Urgency {
    use rust_decimal_macros::dec;
    match phase {
        AnomalyPhase::Fading => Urgency::Immediate,
        AnomalyPhase::Peaking => Urgency::Normal,
        AnomalyPhase::New => Urgency::Relaxed,
        AnomalyPhase::Growing => {
            if conflict_ratio > dec!(0.6) && isolated {
                Urgency::Immediate
            } else if matches!(bucket, HorizonBucket::Fast5m) {
                Urgency::Immediate
            } else {
                Urgency::Normal
            }
        }
    }
}

/// Batch-convert vortices into tactical setups, capped at `max_setups`.
/// Fallback for when no reasoning insights are available.
pub fn vortices_to_tactical_setups(
    vortices: &[PressureVortex],
    timestamp: OffsetDateTime,
    tick: u64,
    max_setups: usize,
) -> Vec<TacticalSetup> {
    vortices
        .iter()
        .filter_map(|v| vortex_to_tactical_setup(v, timestamp, tick))
        .take(max_setups)
        .collect()
}

#[cfg(test)]
mod horizon_tests {
    use super::*;
    use crate::ontology::horizon::{HorizonBucket, Urgency};
    use crate::ontology::reasoning::{IntentOpportunityBias, IntentOpportunityWindow};
    use crate::pipeline::pressure::PressureVortex;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn window(
        bucket: HorizonBucket,
        bias: IntentOpportunityBias,
        conf: Decimal,
    ) -> IntentOpportunityWindow {
        IntentOpportunityWindow::new(
            bucket,
            Urgency::Normal,
            bias,
            conf,
            dec!(0.5),
            "test".into(),
        )
    }

    #[test]
    fn selection_bias_rank_beats_confidence() {
        let opps = vec![
            window(
                HorizonBucket::Fast5m,
                IntentOpportunityBias::Enter,
                dec!(0.6),
            ),
            window(
                HorizonBucket::Mid30m,
                IntentOpportunityBias::Enter,
                dec!(0.7),
            ),
            window(
                HorizonBucket::Session,
                IntentOpportunityBias::Watch,
                dec!(0.9),
            ),
        ];
        let ch = select_case_horizon(&opps);
        // Watch demoted; between two Enters, Mid30m has higher confidence
        assert_eq!(ch.primary, HorizonBucket::Mid30m);
    }

    #[test]
    fn selection_all_watch_still_picks_one() {
        let opps = vec![
            window(
                HorizonBucket::Fast5m,
                IntentOpportunityBias::Watch,
                dec!(0.3),
            ),
            window(
                HorizonBucket::Session,
                IntentOpportunityBias::Watch,
                dec!(0.6),
            ),
        ];
        let ch = select_case_horizon(&opps);
        assert_eq!(ch.primary, HorizonBucket::Session);
    }

    #[test]
    fn selection_enter_prefers_short_bucket_on_tie() {
        let opps = vec![
            window(
                HorizonBucket::Fast5m,
                IntentOpportunityBias::Enter,
                dec!(0.8),
            ),
            window(
                HorizonBucket::Mid30m,
                IntentOpportunityBias::Enter,
                dec!(0.8),
            ),
        ];
        let ch = select_case_horizon(&opps);
        assert_eq!(ch.primary, HorizonBucket::Fast5m);
    }

    #[test]
    fn selection_hold_prefers_long_bucket_on_tie() {
        let opps = vec![
            window(
                HorizonBucket::Fast5m,
                IntentOpportunityBias::Hold,
                dec!(0.7),
            ),
            window(
                HorizonBucket::Mid30m,
                IntentOpportunityBias::Hold,
                dec!(0.7),
            ),
        ];
        let ch = select_case_horizon(&opps);
        assert_eq!(ch.primary, HorizonBucket::Mid30m);
    }

    #[test]
    fn selection_primary_not_in_secondary() {
        let opps = vec![
            window(
                HorizonBucket::Fast5m,
                IntentOpportunityBias::Enter,
                dec!(0.8),
            ),
            window(
                HorizonBucket::Mid30m,
                IntentOpportunityBias::Hold,
                dec!(0.7),
            ),
            window(
                HorizonBucket::Session,
                IntentOpportunityBias::Watch,
                dec!(0.4),
            ),
        ];
        let ch = select_case_horizon(&opps);
        assert!(!ch.secondary.iter().any(|s| s.bucket == ch.primary));
    }

    #[test]
    fn selection_empty_returns_session_default() {
        let ch = select_case_horizon(&[]);
        assert_eq!(ch.primary, HorizonBucket::Session);
        assert_eq!(ch.urgency, Urgency::Normal);
    }

    // ---- derive_opportunities_from_insight tests ----

    use crate::ontology::objects::Symbol;
    use crate::pipeline::pressure::reasoning::{
        AnomalyLifecycle, AnomalyPhase, CompetingExplanation, CompetitionResult,
        PropagationAbsence, StructuralDriverClass, StructuralEvidence, TensionAttribution,
        TensionDriver, VortexInsight,
    };

    fn make_insight(
        phase: AnomalyPhase,
        driver: TensionDriver,
        isolated: bool,
        winner_conf: Decimal,
    ) -> VortexInsight {
        let sym = Symbol("TEST.US".into());
        let driver_for_evidence = driver.clone();
        VortexInsight {
            symbol: sym.clone(),
            attribution: TensionAttribution {
                symbol: sym.clone(),
                driver,
                contributing_channels: vec![],
                silent_channels: vec![],
                narrative: "test".into(),
            },
            absence: PropagationAbsence {
                source_symbol: sym.clone(),
                source_tension: dec!(0.5),
                silent_neighbors: vec![],
                active_neighbors: vec![],
                is_isolated: isolated,
                narrative: "test".into(),
            },
            competition: CompetitionResult {
                symbol: sym.clone(),
                winner: CompetingExplanation {
                    label: "test".into(),
                    confidence: winner_conf,
                    basis: "test".into(),
                },
                runner_up: None,
                narrative: "test".into(),
            },
            lifecycle: AnomalyLifecycle {
                symbol: sym,
                phase,
                tension: dec!(0.5),
                velocity: dec!(0.1),
                acceleration: dec!(0.0),
                ticks_alive: 5,
                peak_tension: dec!(0.5),
                narrative: "test".into(),
            },
            evidence: StructuralEvidence {
                driver_class: match driver_for_evidence {
                    TensionDriver::TradeFlowDriven => StructuralDriverClass::TradeFlow,
                    TensionDriver::CapitalFlowDriven => StructuralDriverClass::CapitalFlow,
                    TensionDriver::MicrostructureDriven => StructuralDriverClass::Microstructure,
                    TensionDriver::InstitutionalDriven => StructuralDriverClass::Institutional,
                    TensionDriver::BroadStructural => {
                        if isolated {
                            StructuralDriverClass::CompanySpecific
                        } else {
                            StructuralDriverClass::MixedStructural
                        }
                    }
                    TensionDriver::SingleChannel { .. } => StructuralDriverClass::SingleChannel,
                },
                driver_confidence: winner_conf,
                peer_active_count: 0,
                peer_silent_count: usize::from(isolated),
                peer_confirmation_ratio: Decimal::ZERO,
                isolation_score: if isolated {
                    Decimal::ONE
                } else {
                    Decimal::ZERO
                },
                competition_margin: winner_conf,
                tension: dec!(0.5),
                cross_channel_conflict: dec!(0.4),
                lifecycle_phase: phase,
                velocity: dec!(0.1),
                acceleration: dec!(0.0),
            },
            summary: "test".into(),
        }
    }

    fn make_vortex(symbol: &str, tension: Decimal, tense_channel_count: usize) -> PressureVortex {
        PressureVortex {
            symbol: Symbol(symbol.into()),
            tension,
            cross_channel_conflict: dec!(0.4),
            temporal_divergence: dec!(0.3),
            hour_direction: dec!(0.2),
            tick_direction: dec!(-0.1),
            tense_channels: PressureChannel::ALL
                .iter()
                .copied()
                .take(tense_channel_count)
                .collect(),
            tense_channel_count,
            edge_violation_source: None,
        }
    }

    #[test]
    fn derive_microstructure_growing_yields_fast5m_enter() {
        let insight = make_insight(
            AnomalyPhase::Growing,
            TensionDriver::MicrostructureDriven,
            true,
            dec!(0.8),
        );
        let opps = derive_opportunities_from_insight(&insight);
        assert_eq!(opps.len(), 2);
        assert_eq!(opps[0].bucket, HorizonBucket::Fast5m);
        assert_eq!(opps[0].bias, IntentOpportunityBias::Enter);
        assert_eq!(opps[0].urgency, Urgency::Immediate); // fast5m + growing
        assert_eq!(opps[1].bucket, HorizonBucket::Mid30m);
        assert_eq!(opps[1].urgency, Urgency::Normal);
    }

    #[test]
    fn derive_institutional_peaking_yields_session_hold() {
        let insight = make_insight(
            AnomalyPhase::Peaking,
            TensionDriver::InstitutionalDriven,
            false,
            dec!(0.7),
        );
        let opps = derive_opportunities_from_insight(&insight);
        assert_eq!(opps.len(), 3);
        assert_eq!(opps[0].bucket, HorizonBucket::Session);
        assert_eq!(opps[0].bias, IntentOpportunityBias::Hold);
        assert_eq!(opps[0].urgency, Urgency::Normal);
    }

    #[test]
    fn derive_fading_yields_exit_immediate() {
        let insight = make_insight(
            AnomalyPhase::Fading,
            TensionDriver::CapitalFlowDriven,
            false,
            dec!(0.6),
        );
        let opps = derive_opportunities_from_insight(&insight);
        assert_eq!(opps[0].bias, IntentOpportunityBias::Exit);
        assert_eq!(opps[0].urgency, Urgency::Immediate);
    }

    #[test]
    fn derive_new_phase_yields_watch_relaxed() {
        let insight = make_insight(
            AnomalyPhase::New,
            TensionDriver::BroadStructural,
            false,
            dec!(0.3),
        );
        let opps = derive_opportunities_from_insight(&insight);
        assert_eq!(opps[0].bias, IntentOpportunityBias::Watch);
        assert_eq!(opps[0].urgency, Urgency::Relaxed);
    }

    #[test]
    fn derive_then_select_produces_real_horizon() {
        // End-to-end: derive opportunities and pipe through select_case_horizon
        let insight = make_insight(
            AnomalyPhase::Growing,
            TensionDriver::MicrostructureDriven,
            true,
            dec!(0.85),
        );
        let horizon = select_case_horizon(&derive_opportunities_from_insight(&insight));
        assert_eq!(horizon.primary, HorizonBucket::Fast5m);
        assert_eq!(horizon.urgency, Urgency::Immediate);
        // Not the default fallback
        assert_ne!(horizon.primary, HorizonBucket::Session);
    }

    #[test]
    fn growing_phase_emits_observe_for_all_action_assignment_is_post_bp() {
        // V2: bridge always emits Observe. The tension-threshold action
        // bands (review at 0.12+2ch, enter at 0.20+4ch) were hardcoded
        // business thresholds; replaced by data-driven percentile
        // promotion in `pipeline::action_promotion` after BP posterior
        // sets the final confidence.
        let insight = make_insight(
            AnomalyPhase::Growing,
            TensionDriver::BroadStructural,
            false,
            dec!(0.7),
        );

        for (sym, tension, channels) in [
            ("OBS.US", dec!(0.09), 1),
            ("REV.US", dec!(0.12), 2),
            ("ENT.US", dec!(0.20), 4),
        ] {
            let setup = insight_to_tactical_setup(
                &insight,
                &make_vortex(sym, tension, channels),
                OffsetDateTime::now_utc(),
                10,
            )
            .expect("setup emitted");
            assert_eq!(
                setup.action, "observe",
                "{sym} must emit Observe pre-promotion (action upgrade is post-BP)",
            );
        }
    }

    #[test]
    fn pressure_case_ids_are_stable_across_ticks_with_same_bucket() {
        let insight = make_insight(
            AnomalyPhase::Growing,
            TensionDriver::MicrostructureDriven,
            true,
            dec!(0.8),
        );
        let vortex = make_vortex("BKNG.US", dec!(0.22), 4);

        let first = insight_to_tactical_setup(&insight, &vortex, OffsetDateTime::now_utc(), 100)
            .expect("first setup");
        let second = insight_to_tactical_setup(&insight, &vortex, OffsetDateTime::now_utc(), 101)
            .expect("second setup");

        assert_eq!(first.setup_id, second.setup_id);
        assert_eq!(first.hypothesis_id, second.hypothesis_id);
        assert_eq!(first.setup_id, "pf:BKNG.US:long:fast5m");
        assert_eq!(first.hypothesis_id, "pfh:BKNG.US:long:microstructure");
    }

    #[test]
    fn pressure_case_ids_change_when_primary_bucket_changes() {
        let vortex = make_vortex("XOM.US", dec!(0.22), 4);
        let micro = make_insight(
            AnomalyPhase::Growing,
            TensionDriver::MicrostructureDriven,
            false,
            dec!(0.8),
        );
        let institutional = make_insight(
            AnomalyPhase::Growing,
            TensionDriver::InstitutionalDriven,
            false,
            dec!(0.8),
        );

        let fast_case = insight_to_tactical_setup(&micro, &vortex, OffsetDateTime::now_utc(), 200)
            .expect("fast case");
        let session_case =
            insight_to_tactical_setup(&institutional, &vortex, OffsetDateTime::now_utc(), 201)
                .expect("session case");

        // The two cases differ on driver → that drives a different
        // primary bucket through select_case_horizon → which yields a
        // different stable setup_id. Microstructure resolves to fast5m;
        // Institutional resolves through the selector to mid30m (its
        // alignment/bias scoring beats the raw "session" primary that
        // derive_opportunities_from_insight emits as a starting point).
        assert_ne!(fast_case.setup_id, session_case.setup_id);
        assert_eq!(fast_case.setup_id, "pf:XOM.US:long:fast5m");
        assert_eq!(session_case.setup_id, "pf:XOM.US:long:mid30m");
    }

    /// BKNG regression: a vortex matching the BKNG volume-14x pattern
    /// must produce a Fast5m primary horizon and a Fast5m archetype key.
    ///
    /// This is the lock against ever silently falling back to Session
    /// for short-burst, isolated, no-conflict opportunities. If this
    /// test breaks, the exit-timing failure that motivated the whole
    /// Horizon System has come back.
    #[test]
    fn bkng_flow_through_horizon_system() {
        use crate::persistence::discovered_archetype::build_archetype_key;

        // Step 1: Intent inference produces opportunities profile mimicking
        // BKNG (volume 14x, isolated, no conflict, sustained momentum).
        let opportunities = vec![
            IntentOpportunityWindow::new(
                HorizonBucket::Fast5m,
                Urgency::Immediate,
                IntentOpportunityBias::Enter,
                dec!(0.85),
                dec!(0.9),
                "volume 14x, isolated, no conflict".into(),
            ),
            IntentOpportunityWindow::new(
                HorizonBucket::Mid30m,
                Urgency::Normal,
                IntentOpportunityBias::Hold,
                dec!(0.70),
                dec!(0.8),
                "sustained momentum".into(),
            ),
            IntentOpportunityWindow::new(
                HorizonBucket::Session,
                Urgency::Relaxed,
                IntentOpportunityBias::Watch,
                dec!(0.45),
                dec!(0.6),
                "session-level confirmation pending".into(),
            ),
        ];

        // Step 2: Case Builder picks the primary.
        let case_horizon = select_case_horizon(&opportunities);

        // Step 3: Lock in the assertions.
        // a) Primary is Fast5m, NOT Session — Enter rank beats Watch's higher confidence.
        assert_eq!(
            case_horizon.primary,
            HorizonBucket::Fast5m,
            "BKNG must produce Fast5m primary, not the Session fallback"
        );

        // b) Urgency is Immediate (Enter + Fast5m + Forming-equivalent).
        assert_eq!(case_horizon.urgency, Urgency::Immediate);

        // c) Expiry is UntilNextBucket because primary is Fast5m.
        assert_eq!(case_horizon.expiry, HorizonExpiry::UntilNextBucket);

        // d) Secondary carries the other two buckets, NOT the primary.
        assert_eq!(case_horizon.secondary.len(), 2);
        assert!(
            !case_horizon
                .secondary
                .iter()
                .any(|s| s.bucket == HorizonBucket::Fast5m),
            "primary bucket must never appear in secondary"
        );

        // e) Main learning key (Intent × Bucket) uses Fast5m.
        let main_key = build_archetype_key(
            "DirectionalAccumulation",
            case_horizon.primary,
            "high_volume_isolated_no_conflict",
        );
        assert!(
            main_key.contains(":fast5m:"),
            "main learning key must use fast5m bucket, got: {main_key}"
        );
        assert!(
            !main_key.contains(":session:"),
            "main learning key must NOT fall back to session, got: {main_key}"
        );
    }
}
