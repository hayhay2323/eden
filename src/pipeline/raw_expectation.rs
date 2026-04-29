//! Y#1 first pass — raw-level expectation layer.
//!
//! The Y#2 / Y#3 / Y#5 / Y#6 machinery all operates on **aggregated**
//! features: `support_fraction`, `peer_confirmation_ratio`, `inst_alignment_delta`
//! — values that have already been collapsed from raw broker / depth / trade
//! data by the pipeline. `feedback_hk_microstructure_first` is explicit that
//! HK's edge lives at the raw layer (broker identities, depth levels), not in
//! these aggregates. Until now Eden has been reading HK edge through a keyhole.
//!
//! This module records two raw observables per tick, per symbol, with no
//! aggregation into scalars:
//!
//! - **RawBrokerPresence** — boolean ring buffer of whether each
//!   `(symbol, broker_id, side)` triple was on the top-N broker queue at
//!   each recent tick. Enables "broker B4497 has sat on the bid for 8 of
//!   the last 10 ticks" or "B4497 just flipped from bid to ask" statements
//!   that the existing institutional_alignment scalar cannot reach.
//!
//! - **RawDepthLevels** — per-`(symbol, side, position)` volume ring buffer.
//!   A specific top-3 bid level whose volume fell 70% in one tick is either
//!   "being eaten" (trades matched it) or "being pulled" (order cancelled).
//!   The distinction is visible in the sequence of that level's volume
//!   versus trade volume at the same price — not in `top3_bid_pct`.
//!
//! Templates turn the current `PersistentStateKind` into concrete raw
//! predictions: "if 700.HK is Continuation, the top-3 bid brokers this tick
//! should include at least 2 of last tick's top-3 bid brokers" — a
//! statement about identity, not a threshold on an aggregate.
//!
//! Per the 2026-04-17 Y-70 / Gotham-style decision, this file hand-codes the
//! template set rather than learning it. Reliability of each template per
//! symbol can flow into the existing `EdgeLearningLedger` the same way
//! vortex outcomes do — "this template has 70% hit rate on 700.HK and 30%
//! on 981.HK" is a credit/debit fact about the symbol-template edge, not a
//! gradient update. No ML dependency.
//!
//! What this module is NOT:
//!   - Not a classifier. State classification stays in state_engine.
//!   - Not a replacement for institutional_alignment. The aggregates still
//!     drive classification; raw expectations add a parallel evidence track.
//!   - Not learned. Templates are written here and can be audited.

use longport::quote::{SecurityBrokers, SecurityDepth, Trade, TradeDirection};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use std::collections::{HashMap, VecDeque};

use crate::ontology::objects::Symbol;
use crate::pipeline::state_engine::{PersistentStateEvidence, PersistentStateKind};

/// Ring buffer depth for per-broker / per-level sequences. Matches
/// `MOMENTUM_HISTORY_LEN` (10 ticks) elsewhere in the codebase so the
/// velocity / acceleration machinery stays comparable.
const RAW_HISTORY_LEN: usize = 10;

/// Top-N brokers tracked per side. Longport broker groups are ordered by
/// `position` (1-indexed). We observe the first three positions because
/// that is the "front line" — broker activity beyond position 3 has much
/// weaker information content and doubling the tracked set would triple
/// memory without comparable signal.
const TOP_BROKER_POSITIONS: usize = 3;

/// Top-N depth levels tracked per side. Same rationale as TOP_BROKER_POSITIONS.
const TOP_DEPTH_LEVELS: usize = 3;

/// Side enum for broker / depth tracking. Kept tiny and `Copy` so it can
/// key into HashMaps cheaply.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BookSide {
    Bid,
    Ask,
}

impl BookSide {
    pub fn as_str(self) -> &'static str {
        match self {
            BookSide::Bid => "bid",
            BookSide::Ask => "ask",
        }
    }
}

/// One broker's presence history on a given (symbol, side) pair. The
/// `values` vector records, one entry per tick observed, whether this
/// broker was in the top-3 on that side.
#[derive(Debug, Clone, Default)]
pub struct BrokerPresenceEntry {
    pub values: VecDeque<bool>,
}

impl BrokerPresenceEntry {
    pub fn push(&mut self, present: bool) {
        self.values.push_back(present);
        while self.values.len() > RAW_HISTORY_LEN {
            self.values.pop_front();
        }
    }

    pub fn count_present(&self) -> usize {
        self.values.iter().filter(|v| **v).count()
    }

    /// "Consistent" means the broker has been present in at least two
    /// thirds of the observed ticks, matching the PEER_MAJORITY operational
    /// definition elsewhere in the codebase.
    pub fn is_consistent(&self) -> bool {
        if self.values.is_empty() {
            return false;
        }
        let present = self.count_present() as u32;
        let total = self.values.len() as u32;
        // `present * 3 >= total * 2` is >=2/3 without decimal arithmetic.
        present * 3 >= total * 2 && total >= 3
    }

    /// True when the broker has been absent for the last two ticks AND
    /// was present on at least one earlier tick in the window. Captures
    /// "this broker used to be here and just walked away".
    pub fn just_withdrew(&self) -> bool {
        let len = self.values.len();
        if len < 3 {
            return false;
        }
        let last_two_absent = !self.values[len - 1] && !self.values[len - 2];
        let had_prior_presence = self.values.iter().take(len.saturating_sub(2)).any(|v| *v);
        last_two_absent && had_prior_presence
    }
}

/// Volume series at a specific (symbol, side, position) depth level.
#[derive(Debug, Clone, Default)]
pub struct DepthLevelEntry {
    pub values: VecDeque<i64>, // volume per tick, newest last
}

impl DepthLevelEntry {
    pub fn push(&mut self, volume: i64) {
        self.values.push_back(volume);
        while self.values.len() > RAW_HISTORY_LEN {
            self.values.pop_front();
        }
    }

    pub fn latest(&self) -> i64 {
        self.values.back().copied().unwrap_or(0)
    }

    /// Ratio of the latest volume to the average of the previous ticks in
    /// the window. `1.0` means "unchanged"; `0.3` means "cut to 30% of
    /// recent average"; `1.5` means "50% thicker than recent".
    ///
    /// Returns `None` when there is insufficient history to form a
    /// meaningful comparison (need at least 3 prior values).
    pub fn relative_to_recent(&self) -> Option<Decimal> {
        let len = self.values.len();
        if len < 4 {
            return None;
        }
        let current = self.values[len - 1];
        let prior: Vec<i64> = self.values.iter().take(len - 1).copied().collect();
        if prior.is_empty() {
            return None;
        }
        let avg = prior.iter().sum::<i64>() / prior.len() as i64;
        if avg <= 0 {
            return None;
        }
        Some(Decimal::from(current) / Decimal::from(avg))
    }
}

/// Per-symbol, per-side raw broker presence tracker.
///
/// `observations[side][broker_id]` yields the sequence for one broker on
/// one side of one symbol. Keyed by side first so the most common read
/// ("who has been sitting on 700.HK's bid?") is a single HashMap lookup
/// followed by a filter.
#[derive(Debug, Clone, Default)]
pub struct RawBrokerPresence {
    pub per_symbol: HashMap<Symbol, PerSymbolBrokerPresence>,
}

#[derive(Debug, Clone, Default)]
pub struct PerSymbolBrokerPresence {
    pub bid: HashMap<i32, BrokerPresenceEntry>,
    pub ask: HashMap<i32, BrokerPresenceEntry>,
}

impl RawBrokerPresence {
    /// Ingest one tick of raw broker data for one symbol. The input
    /// `SecurityBrokers` is what `LiveState.brokers` already holds, so
    /// runtime wiring is a single call per symbol per tick with no
    /// aggregation step in between.
    pub fn record_tick(&mut self, symbol: &Symbol, brokers: &SecurityBrokers) {
        let entry = self.per_symbol.entry(symbol.clone()).or_default();
        record_side(&mut entry.bid, &brokers.bid_brokers);
        record_side(&mut entry.ask, &brokers.ask_brokers);
    }

    pub fn for_symbol(&self, symbol: &Symbol) -> Option<&PerSymbolBrokerPresence> {
        self.per_symbol.get(symbol)
    }
}

fn record_side(map: &mut HashMap<i32, BrokerPresenceEntry>, groups: &[longport::quote::Brokers]) {
    // Build the set of broker ids currently in the top-N positions.
    let mut current = std::collections::HashSet::<i32>::new();
    for group in groups
        .iter()
        .filter(|g| (g.position as usize) <= TOP_BROKER_POSITIONS)
    {
        for &broker_id in &group.broker_ids {
            current.insert(broker_id);
        }
    }
    // Push presence for each broker we already track, and a fresh entry
    // starting with "present" for brokers we see for the first time.
    let existing_ids: Vec<i32> = map.keys().copied().collect();
    for broker_id in &existing_ids {
        map.entry(*broker_id)
            .or_default()
            .push(current.contains(broker_id));
    }
    for broker_id in &current {
        if !existing_ids.contains(broker_id) {
            map.entry(*broker_id).or_default().push(true);
        }
    }
}

/// Per-symbol, per-side raw depth-level tracker. `levels[position]`
/// holds a rolling history of the volume at depth position `position`
/// (1-indexed, matches Longport's convention).
#[derive(Debug, Clone, Default)]
pub struct RawDepthLevels {
    pub per_symbol: HashMap<Symbol, PerSymbolDepthLevels>,
}

#[derive(Debug, Clone, Default)]
pub struct PerSymbolDepthLevels {
    pub bid: HashMap<usize, DepthLevelEntry>,
    pub ask: HashMap<usize, DepthLevelEntry>,
}

impl RawDepthLevels {
    pub fn record_tick(&mut self, symbol: &Symbol, depth: &SecurityDepth) {
        let entry = self.per_symbol.entry(symbol.clone()).or_default();
        record_depth_side(&mut entry.bid, &depth.bids);
        record_depth_side(&mut entry.ask, &depth.asks);
    }

    pub fn for_symbol(&self, symbol: &Symbol) -> Option<&PerSymbolDepthLevels> {
        self.per_symbol.get(symbol)
    }
}

fn record_depth_side(map: &mut HashMap<usize, DepthLevelEntry>, levels: &[longport::quote::Depth]) {
    // Only track the top TOP_DEPTH_LEVELS positions to bound memory; deeper
    // levels rarely carry information that isn't already visible at the top.
    for (idx, level) in levels.iter().take(TOP_DEPTH_LEVELS).enumerate() {
        let position = idx + 1;
        map.entry(position).or_default().push(level.volume);
    }
    // For positions we've seen before but aren't present this tick, push a
    // zero so their "absent" state is recorded as part of the series.
    let observed_positions: Vec<usize> = (1..=TOP_DEPTH_LEVELS).collect();
    for position in &observed_positions {
        if map.contains_key(position) && *position > levels.len() {
            map.entry(*position).or_default().push(0);
        }
    }
}

/// Recent trade window per symbol. Separate from pressure-derived
/// `buy_ratio` (which is an aggregate over seconds of trading) — here we
/// track the raw sequence of trades with their volume, direction, and
/// timestamp, so patterns like "3 large Up trades in a row" or "single
/// 10x average-size Down trade" remain visible at identity level.
///
/// T4 first pass: per-symbol ring buffer of the last 50 raw trades.
/// Size / count tuned small so the structure stays bounded even across
/// ~500 HK symbols without a persistence table.
const RAW_TRADE_HISTORY_LEN: usize = 50;

#[derive(Debug, Clone)]
pub struct RawTradeEntry {
    pub price: Decimal,
    pub volume: i64,
    pub direction: TradeDirection,
    pub timestamp: time::OffsetDateTime,
}

#[derive(Debug, Clone, Default)]
pub struct RawTradeTape {
    pub per_symbol: HashMap<Symbol, VecDeque<RawTradeEntry>>,
}

impl RawTradeTape {
    /// Ingest new trades for a symbol. The caller already holds the ring
    /// of all recent trades in `LiveState.trades`; the tracker merges new
    /// trades by timestamp to avoid double-counting when `LiveState` is
    /// polled multiple times per tick.
    pub fn record_tick(&mut self, symbol: &Symbol, trades: &[Trade]) {
        let entry = self.per_symbol.entry(symbol.clone()).or_default();
        let last_ts = entry.back().map(|t| t.timestamp);
        for trade in trades {
            if let Some(ts) = last_ts {
                if trade.timestamp <= ts {
                    continue;
                }
            }
            entry.push_back(RawTradeEntry {
                price: trade.price,
                volume: trade.volume,
                direction: trade.direction,
                timestamp: trade.timestamp,
            });
        }
        while entry.len() > RAW_TRADE_HISTORY_LEN {
            entry.pop_front();
        }
    }

    pub fn for_symbol(&self, symbol: &Symbol) -> Option<&VecDeque<RawTradeEntry>> {
        self.per_symbol.get(symbol)
    }
}

/// Helper — proportion of directional trades in a window that are `Up`.
/// Neutral trades are excluded from both numerator and denominator, so
/// the result reflects aggressor bias when aggression is happening,
/// rather than being diluted by neutral fills.
fn up_direction_share(trades: &VecDeque<RawTradeEntry>, take_last: usize) -> Option<Decimal> {
    let mut up = 0i64;
    let mut down = 0i64;
    for trade in trades.iter().rev().take(take_last) {
        match trade.direction {
            TradeDirection::Up => up += 1,
            TradeDirection::Down => down += 1,
            TradeDirection::Neutral => {}
        }
    }
    let total = up + down;
    if total == 0 {
        None
    } else {
        Some(Decimal::from(up) / Decimal::from(total))
    }
}

/// Detect block trades — single trades whose volume is materially larger
/// than the symbol's own recent average. Threshold at 3x because the
/// ordinary tick-size distribution tail is usually <2x; returning 3x+
/// outliers as distinct events lets templates key on institutional-size
/// prints without chasing every odd-lot spike.
fn block_trades<'a>(trades: &'a VecDeque<RawTradeEntry>) -> Vec<&'a RawTradeEntry> {
    let len = trades.len();
    if len < 10 {
        return Vec::new();
    }
    let avg: i64 = trades.iter().map(|t| t.volume).sum::<i64>() / len as i64;
    if avg <= 0 {
        return Vec::new();
    }
    trades
        .iter()
        .rev()
        .take(5)
        .filter(|t| t.volume >= avg * 3)
        .collect()
}

/// Evaluation output for a single (symbol, template) pair.
#[derive(Debug, Clone)]
pub struct RawExpectationOutcome {
    /// Evidence to feed into the state's supporting_evidence when the
    /// template was confirmed by raw data.
    pub supporting: Vec<PersistentStateEvidence>,
    /// Evidence for opposing_evidence when raw data refuted the template.
    pub opposing: Vec<PersistentStateEvidence>,
}

impl RawExpectationOutcome {
    fn empty() -> Self {
        Self {
            supporting: Vec::new(),
            opposing: Vec::new(),
        }
    }
}

/// Evaluate the hand-coded expectation template set for `state_kind` against
/// the raw broker / depth tracking for `symbol`. Returns evidence suitable
/// for splicing into the symbol's PersistentStateState evidence arrays.
///
/// Template set (first pass — expand as we learn from live data):
///   1. Continuation → at least one consistent top-3 bid broker (Y#3-ish but
///      read at identity level, not aggregated cohort ratio).
///   2. Continuation → no top-1 bid depth collapse (relative_to_recent >= 0.5).
///   3. TurningPoint → expect depth weakening on the previously-dominant side
///      (relative_to_recent < 0.7 on bid for a "was Continuation" symbol).
///   4. Continuation / Latent → flag broker withdrawal (a previously
///      consistent top-3 bid broker who has just disappeared).
///
/// Each evidence code is prefixed `raw:` so operators can distinguish
/// raw-layer readings from feature-layer evidence at a glance.
pub fn evaluate_raw_expectations(
    state_kind: PersistentStateKind,
    symbol: &Symbol,
    broker_presence: &RawBrokerPresence,
    depth_levels: &RawDepthLevels,
    trade_tape: &RawTradeTape,
) -> RawExpectationOutcome {
    let mut outcome = RawExpectationOutcome::empty();

    let broker_side = broker_presence.for_symbol(symbol);
    let depth_side = depth_levels.for_symbol(symbol);
    let trades = trade_tape.for_symbol(symbol);

    match state_kind {
        PersistentStateKind::Continuation => {
            // Template 1: consistent bid broker presence
            if let Some(pres) = broker_side {
                let consistent_count = pres
                    .bid
                    .values()
                    .filter(|entry| entry.is_consistent())
                    .count();
                if consistent_count >= 1 {
                    outcome.supporting.push(PersistentStateEvidence {
                        code: "raw:consistent_bid_broker".into(),
                        summary: format!(
                            "{} has {} consistent top-3 bid broker(s) across the last {} ticks",
                            symbol.0, consistent_count, RAW_HISTORY_LEN
                        ),
                        weight: dec!(0.18),
                    });
                } else {
                    outcome.opposing.push(PersistentStateEvidence {
                        code: "raw:no_consistent_bid_broker".into(),
                        summary: format!(
                            "{} has no broker that sat on its top-3 bid for >=2/3 of recent ticks",
                            symbol.0
                        ),
                        weight: dec!(0.14),
                    });
                }

                // Template 4: top-3 bid broker withdrawal
                let withdrew: Vec<i32> = pres
                    .bid
                    .iter()
                    .filter(|(_, entry)| entry.just_withdrew())
                    .map(|(id, _)| *id)
                    .collect();
                if !withdrew.is_empty() {
                    outcome.opposing.push(PersistentStateEvidence {
                        code: "raw:bid_broker_withdrew".into(),
                        summary: format!(
                            "{} lost {} previously-consistent top-3 bid broker(s) in the last 2 ticks",
                            symbol.0,
                            withdrew.len()
                        ),
                        weight: dec!(0.18),
                    });
                }
            }

            // T4 — trade tape: Continuation should show persistent
            // aggressor bias aligned with direction. We score the last
            // 20 trades on up_direction_share and also check for block
            // trades on the dominant side.
            if let Some(tape) = trades {
                if let Some(up_share) = up_direction_share(tape, 20) {
                    if up_share >= dec!(0.67) {
                        outcome.supporting.push(PersistentStateEvidence {
                            code: "raw:trade_aggressor_up".into(),
                            summary: format!(
                                "{} last-20 trade aggressor up={}",
                                symbol.0,
                                up_share.round_dp(2)
                            ),
                            weight: dec!(0.14),
                        });
                    } else if up_share <= dec!(0.33) {
                        outcome.supporting.push(PersistentStateEvidence {
                            code: "raw:trade_aggressor_down".into(),
                            summary: format!(
                                "{} last-20 trade aggressor down={}",
                                symbol.0,
                                (Decimal::ONE - up_share).round_dp(2)
                            ),
                            weight: dec!(0.14),
                        });
                    }
                }
                let blocks = block_trades(tape);
                if blocks.len() >= 2 {
                    let latest_dir = blocks[0].direction;
                    outcome.supporting.push(PersistentStateEvidence {
                        code: "raw:block_trade_cluster".into(),
                        summary: format!(
                            "{} saw {} block trade(s) in the last 5 prints (latest dir={:?})",
                            symbol.0,
                            blocks.len(),
                            latest_dir,
                        ),
                        weight: dec!(0.16),
                    });
                }
            }

            // Template 2: top-1 bid depth holding
            if let Some(depth) = depth_side {
                if let Some(entry) = depth.bid.get(&1) {
                    if let Some(ratio) = entry.relative_to_recent() {
                        if ratio < dec!(0.50) {
                            outcome.opposing.push(PersistentStateEvidence {
                                code: "raw:top_bid_collapse".into(),
                                summary: format!(
                                    "{} top-1 bid volume at {}x recent average",
                                    symbol.0,
                                    ratio.round_dp(2)
                                ),
                                weight: dec!(0.20),
                            });
                        } else if ratio >= dec!(1.15) {
                            outcome.supporting.push(PersistentStateEvidence {
                                code: "raw:top_bid_growing".into(),
                                summary: format!(
                                    "{} top-1 bid volume at {}x recent average",
                                    symbol.0,
                                    ratio.round_dp(2)
                                ),
                                weight: dec!(0.14),
                            });
                        }
                    }
                }
            }
        }
        PersistentStateKind::TurningPoint => {
            // Consistent top-3 bid broker identity also matters at turning
            // points: if someone is still absorbing flow while the state
            // turns, that broker signature is the highest-resolution read
            // we have. Emit the same `raw:consistent_bid_broker` code so
            // T22 inference chain has an anchor regardless of state kind.
            if let Some(pres) = broker_side {
                let consistent_count = pres
                    .bid
                    .values()
                    .filter(|entry| entry.is_consistent())
                    .count();
                if consistent_count >= 1 {
                    outcome.supporting.push(PersistentStateEvidence {
                        code: "raw:consistent_bid_broker".into(),
                        summary: format!(
                            "{} has {} consistent top-3 bid broker(s) across the last {} ticks",
                            symbol.0, consistent_count, RAW_HISTORY_LEN
                        ),
                        weight: dec!(0.14),
                    });
                }
            }

            // T4 — trade tape: TurningPoint should show aggressor
            // reversal. A balanced 0.4-0.6 share on last 20 trades means
            // the prior directional commitment has dissolved, which is
            // itself a confirming data point for turning-point state.
            if let Some(tape) = trades {
                if let Some(up_share) = up_direction_share(tape, 20) {
                    if up_share >= dec!(0.40) && up_share <= dec!(0.60) {
                        outcome.supporting.push(PersistentStateEvidence {
                            code: "raw:aggressor_balanced".into(),
                            summary: format!(
                                "{} last-20 aggressor balanced (up={})",
                                symbol.0,
                                up_share.round_dp(2)
                            ),
                            weight: dec!(0.12),
                        });
                    }
                }
            }

            // Template 3: expect depth weakening somewhere. If top-1 on
            // either side is thinning fast, that's a structural
            // confirmation of the turning-point call.
            if let Some(depth) = depth_side {
                let mut thinning_sides = Vec::new();
                if let Some(entry) = depth.bid.get(&1) {
                    if let Some(ratio) = entry.relative_to_recent() {
                        if ratio < dec!(0.70) {
                            thinning_sides.push(format!("bid:{}x", ratio.round_dp(2)));
                        }
                    }
                }
                if let Some(entry) = depth.ask.get(&1) {
                    if let Some(ratio) = entry.relative_to_recent() {
                        if ratio < dec!(0.70) {
                            thinning_sides.push(format!("ask:{}x", ratio.round_dp(2)));
                        }
                    }
                }
                if !thinning_sides.is_empty() {
                    outcome.supporting.push(PersistentStateEvidence {
                        code: "raw:turning_depth_thins".into(),
                        summary: format!(
                            "{} turning-point raw confirmation: {}",
                            symbol.0,
                            thinning_sides.join(", ")
                        ),
                        weight: dec!(0.16),
                    });
                }
            }
        }
        PersistentStateKind::Latent => {
            // Latent: look for a newly appearing broker on bid — a
            // cluster-formation signal.
            if let Some(pres) = broker_side {
                let fresh_count = pres
                    .bid
                    .values()
                    .filter(|entry| {
                        // Freshly appeared = present this tick and in only
                        // the most recent 1-2 ticks of the observed window.
                        let len = entry.values.len();
                        if len == 0 {
                            return false;
                        }
                        let latest = *entry.values.back().unwrap_or(&false);
                        let present = entry.count_present();
                        latest && present <= 2
                    })
                    .count();
                if fresh_count >= 1 {
                    outcome.supporting.push(PersistentStateEvidence {
                        code: "raw:fresh_bid_broker".into(),
                        summary: format!(
                            "{} sees {} fresh top-3 bid broker(s) this tick",
                            symbol.0, fresh_count
                        ),
                        weight: dec!(0.12),
                    });
                }
            }
        }
        PersistentStateKind::Conflicted | PersistentStateKind::LowInformation => {
            // No raw templates for these states yet — Conflicted resolves
            // via propagation_follow_through at the feature layer, and
            // LowInformation by definition has no actionable raw pattern.
        }
    }

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use longport::quote::Brokers;

    fn pi(position: i32, ids: &[i32]) -> Brokers {
        Brokers {
            broker_ids: ids.to_vec(),
            position,
        }
    }

    #[test]
    fn broker_presence_records_top_n_identity_across_ticks() {
        let mut presence = RawBrokerPresence::default();
        let symbol = Symbol("700.HK".into());
        let groups = vec![pi(1, &[4497]), pi(2, &[9988]), pi(3, &[6606])];
        let snapshot = SecurityBrokers {
            ask_brokers: vec![],
            bid_brokers: groups.clone(),
        };
        for _ in 0..5 {
            presence.record_tick(&symbol, &snapshot);
        }
        let pres = presence.for_symbol(&symbol).unwrap();
        assert_eq!(pres.bid.len(), 3);
        assert!(pres.bid.get(&4497).unwrap().is_consistent());
    }

    #[test]
    fn broker_withdrawal_fires_when_consistent_broker_disappears() {
        let mut presence = RawBrokerPresence::default();
        let symbol = Symbol("700.HK".into());
        // 4 ticks with broker 4497 present
        let with_4497 = SecurityBrokers {
            ask_brokers: vec![],
            bid_brokers: vec![pi(1, &[4497])],
        };
        // 2 ticks without
        let without = SecurityBrokers {
            ask_brokers: vec![],
            bid_brokers: vec![pi(1, &[9988])],
        };
        for _ in 0..4 {
            presence.record_tick(&symbol, &with_4497);
        }
        for _ in 0..2 {
            presence.record_tick(&symbol, &without);
        }
        let entry = presence
            .for_symbol(&symbol)
            .unwrap()
            .bid
            .get(&4497)
            .unwrap();
        assert!(entry.just_withdrew());
    }

    #[test]
    fn depth_level_records_volume_series_and_computes_relative_ratio() {
        let mut depth = RawDepthLevels::default();
        let symbol = Symbol("700.HK".into());
        for vol in [10_000i64, 10_500, 9_800, 10_200, 3_000] {
            let snapshot = SecurityDepth {
                asks: vec![],
                bids: vec![longport::quote::Depth {
                    position: 1,
                    price: None,
                    volume: vol,
                    order_num: 5,
                }],
            };
            depth.record_tick(&symbol, &snapshot);
        }
        let entry = depth.for_symbol(&symbol).unwrap().bid.get(&1).unwrap();
        let ratio = entry.relative_to_recent().unwrap();
        // Current 3_000 vs avg of 10_000/10_500/9_800/10_200 ≈ 10_125 → ≈0.30
        assert!(ratio < dec!(0.35));
    }

    #[test]
    fn turning_point_emits_consistent_bid_broker_for_t22_anchor() {
        let mut presence = RawBrokerPresence::default();
        let symbol = Symbol("700.HK".into());
        let snapshot = SecurityBrokers {
            ask_brokers: vec![],
            bid_brokers: vec![pi(1, &[4497])],
        };
        for _ in 0..5 {
            presence.record_tick(&symbol, &snapshot);
        }
        let depth = RawDepthLevels::default();
        let trades = RawTradeTape::default();
        let outcome = evaluate_raw_expectations(
            PersistentStateKind::TurningPoint,
            &symbol,
            &presence,
            &depth,
            &trades,
        );
        assert!(
            outcome
                .supporting
                .iter()
                .any(|e| e.code == "raw:consistent_bid_broker"),
            "TurningPoint + consistent bid broker must emit raw:consistent_bid_broker so T22 inference can pick the symbol as anchor"
        );
    }
}
