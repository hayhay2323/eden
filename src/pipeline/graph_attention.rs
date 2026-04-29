//! Graph-attention budget — centrality-driven compute prioritization.
//!
//! V5 first-principles fix for the Phase 1.5 throughput bottleneck.
//! Eden's emergence detection currently walks all 639 symbols every
//! tick, which under live load (HK open with O(80k) push events / tick)
//! exceeds runtime throughput and triggers `push_channel_full` overflow.
//!
//! Rather than handing-coding a "vol < N" filter (which adds a magic
//! threshold and violates first-principles design), this module
//! derives a per-symbol attention weight from the graph's *own*
//! topology — `centrality` — and lets that weight throttle compute.
//!
//! Centrality sources (all already produced by Eden, no new computation):
//!   - `HubSummary.total_degree` (anticorr + corr peer count)
//!   - peer presence (max_streak, mean_strength) — implicit in degree
//!
//! Symbols with high centrality (hubs that drive 17+ anticorr peers)
//! get processed every tick. Symbols with zero centrality (isolated
//! illiquid securities) get processed every `MAX_SKIP_TICKS`. The
//! gradient is linear in the inverse of normalized centrality:
//!
//!   skip_interval = ((1 - centrality_norm) * MAX_SKIP_TICKS) + 1
//!
//! `MAX_SKIP_TICKS = 100` is a memory-budget design choice — at HK's
//! ~5 sec/tick this means the longest-skip symbols still get observed
//! every ~8 minutes — not a behavioural threshold.

use std::collections::HashMap;

use crate::pipeline::residual::HubSummary;

/// Maximum number of ticks a low-centrality symbol can be skipped
/// before being processed again. Memory-budget design choice; at
/// 5 sec/tick this is ≈8 minutes for the most isolated symbols.
pub const MAX_SKIP_TICKS: u64 = 100;

/// Per-symbol last-processed tick tracker. The attention budget is
/// implicit: a symbol is "due" when current_tick - last_processed
/// >= skip_interval(centrality). Higher centrality ⇒ smaller interval
/// ⇒ processed more often.
#[derive(Debug, Default)]
pub struct AttentionBudget {
    last_processed: HashMap<String, u64>,
}

impl AttentionBudget {
    pub fn new() -> Self {
        Self::default()
    }

    /// Decide whether `symbol` should be processed at `current_tick`
    /// given its current `centrality` (in [0, 1]). Defaults to "process"
    /// if the symbol has never been seen.
    pub fn should_process(&self, symbol: &str, current_tick: u64, centrality: f64) -> bool {
        let centrality = centrality.clamp(0.0, 1.0);
        let interval = ((1.0 - centrality) * MAX_SKIP_TICKS as f64) as u64 + 1;
        let last = self.last_processed.get(symbol).copied().unwrap_or(0);
        if last == 0 {
            // Never processed — always allow.
            return true;
        }
        current_tick.saturating_sub(last) >= interval
    }

    /// Record that `symbol` was processed at `tick`. Caller must call
    /// this after running emergence detection on the symbol so the
    /// next decision uses the latest reference point.
    pub fn mark_processed(&mut self, symbol: &str, tick: u64) {
        self.last_processed.insert(symbol.to_string(), tick);
    }

    /// Diagnostic: number of symbols currently tracked.
    pub fn tracked_count(&self) -> usize {
        self.last_processed.len()
    }
}

/// Convert a slice of `HubSummary` into a per-symbol centrality map in
/// [0, 1]. Centrality = `total_degree / max_total_degree` across the
/// hub set. Symbols absent from the hub list get centrality 0
/// (isolated). The 0 → all-skipped behaviour is fine — emergence will
/// still run for every symbol every `MAX_SKIP_TICKS` ticks via the
/// "never processed" fallback in `should_process`.
pub fn centrality_from_hubs(hubs: &[HubSummary]) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    if hubs.is_empty() {
        return out;
    }
    let max_degree = hubs
        .iter()
        .map(|h| h.total_degree())
        .max()
        .unwrap_or(1)
        .max(1) as f64;
    for h in hubs {
        let c = h.total_degree() as f64 / max_degree;
        out.insert(h.symbol.0.clone(), c.clamp(0.0, 1.0));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::objects::Symbol;
    use rust_decimal::Decimal;

    fn hub(symbol: &str, anticorr: usize, corr: usize) -> HubSummary {
        HubSummary {
            symbol: Symbol(symbol.into()),
            anticorr_degree: anticorr,
            corr_degree: corr,
            peers: Vec::new(),
            max_streak: 0,
            mean_strength: Decimal::ZERO,
        }
    }

    #[test]
    fn first_call_always_processes() {
        let b = AttentionBudget::new();
        assert!(b.should_process("X.HK", 100, 0.0));
        assert!(b.should_process("X.HK", 100, 1.0));
    }

    #[test]
    fn high_centrality_processes_every_tick() {
        let mut b = AttentionBudget::new();
        b.mark_processed("HUB.HK", 10);
        // centrality = 1.0 → interval = 1 → next tick OK.
        assert!(b.should_process("HUB.HK", 11, 1.0));
        assert!(!b.should_process("HUB.HK", 10, 1.0));
    }

    #[test]
    fn zero_centrality_uses_max_skip() {
        let mut b = AttentionBudget::new();
        b.mark_processed("ISOLATED.HK", 10);
        // centrality = 0 → interval = MAX_SKIP_TICKS + 1.
        assert!(!b.should_process("ISOLATED.HK", 50, 0.0));
        assert!(b.should_process("ISOLATED.HK", 10 + MAX_SKIP_TICKS + 1, 0.0));
    }

    #[test]
    fn mid_centrality_proportional_skip() {
        let mut b = AttentionBudget::new();
        b.mark_processed("MID.HK", 10);
        // centrality = 0.5 → interval ≈ 51. 10 + 51 ≤ next tick.
        assert!(!b.should_process("MID.HK", 30, 0.5));
        assert!(b.should_process("MID.HK", 70, 0.5));
    }

    #[test]
    fn centrality_from_hubs_normalizes_to_unit_interval() {
        let hubs = vec![
            hub("BIG.HK", 32, 0),
            hub("MID.HK", 8, 0),
            hub("LOW.HK", 3, 1),
        ];
        let c = centrality_from_hubs(&hubs);
        assert!((c["BIG.HK"] - 1.0).abs() < 1e-9);
        assert!((c["MID.HK"] - 0.25).abs() < 1e-9);
        assert!((c["LOW.HK"] - 4.0 / 32.0).abs() < 1e-9);
    }

    #[test]
    fn empty_hubs_yields_empty_centrality() {
        let c = centrality_from_hubs(&[]);
        assert!(c.is_empty());
    }
}
