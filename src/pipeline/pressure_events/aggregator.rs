//! Per-symbol aggregator. When a channel's state changes, the
//! aggregator reads the latest values across all wired channels,
//! derives a sub-tick `NodePrior`, and calls
//! `BeliefSubstrate::observe_symbol` so the BP residual queue
//! propagates the change.
//!
//! Phase C1: only OrderBook + Structure are wired; the other 4
//! channels pass `None` and contribute zero direction signal.

use std::sync::Arc;

use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use tokio::sync::mpsc;

use crate::ontology::reasoning::TacticalDirection;
use crate::pipeline::event_driven_bp::BeliefSubstrate;
use crate::pipeline::loopy_bp::{self, STATE_BEAR, STATE_BULL};

use super::channel_state::SharedChannelStates;
use super::setup_registry::SharedSetupRegistry;

#[derive(Clone)]
pub struct AggregatorHandle {
    tx: mpsc::Sender<String>,
}

impl AggregatorHandle {
    /// Non-blocking notification that a channel's state has changed
    /// for this symbol. Drops the notification if the channel is full
    /// (the aggregator catches up on the next genuine change).
    pub fn notify_symbol_changed(&self, symbol: String) {
        let _ = self.tx.try_send(symbol);
    }
}

const NOTIFY_QUEUE_CAP: usize = 50_000;

/// Per-symbol cooldown between substrate.observe_symbol calls.
const OBSERVE_SYMBOL_COOLDOWN: std::time::Duration = std::time::Duration::from_millis(100);

/// Cadence for refreshing the Signature Replay (Memory) cache.
const REFRESH_MEMORY_EVERY: std::time::Duration = std::time::Duration::from_secs(5);

pub fn spawn_aggregator(
    states: SharedChannelStates,
    substrate: Arc<dyn BeliefSubstrate>,
    setup_registry: SharedSetupRegistry,
    perception_graph: Arc<std::sync::RwLock<crate::perception::PerceptionGraph>>,
    store: Arc<crate::ontology::store::ObjectStore>,
    market_str: String,
) -> AggregatorHandle {
    let (tx, mut rx) = mpsc::channel::<String>(NOTIFY_QUEUE_CAP);
    tokio::spawn(async move {
        let mut last_observed: std::collections::HashMap<String, std::time::Instant> =
            std::collections::HashMap::new();
        let mut observe_count: u64 = 0;
        
        // Sensory Memory Cache: Signature Replays
        let mut memory_cache: std::collections::HashMap<String, f64> = std::collections::HashMap::new();
        let mut last_memory_refresh = std::time::Instant::now() - REFRESH_MEMORY_EVERY;

        while let Some(symbol) = rx.recv().await {
            // 1. Periodically refresh Memory (Signature Replay) cache
            if last_memory_refresh.elapsed() >= REFRESH_MEMORY_EVERY {
                let replays = crate::pipeline::signature_replay::read_latest_signature_replays(
                    &market_str, 0, 1000
                );
                memory_cache.clear();
                for r in replays {
                    memory_cache.insert(r.symbol, r.mean_forward_belief_5tick);
                }
                last_memory_refresh = std::time::Instant::now();
            }

            // 2. Read all 6 channels
            let (ob_value, st_value) = {
                let map = states.orderbook.read();
                map.get(&symbol)
                    .map(|s| (s.orderbook_value, s.structure_value))
                    .unwrap_or((rust_decimal::Decimal::ZERO, rust_decimal::Decimal::ZERO))
            };
            let (cf_value, mo_value, vol_value) = {
                let map = states.tradeflow.read();
                map.get(&symbol)
                    .map(|s| (s.capital_flow_value, s.momentum_value, s.volume_value))
                    .unwrap_or((0.0, 0.0, 0.0))
            };
            let inst_value = {
                let map = states.broker.read();
                map.get(&symbol)
                    .map(|s| s.institutional_value)
                    .unwrap_or(rust_decimal::Decimal::ZERO)
            };
            let opt_value = {
                let map = states.option.read();
                map.get(&symbol)
                    .map(|s| s.option_value)
                    .unwrap_or(rust_decimal::Decimal::ZERO)
            };

            let ob_f = ob_value.to_f64().unwrap_or(0.0);
            let st_f = st_value.to_f64().unwrap_or(0.0);
            let inst_f = inst_value.to_f64().unwrap_or(0.0);
            let opt_f = opt_value.to_f64().unwrap_or(0.0);
            let mem_f = memory_cache.get(&symbol).copied().unwrap_or(0.0);

            // 3. Y-Archetype: Bottom-Up Energy Flux
            let channels = [
                ("OrderBook", ob_f),
                ("Structure", st_f),
                ("CapitalFlow", cf_value),
                ("Momentum", mo_value),
                ("Institutional", inst_f),
                ("Option", opt_f),
                ("Memory", mem_f),
            ];

            let mut total_flux = 0.0;
            let mut sum_signs = 0.0;
            let mut active_channels = Vec::new();
            let mut n_valid = 0;

            for (name, val) in channels {
                if val.abs() > 1e-6 {
                    total_flux += val.abs();
                    sum_signs += val.signum();
                    active_channels.push(name.to_string());
                    n_valid += 1;
                }
            }

            let coherence = if n_valid > 0 {
                sum_signs.abs() / n_valid as f64
            } else {
                0.0
            };

            // 4. Update the unified perception graph
            let mut collective_bonus = 0.0;
            {
                let mut graph = perception_graph.write().unwrap();
                let sym_obj = crate::ontology::objects::Symbol(symbol.clone());
                graph.sensory_flux.upsert(
                    sym_obj.clone(),
                    crate::perception::SensoryFluxSnapshot {
                        total_flux,
                        coherence,
                        active_channels,
                        last_tick: 0,
                    },
                );

                // Attention Gating: Only trigger expensive Ontological Projection
                // if the symbol's energy is significant (> 0.2). 
                if total_flux > 0.2 {
                    if let Some(sector_id) = store.stocks.get(&sym_obj).and_then(|s| s.sector_id.clone()) {
                        let sector_name = store.sectors.get(&sector_id).map(|s| s.name.clone()).unwrap_or_else(|| sector_id.0.clone());
                        
                        let mut sector_total_energy = 0.0;
                        let mut sector_sum_coherence = 0.0;
                        let mut sector_member_count = 0;
                        let mut leader_symbol = None;
                        let mut leader_energy = -1.0;

                        for stock in store.stocks_in_sector(&sector_id) {
                            if let Some(snap) = graph.sensory_flux.get(&stock.symbol) {
                                sector_total_energy += snap.total_flux;
                                sector_sum_coherence += snap.coherence;
                                sector_member_count += 1;
                                if snap.total_flux > leader_energy {
                                    leader_energy = snap.total_flux;
                                    leader_symbol = Some(stock.symbol.0.clone());
                                }
                            }
                        }

                        if sector_member_count > 0 {
                            let collective_coherence = sector_sum_coherence / sector_member_count as f64;
                            graph.thematic_flux.upsert(
                                sector_id.0.clone(),
                                crate::perception::ThematicFluxSnapshot {
                                    theme_id: sector_id.0.clone(),
                                    theme_name: sector_name,
                                    total_energy: sector_total_energy,
                                    collective_coherence,
                                    active_member_count: sector_member_count as u32,
                                    leader_symbol,
                                    last_tick: 0,
                                },
                            );

                            // 5. Y-Archetype: Top-Down Collective Feedback
                            if sector_total_energy > 2.0 && collective_coherence > 0.85 {
                                collective_bonus = 0.2; 
                            }
                        }
                    }
                }
            }

            // 6. BP Prior Calculation (incorporating Memory and Collective Feedback)
            let prior = {
                let graph = perception_graph.read().unwrap();
                loopy_bp::prior_from_pressure_channels(
                    Some(ob_f),
                    Some(cf_value),
                    Some(inst_f),
                    Some(mo_value),
                    Some(vol_value),
                    Some(st_f),
                    Some(opt_f),
                    Some(mem_f + (sum_signs.signum() * collective_bonus)),
                    Some(&graph.sensory_gain),
                )
            };

            if !prior.observed {
                continue;
            }
            if let Some(last) = last_observed.get(&symbol) {
                if last.elapsed() < OBSERVE_SYMBOL_COOLDOWN {
                    continue;
                }
            }
            last_observed.insert(symbol.clone(), std::time::Instant::now());
            let prior_snap = prior.clone();
            substrate.observe_symbol(&symbol, prior, &[]);
            observe_count = observe_count.wrapping_add(1);

            if let Some(setups) = setup_registry.get(&symbol) {
                let snap = substrate.posterior_snapshot();
                if let Some(post) = snap.beliefs.get(&symbol) {
                    for s in &setups {
                        let p_target = match s.direction {
                            TacticalDirection::Long => post[STATE_BULL],
                            TacticalDirection::Short => post[STATE_BEAR],
                        };
                        let _ = Decimal::try_from(p_target.clamp(0.0, 1.0));
                    }
                }
            }
            if observe_count == 1 || observe_count % 500 == 0 {
                let snap = substrate.posterior_snapshot();
                eprintln!(
                    "[pressure-agg] obs={} sym={} prior=[{:.3},{:.3},{:.3}] cf={:.2} mem={:.2} coll_bonus={:.2} gen={}",
                    observe_count,
                    symbol,
                    prior_snap.belief[0],
                    prior_snap.belief[1],
                    prior_snap.belief[2],
                    cf_value,
                    mem_f,
                    collective_bonus,
                    snap.generation,
                );
            }
        }
    });
    AggregatorHandle { tx }
}
