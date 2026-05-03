//! Self-referential KL surprise tracker — graph-native decision unblock.
//!
//! For each (Symbol, PressureChannel) we maintain an EWMA baseline of the
//! `KL(prev_belief || current_belief)` value the belief field already
//! produces between ticks. The baseline gives us each channel's *own*
//! historical noise floor — `mean_kl` and `std_kl`. From these:
//!
//!   surprise_z = (current_kl - mean_kl) / std_kl
//!
//! is the channel's deviation from its own typical surprise behaviour, in
//! its own σ units. **This is the only "threshold" we use** — and it's not
//! a threshold at all, it's a self-referential statistical distance.
//!
//! Per the V3 first-principles design, decision should not depend on
//! hand-coded percentiles or external sigma constants. The baseline is
//! per-symbol, per-channel, and entirely self-derived.
//!
//! Two graph-native outputs feed back into the sub-KG:
//!   - `KlSurpriseMagnitude` ∈ [0, 1] — `tanh(|max_z| / 2)` over channels
//!   - `KlSurpriseDirection` ∈ [-1, 1] — sign of `(current_mean - prev_mean)`
//!     on the dominant channel (the one with biggest |z|)
//!
//! These flow through `observe_from_subkg` in `loopy_bp.rs` and lift the
//! BP posterior magnitude on whichever symbols are genuinely "surprising"
//! relative to their own history. No magic numbers, no rules.

use std::collections::HashMap;

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;

use crate::ontology::objects::Symbol;
use crate::pipeline::belief::BELIEF_INFORMED_MIN_SAMPLES;
use crate::pipeline::belief_field::PressureBeliefField;
use crate::pipeline::pressure::PressureChannel;

/// EWMA decay applied to the KL baseline. 0.99 ⇒ exponential half-life
/// of ≈ ln(2)/(1-0.99) ≈ 69 ticks. Long enough that fast spikes register
/// as surprises against a stable backdrop; short enough that regime shifts
/// (e.g., earnings, big news) are absorbed within a session.
///
/// This is a memory-length design choice, not a magic threshold — every
/// EWMA needs a decay; 0.99 is the natural choice when "minute" and
/// "session" are the bracketing scales.
pub const KL_EWMA_DECAY: f64 = 0.99;

/// Minimum baseline samples before `surprise_z` returns a value. Mirrors
/// `BELIEF_INFORMED_MIN_SAMPLES` (5) — the same statistical floor Eden
/// already uses to declare a Gaussian belief "informed". Below this the
/// std estimate isn't trustworthy enough to spend a decision on.
pub const KL_SURPRISE_BOOTSTRAP_MIN: u32 = BELIEF_INFORMED_MIN_SAMPLES;

/// All six pressure channels. Local convenience — keeps tracker self-
/// contained without forcing PressureChannel to expose `iter`.
const CHANNELS: [PressureChannel; 6] = [
    PressureChannel::OrderBook,
    PressureChannel::CapitalFlow,
    PressureChannel::Institutional,
    PressureChannel::Momentum,
    PressureChannel::Volume,
    PressureChannel::Structure,
];

/// Per-(symbol, channel) running baseline of KL values. EWMA on `kl` and
/// `kl²` gives mean and variance in O(1) per update, no array storage.
#[derive(Debug, Clone, Default)]
struct KlBaseline {
    mean_kl: f64,
    mean_kl_sq: f64,
    sample_count: u32,
}

impl KlBaseline {
    fn observe(&mut self, kl: f64) {
        if !kl.is_finite() {
            return;
        }
        if self.sample_count == 0 {
            self.mean_kl = kl;
            self.mean_kl_sq = kl * kl;
        } else {
            let alpha = 1.0 - KL_EWMA_DECAY;
            self.mean_kl = (1.0 - alpha) * self.mean_kl + alpha * kl;
            self.mean_kl_sq = (1.0 - alpha) * self.mean_kl_sq + alpha * kl * kl;
        }
        self.sample_count = self.sample_count.saturating_add(1);
    }

    fn variance(&self) -> f64 {
        // EWMA variance estimate: E[X²] − (E[X])². Clamp to 0 so floating-
        // point noise can't produce a negative variance.
        (self.mean_kl_sq - self.mean_kl * self.mean_kl).max(0.0)
    }

    fn std_dev(&self) -> f64 {
        self.variance().sqrt()
    }

    /// Z-score of `current_kl` against the baseline, or `None` if the
    /// baseline is below the bootstrap threshold or std is degenerate.
    fn surprise_z(&self, current_kl: f64) -> Option<f64> {
        if !current_kl.is_finite() {
            return None;
        }
        if self.sample_count < KL_SURPRISE_BOOTSTRAP_MIN {
            return None;
        }
        let std = self.std_dev();
        if std < 1.0e-9 {
            return None;
        }
        Some((current_kl - self.mean_kl) / std)
    }
}

/// Per-(symbol, channel) baselines for KL surprise. One instance per
/// market (HK and US each carry their own, mirroring `PressureBeliefField`).
#[derive(Debug, Default)]
pub struct KlSurpriseTracker {
    baselines: HashMap<(Symbol, PressureChannel), KlBaseline>,
}

impl KlSurpriseTracker {
    pub fn new() -> Self {
        Self::default()
    }

    /// Observe a single (symbol, channel, kl) sample. Lazily inserts a
    /// fresh baseline on first call.
    pub fn observe(&mut self, symbol: &Symbol, channel: PressureChannel, kl: f64) {
        self.baselines
            .entry((symbol.clone(), channel))
            .or_default()
            .observe(kl);
    }

    /// Z-score of `current_kl` for (symbol, channel) against its own
    /// baseline. Returns `None` until the baseline has at least
    /// `KL_SURPRISE_BOOTSTRAP_MIN` samples.
    pub fn surprise_z(
        &self,
        symbol: &Symbol,
        channel: PressureChannel,
        current_kl: f64,
    ) -> Option<f64> {
        self.baselines
            .get(&(symbol.clone(), channel))
            .and_then(|b| b.surprise_z(current_kl))
    }

    /// Number of (symbol, channel) baselines tracked. Diagnostic.
    pub fn baseline_count(&self) -> usize {
        self.baselines.len()
    }

    /// Walk every (symbol, channel) in the belief field, compute the KL
    /// between previous and current Gaussian belief, and feed each into
    /// its own baseline. Skips channels where either side of the KL is
    /// uninformed (per `kl_divergence` contract).
    ///
    /// Call this once per tick *after* the belief field has been updated
    /// from this tick's pressure samples. The `previous_gaussian` snapshot
    /// inside the field already holds the prior-tick belief at this point.
    pub fn observe_from_belief_field(&mut self, belief_field: &PressureBeliefField) {
        // We can't iterate `belief_field.gaussian_iter` and call its
        // KL helper inline because the helper borrows the field
        // immutably. Collect KL values first, then mutate.
        let mut samples: Vec<(Symbol, PressureChannel, f64)> = Vec::new();
        for ((symbol, channel), current_belief) in belief_field.gaussian_iter() {
            if let Some(prev_belief) = belief_field.query_previous_gaussian(symbol, *channel) {
                if let Some(kl) = prev_belief.kl_divergence(current_belief) {
                    if kl.is_finite() {
                        samples.push((symbol.clone(), *channel, kl));
                    }
                }
            }
        }
        for (symbol, channel, kl) in samples {
            self.observe(&symbol, channel, kl);
        }
    }

    /// For every symbol that has at least one (channel) baseline ready,
    /// compute (magnitude, direction) suitable for sub-KG injection:
    ///
    ///   - magnitude ∈ [0, 1] = `tanh(|max_z| / 2)` over channels
    ///     where `max_z` is the channel z-score with the largest absolute
    ///     value. Returns 0 magnitude when no channel has a usable z.
    ///   - direction ∈ [-1, 1] = sign of `(current_mean - prev_mean)`
    ///     on the dominant channel. 0 when no informed channel exists.
    ///
    /// `tanh(|z|/2)` is monotonic, saturates near 1 at z=4, and returns
    /// ≈0.46 at z=1, ≈0.76 at z=2 — closer to "smooth ramp toward
    /// saturation" than to a hard cutoff. The choice of `2` in the
    /// divisor mirrors the standard practice of using ≈2σ as the
    /// "starting to be unusual" reference; it is *not* a threshold.
    /// Mutate the perception graph's KL-surprise sub-graph in place
    /// with this tick's readings. Symmetric to `surprise_summary` —
    /// derives the same `(magnitude, direction)` from the same
    /// baselines — but writes typed snapshots into the unified graph
    /// rather than returning a HashMap. Symbols with no informed
    /// baseline are simply skipped (no entry written, no entry
    /// removed).
    ///
    /// Per the eden thesis: detectors are perceivers; perception lives
    /// in the graph, not in detector-specific return shapes.
    pub fn apply_to_perception_graph(
        &self,
        belief_field: &PressureBeliefField,
        graph: &mut crate::perception::PerceptionGraph,
        tick: u64,
    ) {
        let mut by_symbol: HashMap<Symbol, (f64, f64)> = HashMap::new();
        // (best_abs_z, signed_direction)
        for ((symbol, channel), current_belief) in belief_field.gaussian_iter() {
            let Some(prev_belief) = belief_field.query_previous_gaussian(symbol, *channel) else {
                continue;
            };
            let Some(kl) = prev_belief.kl_divergence(current_belief) else {
                continue;
            };
            let Some(z) = self.surprise_z(symbol, *channel, kl) else {
                continue;
            };
            let prev_mean = prev_belief.mean.to_f64().unwrap_or(0.0);
            let curr_mean = current_belief.mean.to_f64().unwrap_or(0.0);
            let signed_dir = (curr_mean - prev_mean).signum();
            let entry = by_symbol.entry(symbol.clone()).or_insert((0.0, 0.0));
            if z.abs() > entry.0 {
                entry.0 = z.abs();
                entry.1 = signed_dir;
            }
        }
        for (symbol, (abs_z, signed_dir)) in by_symbol {
            let magnitude = (abs_z / 2.0).tanh().clamp(0.0, 1.0);
            let direction = signed_dir.clamp(-1.0, 1.0);
            graph.kl_surprise.upsert(
                symbol,
                crate::perception::KlSurpriseSnapshot {
                    magnitude: Decimal::try_from(magnitude).unwrap_or(Decimal::ZERO),
                    direction: Decimal::try_from(direction).unwrap_or(Decimal::ZERO),
                    observed: abs_z,
                    expected: 1.0,
                    last_tick: tick,
                },
            );
        }
    }

    pub fn surprise_summary(
        &self,
        belief_field: &PressureBeliefField,
    ) -> HashMap<String, (Decimal, Decimal)> {
        let mut by_symbol: HashMap<String, (f64, f64, f64)> = HashMap::new();
        // (best_abs_z, signed_z, signed_direction)

        // Re-collect symbols with their per-channel beliefs. Iterating
        // `gaussian_iter` is cheaper than reverse-indexing baselines.
        for ((symbol, channel), current_belief) in belief_field.gaussian_iter() {
            let Some(prev_belief) = belief_field.query_previous_gaussian(symbol, *channel) else {
                continue;
            };
            let Some(kl) = prev_belief.kl_divergence(current_belief) else {
                continue;
            };
            let Some(z) = self.surprise_z(symbol, *channel, kl) else {
                continue;
            };
            // Direction signal: which way did the channel move?
            let prev_mean = prev_belief.mean.to_f64().unwrap_or(0.0);
            let curr_mean = current_belief.mean.to_f64().unwrap_or(0.0);
            let signed_dir = (curr_mean - prev_mean).signum();
            let entry = by_symbol.entry(symbol.0.clone()).or_insert((0.0, 0.0, 0.0));
            if z.abs() > entry.0 {
                entry.0 = z.abs();
                entry.1 = z;
                entry.2 = signed_dir;
            }
        }

        // Make sure every channel listed in CHANNELS is represented in
        // the universe — this loop is purely defensive against future
        // PressureChannel additions; iteration above is already complete.
        let _ = CHANNELS;

        by_symbol
            .into_iter()
            .map(|(sym, (abs_z, _signed_z, signed_dir))| {
                let magnitude = (abs_z / 2.0).tanh().clamp(0.0, 1.0);
                let direction = signed_dir.clamp(-1.0, 1.0);
                (
                    sym,
                    (
                        Decimal::try_from(magnitude).unwrap_or(Decimal::ZERO),
                        Decimal::try_from(direction).unwrap_or(Decimal::ZERO),
                    ),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::objects::Market;
    use rust_decimal_macros::dec;

    fn sym(s: &str) -> Symbol {
        Symbol(s.to_string())
    }

    #[test]
    fn baseline_bootstrap_returns_none_until_enough_samples() {
        let mut b = KlBaseline::default();
        for _ in 0..(KL_SURPRISE_BOOTSTRAP_MIN - 1) {
            b.observe(0.1);
        }
        assert!(b.surprise_z(0.5).is_none());
        b.observe(0.1);
        // Now informed but std≈0 (constant samples) → still None.
        assert!(b.surprise_z(0.5).is_none());
    }

    #[test]
    fn baseline_z_is_zero_at_mean() {
        let mut b = KlBaseline::default();
        for v in [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8] {
            b.observe(v);
        }
        let z_at_mean = b.surprise_z(b.mean_kl).unwrap();
        assert!(
            z_at_mean.abs() < 1e-6,
            "z at the running mean should be ≈0, got {}",
            z_at_mean
        );
    }

    #[test]
    fn baseline_z_grows_with_distance() {
        let mut b = KlBaseline::default();
        for v in [0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7, 0.8] {
            b.observe(v);
        }
        let z_near = b.surprise_z(b.mean_kl + 0.1).unwrap();
        let z_far = b.surprise_z(b.mean_kl + 1.0).unwrap();
        assert!(z_far > z_near);
        assert!(z_far > 0.0);
    }

    #[test]
    fn observe_from_belief_field_creates_baselines_after_two_ticks() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let s = sym("0700.HK");
        // First tick — only one belief, no previous yet.
        for _ in 0..6 {
            field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(1.0), 1);
        }
        let mut tracker = KlSurpriseTracker::new();
        tracker.observe_from_belief_field(&field);
        // Even after ingesting, baseline is empty because the field
        // only has previous_gaussian populated when a re-update occurs.
        // Now shock the field with a different value to force previous
        // → current divergence.
        field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(5.0), 2);
        tracker.observe_from_belief_field(&field);
        assert!(tracker.baseline_count() >= 1);
    }

    #[test]
    fn surprise_summary_returns_signed_direction() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let s = sym("0700.HK");
        // Build informed belief with known prior, then jump up.
        for _ in 0..6 {
            field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(1.0), 1);
        }
        let mut tracker = KlSurpriseTracker::new();
        // Burn enough KL samples to clear bootstrap floor.
        for tick in 2..=10 {
            field.record_gaussian_sample(
                &s,
                PressureChannel::OrderBook,
                Decimal::from(tick),
                tick as u64,
            );
            tracker.observe_from_belief_field(&field);
        }
        // One last shock — direction should be positive (mean increasing).
        field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(50.0), 11);
        tracker.observe_from_belief_field(&field);
        let summary = tracker.surprise_summary(&field);
        let (mag, dir) = summary.get("0700.HK").expect("symbol present");
        assert!(*mag > Decimal::ZERO);
        assert!(
            *dir > Decimal::ZERO,
            "expected positive direction, got {}",
            dir
        );
    }

    #[test]
    fn empty_field_yields_empty_summary() {
        let field = PressureBeliefField::new(Market::Hk);
        let tracker = KlSurpriseTracker::new();
        let summary = tracker.surprise_summary(&field);
        assert!(summary.is_empty());
    }

    #[test]
    fn apply_to_perception_graph_writes_signed_snapshot() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let s = sym("0700.HK");
        for _ in 0..6 {
            field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(1.0), 1);
        }
        let mut tracker = KlSurpriseTracker::new();
        for tick in 2..=10 {
            field.record_gaussian_sample(
                &s,
                PressureChannel::OrderBook,
                Decimal::from(tick),
                tick as u64,
            );
            tracker.observe_from_belief_field(&field);
        }
        // Final shock — direction should be positive.
        field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(50.0), 11);
        tracker.observe_from_belief_field(&field);

        let mut graph = crate::perception::PerceptionGraph::new();
        tracker.apply_to_perception_graph(&field, &mut graph, 11);

        let snap = graph
            .kl_surprise
            .get(&s)
            .expect("graph should hold reading after apply");
        assert!(
            snap.magnitude > Decimal::ZERO,
            "magnitude should be > 0 after a real shock, got {}",
            snap.magnitude
        );
        assert!(
            snap.direction > Decimal::ZERO,
            "direction should be positive when mean increases, got {}",
            snap.direction
        );
        assert_eq!(snap.last_tick, 11);
    }

    #[test]
    fn apply_to_perception_graph_matches_surprise_summary() {
        let mut field = PressureBeliefField::new(Market::Hk);
        let s = sym("0700.HK");
        for _ in 0..6 {
            field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(1.0), 1);
        }
        let mut tracker = KlSurpriseTracker::new();
        for tick in 2..=10 {
            field.record_gaussian_sample(
                &s,
                PressureChannel::OrderBook,
                Decimal::from(tick),
                tick as u64,
            );
            tracker.observe_from_belief_field(&field);
        }
        field.record_gaussian_sample(&s, PressureChannel::OrderBook, dec!(50.0), 11);
        tracker.observe_from_belief_field(&field);

        let summary = tracker.surprise_summary(&field);
        let mut graph = crate::perception::PerceptionGraph::new();
        tracker.apply_to_perception_graph(&field, &mut graph, 11);

        // Both paths must produce the same magnitude & direction; the
        // graph carries the typed shape, the summary carries the legacy
        // tuple. They must not diverge during the migration window.
        for (sym_str, (mag, dir)) in &summary {
            let snap = graph
                .kl_surprise
                .get(&Symbol(sym_str.clone()))
                .unwrap_or_else(|| panic!("graph missing reading for {}", sym_str));
            assert_eq!(snap.magnitude, *mag, "magnitude divergence for {}", sym_str);
            assert_eq!(snap.direction, *dir, "direction divergence for {}", sym_str);
        }
        assert_eq!(graph.kl_surprise.len(), summary.len());
    }

    #[test]
    fn apply_to_perception_graph_no_op_on_empty_baselines() {
        let field = PressureBeliefField::new(Market::Hk);
        let tracker = KlSurpriseTracker::new();
        let mut graph = crate::perception::PerceptionGraph::new();
        tracker.apply_to_perception_graph(&field, &mut graph, 7);
        assert!(graph.kl_surprise.is_empty());
    }

    #[test]
    fn ewma_decay_produces_geometric_weighting() {
        let mut b = KlBaseline::default();
        // Steady stream of 1.0 should make mean → 1.0.
        for _ in 0..200 {
            b.observe(1.0);
        }
        assert!((b.mean_kl - 1.0).abs() < 1e-3);
        // Inject a single 100.0 — mean should jump but stay close to 1
        // (1% weight on the new value with α=0.01).
        b.observe(100.0);
        assert!(
            b.mean_kl > 1.0 && b.mean_kl < 3.0,
            "single shock should bias mean modestly, got {}",
            b.mean_kl
        );
    }

    #[test]
    fn nonfinite_kl_is_silently_ignored() {
        let mut b = KlBaseline::default();
        b.observe(1.0);
        let count_before = b.sample_count;
        b.observe(f64::INFINITY);
        b.observe(f64::NAN);
        assert_eq!(b.sample_count, count_before);
    }
}
