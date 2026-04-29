//! Cross-ontology intent belief — world-space categorical posterior
//! per symbol.
//!
//! Eden's `PressureBeliefField` holds per-(symbol, channel) Gaussian
//! beliefs — that's the **sensor-space** model: "what does orderbook
//! pressure look like for this symbol?" But Y's world-space model
//! is a different projection: "what is this symbol actually *doing*?"
//! — accumulation, distribution, rotation, volatility, or unknown.
//!
//! This module projects channel pressures into intent evidence and
//! maintains a per-symbol `CategoricalBelief<IntentKind>`. It does
//! NOT replace the per-channel Gaussian beliefs — it coexists as a
//! second projection. Having data and not using it would be silly;
//! having one projection of data is incomplete. So we have both.
//!
//! Semantics of the mapping (channel, signed pressure) → intent:
//!
//!   OrderBook  + positive  → Accumulation (bid-side absorption)
//!   OrderBook  + negative  → Distribution (ask-side absorption)
//!   CapitalFlow + positive → Accumulation (money flowing in)
//!   CapitalFlow + negative → Distribution (money flowing out)
//!   Institutional + any    → Rotation (repositioning)
//!   Volume     + large     → Volatility (regardless of direction)
//!   Momentum + ≈ 0         → Unknown (no directional signal)
//!   Structure  + nonzero   → Rotation (depth structure shift)
//!
//! Noise floor: pressures with absolute value < 0.05 contribute no
//! evidence (avoids building intent posteriors out of jitter).

use std::collections::HashMap;

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::ontology::objects::{Market, Symbol};
use crate::pipeline::belief::CategoricalBelief;
use crate::pipeline::pressure::PressureChannel;

/// World-space intent categories. Deliberately small — these are the
/// primitives Claude Code-as-operator would choose between when
/// reading a symbol's current state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentKind {
    /// Net buying pressure: bid absorption / capital inflow.
    Accumulation,
    /// Net selling pressure: ask absorption / capital outflow.
    Distribution,
    /// Institutional / structural repositioning without clear direction.
    Rotation,
    /// High-variance trading without directional bias.
    Volatility,
    /// Insufficient or ambiguous signal.
    Unknown,
}

pub const INTENT_VARIANTS: &[IntentKind] = &[
    IntentKind::Accumulation,
    IntentKind::Distribution,
    IntentKind::Rotation,
    IntentKind::Volatility,
    IntentKind::Unknown,
];

/// Noise floor: pressures below this magnitude produce no intent evidence.
const INTENT_NOISE_FLOOR: f64 = 0.05;

/// Volume threshold above which pressure counts as Volatility evidence
/// regardless of sign. Below this, Volume is mapped via the generic
/// sign rule (which for Volume means Unknown since it has no natural
/// direction).
const VOLUME_VOLATILITY_THRESHOLD: f64 = 0.40;

/// Map a single (channel, signed pressure) observation to intent
/// evidence. Returns `None` for pressures below noise floor.
pub fn channel_to_intent(channel: PressureChannel, pressure: Decimal) -> Option<IntentKind> {
    let p = pressure.to_f64().unwrap_or(0.0);
    if p.abs() < INTENT_NOISE_FLOOR {
        return None;
    }
    let intent = match channel {
        PressureChannel::OrderBook | PressureChannel::CapitalFlow => {
            if p > 0.0 {
                IntentKind::Accumulation
            } else {
                IntentKind::Distribution
            }
        }
        PressureChannel::Institutional | PressureChannel::Structure => IntentKind::Rotation,
        PressureChannel::Volume => {
            if p.abs() >= VOLUME_VOLATILITY_THRESHOLD {
                IntentKind::Volatility
            } else {
                IntentKind::Unknown
            }
        }
        PressureChannel::Momentum => {
            // Momentum has sign but we treat strong momentum as
            // corroborating whatever direction already showed up in
            // flow — so small-magnitude momentum is Unknown, large
            // magnitude votes with the flow channels via the accumulate/
            // distribute mapping.
            if p > 0.0 {
                IntentKind::Accumulation
            } else {
                IntentKind::Distribution
            }
        }
    };
    Some(intent)
}

/// Per-market intent belief field — coexists with PressureBeliefField.
/// Not persisted through the same belief_snapshot path today (v1); if
/// we want cross-restart continuity we can add that.
pub struct IntentBeliefField {
    market: Market,
    per_symbol: HashMap<Symbol, CategoricalBelief<IntentKind>>,
    /// Last-tick snapshot for posterior-shift detection (same pattern
    /// as PressureBeliefField).
    previous: HashMap<Symbol, CategoricalBelief<IntentKind>>,
}

impl IntentBeliefField {
    pub fn new(market: Market) -> Self {
        Self {
            market,
            per_symbol: HashMap::new(),
            previous: HashMap::new(),
        }
    }

    pub fn market(&self) -> Market {
        self.market
    }

    pub fn informed_count(&self) -> usize {
        self.per_symbol
            .values()
            .filter(|b| b.sample_count >= 1)
            .count()
    }

    fn fresh_belief() -> CategoricalBelief<IntentKind> {
        CategoricalBelief::uniform(INTENT_VARIANTS.to_vec())
    }

    /// Record a batch of channel observations for one symbol, producing
    /// one intent update per observation that clears the noise floor.
    /// All observations for a symbol should be recorded together each
    /// tick so posterior shift detection works on tick boundaries.
    pub fn record_channel_samples(
        &mut self,
        symbol: &Symbol,
        samples: &[(PressureChannel, Decimal)],
    ) {
        // Snapshot pre-update posterior for shift detection.
        if let Some(existing) = self.per_symbol.get(symbol) {
            self.previous.insert(symbol.clone(), existing.clone());
        }

        let evidence: Vec<IntentKind> = samples
            .iter()
            .filter_map(|(c, p)| channel_to_intent(*c, *p))
            .collect();
        if evidence.is_empty() {
            return;
        }
        let belief = self
            .per_symbol
            .entry(symbol.clone())
            .or_insert_with(Self::fresh_belief);
        for intent in &evidence {
            belief.update(intent);
        }
    }

    pub fn query(&self, symbol: &Symbol) -> Option<&CategoricalBelief<IntentKind>> {
        self.per_symbol.get(symbol)
    }

    /// Read-only access to the dominant intent + its probability for a
    /// symbol. Returns None if unobserved.
    pub fn dominant_intent(&self, symbol: &Symbol) -> Option<(IntentKind, f64)> {
        let belief = self.per_symbol.get(symbol)?;
        let mut best: Option<(IntentKind, f64)> = None;
        for (i, p) in belief.probs.iter().enumerate() {
            let pf = p.to_f64().unwrap_or(0.0);
            let variant = *belief.variants.get(i)?;
            if best.map_or(true, |(_, b)| pf > b) {
                best = Some((variant, pf));
            }
        }
        best
    }

    /// Top-K symbols by intent decisiveness — symbols where one intent
    /// dominates strongly (posterior mass on one variant > min_dominance).
    /// Used to surface `intent:` wake lines for the most clearly-actionable
    /// symbols.
    pub fn top_decisive(
        &self,
        k: usize,
        min_samples: u32,
        min_dominance: f64,
    ) -> Vec<IntentDecision> {
        let mut out: Vec<IntentDecision> = self
            .per_symbol
            .iter()
            .filter(|(_, b)| b.sample_count >= min_samples)
            .filter_map(|(symbol, b)| {
                let (intent, prob) = best_in(b)?;
                if prob < min_dominance {
                    return None;
                }
                Some(IntentDecision {
                    symbol: symbol.clone(),
                    intent,
                    probability: prob,
                    sample_count: b.sample_count,
                })
            })
            .collect();
        out.sort_by(|a, b| {
            b.probability
                .partial_cmp(&a.probability)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        out.truncate(k);
        out
    }

    pub fn per_symbol_iter(
        &self,
    ) -> impl Iterator<Item = (&Symbol, &CategoricalBelief<IntentKind>)> {
        self.per_symbol.iter()
    }

    /// Push a single outcome-validated intent observation. Used by the
    /// backward pass (outcome_feedback): when a setup resolves, one
    /// sample in the confirmed/refuted direction closes the forward
    /// loop from decision → KG. Magnitude is intentionally ~1 tick's
    /// worth — the price action during the horizon has already written
    /// many pressure-driven samples, so this is a small but non-zero
    /// grounding signal.
    pub fn observe_outcome_intent(&mut self, symbol: &Symbol, intent: IntentKind) {
        if let Some(existing) = self.per_symbol.get(symbol) {
            self.previous.insert(symbol.clone(), existing.clone());
        }
        let belief = self
            .per_symbol
            .entry(symbol.clone())
            .or_insert_with(Self::fresh_belief);
        belief.update(&intent);
    }

    /// Raw-insert for cross-session restore. Replaces any existing
    /// belief for the symbol; does NOT snapshot into `previous` so
    /// the first recorded sample after restore won't falsely show a
    /// "just shifted" signal.
    pub fn insert_raw(&mut self, symbol: Symbol, belief: CategoricalBelief<IntentKind>) {
        self.per_symbol.insert(symbol, belief);
    }
}

fn best_in(belief: &CategoricalBelief<IntentKind>) -> Option<(IntentKind, f64)> {
    let mut best: Option<(IntentKind, f64)> = None;
    for (i, p) in belief.probs.iter().enumerate() {
        let pf = p.to_f64().unwrap_or(0.0);
        let variant = *belief.variants.get(i)?;
        if best.map_or(true, |(_, b)| pf > b) {
            best = Some((variant, pf));
        }
    }
    best
}

#[derive(Debug, Clone)]
pub struct IntentDecision {
    pub symbol: Symbol,
    pub intent: IntentKind,
    pub probability: f64,
    pub sample_count: u32,
}

fn intent_name(intent: IntentKind) -> &'static str {
    match intent {
        IntentKind::Accumulation => "accumulation",
        IntentKind::Distribution => "distribution",
        IntentKind::Rotation => "rotation",
        IntentKind::Volatility => "volatility",
        IntentKind::Unknown => "unknown",
    }
}

pub fn format_intent_wake_line(decision: &IntentDecision) -> String {
    format!(
        "intent: {} {} {:.2} (n={})",
        decision.symbol.0,
        intent_name(decision.intent),
        decision.probability,
        decision.sample_count,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn noise_floor_produces_no_evidence() {
        assert!(channel_to_intent(PressureChannel::OrderBook, dec!(0.02)).is_none());
        assert!(channel_to_intent(PressureChannel::OrderBook, dec!(-0.04)).is_none());
    }

    #[test]
    fn orderbook_positive_maps_to_accumulation() {
        assert_eq!(
            channel_to_intent(PressureChannel::OrderBook, dec!(0.5)),
            Some(IntentKind::Accumulation)
        );
    }

    #[test]
    fn capital_flow_negative_maps_to_distribution() {
        assert_eq!(
            channel_to_intent(PressureChannel::CapitalFlow, dec!(-0.3)),
            Some(IntentKind::Distribution)
        );
    }

    #[test]
    fn institutional_any_direction_maps_to_rotation() {
        assert_eq!(
            channel_to_intent(PressureChannel::Institutional, dec!(0.2)),
            Some(IntentKind::Rotation)
        );
        assert_eq!(
            channel_to_intent(PressureChannel::Institutional, dec!(-0.2)),
            Some(IntentKind::Rotation)
        );
    }

    #[test]
    fn large_volume_maps_to_volatility_small_volume_to_unknown() {
        assert_eq!(
            channel_to_intent(PressureChannel::Volume, dec!(0.5)),
            Some(IntentKind::Volatility)
        );
        assert_eq!(
            channel_to_intent(PressureChannel::Volume, dec!(0.1)),
            Some(IntentKind::Unknown)
        );
    }

    #[test]
    fn record_builds_intent_belief_from_channels() {
        let mut field = IntentBeliefField::new(Market::Hk);
        let s = Symbol("0700.HK".to_string());

        // Strong accumulation signal across orderbook + capital_flow.
        for _ in 0..10 {
            field.record_channel_samples(
                &s,
                &[
                    (PressureChannel::OrderBook, dec!(0.5)),
                    (PressureChannel::CapitalFlow, dec!(0.4)),
                ],
            );
        }

        let belief = field.query(&s).unwrap();
        assert!(belief.sample_count >= 20, "got {}", belief.sample_count);

        let (dominant, prob) = field.dominant_intent(&s).unwrap();
        assert_eq!(dominant, IntentKind::Accumulation);
        assert!(prob > 0.5, "accumulation should dominate, got {}", prob);
    }

    #[test]
    fn mixed_signals_don_t_force_false_dominance() {
        let mut field = IntentBeliefField::new(Market::Hk);
        let s = Symbol("X.HK".to_string());

        // Balanced accum + distribution + rotation signals.
        for _ in 0..3 {
            field.record_channel_samples(
                &s,
                &[
                    (PressureChannel::OrderBook, dec!(0.3)),
                    (PressureChannel::CapitalFlow, dec!(-0.3)),
                    (PressureChannel::Institutional, dec!(0.3)),
                ],
            );
        }

        let (dominant, prob) = field.dominant_intent(&s).unwrap();
        // No intent should own > 0.5 of posterior given balanced evidence.
        assert!(
            prob < 0.5,
            "balanced evidence should not produce strong dominance, got {} {:?}",
            prob,
            dominant
        );
    }

    #[test]
    fn top_decisive_filters_by_dominance_threshold() {
        let mut field = IntentBeliefField::new(Market::Hk);
        let strong = Symbol("STRONG.HK".to_string());
        let weak = Symbol("WEAK.HK".to_string());

        // Strong: 20 consecutive accumulation signals.
        for _ in 0..20 {
            field.record_channel_samples(&strong, &[(PressureChannel::OrderBook, dec!(0.5))]);
        }
        // Weak: only 2 accumulation signals, below min_samples.
        for _ in 0..2 {
            field.record_channel_samples(&weak, &[(PressureChannel::OrderBook, dec!(0.5))]);
        }

        let top = field.top_decisive(5, 10, 0.5);
        assert_eq!(top.len(), 1, "only STRONG meets thresholds");
        assert_eq!(top[0].symbol.0, "STRONG.HK");
        assert_eq!(top[0].intent, IntentKind::Accumulation);
    }

    #[test]
    fn format_intent_wake_line_shape() {
        let decision = IntentDecision {
            symbol: Symbol("0700.HK".to_string()),
            intent: IntentKind::Accumulation,
            probability: 0.73,
            sample_count: 45,
        };
        let line = format_intent_wake_line(&decision);
        assert_eq!(line, "intent: 0700.HK accumulation 0.73 (n=45)");
    }

    #[test]
    fn noise_floor_skips_samples_silently() {
        let mut field = IntentBeliefField::new(Market::Hk);
        let s = Symbol("JITTER.HK".to_string());
        for _ in 0..100 {
            field.record_channel_samples(
                &s,
                &[
                    (PressureChannel::OrderBook, dec!(0.01)),
                    (PressureChannel::Volume, dec!(0.02)),
                ],
            );
        }
        assert!(
            field.query(&s).is_none(),
            "jitter below noise floor should not build belief"
        );
    }
}
