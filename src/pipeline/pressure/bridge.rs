//! Vortex → TacticalSetup bridge.
//!
//! Converts pressure-field vortices into TacticalSetup objects so the existing
//! action pipeline (workflows, console display, persistence) can surface them.

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

/// Convert a single vortex into an optional TacticalSetup.
///
/// Returns `None` if the vortex is too weak or too narrow to act on.
pub fn vortex_to_tactical_setup(
    vortex: &PressureVortex,
    timestamp: OffsetDateTime,
    tick: u64,
) -> Option<TacticalSetup> {
    // Minimum gate: need at least 3 channels and strength above noise floor.
    if vortex.channel_count < 3 || vortex.strength < Decimal::new(5, 3) {
        return None;
    }

    // Action mapping based on coherence, channel count, and strength.
    let action = if vortex.coherence >= Decimal::new(8, 1)
        && vortex.channel_count >= 4
        && vortex.strength >= Decimal::new(10, 2)
    {
        "enter"
    } else if vortex.coherence >= Decimal::new(75, 2)
        && vortex.channel_count >= 3
        && vortex.strength >= Decimal::new(3, 2)
    {
        "review"
    } else {
        "observe"
    };

    let confidence = (vortex.coherence * vortex.strength * Decimal::TWO)
        .min(Decimal::ONE)
        .max(Decimal::ZERO);

    let direction_label = if vortex.direction >= Decimal::ZERO {
        "Long"
    } else {
        "Short"
    };

    let symbol_str = &vortex.symbol.0;

    let channel_list: Vec<&str> = vortex.active_channels.iter().map(channel_name).collect();
    let entry_rationale = format!(
        "{} channels converge on {} {}: {}",
        vortex.channel_count,
        direction_label.to_lowercase(),
        symbol_str,
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
        title: format!("{} {} (pressure vortex)", direction_label, symbol_str),
        action: action.to_string(),
        time_horizon: "intraday".to_string(),
        confidence,
        confidence_gap: Decimal::ZERO,
        heuristic_edge: vortex.strength,
        convergence_score: Some(vortex.coherence),
        convergence_detail: None,
        workflow_id: None,
        entry_rationale,
        causal_narrative: Some(format!(
            "Pressure vortex: {} independent channels align {} on {}",
            vortex.channel_count,
            direction_label.to_lowercase(),
            symbol_str
        )),
        risk_notes: vec!["family=pressure_vortex".to_string()],
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

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    use crate::ontology::objects::Symbol;
    use crate::pipeline::pressure::PressureChannel;

    fn make_vortex(
        symbol: &str,
        strength: Decimal,
        coherence: Decimal,
        direction: Decimal,
        channels: &[PressureChannel],
    ) -> PressureVortex {
        PressureVortex {
            symbol: Symbol(symbol.into()),
            strength,
            coherence,
            direction,
            active_channels: channels.to_vec(),
            channel_count: channels.len(),
        }
    }

    #[test]
    fn strong_vortex_produces_enter_setup() {
        let vortex = make_vortex(
            "AAPL",
            dec!(0.45),
            dec!(1.0),
            dec!(0.3),
            &[
                PressureChannel::OrderBook,
                PressureChannel::CapitalFlow,
                PressureChannel::Institutional,
                PressureChannel::Momentum,
            ],
        );
        let now = OffsetDateTime::now_utc();
        let setup = vortex_to_tactical_setup(&vortex, now, 42).expect("should produce a setup");

        assert_eq!(setup.action, "enter");
        assert!(
            setup.title.starts_with("Long ") || setup.title.starts_with("Short "),
            "title should start with direction, got: {}",
            setup.title
        );
        assert!(
            setup.confidence > dec!(0.5),
            "confidence should be > 0.5, got: {}",
            setup.confidence
        );
        assert_eq!(setup.setup_id, "pf:AAPL:42");
        assert_eq!(setup.hypothesis_id, "pfh:AAPL:42");
        assert!(setup.risk_notes.contains(&"family=pressure_vortex".to_string()));
    }

    #[test]
    fn weak_vortex_produces_observe() {
        let vortex = make_vortex(
            "MSFT",
            dec!(0.02),
            dec!(0.75),
            dec!(-0.01),
            &[
                PressureChannel::CapitalFlow,
                PressureChannel::Momentum,
                PressureChannel::Volume,
            ],
        );
        let now = OffsetDateTime::now_utc();
        let setup = vortex_to_tactical_setup(&vortex, now, 10).expect("should produce a setup");

        // strength=0.02 < 0.03 review threshold → falls through to observe
        assert_eq!(setup.action, "observe");
    }

    #[test]
    fn below_minimum_strength_produces_nothing() {
        let vortex = make_vortex(
            "GOOG",
            dec!(0.003),
            dec!(0.5),
            dec!(0.001),
            &[
                PressureChannel::OrderBook,
                PressureChannel::CapitalFlow,
            ],
        );
        let now = OffsetDateTime::now_utc();
        let result = vortex_to_tactical_setup(&vortex, now, 5);
        assert!(result.is_none(), "below minimum should produce None");
    }
}
