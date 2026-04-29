//! [`SyncTickSubstrate`] — wraps the existing tick-batched BP path
//! behind the [`BeliefSubstrate`] trait. Pure refactor; behaviour is
//! bit-identical to the pre-Phase-B HK / US runtime BP block.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};

use chrono::Utc;

use crate::ontology::TacticalSetup;
use crate::pipeline::loopy_bp::{self, BpInputEdge, NodePrior};
use crate::pipeline::sub_kg_emergence;

use super::substrate::{BeliefSubstrate, PosteriorView};

/// Synchronous substrate. `observe_tick` runs BP synchronously and
/// stashes the resulting posterior in an `Arc<PosteriorView>` reachable
/// via `posterior_snapshot()`. Behaviour is bit-identical to today's
/// in-tick BP block.
pub struct SyncTickSubstrate {
    posterior: RwLock<Arc<PosteriorView>>,
    generation: AtomicU64,
}

impl SyncTickSubstrate {
    pub fn new() -> Self {
        Self {
            posterior: RwLock::new(Arc::new(PosteriorView::empty())),
            generation: AtomicU64::new(0),
        }
    }
}

impl Default for SyncTickSubstrate {
    fn default() -> Self {
        Self::new()
    }
}

impl BeliefSubstrate for SyncTickSubstrate {
    fn observe_tick(
        &self,
        priors: &HashMap<String, NodePrior>,
        edges: &[BpInputEdge],
        _tick: u64,
    ) {
        let bp_result = loopy_bp::run_with_messages(priors, edges);
        let generation = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let view = PosteriorView {
            beliefs: bp_result.beliefs,
            iterations: bp_result.iterations,
            converged: bp_result.converged,
            generation,
            last_updated: Utc::now(),
        };
        // RwLock write is fast here — there's exactly one observer
        // (the runtime tick loop) and readers never block long.
        let mut slot = self.posterior.write().expect("posterior RwLock poisoned");
        *slot = Arc::new(view);
    }

    fn posterior_snapshot(&self) -> Arc<PosteriorView> {
        self.posterior
            .read()
            .expect("posterior RwLock poisoned")
            .clone()
    }

    fn apply_posterior_confidence(&self, setups: &mut [TacticalSetup]) -> (usize, usize) {
        let view = self.posterior_snapshot();
        let mut applied = 0usize;
        let mut skipped = 0usize;
        for setup in setups.iter_mut() {
            if loopy_bp::apply_posterior_confidence(setup, &view.beliefs) {
                applied += 1;
            } else {
                skipped += 1;
            }
        }
        (applied, skipped)
    }

    fn reconcile_direction(&self, setups: &mut [TacticalSetup]) -> usize {
        let view = self.posterior_snapshot();
        sub_kg_emergence::reconcile_direction_with_bp(setups, &view.beliefs)
    }

    fn drain_pending(&self) {
        // No-op for sync substrate. observe_tick has already returned
        // by the time this is called, so there is nothing pending.
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::loopy_bp::{BpEdgeKind, MessagePassingResult};

    #[test]
    fn empty_substrate_reports_generation_zero() {
        let substrate = SyncTickSubstrate::new();
        let view = substrate.posterior_snapshot();
        assert_eq!(view.generation, 0);
        assert!(view.beliefs.is_empty());
        assert!(!view.converged);
    }

    #[test]
    fn observe_tick_increments_generation_and_publishes_beliefs() {
        let substrate = SyncTickSubstrate::new();
        let mut priors: HashMap<String, NodePrior> = HashMap::new();
        priors.insert("A".to_string(), NodePrior::default());
        priors.insert("B".to_string(), NodePrior::default());
        let edges = vec![BpInputEdge {
            from: "A".to_string(),
            to: "B".to_string(),
            weight: 0.5,
            kind: BpEdgeKind::StockToStock,
        }];
        substrate.observe_tick(&priors, &edges, 1);
        let view = substrate.posterior_snapshot();
        assert_eq!(view.generation, 1);
        assert!(view.beliefs.contains_key("A"));
        assert!(view.beliefs.contains_key("B"));
    }

    #[test]
    fn observe_tick_bit_matches_direct_loopy_bp_call() {
        // Substrate must produce posteriors bit-identical to a direct
        // call. Phase B is pure refactor; any divergence breaks the
        // contract that callers can replace the inline BP block with
        // substrate.observe_tick + substrate.apply_* without behaviour
        // change.
        let mut priors: HashMap<String, NodePrior> = HashMap::new();
        for i in 0..6 {
            priors.insert(format!("N{i}"), NodePrior::default());
        }
        let edges: Vec<BpInputEdge> = (0..6)
            .flat_map(|i| {
                ((i + 1)..6).map(move |j| BpInputEdge {
                    from: format!("N{i}"),
                    to: format!("N{j}"),
                    weight: 0.4,
                    kind: BpEdgeKind::StockToStock,
                })
            })
            .collect();
        let MessagePassingResult { beliefs: direct, .. } =
            loopy_bp::run_with_messages(&priors, &edges);
        let substrate = SyncTickSubstrate::new();
        substrate.observe_tick(&priors, &edges, 1);
        let view = substrate.posterior_snapshot();
        for (sym, direct_belief) in &direct {
            let via_substrate = view
                .beliefs
                .get(sym)
                .expect("substrate posterior missing symbol");
            for i in 0..3 {
                assert!(
                    (direct_belief[i] - via_substrate[i]).abs() < 1e-12,
                    "{sym}[{i}] direct={} substrate={}",
                    direct_belief[i],
                    via_substrate[i]
                );
            }
        }
    }
}
