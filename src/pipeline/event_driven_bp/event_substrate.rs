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
//! Cutover (2026-04-29) deleted the sync + shadow substrates; this
//! module is now the only `BeliefSubstrate` impl. Quiescence semantics
//! are exposed via [`EventDrivenSubstrate::wait_until_quiescent`] —
//! runtime callers that need a converged posterior view (e.g. before
//! reading `posterior_snapshot()` for setup confidence application)
//! must invoke it between `observe_tick` and `posterior_snapshot`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use arc_swap::ArcSwap;
use chrono::Utc;
use dashmap::DashMap;

use crate::ontology::TacticalSetup;
use crate::pipeline::loopy_bp::{self, GraphEdge, NodePrior, N_STATES};
use crate::pipeline::sub_kg_emergence;

use super::node_state::NodeState;
use super::residual_queue::{EdgeUpdate, ResidualQueue};
use super::substrate::{BeliefSubstrate, PosteriorView};
use super::worker_pool::{residual, spawn_worker_pool, WorkerPoolHandle};

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
            let queue = Arc::clone(&queue);
            let pool_iterations = Arc::clone(&pool.iterations);
            let pool_idle = Arc::clone(&pool.idle);
            let pool_workers = pool.workers;
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
                    let iters = pool_iterations.load(Ordering::Relaxed);
                    let converged =
                        queue.is_empty() && pool_idle.load(Ordering::Relaxed) == pool_workers;
                    posterior.store(Arc::new(PosteriorView {
                        beliefs,
                        iterations: iters as usize,
                        converged,
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

    /// Cumulative count of `EdgeUpdate`s processed by workers since
    /// pool start. Diff against a prior snapshot to recover the
    /// per-tick iteration count (for logging / convergence diagnostics).
    pub fn iterations_total(&self) -> u64 {
        self.pool
            .as_ref()
            .map(|p| p.iterations.load(Ordering::Relaxed))
            .unwrap_or(0)
    }

    /// True when the residual queue is empty and every worker is idle.
    /// Cheap (two atomic loads). Use as the predicate inside
    /// [`Self::wait_until_quiescent`] or for ad-hoc readiness checks.
    pub fn is_quiescent(&self) -> bool {
        let workers = self.pool.as_ref().map(|p| p.workers).unwrap_or(0);
        let idle = self
            .pool
            .as_ref()
            .map(|p| p.idle.load(Ordering::Relaxed))
            .unwrap_or(0);
        self.queue.is_empty() && idle == workers
    }

    async fn wait_until_quiescent_impl(&self, budget: std::time::Duration) -> bool {
        let poll_interval = std::time::Duration::from_millis(1);
        let deadline = std::time::Instant::now() + budget;
        loop {
            if self.is_quiescent() {
                BeliefSubstrate::drain_pending(self);
                return true;
            }
            if std::time::Instant::now() >= deadline {
                BeliefSubstrate::drain_pending(self);
                return false;
            }
            tokio::time::sleep(poll_interval).await;
        }
    }

    /// Per-symbol prior update. Shared between `observe_tick` and
    /// `observe_symbol`. When `adj_neighbours` is `Some`, that vector
    /// is installed as the node's neighbour cache; when `None`, the
    /// existing cached neighbours are reused (event-driven path).
    /// Returns whether the prior actually changed.
    fn observe_symbol_inner(
        &self,
        symbol: &str,
        prior: NodePrior,
        adj_neighbours: Option<Vec<(String, f64)>>,
        force_reset: bool,
    ) -> bool {
        let entry = self
            .nodes
            .entry(symbol.to_string())
            .or_insert_with(|| Arc::new(NodeState::new()));
        let state = Arc::clone(entry.value());
        drop(entry);

        let mut lite = state.snapshot_lite();
        let prior_changed = lite.prior != prior.belief || lite.observed != prior.observed;
        lite.prior = prior.belief;
        lite.observed = prior.observed;
        if prior_changed || force_reset {
            lite.belief = prior.belief;
            if !lite.observed && lite.belief.iter().all(|v| v.abs() < 1e-9) {
                let uniform = 1.0 / N_STATES as f64;
                lite.belief = [uniform; N_STATES];
            }
        }
        state.store_lite(lite);

        let neighbours = match adj_neighbours.as_ref() {
            Some(ns) => ns.clone(),
            None => state.aux.lock().neighbours.clone(),
        };

        {
            let mut aux = state.aux.lock();
            if adj_neighbours.is_some() {
                aux.neighbours = neighbours.clone();
            }
            if prior_changed || force_reset {
                aux.clear_inbox();
            }
        }

        if prior_changed || force_reset {
            let belief = state.snapshot_lite().belief;
            for (k, weight) in &neighbours {
                let msg = compute_outgoing_message(&belief, *weight);
                self.queue.push(EdgeUpdate {
                    from: symbol.to_string(),
                    to: k.clone(),
                    message: msg,
                    residual: 1.0,
                });
            }
        }

        prior_changed || force_reset
    }
}

/// Build the worker process function. Each call: pop pulls an
/// `EdgeUpdate` from `from → to`; we look up `to`'s node, multiply
/// the new message into its belief, recompute outgoing messages to
/// `to`'s other neighbours, return them as new updates.
fn process_edge_update_fn(
    residual_threshold: f64,
    message_damping: f64,
) -> impl Fn(&Arc<DashMap<String, Arc<NodeState>>>, &EdgeUpdate) -> Vec<EdgeUpdate> + Send + Sync + 'static
{
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
    fn observe_tick(&self, priors: &HashMap<String, NodePrior>, edges: &[GraphEdge], _tick: u64) {
        // Build edge adjacency once and delegate per-symbol work to
        // observe_symbol_inner so observe_tick and observe_symbol share
        // identical seeding semantics.
        // Tick-sync parity: force_reset=true clears inboxes so each tick
        // starts from fresh priors without remnant cross-tick messages.
        let mut adj: HashMap<String, Vec<(String, f64)>> = HashMap::new();
        for edge in edges {
            adj.entry(edge.from.clone())
                .or_default()
                .push((edge.to.clone(), edge.weight));
            adj.entry(edge.to.clone())
                .or_default()
                .push((edge.from.clone(), edge.weight));
        }
        for (sym, prior) in priors {
            let neighbours = adj.get(sym).cloned().unwrap_or_default();
            self.observe_symbol_inner(sym, prior.clone(), Some(neighbours), true);
        }
    }

    fn observe_symbol(&self, symbol: &str, prior: NodePrior, neighbours: &[GraphEdge]) {
        let adj: Vec<(String, f64)> = neighbours
            .iter()
            .filter_map(|e| {
                if e.from == symbol {
                    Some((e.to.clone(), e.weight))
                } else if e.to == symbol {
                    Some((e.from.clone(), e.weight))
                } else {
                    None
                }
            })
            .collect();
        let pass = if adj.is_empty() { None } else { Some(adj) };
        self.observe_symbol_inner(symbol, prior, pass, false);
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

    fn wait_until_quiescent(
        &self,
        budget: std::time::Duration,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = bool> + Send + '_>> {
        Box::pin(self.wait_until_quiescent_impl(budget))
    }

    fn drain_pending(&self) {
        // Publish-only — never blocks. Refreshes the ArcSwap posterior
        // cache from current DashMap node states. For barrier semantics
        // (wait until queue is empty + workers idle, then publish), use
        // `wait_until_quiescent` instead — that method is the
        // production hook for tick-correct reads.
        let workers = self.pool.as_ref().map(|p| p.workers).unwrap_or(0);
        let idle_count = self
            .pool
            .as_ref()
            .map(|p| p.idle.load(Ordering::Relaxed))
            .unwrap_or(0);
        let iters = self
            .pool
            .as_ref()
            .map(|p| p.iterations.load(Ordering::Relaxed))
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
            iterations: iters as usize,
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

    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    async fn observe_symbol_propagates_to_cached_neighbours() {
        let substrate = EventDrivenSubstrate::new(EventConfig {
            workers: 2,
            residual_threshold: 1e-6,
            publish_interval_ms: 5,
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
        // Tick once to populate neighbour cache.
        substrate.observe_tick(&priors, &edges, 1);
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        substrate.drain_pending();
        let gen_before = substrate.posterior_snapshot().generation;

        // Observe A only — B should still receive a propagated message
        // because A's cached neighbour list still contains B.
        let a_prior = NodePrior {
            belief: [0.7, 0.2, 0.1],
            observed: true,
        };
        substrate.observe_symbol("A", a_prior, &[]);
        tokio::time::sleep(std::time::Duration::from_millis(40)).await;
        substrate.drain_pending();

        let view_after = substrate.posterior_snapshot();
        assert!(
            view_after.generation > gen_before,
            "generation must advance after observe_symbol (was {} now {})",
            gen_before,
            view_after.generation
        );
        let belief_b = view_after
            .beliefs
            .get("B")
            .expect("B must be present in posterior");
        assert!(
            belief_b[0] > 1.0 / 3.0 + 1e-6,
            "B's bull mass should rise after A propagates a bullish prior; got {:?}",
            belief_b
        );
    }
}
