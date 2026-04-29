//! Broker-level belief — ontology re-enters reasoning.
//!
//! All belief layers shipped 2026-04-19 morning/afternoon were
//! per-symbol HashMaps: channel beliefs, intent beliefs, decision
//! summaries. None of them queried `ObjectStore`'s Broker/Institution
//! entities — KG and ontology became passive scaffolding.
//!
//! This module restores an ontology-entity-level belief: per-Broker
//! `CategoricalBelief<BrokerArchetype>`. Eden now maintains a posterior
//! about each broker's *behavioral role* — accumulative, distributive,
//! arbitrage, algo, or unknown — derived from raw broker-queue
//! presence, not from aggregated pressure.
//!
//! Because broker identity is ontology-level (same desk across all
//! symbols they touch), this posterior is **cross-symbol**: when
//! `4828 UBS` appears on 0700.HK's bid and 3690.HK's bid, both are
//! evidence for the same accumulative posterior. This is the
//! graph-aware belief propagation the session had been missing.

use std::collections::HashMap;

use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use crate::ontology::objects::{BrokerId, Market};
use crate::pipeline::belief::CategoricalBelief;
use crate::pipeline::raw_expectation::{BrokerPresenceEntry, RawBrokerPresence};

/// Behavioral role Eden infers from raw broker-queue behavior.
/// Deliberately small: these are the meaningful categories an operator
/// would discriminate between when reading HK queue microstructure.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BrokerArchetype {
    /// Consistently present on the bid side — accumulating inventory.
    Accumulative,
    /// Consistently present on the ask side — distributing.
    Distributive,
    /// Present on both bid and ask — liquidity provision or arbitrage.
    Arbitrage,
    /// Short high-turnover bursts — algorithmic flow.
    Algo,
    /// Insufficient data or ambiguous pattern.
    Unknown,
}

pub const BROKER_ARCHETYPE_VARIANTS: &[BrokerArchetype] = &[
    BrokerArchetype::Accumulative,
    BrokerArchetype::Distributive,
    BrokerArchetype::Arbitrage,
    BrokerArchetype::Algo,
    BrokerArchetype::Unknown,
];

/// Threshold ratios. Bid-dominant ≥ 0.70 → Accumulative;
/// ask-dominant ≥ 0.70 → Distributive; both bid_ratio and
/// ask_ratio ≥ 0.30 (and sum ≥ 0.80) → Arbitrage. Short streaks +
/// high flip rate → Algo. Otherwise Unknown.
const SIDE_DOMINANCE_THRESHOLD: f64 = 0.70;
const ARBITRAGE_BOTH_SIDE_MIN: f64 = 0.30;
/// Flip rate threshold for Algo classification: number of
/// presence→absence or absence→presence transitions per window.
const ALGO_FLIP_RATE_MIN: f64 = 0.30;
/// Minimum observations on a side before we treat the ratio as
/// meaningful. Entries shorter than this produce Unknown evidence.
const MIN_SIDE_OBSERVATIONS: usize = 4;

/// Classify one tick's observation of a broker into one archetype
/// evidence vote. Consumes both sides of the broker's presence history
/// and picks the single most-likely archetype label.
pub fn classify_broker_tick(
    bid: Option<&BrokerPresenceEntry>,
    ask: Option<&BrokerPresenceEntry>,
) -> BrokerArchetype {
    let bid_obs = bid.map(|e| e.values.len()).unwrap_or(0);
    let ask_obs = ask.map(|e| e.values.len()).unwrap_or(0);
    if bid_obs + ask_obs < MIN_SIDE_OBSERVATIONS {
        return BrokerArchetype::Unknown;
    }
    let bid_ratio = bid
        .map(|e| {
            if e.values.is_empty() {
                0.0
            } else {
                e.count_present() as f64 / e.values.len() as f64
            }
        })
        .unwrap_or(0.0);
    let ask_ratio = ask
        .map(|e| {
            if e.values.is_empty() {
                0.0
            } else {
                e.count_present() as f64 / e.values.len() as f64
            }
        })
        .unwrap_or(0.0);

    // Arbitrage check first: both sides above min_threshold.
    if bid_ratio >= ARBITRAGE_BOTH_SIDE_MIN
        && ask_ratio >= ARBITRAGE_BOTH_SIDE_MIN
        && bid_ratio + ask_ratio >= 0.80
    {
        return BrokerArchetype::Arbitrage;
    }

    // Dominant side check.
    if bid_ratio >= SIDE_DOMINANCE_THRESHOLD && ask_ratio < ARBITRAGE_BOTH_SIDE_MIN {
        return BrokerArchetype::Accumulative;
    }
    if ask_ratio >= SIDE_DOMINANCE_THRESHOLD && bid_ratio < ARBITRAGE_BOTH_SIDE_MIN {
        return BrokerArchetype::Distributive;
    }

    // Algo check: high flip rate on either side.
    let bid_flip = bid.map(flip_rate).unwrap_or(0.0);
    let ask_flip = ask.map(flip_rate).unwrap_or(0.0);
    if bid_flip >= ALGO_FLIP_RATE_MIN || ask_flip >= ALGO_FLIP_RATE_MIN {
        return BrokerArchetype::Algo;
    }

    BrokerArchetype::Unknown
}

fn flip_rate(entry: &BrokerPresenceEntry) -> f64 {
    let n = entry.values.len();
    if n < 2 {
        return 0.0;
    }
    let flips: usize = entry
        .values
        .iter()
        .zip(entry.values.iter().skip(1))
        .filter(|(a, b)| a != b)
        .count();
    flips as f64 / (n - 1) as f64
}

/// Per-market broker archetype belief field. Each broker gets one
/// CategoricalBelief<BrokerArchetype> aggregated across every symbol
/// the broker touches.
pub struct BrokerArchetypeBeliefField {
    market: Market,
    per_broker: HashMap<BrokerId, CategoricalBelief<BrokerArchetype>>,
}

impl BrokerArchetypeBeliefField {
    pub fn new(market: Market) -> Self {
        Self {
            market,
            per_broker: HashMap::new(),
        }
    }

    pub fn market(&self) -> Market {
        self.market
    }

    pub fn known_brokers(&self) -> usize {
        self.per_broker.len()
    }

    fn fresh_belief() -> CategoricalBelief<BrokerArchetype> {
        CategoricalBelief::uniform(BROKER_ARCHETYPE_VARIANTS.to_vec())
    }

    /// Ingest one tick of RawBrokerPresence. For every broker that has
    /// any presence on any symbol, classify their current state and
    /// push one observation to their belief. Aggregates broker activity
    /// across all symbols (broker identity is cross-symbol).
    pub fn observe_tick(&mut self, presence: &RawBrokerPresence) {
        // Gather unique broker_ids observed this tick, pairing with
        // the bid+ask entries the broker has on any symbol. For
        // classification we take the *max* bid/ask presence entry
        // across symbols — a broker that's accumulating on one
        // symbol is a strong accumulator, even if silent on others.
        let mut best_bid: HashMap<i32, &BrokerPresenceEntry> = HashMap::new();
        let mut best_ask: HashMap<i32, &BrokerPresenceEntry> = HashMap::new();
        for (_symbol, per_sym) in &presence.per_symbol {
            for (broker_id, entry) in &per_sym.bid {
                let existing = best_bid.get(broker_id);
                if existing
                    .map(|e| e.count_present() < entry.count_present())
                    .unwrap_or(true)
                {
                    best_bid.insert(*broker_id, entry);
                }
            }
            for (broker_id, entry) in &per_sym.ask {
                let existing = best_ask.get(broker_id);
                if existing
                    .map(|e| e.count_present() < entry.count_present())
                    .unwrap_or(true)
                {
                    best_ask.insert(*broker_id, entry);
                }
            }
        }

        let mut all_brokers: std::collections::HashSet<i32> = std::collections::HashSet::new();
        all_brokers.extend(best_bid.keys().copied());
        all_brokers.extend(best_ask.keys().copied());

        for broker_id in all_brokers {
            let bid_entry = best_bid.get(&broker_id).copied();
            let ask_entry = best_ask.get(&broker_id).copied();
            let archetype = classify_broker_tick(bid_entry, ask_entry);
            let belief = self
                .per_broker
                .entry(BrokerId(broker_id))
                .or_insert_with(Self::fresh_belief);
            belief.update(&archetype);
        }
    }

    pub fn query(&self, broker_id: BrokerId) -> Option<&CategoricalBelief<BrokerArchetype>> {
        self.per_broker.get(&broker_id)
    }

    pub fn dominant_archetype(&self, broker_id: BrokerId) -> Option<(BrokerArchetype, f64)> {
        let belief = self.per_broker.get(&broker_id)?;
        best_in(belief)
    }

    /// Top-K brokers by archetype confidence — brokers where one
    /// archetype has dominated the belief (≥ min_dominance) with ≥
    /// min_samples observations.
    pub fn top_confident_archetypes(
        &self,
        k: usize,
        min_samples: u32,
        min_dominance: f64,
    ) -> Vec<BrokerArchetypeVerdict> {
        let mut out: Vec<BrokerArchetypeVerdict> = self
            .per_broker
            .iter()
            .filter(|(_, b)| b.sample_count >= min_samples)
            .filter_map(|(broker_id, b)| {
                let (archetype, prob) = best_in(b)?;
                if prob < min_dominance {
                    return None;
                }
                Some(BrokerArchetypeVerdict {
                    broker_id: *broker_id,
                    archetype,
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

    pub fn per_broker_iter(
        &self,
    ) -> impl Iterator<Item = (&BrokerId, &CategoricalBelief<BrokerArchetype>)> {
        self.per_broker.iter()
    }

    /// Raw-insert for cross-session restore. Replaces any existing
    /// belief for the broker.
    pub fn insert_raw(&mut self, broker_id: BrokerId, belief: CategoricalBelief<BrokerArchetype>) {
        self.per_broker.insert(broker_id, belief);
    }

    /// Backward-pass sample push: outcome-confirmed observation that
    /// this broker's recent behaviour fits `archetype`. Used by
    /// broker_outcome_feedback on winning setups. One sample per call.
    pub fn observe_outcome_archetype(&mut self, broker_id: BrokerId, archetype: BrokerArchetype) {
        let belief = self
            .per_broker
            .entry(broker_id)
            .or_insert_with(Self::fresh_belief);
        belief.update(&archetype);
    }
}

fn best_in(belief: &CategoricalBelief<BrokerArchetype>) -> Option<(BrokerArchetype, f64)> {
    let mut best: Option<(BrokerArchetype, f64)> = None;
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
pub struct BrokerArchetypeVerdict {
    pub broker_id: BrokerId,
    pub archetype: BrokerArchetype,
    pub probability: f64,
    pub sample_count: u32,
}

fn archetype_name(a: BrokerArchetype) -> &'static str {
    match a {
        BrokerArchetype::Accumulative => "accumulative",
        BrokerArchetype::Distributive => "distributive",
        BrokerArchetype::Arbitrage => "arbitrage",
        BrokerArchetype::Algo => "algo",
        BrokerArchetype::Unknown => "unknown",
    }
}

pub fn format_broker_archetype_line(v: &BrokerArchetypeVerdict) -> String {
    format!(
        "broker_archetype: {} {} {:.2} (n={})",
        v.broker_id.0,
        archetype_name(v.archetype),
        v.probability,
        v.sample_count,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry_with_values(values: Vec<bool>) -> BrokerPresenceEntry {
        let mut e = BrokerPresenceEntry::default();
        for v in values {
            e.push(v);
        }
        e
    }

    #[test]
    fn dominant_bid_classifies_as_accumulative() {
        let bid = entry_with_values(vec![true, true, true, true, true, true, true, false]);
        let ask = entry_with_values(vec![false, false, false, false]);
        assert_eq!(
            classify_broker_tick(Some(&bid), Some(&ask)),
            BrokerArchetype::Accumulative
        );
    }

    #[test]
    fn dominant_ask_classifies_as_distributive() {
        let bid = entry_with_values(vec![false; 5]);
        let ask = entry_with_values(vec![true, true, true, true, true, true, true, false]);
        assert_eq!(
            classify_broker_tick(Some(&bid), Some(&ask)),
            BrokerArchetype::Distributive
        );
    }

    #[test]
    fn both_sides_active_classifies_as_arbitrage() {
        let bid = entry_with_values(vec![true, true, false, true, false, true]);
        let ask = entry_with_values(vec![true, false, true, true, false, true]);
        assert_eq!(
            classify_broker_tick(Some(&bid), Some(&ask)),
            BrokerArchetype::Arbitrage
        );
    }

    #[test]
    fn very_short_history_is_unknown() {
        let bid = entry_with_values(vec![true, true]);
        assert_eq!(
            classify_broker_tick(Some(&bid), None),
            BrokerArchetype::Unknown
        );
    }

    #[test]
    fn high_flip_rate_classifies_as_algo() {
        let bid = entry_with_values(vec![true, false, true, false, true, false, true, false]);
        let ask = entry_with_values(vec![false; 8]);
        assert_eq!(
            classify_broker_tick(Some(&bid), Some(&ask)),
            BrokerArchetype::Algo
        );
    }

    #[test]
    fn field_accumulates_posterior_over_ticks() {
        let mut field = BrokerArchetypeBeliefField::new(Market::Hk);
        // Simulate 10 ticks of RawBrokerPresence where broker 4828 is
        // consistently on the bid side of 0700.HK.
        let mut presence = RawBrokerPresence::default();
        // Build a PerSymbolBrokerPresence with 8 bid-true observations.
        for _ in 0..10 {
            let bid_entry = presence
                .per_symbol
                .entry(crate::ontology::objects::Symbol("0700.HK".into()))
                .or_default()
                .bid
                .entry(4828)
                .or_default();
            bid_entry.push(true);
            field.observe_tick(&presence);
        }

        let (arch, prob) = field.dominant_archetype(BrokerId(4828)).unwrap();
        // After 10 ticks with sustained bid presence, broker should
        // tilt toward Accumulative.
        // Note: first few ticks are Unknown because of
        // MIN_SIDE_OBSERVATIONS=4. That's fine — by tick 10 the
        // posterior should have Accumulative as dominant.
        assert_eq!(
            arch,
            BrokerArchetype::Accumulative,
            "expected Accumulative, got {:?}",
            arch
        );
        assert!(prob > 0.35, "prob={}", prob);
    }

    #[test]
    fn top_confident_respects_dominance_threshold() {
        let mut field = BrokerArchetypeBeliefField::new(Market::Hk);
        let mut presence = RawBrokerPresence::default();
        // Strong broker: 20 consecutive bid observations.
        for _ in 0..20 {
            presence
                .per_symbol
                .entry(crate::ontology::objects::Symbol("S1.HK".into()))
                .or_default()
                .bid
                .entry(111)
                .or_default()
                .push(true);
            field.observe_tick(&presence);
        }
        // Weak broker: 2 observations only.
        for _ in 0..2 {
            presence
                .per_symbol
                .entry(crate::ontology::objects::Symbol("S2.HK".into()))
                .or_default()
                .bid
                .entry(222)
                .or_default()
                .push(true);
            field.observe_tick(&presence);
        }

        let top = field.top_confident_archetypes(5, 10, 0.3);
        assert!(top.iter().any(|v| v.broker_id.0 == 111));
        // Broker 222 either filtered by min_samples or by dominance.
        let broker_222 = top.iter().find(|v| v.broker_id.0 == 222);
        if let Some(v) = broker_222 {
            assert!(v.sample_count >= 10);
        }
    }

    #[test]
    fn format_line_shape_is_greppable() {
        let v = BrokerArchetypeVerdict {
            broker_id: BrokerId(4828),
            archetype: BrokerArchetype::Accumulative,
            probability: 0.73,
            sample_count: 120,
        };
        let line = format_broker_archetype_line(&v);
        assert_eq!(line, "broker_archetype: 4828 accumulative 0.73 (n=120)");
    }
}
