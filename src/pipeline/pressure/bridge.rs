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
use crate::ontology::reasoning::{DecisionLineage, ReasoningScope, TacticalSetup};
use crate::pipeline::pressure::{PressureChannel, PressureVortex};

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

/// Convert a tension vortex into an optional TacticalSetup.
pub fn vortex_to_tactical_setup(
    vortex: &PressureVortex,
    timestamp: OffsetDateTime,
    tick: u64,
) -> Option<TacticalSetup> {
    // Need at least 1 tense channel and material tension.
    if vortex.tense_channel_count == 0 || vortex.tension < Decimal::new(1, 2) {
        return None;
    }

    // Action based on tension strength and channel count.
    let action = if vortex.tension >= Decimal::new(15, 2) && vortex.tense_channel_count >= 3 {
        "review"
    } else {
        "observe"
    };

    let confidence = (vortex.tension * Decimal::TWO)
        .min(Decimal::ONE)
        .max(Decimal::ZERO);

    // Direction: hour layer is the "background truth".
    let direction_label = if vortex.hour_direction >= Decimal::ZERO {
        "Long"
    } else {
        "Short"
    };

    let symbol_str = &vortex.symbol.0;
    let channel_list: Vec<&str> = vortex.tense_channels.iter().map(channel_name).collect();

    let entry_rationale = format!(
        "Tension: tick says {:.3} but hour says {:.3}. {} channels diverge: {}",
        vortex.tick_direction,
        vortex.hour_direction,
        vortex.tense_channel_count,
        channel_list.join(", ")
    );

    Some(TacticalSetup {
        setup_id: format!("pf:{}:{}", symbol_str, tick),
        hypothesis_id: format!("pfh:{}:{}", symbol_str, tick),
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
        action: action.to_string(),
        time_horizon: "intraday".to_string(),
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
        risk_notes: vec!["family=tension_vortex".to_string()],
        review_reason_code: None,
        policy_verdict: None,
    })
}

/// Batch-convert vortices into tactical setups, capped at `max_setups`.
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
