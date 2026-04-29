//! [`EventDrivenSubstrate`] — async Residual BP impl of
//! [`BeliefSubstrate`].
//!
//! Phase C of the migration. Per-symbol state in `Arc<DashMap>`,
//! shared priority queue of pending message updates, 4–8 worker
//! tokio tasks draining the queue, posterior published via
//! `ArcSwap<PosteriorView>`.
//!
//! Convergence pattern: `observe_tick` seeds the queue with one
//! `EdgeUpdate` per (changed-prior, neighbour) pair. Workers pop the
//! highest-residual update, apply it to the destination node, compute
//! the resulting outgoing messages to that node's other neighbours,
//! and push them back. The queue empties when residuals fall below
//! `residual_threshold`.
//!
//! Phase D will run a [`ShadowSubstrate`] that delegates posterior
//! reads to the sync substrate while feeding both with the same
//! observations, emitting per-tick KL parity rows for cutover gating.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwap;
use chrono::Utc;
use dashmap::DashMap;

use crate::ontology::TacticalSetup;
use crate::pipeline::loopy_bp::{self, GraphEdge, NodePrior, N_STATES};
use crate::pipeline::sub_kg_emergence;

use super::node_state::{NodeAux, NodeState, NodeStateLite};
use super::residual_queue::{EdgeUpdate, ResidualQueue};
use super::substrate::{BeliefSubstrate, PosteriorView};
use super::worker_pool::{multiply_and_normalise, residual, spawn_worker_pool, WorkerPoolHandle};

/// Configuration for [`EventDrivenSubstrate`].
#[derive(Debug, Clone, Copy)]
pub struct EventConfig {
    /// Number of worker tokio tasks draining the residual queue.
    /// 4–8 typical; tunable. Plan agent's spike target = 6.
    pub workers: usize,
    /// Edge updates whose residual falls below this are dropped
    /// (no further propagation). 1e-3 matches `loopy_bp::CONVERGENCE_TOL`
    /// for parity with sync — too tight (e.g. 5e-3) drops mid-fixpoint
    /// updates and the posteriors diverge from sync's converged state.
    pub residual_threshold: f64,
    /// Cadence for the posterior publisher task (ms). 75 ms means
    /// operator queries see a posterior at most ~75 ms stale, vs the
    /// 5 s tick boundary on the sync substrate.
    pub publish_interval_ms: u64,
    /// Message damping for cyclic-graph stability. Mirrors
    /// `loopy_bp::MESSAGE_DAMPING`. Applied at inbox update so the
    /// destination's belief evolves as a damped average of incoming
    /// messages — same semantics as sync's per-iteration damp.
    pub message_damping: f64,
}

impl Default for EventConfig {
    fn default() -> Self {
        Self {
            // 8 workers for HK's ~31-degree master KG so a single
            // observation's fanout-30 seeded updates can drain in
            // parallel before the next tick lands.
            workers: 8,
            // 1e-4 = same as `loopy_bp::CONVERGENCE_TOL / 10` — let
            // smaller residuals propagate so the per-tick re-converge
            // catches up even though we carry history across ticks.
            residual_threshold: 1e-4,
            publish_interval_ms: 75,
            message_damping: 0.3,
        }
    }
}

/// Asynchronous Residual BP substrate.
///
/// Drop ends the worker pool and publisher task gracefully.
pub struct EventDrivenSubstrate {
    nodes: Arc<DashMap<String, Arc<NodeState>>>,
    queue: Arc<ResidualQueue>,
    posterior: Arc<ArcSwap<PosteriorView>>,
    pool: Option<WorkerPoolHandle>,
    publisher: Option<tokio::task::JoinHandle<()>>,
    publisher_shutdown: tokio::sync::watch::Sender<bool>,
    config: EventConfig,
    generation: Arc<AtomicU64>,
}

impl EventDrivenSubstrate {
    pub fn new(config: EventConfig) -> Self {
        let nodes: Arc<DashMap<String, Arc<NodeState>>> = Arc::new(DashMap::new());
        let queue = Arc::new(ResidualQueue::new());
        let posterior = Arc::new(ArcSwap::from_pointee(PosteriorView::empty()));
        let generation = Arc::new(AtomicU64::new(0));

        // Worker pool: process_fn is the message-update step.
        let pool = spawn_worker_pool(
            Arc::clone(&nodes),
            Arc::clone(&queue),
            config.workers,
            process_edge_update_fn(config.residual_threshold, config.message_damping),
        );

        // Publisher task: snapshot DashMap → ArcSwap on a timer.
        let (publisher_shutdown_tx, mut publisher_shutdown_rx) = tokio::sync::watch::channel(false);
        let publisher = {
            let nodes = Arc::clone(&nodes);
            let posterior = Arc::clone(&posterior);
            let generation = Arc::clone(&generation);
            let interval = std::time::Duration::from_millis(config.publish_interval_ms);
            tokio::spawn(async move {
                let mut ticker = tokio::time::interval(interval);
                loop {
                    tokio::select! {
                        _ = ticker.tick() => {}
                        _ = publisher_shutdown_rx.changed() => {
                            if *publisher_shutdown_rx.borrow() { break; }
                            continue;
                        }
                    }
                    let beliefs: HashMap<String, [f64; N_STATES]> = nodes
                        .iter()
                        .map(|kv| (kv.key().clone(), kv.value().lite.load().belief))
                        .collect();
                    let g = generation.fetch_add(1, Ordering::Relaxed) + 1;
                    posterior.store(Arc::new(PosteriorView {
                        beliefs,
                        iterations: 0,
                        converged: false,
                        generation: g,
                        last_updated: Utc::now(),
                    }));
                }
            })
        };

        Self {
            nodes,
            queue,
            posterior,
            pool: Some(pool),
            publisher: Some(publisher),
            publisher_shutdown: publisher_shutdown_tx,
            config,
            generation,
        }
    }
}

impl Default for EventDrivenSubstrate {
    fn default() -> Self {
        Self::new(EventConfig::default())
    }
}

impl EventDrivenSubstrate {
    /// Current depth of the pending residual queue. Production
    /// observability — operators watching for "is the substrate
    /// keeping up?" can chart this against tick rate. Cheap (single
    /// mutex). Not part of [`BeliefSubstrate`] because the trait
    /// describes belief access, not implementation health.
    pub fn pending_queue_len(&self) -> usize {
        self.queue.len()
    }

    /// Cumulative count of `EdgeUpdate`s shed by the queue's
    /// backpressure logic since startup. A monotonically non-zero
    /// value means workers fell behind under load and the queue
    /// dropped low-residual messages to stay bounded. Pair with
    /// `pending_queue_len()` for the full picture: queue length
    /// describes momentary load, drop count describes accumulated
    /// load shedding.
    pub fn dropped_message_count(&self) -> u64 {
        self.queue.dropped_count()
    }
}

/// Build the worker process function. Each call: pop pulls an
/// `EdgeUpdate` from `from → to`; we look up `to`'s node, multiply
/// the new message into its belief, recompute outgoing messages to
/// `to`'s other neighbours, return them as new updates.
fn process_edge_update_fn(
    residual_threshold: f64,
    message_damping: f64,
) -> impl Fn(&Arc<DashMap<String, Arc<NodeState>>>, &EdgeUpdate) -> Vec<EdgeUpdate>
       + Send
       + Sync
       + 'static {
    move |nodes, update| {
        // Look up destination node. If unknown, drop (graph topology
        // race or stale update post-shutdown).
        let to_state = match nodes.get(&update.to) {
            Some(state) => Arc::clone(state.value()),
            None => return Vec::new(),
        };

        // Update inbox: damp new message against the previous slot to
        // mirror sync substrate's per-iteration message damping.
        // m_new = damping * incoming + (1 - damping) * m_prev. First
        // arrival from a neighbour has no prev — just store as-is.
        {
            let mut aux = to_state.aux.lock();
            let mut damped = update.message;
            if let Some(slot) = aux.inbox.iter_mut().find(|(k, _)| k == &update.from) {
                let prev = slot.1;
                for i in 0..N_STATES {
                    damped[i] =
                        message_damping * update.message[i] + (1.0 - message_damping) * prev[i];
                }
                slot.1 = damped;
            } else {
                aux.inbox.push((update.from.clone(), damped));
            }
        }

        // Recompute belief = prior * Π(inbox messages); store and
        // measure residual against previous belief.
        let mut lite = to_state.snapshot_lite();
        let prev_belief = lite.belief;
        let mut new_belief = lite.prior;
        let aux = to_state.aux.lock();
        for (_, msg) in &aux.inbox {
            for i in 0..N_STATES {
                new_belief[i] *= msg[i];
            }
        }
        let neighbour_list = aux.neighbours.clone();
        drop(aux);
        let sum: f64 = new_belief.iter().sum();
        if sum < 1e-9 {
            let uniform = 1.0 / N_STATES as f64;
            new_belief = [uniform; N_STATES];
        } else {
            for v in new_belief.iter_mut() {
                *v /= sum;
            }
        }
        let r = residual(&new_belief, &prev_belief);
        lite.belief = new_belief;
        lite.last_residual = r;
        to_state.store_lite(lite);

        if r < residual_threshold {
            // Belief barely moved; don't propagate.
            return Vec::new();
        }

        // Build outgoing messages to each neighbour except the source.
        // Edge potential ratio is identical to sync `loopy_bp::edge_potential`
        // — keeps parity by construction.
        let mut outgoing = Vec::with_capacity(neighbour_list.len());
        for (k, weight) in neighbour_list {
            if k == update.from {
                continue;
            }
            let msg = compute_outgoing_message(&new_belief, weight);
            outgoing.push(EdgeUpdate {
                from: update.to.clone(),
                to: k,
                message: msg,
                residual: r,
            });
        }
        outgoing
    }
}

/// Compute the message a node sends to a neighbour given its current
/// belief and the edge weight. Uses the same edge potential function
/// as the synchronous BP path so converged posteriors match.
fn compute_outgoing_message(belief: &[f64; N_STATES], weight: f64) -> [f64; N_STATES] {
    let mut msg = [0.0; N_STATES];
    for x_to in 0..N_STATES {
        let mut s = 0.0;
        for x_from in 0..N_STATES {
            s += loopy_bp::edge_potential_value(weight, x_from, x_to) * belief[x_from];
        }
        msg[x_to] = s;
    }
    let sum: f64 = msg.iter().sum();
    if sum < 1e-9 {
        let uniform = 1.0 / N_STATES as f64;
        msg = [uniform; N_STATES];
    } else {
        for v in msg.iter_mut() {
            *v /= sum;
        }
    }
    msg
}

impl BeliefSubstrate for EventDrivenSubstrate {
    fn observe_tick(
        &self,
        priors: &HashMap<String, NodePrior>,
        edges: &[GraphEdge],
        _tick: u64,
    ) {
        // 1. Refresh node priors and rebuild neighbour lists from edges.
        // 2. For nodes whose prior changed, push initial outgoing
        //    messages to all neighbours into the queue.

        // Build edge adjacency once; reuse during seeding.
        let mut adj: HashMap<String, Vec<(String, f64)>> = HashMap::new();
        for edge in edges {
            adj.entry(edge.from.clone())
                .or_default()
                .push((edge.to.clone(), edge.weight));
            // Treat as bidirectional (matches sync path).
            adj.entry(edge.to.clone())
                .or_default()
                .push((edge.from.clone(), edge.weight));
        }

        for (sym, prior) in priors {
            let entry = self
                .nodes
                .entry(sym.clone())
                .or_insert_with(|| Arc::new(NodeState::new()));
            let state = Arc::clone(entry.value());
            drop(entry);

            // Update prior in lite snapshot.
            let mut lite = state.snapshot_lite();
            let prior_changed = lite.prior != prior.belief || lite.observed != prior.observed;
            lite.prior = prior.belief;
            lite.observed = prior.observed;
            if prior_changed {
                // Reset belief to the fresh prior. Without this the
                // belief carries the previous tick's posterior, and
                // the inbox-cleared re-seed below would damp new
                // messages against stale state.
                lite.belief = prior.belief;
                if !lite.observed && lite.belief.iter().all(|v| v.abs() < 1e-9) {
                    let uniform = 1.0 / N_STATES as f64;
                    lite.belief = [uniform; N_STATES];
                }
            }
            state.store_lite(lite);

            // Refresh neighbour list (cheap: same Vec built per tick;
            // can optimise later by caching adjacency hashes).
            let neighbours = adj.get(sym).cloned().unwrap_or_default();
            {
                let mut aux = state.aux.lock();
                aux.neighbours = neighbours.clone();
                if prior_changed {
                    // Clear stale messages from the previous tick's
                    // priors. Event-driven BP's per-tick fixpoint is
                    // recovered by re-seeding from the fresh prior
                    // below; carrying over inbox slots would damp
                    // new messages against obsolete remnants and
                    // produce a different fixed point than sync BP
                    // (observed in 2026-04-29 live HK shadow run:
                    // KL pass dropped 35 % → 14 % over 30 ticks).
                    aux.clear_inbox();
                }
            }

            if prior_changed {
                // Seed outgoing messages from this node to every neighbour.
                let belief = state.snapshot_lite().belief;
                for (k, weight) in &neighbours {
                    let msg = compute_outgoing_message(&belief, *weight);
                    self.queue.push(EdgeUpdate {
                        from: sym.clone(),
                        to: k.clone(),
                        message: msg,
                        residual: 1.0, // Force initial propagation.
                    });
                }
            }
        }
    }

    fn posterior_snapshot(&self) -> Arc<PosteriorView> {
        self.posterior.load_full()
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
        // Publish-only — never blocks. The whole point of event-driven
        // BP is that workers run continuously in the background and
        // posterior reads are wait-free. A blocking drain would force
        // sync-style barrier semantics on an async substrate, which is
        // exactly the architecture we replaced.
        //
        // What this DOES: refresh the ArcSwap posterior cache from
        // current DashMap node states. Cheap; lock-free per node via
        // arc-swap; reflects whatever progress workers have made by
        // the time of call.
        //
        // What it does NOT do: wait for the residual queue to drain or
        // for workers to go idle. Production reads (`posterior_snapshot`)
        // accept eventual consistency; tests that need a converged
        // snapshot should use `wait_until_quiescent` (test-only helper)
        // or push observations and yield in a tokio runtime.
        let workers = self.pool.as_ref().map(|p| p.workers).unwrap_or(0);
        let idle_count = self
            .pool
            .as_ref()
            .map(|p| p.idle.load(Ordering::Relaxed))
            .unwrap_or(0);
        let beliefs: HashMap<String, [f64; N_STATES]> = self
            .nodes
            .iter()
            .map(|kv| (kv.key().clone(), kv.value().lite.load().belief))
            .collect();
        let g = self.generation.fetch_add(1, Ordering::Relaxed) + 1;
        let converged = self.queue.is_empty() && idle_count == workers;
        self.posterior.store(Arc::new(PosteriorView {
            beliefs,
            iterations: 0,
            converged,
            generation: g,
            last_updated: Utc::now(),
        }));
    }
}

impl Drop for EventDrivenSubstrate {
    fn drop(&mut self) {
        if let Some(pool) = self.pool.take() {
            pool.shutdown();
        }
        let _ = self.publisher_shutdown.send(true);
        if let Some(handle) = self.publisher.take() {
            handle.abort();
        }
        self.queue.wake_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pipeline::loopy_bp::BpEdgeKind;

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn observe_then_drain_publishes_posterior() {
        let substrate = EventDrivenSubstrate::new(EventConfig {
            workers: 2,
            residual_threshold: 1e-6,
            publish_interval_ms: 10,
            message_damping: 0.3,
        });
        let mut priors = HashMap::new();
        priors.insert("A".to_string(), NodePrior::default());
        priors.insert("B".to_string(), NodePrior::default());
        let edges = vec![GraphEdge {
            from: "A".to_string(),
            to: "B".to_string(),
            weight: 0.5,
            kind: BpEdgeKind::StockToStock,
        }];
        substrate.observe_tick(&priors, &edges, 1);
        // Give workers a chance to drain.
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        substrate.drain_pending();
        let view = substrate.posterior_snapshot();
        assert!(view.beliefs.contains_key("A"));
        assert!(view.beliefs.contains_key("B"));
    }
}
