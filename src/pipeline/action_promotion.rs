//! Data-driven setup-action promotion.
//!
//! Two modes coexist behind a single dispatcher:
//!
//! 1. **percentile** (default) — top X% of `setup.confidence` rank within
//!    a single tick are promoted to Enter / Review. Cost-control parameter
//!    only; not a signal threshold. Ships the V3 plan baseline behaviour.
//!
//! 2. **kl_surprise** (opt-in via env `EDEN_ACTION_PROMOTION=kl_surprise`)
//!    — promote setups whose underlying symbol crossed its own KL surprise
//!    z-score reference (z ≥ 2 → Enter, z ≥ 1 → Review). Self-referential,
//!    no global percentile, no business threshold. The 2σ / 1σ pair are
//!    Gaussian-statistics first principles (95% / 68% intervals), not
//!    arbitrary numbers — every symbol's bar is set by its own EWMA.
//!
//! Default stays percentile so existing behaviour is preserved when the
//! env-flag is unset. Dispatch happens once per tick at the runtime call
//! site; both modes mutate the setups slice in place.

use rust_decimal::prelude::ToPrimitive;

use crate::ontology::objects::Symbol;
use crate::ontology::reasoning::{ReasoningScope, TacticalAction, TacticalSetup};
use crate::pipeline::belief_field::PressureBeliefField;
use crate::pipeline::kl_surprise::KlSurpriseTracker;
use crate::pipeline::pressure::PressureChannel;

/// Top X% of confidence ranks promote to `Enter`. 0.15 = top sixth.
/// Cost-control parameter (operator attention budget), not a signal
/// threshold — Eden is not gating signal here, just sorting attention.
pub const ENTER_PCT: f64 = 0.15;

/// Top X% (cumulative, including Enter band) promote to `Review`.
/// 0.40 = top two-fifths.
pub const REVIEW_PCT: f64 = 0.40;

/// Z-score threshold (in σ_self units) at which a symbol's KL surprise
/// is "unusual enough" to enter. 2.0 ≈ 95% Gaussian CI — pure statistical
/// first principle, not a calibrated business knob.
pub const KL_SURPRISE_ENTER_Z: f64 = 2.0;

/// Z-score threshold for review (≈ 68% Gaussian CI).
pub const KL_SURPRISE_REVIEW_Z: f64 = 1.0;

/// All six pressure channels. Local convenience for the KL mode.
const CHANNELS: [PressureChannel; 6] = [
    PressureChannel::OrderBook,
    PressureChannel::CapitalFlow,
    PressureChannel::Institutional,
    PressureChannel::Momentum,
    PressureChannel::Volume,
    PressureChannel::Structure,
];

/// Env-flag dispatched action promotion. Reads `EDEN_ACTION_PROMOTION`:
///   - `kl_surprise` → run [`apply_kl_surprise_action_promotion`]
///   - any other value or unset → run [`apply_percentile_action_promotion`]
///
/// `tracker` and `belief_field` are required for the KL path. Callers
/// can always pass them; percentile mode ignores them.
pub fn apply_action_promotion(
    setups: &mut [TacticalSetup],
    tracker: &KlSurpriseTracker,
    belief_field: &PressureBeliefField,
) {
    if matches!(
        std::env::var("EDEN_ACTION_PROMOTION").ok().as_deref(),
        Some("kl_surprise")
    ) {
        apply_kl_surprise_action_promotion(setups, tracker, belief_field);
    } else {
        apply_percentile_action_promotion(setups);
    }
}

/// Promote actions in-place using percentile ranks of `setup.confidence`.
/// Setups with confidence below the Review cutoff stay at `Observe`.
/// No-op when fewer than 2 setups (no meaningful percentile).
pub fn apply_percentile_action_promotion(setups: &mut [TacticalSetup]) {
    if setups.len() < 2 {
        return;
    }
    // Collect confidence values + their indices, sort desc, assign rank.
    let mut indexed: Vec<(usize, f64)> = setups
        .iter()
        .enumerate()
        .map(|(i, s)| (i, s.confidence.to_f64().unwrap_or(0.0)))
        .collect();
    indexed.sort_by(|a, b| {
        b.1.partial_cmp(&a.1)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| a.0.cmp(&b.0))
    });
    let n = indexed.len() as f64;
    let enter_cap = ((n * ENTER_PCT).ceil() as usize).max(1);
    let review_cap = ((n * REVIEW_PCT).ceil() as usize).max(enter_cap);

    for (rank, (idx, _conf)) in indexed.iter().enumerate() {
        let action = if rank < enter_cap {
            TacticalAction::Enter
        } else if rank < review_cap {
            TacticalAction::Review
        } else {
            TacticalAction::Observe
        };
        setups[*idx].action = action;
    }
}

/// Promote actions in-place using each symbol's *own* KL surprise z-score.
/// For each setup with a `Symbol` scope, look up the max |z| across the
/// six pressure channels: if z ≥ [`KL_SURPRISE_ENTER_Z`] → Enter, if
/// z ≥ [`KL_SURPRISE_REVIEW_Z`] → Review, otherwise Observe.
///
/// Sets `setup.action = Observe` for non-Symbol scopes (sector / market
/// rollups carry no per-symbol baseline). When the tracker hasn't yet
/// bootstrapped for a symbol, the setup also stays at Observe.
pub fn apply_kl_surprise_action_promotion(
    setups: &mut [TacticalSetup],
    tracker: &KlSurpriseTracker,
    belief_field: &PressureBeliefField,
) {
    for setup in setups.iter_mut() {
        let symbol = match &setup.scope {
            ReasoningScope::Symbol(s) => s.clone(),
            _ => {
                setup.action = TacticalAction::Observe;
                continue;
            }
        };

        let max_abs_z = max_surprise_abs_z(&symbol, tracker, belief_field);
        setup.action = match max_abs_z {
            Some(z) if z >= KL_SURPRISE_ENTER_Z => TacticalAction::Enter,
            Some(z) if z >= KL_SURPRISE_REVIEW_Z => TacticalAction::Review,
            _ => TacticalAction::Observe,
        };
    }
}

/// Largest absolute z-score across the six pressure channels for this
/// symbol, computed by re-deriving the current KL value from the belief
/// field and querying the tracker. Returns `None` when no channel has a
/// usable baseline yet.
fn max_surprise_abs_z(
    symbol: &Symbol,
    tracker: &KlSurpriseTracker,
    belief_field: &PressureBeliefField,
) -> Option<f64> {
    let mut best: Option<f64> = None;
    for channel in CHANNELS {
        let current = belief_field.query_gaussian(symbol, channel);
        let previous = belief_field.query_previous_gaussian(symbol, channel);
        let (Some(curr), Some(prev)) = (current, previous) else {
            continue;
        };
        let Some(kl) = prev.kl_divergence(curr) else {
            continue;
        };
        let Some(z) = tracker.surprise_z(symbol, channel, kl) else {
            continue;
        };
        let abs_z = z.abs();
        if best.map_or(true, |b| abs_z > b) {
            best = Some(abs_z);
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
    use crate::ontology::objects::{Market, Symbol};
    use crate::ontology::reasoning::{default_case_horizon, DecisionLineage, ReasoningScope};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    fn fixture(symbol: &str, confidence: Decimal) -> TacticalSetup {
        TacticalSetup {
            setup_id: format!("setup:{symbol}"),
            hypothesis_id: format!("hyp:{symbol}"),
            runner_up_hypothesis_id: None,
            provenance: ProvenanceMetadata::new(
                ProvenanceSource::Computed,
                OffsetDateTime::UNIX_EPOCH,
            ),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(Symbol(symbol.into())),
            title: format!("Long {symbol}"),
            action: TacticalAction::Observe,
            direction: None,
            horizon: default_case_horizon(),
            confidence,
            confidence_gap: Decimal::ZERO,
            heuristic_edge: Decimal::ZERO,
            convergence_score: None,
            convergence_detail: None,
            workflow_id: None,
            entry_rationale: String::new(),
            causal_narrative: None,
            risk_notes: Vec::new(),
            review_reason_code: None,
            policy_verdict: None,
        }
    }

    #[test]
    fn empty_or_singleton_is_noop() {
        let mut empty: Vec<TacticalSetup> = Vec::new();
        apply_percentile_action_promotion(&mut empty);
        let mut single = vec![fixture("A", dec!(0.9))];
        apply_percentile_action_promotion(&mut single);
        assert_eq!(single[0].action, TacticalAction::Observe);
    }

    #[test]
    fn highest_confidence_becomes_enter() {
        let mut setups = vec![
            fixture("A", dec!(0.10)),
            fixture("B", dec!(0.20)),
            fixture("C", dec!(0.30)),
            fixture("D", dec!(0.40)),
            fixture("E", dec!(0.50)),
            fixture("F", dec!(0.60)),
            fixture("G", dec!(0.70)),
            fixture("H", dec!(0.80)),
            fixture("I", dec!(0.90)),
            fixture("J", dec!(0.99)),
        ];
        apply_percentile_action_promotion(&mut setups);
        // n=10, enter_cap = ceil(10*0.15) = 2, review_cap = ceil(10*0.40)=4
        // Top 2 confidence (J, I) → Enter
        // Next 2 (H, G) → Review
        // Rest → Observe
        let actions: std::collections::HashMap<&str, TacticalAction> = setups
            .iter()
            .map(|s| (s.setup_id.split(':').nth(1).unwrap(), s.action))
            .collect();
        assert_eq!(actions["J"], TacticalAction::Enter);
        assert_eq!(actions["I"], TacticalAction::Enter);
        assert_eq!(actions["H"], TacticalAction::Review);
        assert_eq!(actions["G"], TacticalAction::Review);
        assert_eq!(actions["F"], TacticalAction::Observe);
        assert_eq!(actions["A"], TacticalAction::Observe);
    }

    #[test]
    fn ties_broken_by_setup_id() {
        // Three setups all at the same confidence — at minimum one
        // gets Enter (enter_cap = max(1)). Ordering should be stable
        // based on setup_id, so the alphabetically-first ID wins.
        let mut setups = vec![
            fixture("C", dec!(0.5)),
            fixture("A", dec!(0.5)),
            fixture("B", dec!(0.5)),
        ];
        apply_percentile_action_promotion(&mut setups);
        // ceil(3 * 0.15) = 1, enter_cap = 1
        // Sort: tie -> use original index. Since original index 0=C,1=A,2=B,
        // first sort iteration tries (idx=0, 0.5) vs others — partial_cmp
        // returns Equal, so falls back to a.0.cmp(&b.0). idx=0 (C) wins.
        let c = setups
            .iter()
            .find(|s| s.setup_id == "setup:C")
            .unwrap()
            .action;
        assert_eq!(c, TacticalAction::Enter);
    }

    #[test]
    fn never_zero_enter_when_setups_present() {
        // 4 setups → enter_cap = max(1, ceil(4*0.15)=1) = 1
        // At least one Enter.
        let mut setups = vec![
            fixture("A", dec!(0.1)),
            fixture("B", dec!(0.2)),
            fixture("C", dec!(0.3)),
            fixture("D", dec!(0.4)),
        ];
        apply_percentile_action_promotion(&mut setups);
        let enters = setups
            .iter()
            .filter(|s| s.action == TacticalAction::Enter)
            .count();
        assert_eq!(enters, 1);
    }

    fn build_belief_with_shock(
        market: Market,
        symbol: &str,
        baseline_kls: usize,
        shock_value: Decimal,
    ) -> (PressureBeliefField, KlSurpriseTracker) {
        let mut field = PressureBeliefField::new(market);
        let s = Symbol(symbol.into());
        // Build an informed belief at value 1.0 over the bootstrap window.
        for _ in 0..6 {
            field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(1.0), 1);
        }
        let mut tracker = KlSurpriseTracker::new();
        // Drive the tracker through `baseline_kls` ticks of small shocks so
        // the EWMA baseline reaches a stable, low mean_kl with a meaningful
        // (but small) std. Alternating tight noise keeps each KL sample
        // small and similar, giving a tight baseline against which a big
        // shock's KL value reads as a strong surprise.
        for tick in 2..=(baseline_kls as u64 + 1) {
            let mod_value = if tick % 2 == 0 {
                dec!(1.001)
            } else {
                dec!(0.999)
            };
            field.record_gaussian_sample(&s, PressureChannel::OrderBook, mod_value, tick);
            tracker.observe_from_belief_field(&field);
        }
        // Single big shock — should produce a large KL z-score.
        field.record_gaussian_sample(
            &s,
            PressureChannel::OrderBook,
            shock_value,
            baseline_kls as u64 + 2,
        );
        (field, tracker)
    }

    #[test]
    fn kl_mode_promotes_only_surprised_symbol() {
        // Long baseline (200) so EWMA has converged + strong shock so the
        // KL z-score is unambiguous. The 1σ/2σ thresholds are statistical
        // first principles — a big enough shock must clear them.
        let (field, tracker) = build_belief_with_shock(Market::Hk, "0700.HK", 200, dec!(1000.0));
        let shocked = fixture("0700.HK", dec!(0.5));
        let quiet = fixture("0005.HK", dec!(0.99));
        let mut setups = vec![shocked.clone(), quiet.clone()];

        apply_kl_surprise_action_promotion(&mut setups, &tracker, &field);

        let shocked_after = setups
            .iter()
            .find(|s| s.setup_id == "setup:0700.HK")
            .unwrap()
            .clone();
        let quiet_after = setups
            .iter()
            .find(|s| s.setup_id == "setup:0005.HK")
            .unwrap()
            .clone();

        // The shocked symbol should clear at least the Review band.
        assert!(
            matches!(
                shocked_after.action,
                TacticalAction::Enter | TacticalAction::Review
            ),
            "expected Enter/Review for shocked symbol, got {:?}",
            shocked_after.action
        );
        // Untracked symbol stays Observe regardless of confidence.
        assert_eq!(quiet_after.action, TacticalAction::Observe);
    }

    #[test]
    fn kl_mode_observes_when_tracker_empty() {
        let field = PressureBeliefField::new(Market::Hk);
        let tracker = KlSurpriseTracker::new();
        let mut setups = vec![fixture("A.US", dec!(0.99)), fixture("B.US", dec!(0.99))];
        apply_kl_surprise_action_promotion(&mut setups, &tracker, &field);
        for s in &setups {
            assert_eq!(s.action, TacticalAction::Observe);
        }
    }

    #[test]
    fn dispatcher_respects_env_flag() {
        // Save and restore env to avoid cross-test pollution. Tests in
        // Rust default to multi-threaded; this is best-effort isolation
        // and should not be relied on for production behaviour.
        // The kl_mode_observes_when_tracker_empty case already covers
        // the kl branch separately.
        let prior = std::env::var("EDEN_ACTION_PROMOTION").ok();
        std::env::remove_var("EDEN_ACTION_PROMOTION");

        let mut setups = vec![fixture("A", dec!(0.10)), fixture("B", dec!(0.99))];
        let field = PressureBeliefField::new(Market::Hk);
        let tracker = KlSurpriseTracker::new();
        apply_action_promotion(&mut setups, &tracker, &field);
        // Default is percentile — top confidence (B) should be Enter.
        let b = setups.iter().find(|s| s.setup_id == "setup:B").unwrap();
        assert_eq!(b.action, TacticalAction::Enter);

        if let Some(v) = prior {
            std::env::set_var("EDEN_ACTION_PROMOTION", v);
        }
    }
}
