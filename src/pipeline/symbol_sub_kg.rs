//! Per-symbol sub-knowledge-graph with max granularity.
//!
//! Every symbol has the SAME fixed-template structural skeleton:
//!
//!   • 1   Symbol root
//!   • 6   Price scalars  (LastPrice, MidPrice, PrevClose, Spread, DayHigh, DayLow)
//!   • 4   Volume scalars (Volume, Turnover, VolRatio, FiveMinChgRate)
//!   • 2   Time bucket    (Time5min, Time30min)
//!   • 2   Membership ref (StateClassification, SectorRef)
//!   • 6   PressureChannel nodes (one per channel)
//!   • 5   IntentBelief modes
//!   • 10  BidLevel + 10 AskLevel depth nodes (each with price/volume/order-count attrs)
//!   • N   Broker nodes (dynamic, one per broker active on this symbol)
//!
//! Total: 46 fixed + N broker.
//!
//! Edges are also fixed-template (book chain, broker→level, raw→pressure,
//! pressure→intent, intent→state, temporal hierarchy, sector membership).
//! Broker → BidLevel/AskLevel edges are dynamic.
//!
//! This module defines DATA STRUCTURE only. There is no direction
//! inference, no Long/Short labelling, no thresholds. Activations on
//! nodes update each tick from existing Eden data sources. Downstream
//! detectors (cluster sync, motif extraction) read structural state and
//! emit emergence events without per-pattern rules.
//!
//! Two-level KG architecture:
//!   • Master KG (BrainGraph / UsGraph): symbol-symbol edges
//!     (peer, sector, shareholder, fund, calendar, supply-chain[future])
//!   • Per-symbol sub-KG (this module): typed nodes with activations
//!
//! Signal = synchronized activation across master-KG-connected symbols
//! on the same sub-KG node subset. Detection lives in cluster_sync (TODO).

use std::collections::HashMap;
use std::io::Write;

use chrono::{DateTime, Utc};
use rust_decimal::prelude::ToPrimitive;
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::core::market::MarketDataCapability;
use crate::ontology::objects::Symbol;
use crate::pipeline::belief_field::PressureBeliefField;
use crate::pipeline::decision_ledger::DecisionLedger;
use crate::pipeline::regime_analog_index::AnalogSummary;
use crate::pipeline::symbol_wl_analog_index::AnalogMatch;
use crate::temporal::lineage::CaseRealizedOutcome;

// ---------------- Node ID ----------------

/// Stable identifier for any node in any symbol's sub-KG. Used as
/// HashMap key. The fixed singletons are unit variants; depth levels
/// and brokers carry parameters.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NodeId {
    // Singletons (1 + 6 + 4 + 2 + 2 + 6 + 5 = 26)
    Symbol,
    LastPrice,
    MidPrice,
    PrevClose,
    Spread,
    DayHigh,
    DayLow,
    Volume,
    Turnover,
    VolRatio,
    FiveMinChgRate,
    Time5min,
    Time30min,
    StateClassification,
    SectorRef,
    PressureOrderBook,
    PressureCapitalFlow,
    PressureInstitutional,
    PressureMomentum,
    PressureVolume,
    PressureStructure,
    IntentAccumulation,
    IntentDistribution,
    IntentRotation,
    IntentVolatility,
    IntentUnknown,

    // Warrant pool (HK only — call/put warrant ecosystem around the underlying)
    CallWarrantCount,
    PutWarrantCount,
    WarrantIvGap,     // weighted IV gap call vs put
    CallWarrantShare, // call_count / total
    PutWarrantShare,  // put_count / total

    // Capital / fund flow (cumulative inflow/outflow over session)
    CapitalFlowCum,
    CapitalFlowAccelLast30m,

    // Market session phase (one categorical node — label set by updater)
    SessionPhase,

    // Earnings / event calendar
    NextEarningsDays, // days until next earnings (i32 stored as Decimal)
    InEarningsWindow, // 1.0 if within earnings week, else 0.0

    // Cross-market (HK ↔ US bridge)
    CrossMarketBridge,

    // Index membership (HSI / HSCEI / HSTECH for HK; SP500 / NDX for US)
    IndexMembership,

    // Macro context
    OvernightSpx,     // proxy for US overnight move (US only emit)
    SectorIndexLevel, // sector aggregate index level

    // Microstructure (trade tape + book quality)
    TradeTapeBuyMinusSell30s, // signed buy_vol - sell_vol over last 30s
    TradeTapeAccelLast1m,     // d/dt(trade rate) over last minute
    DepthAsymmetryTop3,       // bid_top3 / (bid_top3 + ask_top3); 0.5 = balanced
    QueueStabilityBid1,       // ticks bid1 has been unchanged
    QueueStabilityAsk1,       // ticks ask1 has been unchanged

    // Volume profile
    Vwap,             // session VWAP
    VwapDeviationPct, // (last_done - VWAP) / VWAP * 100

    // Holding structure
    InsiderHoldingPct,        // insider/founder holding percent
    InstitutionalHolderCount, // distinct institutional holders
    SouthboundFlowToday,      // HK only — net southbound flow this session
    EtfHoldingPct,            // total ETF ownership percent

    // Event context
    BigTradeCountLast1h, // count of block trades in last hour
    HasHaltedToday,      // 1.0 if symbol halted any time today, else 0
    VolumeSpikeFresh,    // 1.0 if vol_ratio > 3x within last 5 min

    // Cross-symbol role (within sector / peer cluster)
    LeaderLaggardScore,     // signed: + leads peers, − lags
    SectorRelativeStrength, // (symbol_change - sector_change) %

    // Short interest / borrow
    ShortInterestPct, // outstanding short / float
    DaysToCover,      // short_interest / avg_daily_volume

    // FX / currency
    HkdUsdRate, // HKD/USD spot
    UsdCnyRate, // USD/CNY spot

    // Tick rule (microstructure tape direction)
    TickRule, // categorical label: Uptick/Downtick/Zero

    // Book quality
    SpreadVelocity,     // signed: + widening, − tightening
    BookChurnRate,      // depth updates per second
    TradeSizeAvg30s,    // average trade size last 30s
    LargestTradeLast5m, // single biggest trade in last 5 min

    // Sentiment / signal
    AnalystRatingMean, // analyst consensus mean (1–5)
    NewsFlowDensity1h, // news headlines per hour

    // Option surface (US only — positional / sentiment layer from
    // option market). OI ratio + skew carry institutional positioning
    // information missing from HK side (which uses broker queue instead).
    OptionAtmCallIv,      // at-the-money call implied vol
    OptionAtmPutIv,       // at-the-money put implied vol
    OptionPutCallSkew,    // put_iv - call_iv (positive = fear)
    OptionPutCallOiRatio, // put_oi / call_oi (>1 = bearish lean)
    OptionTotalOi,        // total_call_oi + total_put_oi

    // Memory / belief / causal evidence produced by Eden substrate.
    OutcomeMemory,      // recent resolved outcome mean signed return
    EngramAlignment,    // current regime fingerprint outcome bias
    WlAnalogConfidence, // current WL signature historical recurrence
    BeliefEntropy,      // normalized categorical belief entropy
    BeliefSampleCount,  // normalized categorical belief sample count
    ForecastAccuracy,   // active-probe forecast accuracy accumulator

    // Cross-ontology: parent-sector aggregate intent signal (V3.2).
    // Each symbol carries a copy of its sector's verdict so BP's
    // observe_from_subkg picks it up without changing the BP entity
    // type to heterogeneous. Bull / Bear are decoupled because
    // 5-state sector posterior (Accumulation / Distribution /
    // Rotation / Volatility / Unknown) doesn't collapse cleanly
    // into a single signed scalar.
    SectorIntentBull, // sector posterior on Accumulation (∈ [0,1])
    SectorIntentBear, // sector posterior on Distribution (∈ [0,1])

    // Self-referential KL surprise (V4 decision unblock). Magnitude is
    // tanh(|max_z|/2) over channel KL z-scores against each channel's
    // own EWMA baseline; direction is the sign of the dominant channel's
    // mean shift. Both flow through observe_from_subkg in loopy_bp.
    KlSurpriseMagnitude, // [0, 1] — saturating size of the surprise
    KlSurpriseDirection, // [-1, 1] — sign of the dominant channel mean shift

    // Depth book (10 each side)
    BidLevel(u8),
    AskLevel(u8),

    // Brokers (dynamic, by broker_id)
    Broker(String),

    // Funds / institutions holding this symbol (dynamic, by institution_id)
    FundHolder(String),
}

// ---------------- Node Kind ----------------

/// Categorical type for grouping. Useful for motif detection
/// (e.g., "all Pressure nodes activated") without enumerating concrete IDs.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum NodeKind {
    SymbolRoot,
    Price,
    Volume,
    Time,
    BidDepth,
    AskDepth,
    Pressure,
    Intent,
    State,
    Broker,
    SectorRef,
    Warrant,
    CapitalFlow,
    Session,
    Earnings,
    CrossMarket,
    Index,
    Macro,
    FundHolder,
    Microstructure,
    Holder,
    Event,
    Role,
    ShortInterest,
    Fx,
    TickRule,
    BookQuality,
    Sentiment,
    Option,
    Memory,
    Belief,
    Causal,
    /// Cross-ontology: parent-sector aggregate signals attached to
    /// the symbol's sub-KG so BP picks them up via the standard
    /// observe_from_subkg path (V3.2).
    Sector,
    /// Self-referential KL-surprise carrier — magnitude/direction signals
    /// from `kl_surprise::KlSurpriseTracker`. Same role as Pressure or
    /// Memory: pure substrate evidence, no rules attached.
    Surprise,
}

// ---------------- Node Activation ----------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NodeProvenanceSource {
    Unknown,
    MarketQuote,
    MarketDepth,
    BrokerQueue,
    PressureField,
    IntentBelief,
    StateEngine,
    SubstrateEvidence,
    WarrantPool,
    CapitalFlow,
    SessionClock,
    EarningsCalendar,
    CrossMarketBridge,
    IndexMembership,
    MacroContext,
    Microstructure,
    HolderData,
    OptionSurface,
    Derived,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "snake_case")]
pub enum NodeFreshness {
    Missing,
    Fresh,
    Unchanged,
    Stale,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct NodeUpdateProvenance {
    pub source: NodeProvenanceSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_capability: Option<MarketDataCapability>,
}

impl NodeUpdateProvenance {
    pub fn new(
        source: NodeProvenanceSource,
        market_capability: Option<MarketDataCapability>,
    ) -> Self {
        Self {
            source,
            market_capability,
        }
    }

    pub fn for_node(id: &NodeId) -> Self {
        use MarketDataCapability as Capability;
        use NodeProvenanceSource as Source;

        let (source, market_capability) = match id {
            NodeId::LastPrice
            | NodeId::PrevClose
            | NodeId::DayHigh
            | NodeId::DayLow
            | NodeId::Volume
            | NodeId::Turnover
            | NodeId::VolRatio
            | NodeId::FiveMinChgRate
            | NodeId::Vwap
            | NodeId::VwapDeviationPct => (Source::MarketQuote, None),
            NodeId::BidLevel(_)
            | NodeId::AskLevel(_)
            | NodeId::MidPrice
            | NodeId::Spread
            | NodeId::DepthAsymmetryTop3
            | NodeId::QueueStabilityBid1
            | NodeId::QueueStabilityAsk1
            | NodeId::SpreadVelocity
            | NodeId::BookChurnRate => (Source::MarketDepth, Some(Capability::DepthL2)),
            NodeId::Broker(_) => (Source::BrokerQueue, Some(Capability::BrokerQueue)),
            NodeId::PressureOrderBook
            | NodeId::PressureCapitalFlow
            | NodeId::PressureInstitutional
            | NodeId::PressureMomentum
            | NodeId::PressureVolume
            | NodeId::PressureStructure => (Source::PressureField, None),
            NodeId::IntentAccumulation
            | NodeId::IntentDistribution
            | NodeId::IntentRotation
            | NodeId::IntentVolatility
            | NodeId::IntentUnknown => (Source::IntentBelief, None),
            NodeId::StateClassification => (Source::StateEngine, None),
            NodeId::OutcomeMemory
            | NodeId::EngramAlignment
            | NodeId::WlAnalogConfidence
            | NodeId::BeliefEntropy
            | NodeId::BeliefSampleCount
            | NodeId::ForecastAccuracy
            | NodeId::SectorIntentBull
            | NodeId::SectorIntentBear
            | NodeId::KlSurpriseMagnitude
            | NodeId::KlSurpriseDirection => (Source::SubstrateEvidence, None),
            NodeId::CallWarrantCount
            | NodeId::PutWarrantCount
            | NodeId::WarrantIvGap
            | NodeId::CallWarrantShare
            | NodeId::PutWarrantShare => (Source::WarrantPool, None),
            NodeId::CapitalFlowCum
            | NodeId::CapitalFlowAccelLast30m
            | NodeId::SouthboundFlowToday => (Source::CapitalFlow, Some(Capability::CapitalFlow)),
            NodeId::SessionPhase | NodeId::Time5min | NodeId::Time30min => {
                (Source::SessionClock, None)
            }
            NodeId::NextEarningsDays | NodeId::InEarningsWindow => (Source::EarningsCalendar, None),
            NodeId::CrossMarketBridge => (
                Source::CrossMarketBridge,
                Some(Capability::DualListingBridge),
            ),
            NodeId::IndexMembership | NodeId::SectorRef | NodeId::SectorIndexLevel => {
                (Source::IndexMembership, None)
            }
            NodeId::OvernightSpx | NodeId::HkdUsdRate | NodeId::UsdCnyRate => {
                (Source::MacroContext, None)
            }
            NodeId::TradeTapeBuyMinusSell30s
            | NodeId::TradeTapeAccelLast1m
            | NodeId::TickRule
            | NodeId::TradeSizeAvg30s
            | NodeId::LargestTradeLast5m => (Source::Microstructure, None),
            NodeId::InsiderHoldingPct
            | NodeId::InstitutionalHolderCount
            | NodeId::EtfHoldingPct
            | NodeId::FundHolder(_) => (Source::HolderData, Some(Capability::ExternalPriors)),
            NodeId::OptionAtmCallIv
            | NodeId::OptionAtmPutIv
            | NodeId::OptionPutCallSkew
            | NodeId::OptionPutCallOiRatio
            | NodeId::OptionTotalOi => (Source::OptionSurface, Some(Capability::OptionSurface)),
            NodeId::LeaderLaggardScore
            | NodeId::SectorRelativeStrength
            | NodeId::BigTradeCountLast1h
            | NodeId::HasHaltedToday
            | NodeId::VolumeSpikeFresh
            | NodeId::ShortInterestPct
            | NodeId::DaysToCover
            | NodeId::AnalystRatingMean
            | NodeId::NewsFlowDensity1h
            | NodeId::Symbol => (Source::Derived, None),
        };
        Self::new(source, market_capability)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeActivation {
    pub kind: NodeKind,
    /// Primary numerical activation:
    ///   Price → price level
    ///   Volume → cumulative volume
    ///   BidLevel/AskLevel → price at that level
    ///   Pressure → channel pressure scalar [-1..1] or [0..1]
    ///   Intent → posterior probability of that mode [0..1]
    ///   State → ordinal index of classification
    ///   Broker → 1.0 if currently sitting on book, else 0.0
    pub value: Option<Decimal>,
    /// Secondary numerical attribute:
    ///   BidLevel/AskLevel → volume at that level
    ///   Pressure → propagated component
    ///   Intent → effective sample count n
    ///   Broker → bid_count + ask_count
    pub aux: Option<Decimal>,
    /// Categorical state label (State node, SectorRef node, etc.)
    pub label: Option<String>,
    pub last_update_ts: DateTime<Utc>,
    pub last_seen_tick: u64,
    /// Ticks since value last meaningfully changed.
    pub age_ticks: u64,
    pub freshness: NodeFreshness,
    pub provenance_source: NodeProvenanceSource,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub market_capability: Option<MarketDataCapability>,
}

impl NodeActivation {
    pub fn empty(kind: NodeKind, ts: DateTime<Utc>) -> Self {
        Self {
            kind,
            value: None,
            aux: None,
            label: None,
            last_update_ts: ts,
            last_seen_tick: 0,
            age_ticks: 0,
            freshness: NodeFreshness::Missing,
            provenance_source: NodeProvenanceSource::Unknown,
            market_capability: None,
        }
    }

    fn mark_seen(
        &mut self,
        ts: DateTime<Utc>,
        tick: u64,
        changed: bool,
        provenance: NodeUpdateProvenance,
    ) {
        self.last_update_ts = ts;
        self.last_seen_tick = tick;
        self.provenance_source = provenance.source;
        self.market_capability = provenance.market_capability;
        if changed {
            self.age_ticks = 0;
            self.freshness = NodeFreshness::Fresh;
        } else {
            self.age_ticks = self.age_ticks.saturating_add(1);
            self.freshness = NodeFreshness::Unchanged;
        }
    }
}

// ---------------- Edge ----------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum EdgeKind {
    /// Symbol root → all member nodes
    Membership,
    /// BidLn → BidL(n+1), AskLn → AskL(n+1)
    BookChain,
    /// BidLn ↔ AskLn (top-of-book correspondence)
    BookOpposing,
    /// Broker → BidLevel/AskLevel they sit at
    BrokerSits,
    /// Raw scalar (Volume/Turnover/Spread/Depth) → PressureChannel that aggregates it
    Contributes,
    /// PressureChannel → IntentMode it evidences
    Evidence,
    /// Time5min → Time30min
    TemporalHierarchy,
    /// Symbol → SectorRef
    SectorMember,
    /// IntentMode → StateClassification (aggregate)
    IntentToState,
    /// Warrant pool nodes ↔ underlying (Symbol)
    WarrantPool,
    /// CapitalFlow nodes feed PressureCapitalFlow
    FlowToPressure,
    /// SessionPhase governs everything (root edge to all activations)
    SessionGoverns,
    /// EarningsWindow node → State (event-driven state shift)
    EarningsWindow,
    /// FundHolder → Symbol (institution holds this symbol)
    FundHolds,
    /// CrossMarketBridge → Symbol (HK↔US arbitrage corridor)
    CrossMarketLink,
    /// IndexMembership → Symbol (member of an index)
    IndexContains,
    /// Macro node → Symbol (overnight context affects symbol)
    MacroContext,
    /// Holder (fund / institution / insider) → Symbol
    HoldsShare,
    /// Event node → Symbol it affects
    NewsAffects,
    /// Symbol → Symbol with lead/lag relationship in same cluster
    LeadsLags,
    /// Short interest → Price (shorts bet against price)
    BorrowsFrom,
    /// FX node → Symbol (pricing currency context)
    PricedIn,
    /// Sentiment node → State
    SentimentToState,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Edge {
    pub from: NodeId,
    pub to: NodeId,
    pub kind: EdgeKind,
    /// Edge weight = co-activation strength accumulated over recent ticks.
    /// For BrokerSits: increment per tick the broker remains seated.
    pub weight: Decimal,
    pub last_active_ts: DateTime<Utc>,
}

// ---------------- Side ----------------

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum Side {
    Bid,
    Ask,
}

// ---------------- SymbolSubKG ----------------

#[derive(Debug, Clone)]
pub struct SymbolSubKG {
    pub symbol: String,
    pub ts: DateTime<Utc>,
    pub tick: u64,
    pub nodes: HashMap<NodeId, NodeActivation>,
    pub edges: Vec<Edge>,
}

impl NodeId {
    pub fn to_serde_key(&self) -> String {
        match self {
            NodeId::BidLevel(n) => format!("BidLevel_{}", n),
            NodeId::AskLevel(n) => format!("AskLevel_{}", n),
            NodeId::Broker(b) => format!("Broker_{}", b),
            other => format!("{:?}", other),
        }
    }
}

impl Serialize for SymbolSubKG {
    fn serialize<S: serde::Serializer>(&self, ser: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeStruct;
        // Convert HashMap<NodeId, NodeActivation> → BTreeMap<String, &NodeActivation>
        // for JSON-friendly stable-ordered output.
        let mut node_map: std::collections::BTreeMap<String, &NodeActivation> =
            std::collections::BTreeMap::new();
        for (k, v) in &self.nodes {
            node_map.insert(k.to_serde_key(), v);
        }
        let mut s = ser.serialize_struct("SymbolSubKG", 5)?;
        s.serialize_field("symbol", &self.symbol)?;
        s.serialize_field("ts", &self.ts)?;
        s.serialize_field("tick", &self.tick)?;
        s.serialize_field("nodes", &node_map)?;
        s.serialize_field("edges", &self.edges)?;
        s.end()
    }
}

impl SymbolSubKG {
    /// Create empty sub-KG with all fixed-template nodes initialized
    /// (no activations yet) and template edges in place.
    pub fn new_empty(symbol: String, ts: DateTime<Utc>) -> Self {
        let mut nodes = HashMap::new();

        for (id, kind) in fixed_singleton_nodes() {
            nodes.insert(id, NodeActivation::empty(kind, ts));
        }
        for level in 1..=10u8 {
            nodes.insert(
                NodeId::BidLevel(level),
                NodeActivation::empty(NodeKind::BidDepth, ts),
            );
            nodes.insert(
                NodeId::AskLevel(level),
                NodeActivation::empty(NodeKind::AskDepth, ts),
            );
        }

        let edges = build_template_edges(ts);
        Self {
            symbol,
            ts,
            tick: 0,
            nodes,
            edges,
        }
    }

    /// Update a singleton or depth-level node's primary value.
    /// Marks the activation as fresh (age_ticks → 0) when value changed.
    pub fn set_node_value(&mut self, id: NodeId, value: Decimal, ts: DateTime<Utc>) {
        let provenance = NodeUpdateProvenance::for_node(&id);
        self.set_node_value_with_provenance(id, value, ts, provenance);
    }

    pub fn set_node_value_with_provenance(
        &mut self,
        id: NodeId,
        value: Decimal,
        ts: DateTime<Utc>,
        provenance: NodeUpdateProvenance,
    ) {
        if let Some(entry) = self.nodes.get_mut(&id) {
            let changed = entry.value != Some(value);
            entry.value = Some(value);
            entry.mark_seen(ts, self.tick, changed, provenance);
        }
    }

    pub fn set_node_aux(&mut self, id: NodeId, aux: Decimal, ts: DateTime<Utc>) {
        let provenance = NodeUpdateProvenance::for_node(&id);
        self.set_node_aux_with_provenance(id, aux, ts, provenance);
    }

    pub fn set_node_aux_with_provenance(
        &mut self,
        id: NodeId,
        aux: Decimal,
        ts: DateTime<Utc>,
        provenance: NodeUpdateProvenance,
    ) {
        if let Some(entry) = self.nodes.get_mut(&id) {
            let changed = entry.aux != Some(aux);
            entry.aux = Some(aux);
            entry.mark_seen(ts, self.tick, changed, provenance);
        }
    }

    pub fn set_node_label(&mut self, id: NodeId, label: String, ts: DateTime<Utc>) {
        let provenance = NodeUpdateProvenance::for_node(&id);
        self.set_node_label_with_provenance(id, label, ts, provenance);
    }

    pub fn set_node_label_with_provenance(
        &mut self,
        id: NodeId,
        label: String,
        ts: DateTime<Utc>,
        provenance: NodeUpdateProvenance,
    ) {
        if let Some(entry) = self.nodes.get_mut(&id) {
            let changed = entry.label.as_deref() != Some(label.as_str());
            entry.label = Some(label);
            entry.mark_seen(ts, self.tick, changed, provenance);
        }
    }

    /// Register a broker as sitting on a depth level.
    /// Creates the broker node lazily; adds or strengthens BrokerSits edge.
    pub fn add_or_update_broker(
        &mut self,
        broker_id: String,
        side_level: Option<(Side, u8)>,
        ts: DateTime<Utc>,
    ) {
        let node_id = NodeId::Broker(broker_id.clone());
        let entry = self
            .nodes
            .entry(node_id.clone())
            .or_insert_with(|| NodeActivation::empty(NodeKind::Broker, ts));
        let changed = entry.value != Some(Decimal::ONE);
        entry.value = Some(Decimal::ONE);
        entry.mark_seen(
            ts,
            self.tick,
            changed,
            NodeUpdateProvenance::for_node(&node_id),
        );

        if let Some((side, level)) = side_level {
            let target = match side {
                Side::Bid => NodeId::BidLevel(level),
                Side::Ask => NodeId::AskLevel(level),
            };
            let existing = self
                .edges
                .iter_mut()
                .find(|e| e.from == node_id && e.to == target && e.kind == EdgeKind::BrokerSits);
            match existing {
                Some(e) => {
                    e.weight += Decimal::ONE;
                    e.last_active_ts = ts;
                }
                None => {
                    self.edges.push(Edge {
                        from: node_id.clone(),
                        to: target,
                        kind: EdgeKind::BrokerSits,
                        weight: Decimal::ONE,
                        last_active_ts: ts,
                    });
                }
            }
        }
    }

    /// Decay broker presence values whose last_update is older than max_age_ticks.
    /// Brokers no longer on the book naturally fade out without being removed.
    pub fn decay_brokers(&mut self, max_age_ticks: u64, now_tick: u64) {
        let _ = now_tick; // reserved for future tick-based decay
        for (_, act) in self.nodes.iter_mut() {
            if act.kind == NodeKind::Broker {
                act.age_ticks = act.age_ticks.saturating_add(1);
                if act.age_ticks > max_age_ticks {
                    act.value = Some(Decimal::ZERO);
                    act.freshness = NodeFreshness::Stale;
                }
            }
        }
    }

    /// Set tick number; called by updater each tick.
    pub fn set_tick(&mut self, tick: u64, ts: DateTime<Utc>) {
        self.tick = tick;
        self.ts = ts;
    }

    /// Number of nodes currently active (value present and > 0).
    pub fn active_node_count(&self) -> usize {
        self.nodes
            .values()
            .filter(|n| n.value.map(|v| v > Decimal::ZERO).unwrap_or(false))
            .count()
    }

    /// Get all nodes of a given kind.
    pub fn nodes_of_kind(&self, kind: NodeKind) -> Vec<(&NodeId, &NodeActivation)> {
        self.nodes.iter().filter(|(_, a)| a.kind == kind).collect()
    }
}

// ---------------- Helpers ----------------

fn fixed_singleton_nodes() -> Vec<(NodeId, NodeKind)> {
    vec![
        (NodeId::Symbol, NodeKind::SymbolRoot),
        (NodeId::LastPrice, NodeKind::Price),
        (NodeId::MidPrice, NodeKind::Price),
        (NodeId::PrevClose, NodeKind::Price),
        (NodeId::Spread, NodeKind::Price),
        (NodeId::DayHigh, NodeKind::Price),
        (NodeId::DayLow, NodeKind::Price),
        (NodeId::Volume, NodeKind::Volume),
        (NodeId::Turnover, NodeKind::Volume),
        (NodeId::VolRatio, NodeKind::Volume),
        (NodeId::FiveMinChgRate, NodeKind::Volume),
        (NodeId::Time5min, NodeKind::Time),
        (NodeId::Time30min, NodeKind::Time),
        (NodeId::StateClassification, NodeKind::State),
        (NodeId::SectorRef, NodeKind::SectorRef),
        (NodeId::PressureOrderBook, NodeKind::Pressure),
        (NodeId::PressureCapitalFlow, NodeKind::Pressure),
        (NodeId::PressureInstitutional, NodeKind::Pressure),
        (NodeId::PressureMomentum, NodeKind::Pressure),
        (NodeId::PressureVolume, NodeKind::Pressure),
        (NodeId::PressureStructure, NodeKind::Pressure),
        (NodeId::IntentAccumulation, NodeKind::Intent),
        (NodeId::IntentDistribution, NodeKind::Intent),
        (NodeId::IntentRotation, NodeKind::Intent),
        (NodeId::IntentVolatility, NodeKind::Intent),
        (NodeId::IntentUnknown, NodeKind::Intent),
        (NodeId::CallWarrantCount, NodeKind::Warrant),
        (NodeId::PutWarrantCount, NodeKind::Warrant),
        (NodeId::WarrantIvGap, NodeKind::Warrant),
        (NodeId::CallWarrantShare, NodeKind::Warrant),
        (NodeId::PutWarrantShare, NodeKind::Warrant),
        (NodeId::CapitalFlowCum, NodeKind::CapitalFlow),
        (NodeId::CapitalFlowAccelLast30m, NodeKind::CapitalFlow),
        (NodeId::SessionPhase, NodeKind::Session),
        (NodeId::NextEarningsDays, NodeKind::Earnings),
        (NodeId::InEarningsWindow, NodeKind::Earnings),
        (NodeId::CrossMarketBridge, NodeKind::CrossMarket),
        (NodeId::IndexMembership, NodeKind::Index),
        (NodeId::OvernightSpx, NodeKind::Macro),
        (NodeId::SectorIndexLevel, NodeKind::Macro),
        (NodeId::TradeTapeBuyMinusSell30s, NodeKind::Microstructure),
        (NodeId::TradeTapeAccelLast1m, NodeKind::Microstructure),
        (NodeId::DepthAsymmetryTop3, NodeKind::Microstructure),
        (NodeId::QueueStabilityBid1, NodeKind::Microstructure),
        (NodeId::QueueStabilityAsk1, NodeKind::Microstructure),
        (NodeId::Vwap, NodeKind::Microstructure),
        (NodeId::VwapDeviationPct, NodeKind::Microstructure),
        (NodeId::InsiderHoldingPct, NodeKind::Holder),
        (NodeId::InstitutionalHolderCount, NodeKind::Holder),
        (NodeId::SouthboundFlowToday, NodeKind::Holder),
        (NodeId::EtfHoldingPct, NodeKind::Holder),
        (NodeId::BigTradeCountLast1h, NodeKind::Event),
        (NodeId::HasHaltedToday, NodeKind::Event),
        (NodeId::VolumeSpikeFresh, NodeKind::Event),
        (NodeId::LeaderLaggardScore, NodeKind::Role),
        (NodeId::SectorRelativeStrength, NodeKind::Role),
        (NodeId::ShortInterestPct, NodeKind::ShortInterest),
        (NodeId::DaysToCover, NodeKind::ShortInterest),
        (NodeId::HkdUsdRate, NodeKind::Fx),
        (NodeId::UsdCnyRate, NodeKind::Fx),
        (NodeId::TickRule, NodeKind::TickRule),
        (NodeId::SpreadVelocity, NodeKind::BookQuality),
        (NodeId::BookChurnRate, NodeKind::BookQuality),
        (NodeId::TradeSizeAvg30s, NodeKind::BookQuality),
        (NodeId::LargestTradeLast5m, NodeKind::BookQuality),
        (NodeId::AnalystRatingMean, NodeKind::Sentiment),
        (NodeId::NewsFlowDensity1h, NodeKind::Sentiment),
        (NodeId::OptionAtmCallIv, NodeKind::Option),
        (NodeId::OptionAtmPutIv, NodeKind::Option),
        (NodeId::OptionPutCallSkew, NodeKind::Option),
        (NodeId::OptionPutCallOiRatio, NodeKind::Option),
        (NodeId::OptionTotalOi, NodeKind::Option),
        (NodeId::OutcomeMemory, NodeKind::Memory),
        (NodeId::EngramAlignment, NodeKind::Memory),
        (NodeId::WlAnalogConfidence, NodeKind::Memory),
        (NodeId::BeliefEntropy, NodeKind::Belief),
        (NodeId::BeliefSampleCount, NodeKind::Belief),
        (NodeId::ForecastAccuracy, NodeKind::Causal),
        (NodeId::SectorIntentBull, NodeKind::Sector),
        (NodeId::SectorIntentBear, NodeKind::Sector),
        (NodeId::KlSurpriseMagnitude, NodeKind::Surprise),
        (NodeId::KlSurpriseDirection, NodeKind::Surprise),
    ]
}

fn build_template_edges(ts: DateTime<Utc>) -> Vec<Edge> {
    let mut edges = Vec::new();
    let unit = Decimal::ONE;

    // Symbol → all singletons + first depth levels (Membership)
    let membership_targets = [
        NodeId::LastPrice,
        NodeId::MidPrice,
        NodeId::PrevClose,
        NodeId::Spread,
        NodeId::DayHigh,
        NodeId::DayLow,
        NodeId::Volume,
        NodeId::Turnover,
        NodeId::VolRatio,
        NodeId::FiveMinChgRate,
        NodeId::Time5min,
        NodeId::Time30min,
        NodeId::StateClassification,
        NodeId::SectorRef,
        NodeId::PressureOrderBook,
        NodeId::PressureCapitalFlow,
        NodeId::PressureInstitutional,
        NodeId::PressureMomentum,
        NodeId::PressureVolume,
        NodeId::PressureStructure,
        NodeId::IntentAccumulation,
        NodeId::IntentDistribution,
        NodeId::IntentRotation,
        NodeId::IntentVolatility,
        NodeId::IntentUnknown,
        NodeId::CapitalFlowCum,
        NodeId::CapitalFlowAccelLast30m,
        NodeId::SessionPhase,
        NodeId::NextEarningsDays,
        NodeId::InEarningsWindow,
        NodeId::CrossMarketBridge,
        NodeId::IndexMembership,
        NodeId::OvernightSpx,
        NodeId::SectorIndexLevel,
        NodeId::TradeTapeBuyMinusSell30s,
        NodeId::TradeTapeAccelLast1m,
        NodeId::DepthAsymmetryTop3,
        NodeId::QueueStabilityBid1,
        NodeId::QueueStabilityAsk1,
        NodeId::Vwap,
        NodeId::VwapDeviationPct,
        NodeId::InsiderHoldingPct,
        NodeId::InstitutionalHolderCount,
        NodeId::SouthboundFlowToday,
        NodeId::EtfHoldingPct,
        NodeId::BigTradeCountLast1h,
        NodeId::HasHaltedToday,
        NodeId::VolumeSpikeFresh,
        NodeId::LeaderLaggardScore,
        NodeId::SectorRelativeStrength,
        NodeId::ShortInterestPct,
        NodeId::DaysToCover,
        NodeId::HkdUsdRate,
        NodeId::UsdCnyRate,
        NodeId::TickRule,
        NodeId::SpreadVelocity,
        NodeId::BookChurnRate,
        NodeId::TradeSizeAvg30s,
        NodeId::LargestTradeLast5m,
        NodeId::AnalystRatingMean,
        NodeId::NewsFlowDensity1h,
        NodeId::OptionAtmCallIv,
        NodeId::OptionAtmPutIv,
        NodeId::OptionPutCallSkew,
        NodeId::OptionPutCallOiRatio,
        NodeId::OptionTotalOi,
        NodeId::OutcomeMemory,
        NodeId::EngramAlignment,
        NodeId::WlAnalogConfidence,
        NodeId::BeliefEntropy,
        NodeId::BeliefSampleCount,
        NodeId::ForecastAccuracy,
        NodeId::SectorIntentBull,
        NodeId::SectorIntentBear,
        NodeId::BidLevel(1),
        NodeId::AskLevel(1),
    ];
    for target in membership_targets {
        edges.push(Edge {
            from: NodeId::Symbol,
            to: target,
            kind: EdgeKind::Membership,
            weight: unit,
            last_active_ts: ts,
        });
    }

    // Warrant pool: Symbol ↔ CallWarrantCount/PutWarrantCount/WarrantIvGap
    for warrant_node in [
        NodeId::CallWarrantCount,
        NodeId::PutWarrantCount,
        NodeId::WarrantIvGap,
        NodeId::CallWarrantShare,
        NodeId::PutWarrantShare,
    ] {
        edges.push(Edge {
            from: NodeId::Symbol,
            to: warrant_node,
            kind: EdgeKind::WarrantPool,
            weight: unit,
            last_active_ts: ts,
        });
    }

    // CapitalFlow nodes feed PressureCapitalFlow channel
    edges.push(Edge {
        from: NodeId::CapitalFlowCum,
        to: NodeId::PressureCapitalFlow,
        kind: EdgeKind::FlowToPressure,
        weight: unit,
        last_active_ts: ts,
    });
    edges.push(Edge {
        from: NodeId::CapitalFlowAccelLast30m,
        to: NodeId::PressureCapitalFlow,
        kind: EdgeKind::FlowToPressure,
        weight: unit,
        last_active_ts: ts,
    });

    // Session governs: SessionPhase → Symbol (informational gating)
    edges.push(Edge {
        from: NodeId::SessionPhase,
        to: NodeId::Symbol,
        kind: EdgeKind::SessionGoverns,
        weight: unit,
        last_active_ts: ts,
    });

    // Earnings window → StateClassification (event flag affects state read)
    edges.push(Edge {
        from: NodeId::InEarningsWindow,
        to: NodeId::StateClassification,
        kind: EdgeKind::EarningsWindow,
        weight: unit,
        last_active_ts: ts,
    });
    edges.push(Edge {
        from: NodeId::NextEarningsDays,
        to: NodeId::InEarningsWindow,
        kind: EdgeKind::EarningsWindow,
        weight: unit,
        last_active_ts: ts,
    });

    // Cross-market bridge → Symbol
    edges.push(Edge {
        from: NodeId::CrossMarketBridge,
        to: NodeId::Symbol,
        kind: EdgeKind::CrossMarketLink,
        weight: unit,
        last_active_ts: ts,
    });

    // Index membership → Symbol
    edges.push(Edge {
        from: NodeId::IndexMembership,
        to: NodeId::Symbol,
        kind: EdgeKind::IndexContains,
        weight: unit,
        last_active_ts: ts,
    });

    // Macro context → Symbol / SectorRef
    edges.push(Edge {
        from: NodeId::OvernightSpx,
        to: NodeId::Symbol,
        kind: EdgeKind::MacroContext,
        weight: unit,
        last_active_ts: ts,
    });
    edges.push(Edge {
        from: NodeId::SectorIndexLevel,
        to: NodeId::SectorRef,
        kind: EdgeKind::MacroContext,
        weight: unit,
        last_active_ts: ts,
    });

    // Microstructure aggregators feed PressureOrderBook/PressureMomentum
    for (from, to) in [
        (NodeId::TradeTapeBuyMinusSell30s, NodeId::PressureMomentum),
        (NodeId::TradeTapeAccelLast1m, NodeId::PressureMomentum),
        (NodeId::DepthAsymmetryTop3, NodeId::PressureOrderBook),
        (NodeId::QueueStabilityBid1, NodeId::PressureOrderBook),
        (NodeId::QueueStabilityAsk1, NodeId::PressureOrderBook),
        (NodeId::Vwap, NodeId::PressureVolume),
        (NodeId::VwapDeviationPct, NodeId::PressureMomentum),
    ] {
        edges.push(Edge {
            from,
            to,
            kind: EdgeKind::Contributes,
            weight: unit,
            last_active_ts: ts,
        });
    }
    // Holder structure → PressureInstitutional / CapitalFlow
    for (from, to) in [
        (NodeId::InsiderHoldingPct, NodeId::PressureInstitutional),
        (
            NodeId::InstitutionalHolderCount,
            NodeId::PressureInstitutional,
        ),
        (NodeId::SouthboundFlowToday, NodeId::PressureCapitalFlow),
        (NodeId::EtfHoldingPct, NodeId::PressureInstitutional),
    ] {
        edges.push(Edge {
            from,
            to,
            kind: EdgeKind::HoldsShare,
            weight: unit,
            last_active_ts: ts,
        });
    }
    // Event nodes → State (event-driven state shifts)
    for from in [
        NodeId::BigTradeCountLast1h,
        NodeId::HasHaltedToday,
        NodeId::VolumeSpikeFresh,
    ] {
        edges.push(Edge {
            from,
            to: NodeId::StateClassification,
            kind: EdgeKind::NewsAffects,
            weight: unit,
            last_active_ts: ts,
        });
    }
    // Role nodes
    for to in [NodeId::LeaderLaggardScore, NodeId::SectorRelativeStrength] {
        edges.push(Edge {
            from: NodeId::Symbol,
            to,
            kind: EdgeKind::LeadsLags,
            weight: unit,
            last_active_ts: ts,
        });
    }
    // Short interest → Price (bearish bet)
    for from in [NodeId::ShortInterestPct, NodeId::DaysToCover] {
        edges.push(Edge {
            from,
            to: NodeId::LastPrice,
            kind: EdgeKind::BorrowsFrom,
            weight: unit,
            last_active_ts: ts,
        });
    }
    // FX → Symbol
    for from in [NodeId::HkdUsdRate, NodeId::UsdCnyRate] {
        edges.push(Edge {
            from,
            to: NodeId::Symbol,
            kind: EdgeKind::PricedIn,
            weight: unit,
            last_active_ts: ts,
        });
    }
    // Tick rule → Momentum
    edges.push(Edge {
        from: NodeId::TickRule,
        to: NodeId::PressureMomentum,
        kind: EdgeKind::Contributes,
        weight: unit,
        last_active_ts: ts,
    });
    // Book quality → OrderBook / Volume pressure
    for (from, to) in [
        (NodeId::SpreadVelocity, NodeId::PressureOrderBook),
        (NodeId::BookChurnRate, NodeId::PressureOrderBook),
        (NodeId::TradeSizeAvg30s, NodeId::PressureVolume),
        (NodeId::LargestTradeLast5m, NodeId::PressureVolume),
    ] {
        edges.push(Edge {
            from,
            to,
            kind: EdgeKind::Contributes,
            weight: unit,
            last_active_ts: ts,
        });
    }
    // Sentiment → State
    for from in [NodeId::AnalystRatingMean, NodeId::NewsFlowDensity1h] {
        edges.push(Edge {
            from,
            to: NodeId::StateClassification,
            kind: EdgeKind::SentimentToState,
            weight: unit,
            last_active_ts: ts,
        });
    }
    // Option surface → institutional pressure (skew and OI ratio both
    // read as positional information when institutions hedge). Reuses
    // Contributes because no dedicated option edge kind is needed — the
    // semantic is the same as other inputs into a pressure channel.
    for from in [
        NodeId::OptionAtmCallIv,
        NodeId::OptionAtmPutIv,
        NodeId::OptionPutCallSkew,
        NodeId::OptionPutCallOiRatio,
        NodeId::OptionTotalOi,
    ] {
        edges.push(Edge {
            from,
            to: NodeId::PressureInstitutional,
            kind: EdgeKind::Contributes,
            weight: unit,
            last_active_ts: ts,
        });
    }

    // Bid level chain (1→2→...→10) and Ask level chain
    for n in 1..=9u8 {
        edges.push(Edge {
            from: NodeId::BidLevel(n),
            to: NodeId::BidLevel(n + 1),
            kind: EdgeKind::BookChain,
            weight: unit,
            last_active_ts: ts,
        });
        edges.push(Edge {
            from: NodeId::AskLevel(n),
            to: NodeId::AskLevel(n + 1),
            kind: EdgeKind::BookChain,
            weight: unit,
            last_active_ts: ts,
        });
    }

    // Bid-Ask correspondence at each level
    for n in 1..=10u8 {
        edges.push(Edge {
            from: NodeId::BidLevel(n),
            to: NodeId::AskLevel(n),
            kind: EdgeKind::BookOpposing,
            weight: unit,
            last_active_ts: ts,
        });
    }

    // Raw → Pressure (Contributes)
    let contrib = [
        (NodeId::Volume, NodeId::PressureVolume),
        (NodeId::Turnover, NodeId::PressureCapitalFlow),
        (NodeId::FiveMinChgRate, NodeId::PressureMomentum),
        (NodeId::Spread, NodeId::PressureOrderBook),
    ];
    for (from, to) in contrib {
        edges.push(Edge {
            from,
            to,
            kind: EdgeKind::Contributes,
            weight: unit,
            last_active_ts: ts,
        });
    }
    for n in 1..=3u8 {
        edges.push(Edge {
            from: NodeId::BidLevel(n),
            to: NodeId::PressureOrderBook,
            kind: EdgeKind::Contributes,
            weight: unit,
            last_active_ts: ts,
        });
        edges.push(Edge {
            from: NodeId::AskLevel(n),
            to: NodeId::PressureOrderBook,
            kind: EdgeKind::Contributes,
            weight: unit,
            last_active_ts: ts,
        });
    }

    // Pressure → Intent (Evidence)
    let evidence = [
        (
            NodeId::PressureCapitalFlow,
            vec![NodeId::IntentAccumulation, NodeId::IntentDistribution],
        ),
        (
            NodeId::PressureInstitutional,
            vec![NodeId::IntentAccumulation, NodeId::IntentDistribution],
        ),
        (
            NodeId::PressureMomentum,
            vec![NodeId::IntentRotation, NodeId::IntentVolatility],
        ),
        (
            NodeId::PressureVolume,
            vec![NodeId::IntentVolatility, NodeId::IntentRotation],
        ),
        (
            NodeId::PressureOrderBook,
            vec![NodeId::IntentAccumulation, NodeId::IntentDistribution],
        ),
        (NodeId::PressureStructure, vec![NodeId::IntentRotation]),
    ];
    for (p, intents) in evidence {
        for intent in intents {
            edges.push(Edge {
                from: p.clone(),
                to: intent,
                kind: EdgeKind::Evidence,
                weight: unit,
                last_active_ts: ts,
            });
        }
    }

    // Intent → State
    for intent in [
        NodeId::IntentAccumulation,
        NodeId::IntentDistribution,
        NodeId::IntentRotation,
        NodeId::IntentVolatility,
        NodeId::IntentUnknown,
    ] {
        edges.push(Edge {
            from: intent,
            to: NodeId::StateClassification,
            kind: EdgeKind::IntentToState,
            weight: unit,
            last_active_ts: ts,
        });
    }

    // Time hierarchy + Sector membership
    edges.push(Edge {
        from: NodeId::Time5min,
        to: NodeId::Time30min,
        kind: EdgeKind::TemporalHierarchy,
        weight: unit,
        last_active_ts: ts,
    });
    edges.push(Edge {
        from: NodeId::Symbol,
        to: NodeId::SectorRef,
        kind: EdgeKind::SectorMember,
        weight: unit,
        last_active_ts: ts,
    });

    edges
}

// ---------------- Registry ----------------

/// Holds one SymbolSubKG per symbol per market. Constructed once at
/// runtime startup, mutated each tick by the update step, periodically
/// snapshotted to `.run/eden-subkg-{market}.ndjson` for operator
/// inspection (or downstream consumption by cluster_sync detector).
#[derive(Debug, Default)]
pub struct SubKgRegistry {
    pub graphs: HashMap<String, SymbolSubKG>,
}

impl SubKgRegistry {
    pub fn new() -> Self {
        Self {
            graphs: HashMap::new(),
        }
    }

    /// Get-or-init the sub-KG for a symbol. Returns a mutable reference
    /// for the caller to update node values.
    pub fn upsert(&mut self, symbol: &str, ts: DateTime<Utc>) -> &mut SymbolSubKG {
        self.graphs
            .entry(symbol.to_string())
            .or_insert_with(|| SymbolSubKG::new_empty(symbol.to_string(), ts))
    }

    pub fn get(&self, symbol: &str) -> Option<&SymbolSubKG> {
        self.graphs.get(symbol)
    }

    pub fn len(&self) -> usize {
        self.graphs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.graphs.is_empty()
    }

    /// Append a JSON snapshot of every sub-KG to
    /// `.run/eden-subkg-{market}.ndjson` (one line per symbol). Skips
    /// symbols with zero active nodes (avoid noise in the file).
    /// Caller owns rotation/truncation.
    pub fn snapshot_to_ndjson(&self, market: &str) -> std::io::Result<usize> {
        let path = format!(".run/eden-subkg-{}.ndjson", market);
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)?;
        let mut written = 0usize;
        for kg in self.graphs.values() {
            if kg.active_node_count() == 0 {
                continue;
            }
            let line = serde_json::to_string(kg)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            writeln!(file, "{}", line)?;
            written += 1;
        }
        Ok(written)
    }

    /// Serialize all active per-symbol sub-KGs to one JSON line each.
    /// 2026-04-29: split out so the runtime consumer can do the
    /// (CPU-bound) serialization synchronously and hand the resulting
    /// `Vec<String>` to the background NDJSON writer task. Pairs with
    /// [`append_subkg_lines_to_ndjson`] which performs the IO. See
    /// `src/core/ndjson_writer.rs` doc for context.
    pub fn serialize_active_to_lines(&self) -> std::io::Result<Vec<String>> {
        let mut lines = Vec::with_capacity(self.graphs.len());
        for kg in self.graphs.values() {
            if kg.active_node_count() == 0 {
                continue;
            }
            let line = serde_json::to_string(kg)
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
            lines.push(line);
        }
        Ok(lines)
    }

    /// Compact summary line per market suitable for stderr.
    pub fn summary_line(&self, market: &str) -> String {
        let total = self.graphs.len();
        let active: usize = self.graphs.values().map(|kg| kg.active_node_count()).sum();
        let with_brokers: usize = self
            .graphs
            .values()
            .filter(|kg| {
                kg.nodes
                    .iter()
                    .any(|(id, _)| matches!(id, NodeId::Broker(_)))
            })
            .count();
        format!(
            "[sub_kg:{}] symbols={} active_nodes={} with_brokers={}",
            market, total, active, with_brokers
        )
    }
}

// ---------------- Updater ----------------

/// Pure-mechanical wiring from Longport live state to sub-KG nodes.
/// One-to-one: Quote.last_done → LastPrice node; Depth.bids[n] → BidLevel(n+1).
/// No thresholds, no inference, no rules. The KG faithfully mirrors what
/// Longport pushed plus what Eden's raw trackers observed.
///
/// Pressure / intent / state nodes are updated by separate functions
/// (`update_from_pressure`, `update_from_intent_belief`, `update_state_classification`)
/// because their owning structs live outside this module's import scope.
pub fn update_from_quotes_depths_brokers(
    registry: &mut SubKgRegistry,
    quotes: &HashMap<String, QuoteData>,
    depths: &HashMap<String, DepthData>,
    broker_per_symbol: &HashMap<String, Vec<BrokerSeat>>,
    ts: DateTime<Utc>,
    tick: u64,
) {
    // Quotes (price + volume + day range)
    for (sym, q) in quotes {
        let kg = registry.upsert(sym, ts);
        kg.set_tick(tick, ts);
        kg.set_node_value(NodeId::LastPrice, q.last_done, ts);
        kg.set_node_value(NodeId::PrevClose, q.prev_close, ts);
        kg.set_node_value(NodeId::DayHigh, q.day_high, ts);
        kg.set_node_value(NodeId::DayLow, q.day_low, ts);
        kg.set_node_value(NodeId::Volume, q.volume, ts);
        kg.set_node_value(NodeId::Turnover, q.turnover, ts);
    }

    // Depth book (10 levels per side; sub-KG holds up to 10 nodes per side)
    for (sym, d) in depths {
        let kg = registry.upsert(sym, ts);
        kg.set_tick(tick, ts);
        for (idx, lvl) in d.bids.iter().take(10).enumerate() {
            let n = (idx + 1) as u8;
            kg.set_node_value(NodeId::BidLevel(n), lvl.price, ts);
            kg.set_node_aux(NodeId::BidLevel(n), Decimal::from(lvl.volume), ts);
        }
        for (idx, lvl) in d.asks.iter().take(10).enumerate() {
            let n = (idx + 1) as u8;
            kg.set_node_value(NodeId::AskLevel(n), lvl.price, ts);
            kg.set_node_aux(NodeId::AskLevel(n), Decimal::from(lvl.volume), ts);
        }
        // Derived: spread + mid from L1
        let bid1 = d.bids.first();
        let ask1 = d.asks.first();
        if let (Some(b), Some(a)) = (bid1, ask1) {
            let mid = (b.price + a.price) / Decimal::from(2);
            kg.set_node_value(NodeId::MidPrice, mid, ts);
            kg.set_node_value(NodeId::Spread, a.price - b.price, ts);
        }
    }

    // Broker queue presence
    for (sym, seats) in broker_per_symbol {
        let kg = registry.upsert(sym, ts);
        kg.set_tick(tick, ts);
        for seat in seats {
            kg.add_or_update_broker(seat.broker_id.clone(), Some((seat.side, seat.position)), ts);
        }
    }
}

/// Minimal value type for quote data. Owned by the runtime caller; this
/// module doesn't depend on Longport types so the updater stays
/// reusable in tests and in non-Longport contexts.
#[derive(Debug, Clone)]
pub struct QuoteData {
    pub last_done: Decimal,
    pub prev_close: Decimal,
    pub day_high: Decimal,
    pub day_low: Decimal,
    pub volume: Decimal,
    pub turnover: Decimal,
}

#[derive(Debug, Clone)]
pub struct DepthData {
    pub bids: Vec<DepthLevel>,
    pub asks: Vec<DepthLevel>,
}

#[derive(Debug, Clone)]
pub struct DepthLevel {
    pub price: Decimal,
    pub volume: u64,
}

#[derive(Debug, Clone)]
pub struct BrokerSeat {
    pub broker_id: String,
    pub side: Side,
    pub position: u8,
}

/// Per-symbol per-channel pressure scalar (channel net = local + propagated).
/// 6 entries expected, one per PressureChannel variant.
#[derive(Debug, Clone)]
pub struct PressureSnapshot {
    pub order_book: Decimal,
    pub capital_flow: Decimal,
    pub institutional: Decimal,
    pub momentum: Decimal,
    pub volume: Decimal,
    pub structure: Decimal,
    /// Signed composite across channels (+ bullish, - bearish).
    pub composite: Decimal,
    /// Convergence (channels agree) [0,1].
    pub convergence: Decimal,
    /// Conflict (channels disagree) [0,1].
    pub conflict: Decimal,
}

/// Update each sub-KG's 6 Pressure nodes from a per-symbol pressure
/// snapshot. composite/convergence/conflict are stashed as aux on
/// PressureOrderBook (no dedicated node yet — Phase 2 may add).
pub fn update_from_pressure(
    registry: &mut SubKgRegistry,
    pressures: &HashMap<String, PressureSnapshot>,
    ts: DateTime<Utc>,
    tick: u64,
) {
    for (sym, p) in pressures {
        let kg = registry.upsert(sym, ts);
        kg.set_tick(tick, ts);
        kg.set_node_value(NodeId::PressureOrderBook, p.order_book, ts);
        kg.set_node_value(NodeId::PressureCapitalFlow, p.capital_flow, ts);
        kg.set_node_value(NodeId::PressureInstitutional, p.institutional, ts);
        kg.set_node_value(NodeId::PressureMomentum, p.momentum, ts);
        kg.set_node_value(NodeId::PressureVolume, p.volume, ts);
        kg.set_node_value(NodeId::PressureStructure, p.structure, ts);
        // Composite/convergence/conflict piggyback on OrderBook aux for now.
        kg.set_node_aux(NodeId::PressureOrderBook, p.composite, ts);
    }
}

/// Per-symbol categorical posterior over 5 IntentBelief modes.
/// Probabilities should sum to ~1.0 (uniform = 0.2 each).
#[derive(Debug, Clone)]
pub struct IntentSnapshot {
    pub accumulation: Decimal,
    pub distribution: Decimal,
    pub rotation: Decimal,
    pub volatility: Decimal,
    pub unknown: Decimal,
    /// Effective sample count (Welford n).
    pub n: u64,
}

/// Write each sub-KG's 5 IntentMode nodes' posterior as node value
/// (probability) and effective n as aux.
pub fn update_from_intent(
    registry: &mut SubKgRegistry,
    intents: &HashMap<String, IntentSnapshot>,
    ts: DateTime<Utc>,
    tick: u64,
) {
    for (sym, i) in intents {
        let kg = registry.upsert(sym, ts);
        kg.set_tick(tick, ts);
        let n = Decimal::from(i.n);
        kg.set_node_value(NodeId::IntentAccumulation, i.accumulation, ts);
        kg.set_node_aux(NodeId::IntentAccumulation, n, ts);
        kg.set_node_value(NodeId::IntentDistribution, i.distribution, ts);
        kg.set_node_aux(NodeId::IntentDistribution, n, ts);
        kg.set_node_value(NodeId::IntentRotation, i.rotation, ts);
        kg.set_node_aux(NodeId::IntentRotation, n, ts);
        kg.set_node_value(NodeId::IntentVolatility, i.volatility, ts);
        kg.set_node_aux(NodeId::IntentVolatility, n, ts);
        kg.set_node_value(NodeId::IntentUnknown, i.unknown, ts);
        kg.set_node_aux(NodeId::IntentUnknown, n, ts);
    }
}

pub struct SubstrateEvidenceInput<'a> {
    pub decision_ledger: Option<&'a DecisionLedger>,
    pub synthetic_outcomes: &'a [CaseRealizedOutcome],
    pub engram_summary: Option<&'a AnalogSummary>,
    pub wl_analogs_by_symbol: Option<&'a HashMap<String, AnalogMatch>>,
    pub belief_field: Option<&'a PressureBeliefField>,
    pub forecast_accuracy_by_symbol: Option<&'a HashMap<String, f64>>,
    /// V3.2 cross-ontology: per-symbol parent-sector intent posterior
    /// pair (Accumulation, Distribution) ∈ [0,1]². Symbols sharing a
    /// sector get an identical pair (their sector's verdict).
    pub sector_intent_by_symbol: Option<&'a HashMap<String, (f64, f64)>>,
    /// V4 self-referential surprise: per-symbol (magnitude ∈ [0,1],
    /// direction ∈ [-1,1]) computed by `kl_surprise::KlSurpriseTracker`
    /// from the per-channel KL EWMA baselines. Symbols absent from the
    /// map default to (0, 0) — equivalent to "no surprise".
    pub kl_surprise_by_symbol: Option<&'a HashMap<String, (Decimal, Decimal)>>,
}

impl<'a> SubstrateEvidenceInput<'a> {
    pub fn empty() -> Self {
        Self {
            decision_ledger: None,
            synthetic_outcomes: &[],
            engram_summary: None,
            wl_analogs_by_symbol: None,
            belief_field: None,
            forecast_accuracy_by_symbol: None,
            sector_intent_by_symbol: None,
            kl_surprise_by_symbol: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct SubstrateEvidenceSnapshot {
    pub outcome_memory: Decimal,
    pub engram_alignment: Decimal,
    pub wl_analog_confidence: Decimal,
    pub belief_entropy: Decimal,
    pub belief_sample_count: Decimal,
    pub forecast_accuracy: Decimal,
    pub sector_intent_bull: Decimal,
    pub sector_intent_bear: Decimal,
    pub kl_surprise_magnitude: Decimal,
    pub kl_surprise_direction: Decimal,
}

pub fn build_substrate_evidence_snapshots(
    registry: &SubKgRegistry,
    input: SubstrateEvidenceInput<'_>,
) -> HashMap<String, SubstrateEvidenceSnapshot> {
    let symbols: Vec<String> = registry.graphs.keys().cloned().collect();
    let raw_outcome_memory = raw_outcome_memory_by_symbol(&symbols, &input);
    let outcome_ref = raw_outcome_memory
        .values()
        .map(|v| v.abs())
        .fold(0.0_f64, f64::max);
    let engram_alignment = input
        .engram_summary
        .and_then(|summary| summary.outcomes.get("30"))
        .map(|stats| signed_unit_decimal(stats.mean_bull_bias_delta))
        .unwrap_or(Decimal::ZERO);
    let wl_ref = input
        .wl_analogs_by_symbol
        .map(|m| m.values().map(|a| a.historical_visits).max().unwrap_or(0))
        .unwrap_or(0);
    let belief_sample_ref = symbols
        .iter()
        .filter_map(|sym| {
            input
                .belief_field
                .and_then(|field| field.query_state_posterior(&Symbol(sym.clone())))
                .map(|belief| belief.sample_count)
        })
        .max()
        .unwrap_or(0);

    symbols
        .into_iter()
        .map(|sym| {
            let outcome_memory = raw_outcome_memory
                .get(&sym)
                .copied()
                .map(|value| {
                    if outcome_ref > 0.0 {
                        signed_unit_decimal(value / outcome_ref)
                    } else {
                        Decimal::ZERO
                    }
                })
                .unwrap_or(Decimal::ZERO);
            let wl_analog_confidence = input
                .wl_analogs_by_symbol
                .and_then(|m| m.get(&sym))
                .map(|analog| log_normalized(analog.historical_visits, wl_ref))
                .unwrap_or(Decimal::ZERO);
            let (belief_entropy, belief_sample_count) = input
                .belief_field
                .and_then(|field| field.query_state_posterior(&Symbol(sym.clone())))
                .map(|belief| {
                    let max_entropy = (belief.variants.len().max(1) as f64).ln();
                    let entropy = belief.entropy().unwrap_or(0.0);
                    let entropy_norm = if max_entropy > 0.0 {
                        (entropy / max_entropy).clamp(0.0, 1.0)
                    } else {
                        0.0
                    };
                    let sample_norm = if belief_sample_ref > 0 {
                        belief.sample_count as f64 / belief_sample_ref as f64
                    } else {
                        0.0
                    };
                    (unit_decimal(entropy_norm), unit_decimal(sample_norm))
                })
                .unwrap_or((Decimal::ZERO, Decimal::ZERO));
            let forecast_accuracy = input
                .forecast_accuracy_by_symbol
                .and_then(|m| m.get(&sym))
                .copied()
                .map(unit_decimal)
                .unwrap_or(Decimal::new(5, 1));
            let (sector_intent_bull, sector_intent_bear) = input
                .sector_intent_by_symbol
                .and_then(|m| m.get(&sym))
                .copied()
                .map(|(bull, bear)| (unit_decimal(bull), unit_decimal(bear)))
                .unwrap_or((Decimal::ZERO, Decimal::ZERO));
            let (kl_surprise_magnitude, kl_surprise_direction) = input
                .kl_surprise_by_symbol
                .and_then(|m| m.get(&sym))
                .copied()
                .unwrap_or((Decimal::ZERO, Decimal::ZERO));

            (
                sym,
                SubstrateEvidenceSnapshot {
                    outcome_memory,
                    engram_alignment,
                    wl_analog_confidence,
                    belief_entropy,
                    belief_sample_count,
                    forecast_accuracy,
                    sector_intent_bull,
                    sector_intent_bear,
                    kl_surprise_magnitude,
                    kl_surprise_direction,
                },
            )
        })
        .collect()
}

pub fn update_from_substrate_evidence(
    registry: &mut SubKgRegistry,
    evidence: &HashMap<String, SubstrateEvidenceSnapshot>,
    ts: DateTime<Utc>,
    tick: u64,
) {
    for (sym, snapshot) in evidence {
        let kg = registry.upsert(sym, ts);
        kg.set_tick(tick, ts);
        kg.set_node_value(NodeId::OutcomeMemory, snapshot.outcome_memory, ts);
        kg.set_node_value(NodeId::EngramAlignment, snapshot.engram_alignment, ts);
        kg.set_node_value(
            NodeId::WlAnalogConfidence,
            snapshot.wl_analog_confidence,
            ts,
        );
        kg.set_node_value(NodeId::BeliefEntropy, snapshot.belief_entropy, ts);
        kg.set_node_value(NodeId::BeliefSampleCount, snapshot.belief_sample_count, ts);
        kg.set_node_value(NodeId::ForecastAccuracy, snapshot.forecast_accuracy, ts);
        kg.set_node_value(NodeId::SectorIntentBull, snapshot.sector_intent_bull, ts);
        kg.set_node_value(NodeId::SectorIntentBear, snapshot.sector_intent_bear, ts);
        kg.set_node_value(
            NodeId::KlSurpriseMagnitude,
            snapshot.kl_surprise_magnitude,
            ts,
        );
        kg.set_node_value(
            NodeId::KlSurpriseDirection,
            snapshot.kl_surprise_direction,
            ts,
        );
    }
}

fn raw_outcome_memory_by_symbol(
    symbols: &[String],
    input: &SubstrateEvidenceInput<'_>,
) -> HashMap<String, f64> {
    let mut out = HashMap::new();
    for sym in symbols {
        let mut values: Vec<(i128, f64)> = Vec::new();
        if let Some(ledger) = input.decision_ledger {
            let symbol = Symbol(sym.clone());
            for record in ledger.decisions_for(&symbol) {
                if let Some(outcome) = &record.outcome {
                    values.push((
                        record.timestamp.timestamp_millis() as i128,
                        outcome.pnl_bps / 10_000.0,
                    ));
                }
            }
        }
        for outcome in input
            .synthetic_outcomes
            .iter()
            .filter(|o| o.symbol.as_deref() == Some(sym.as_str()))
        {
            values.push((
                outcome.resolved_tick as i128,
                outcome.net_return.to_f64().unwrap_or(0.0),
            ));
        }
        values.sort_by(|a, b| b.0.cmp(&a.0));
        let recent = values.into_iter().take(20).map(|(_, value)| value);
        let mut total = 0.0_f64;
        let mut count = 0usize;
        for value in recent {
            total += value;
            count += 1;
        }
        if count > 0 {
            out.insert(sym.clone(), total / count as f64);
        }
    }
    out
}

fn log_normalized(value: usize, reference: usize) -> Decimal {
    if reference == 0 {
        return Decimal::ZERO;
    }
    let numerator = (value as f64 + 1.0).ln();
    let denominator = (reference as f64 + 1.0).ln();
    unit_decimal(numerator / denominator)
}

fn unit_decimal(value: f64) -> Decimal {
    Decimal::try_from(value.clamp(0.0, 1.0)).unwrap_or(Decimal::ZERO)
}

fn signed_unit_decimal(value: f64) -> Decimal {
    Decimal::try_from(value.clamp(-1.0, 1.0)).unwrap_or(Decimal::ZERO)
}

/// Per-broker dominant archetype with posterior probability.
/// broker_id is opaque string; both `(broker_id, archetype, prob, n)` keyed.
#[derive(Debug, Clone)]
pub struct BrokerArchetypeSnapshot {
    pub broker_id: String,
    pub archetype_label: String, // e.g., "Accumulative", "Distributive", ...
    pub posterior_prob: Decimal, // dominant variant probability [0,1]
    pub sample_count: u64,
}

/// Walk every sub-KG in registry, set the archetype label + dominant
/// posterior prob on every Broker node we have data for.
/// Brokers appearing in multiple symbols' sub-KGs all get the same
/// archetype (broker identity is ontology-level, not per-symbol).
pub fn update_from_broker_archetype(
    registry: &mut SubKgRegistry,
    archetypes: &HashMap<String, BrokerArchetypeSnapshot>,
    ts: DateTime<Utc>,
    tick: u64,
) {
    for kg in registry.graphs.values_mut() {
        kg.set_tick(tick, ts);
        let broker_node_ids: Vec<NodeId> = kg
            .nodes
            .keys()
            .filter(|id| matches!(id, NodeId::Broker(_)))
            .cloned()
            .collect();
        for nid in broker_node_ids {
            let bid = match &nid {
                NodeId::Broker(b) => b.clone(),
                _ => continue,
            };
            if let Some(arch) = archetypes.get(&bid) {
                if let Some(node) = kg.nodes.get_mut(&nid) {
                    let sample_count = Decimal::from(arch.sample_count);
                    let changed = node.label.as_deref() != Some(arch.archetype_label.as_str())
                        || node.value != Some(arch.posterior_prob)
                        || node.aux != Some(sample_count);
                    node.label = Some(arch.archetype_label.clone());
                    // Replace presence (1.0) with posterior probability so
                    // archetype confidence is the broker node's value.
                    node.value = Some(arch.posterior_prob);
                    node.aux = Some(sample_count);
                    node.mark_seen(
                        ts,
                        kg.tick,
                        changed,
                        NodeUpdateProvenance::new(
                            NodeProvenanceSource::BrokerQueue,
                            Some(MarketDataCapability::BrokerQueue),
                        ),
                    );
                }
            }
        }
    }
}

/// HK warrant pool snapshot: per-underlying call/put counts + IV gap.
#[derive(Debug, Clone, Default)]
pub struct WarrantPoolSnapshot {
    pub call_warrant_count: u32,
    pub put_warrant_count: u32,
    /// Weighted call IV - put IV gap. Positive = calls richer.
    pub iv_gap: Decimal,
}

pub fn update_from_warrant_pool(
    registry: &mut SubKgRegistry,
    pools: &HashMap<String, WarrantPoolSnapshot>,
    ts: DateTime<Utc>,
    tick: u64,
) {
    for (sym, w) in pools {
        let kg = registry.upsert(sym, ts);
        kg.set_tick(tick, ts);
        let total = (w.call_warrant_count + w.put_warrant_count) as i64;
        kg.set_node_value(
            NodeId::CallWarrantCount,
            Decimal::from(w.call_warrant_count as i64),
            ts,
        );
        kg.set_node_value(
            NodeId::PutWarrantCount,
            Decimal::from(w.put_warrant_count as i64),
            ts,
        );
        kg.set_node_value(NodeId::WarrantIvGap, w.iv_gap, ts);
        if total > 0 {
            let total_d = Decimal::from(total);
            kg.set_node_value(
                NodeId::CallWarrantShare,
                Decimal::from(w.call_warrant_count as i64) / total_d,
                ts,
            );
            kg.set_node_value(
                NodeId::PutWarrantShare,
                Decimal::from(w.put_warrant_count as i64) / total_d,
                ts,
            );
        }
    }
}

/// Per-symbol cumulative capital flow (positive = inflow).
#[derive(Debug, Clone, Default)]
pub struct CapitalFlowSnapshot {
    pub cumulative_inflow: Decimal,
    /// 30-min derivative of cumulative inflow.
    pub accel_last_30m: Decimal,
}

pub fn update_from_capital_flow(
    registry: &mut SubKgRegistry,
    flows: &HashMap<String, CapitalFlowSnapshot>,
    ts: DateTime<Utc>,
    tick: u64,
) {
    for (sym, f) in flows {
        let kg = registry.upsert(sym, ts);
        kg.set_tick(tick, ts);
        kg.set_node_value(NodeId::CapitalFlowCum, f.cumulative_inflow, ts);
        kg.set_node_value(NodeId::CapitalFlowAccelLast30m, f.accel_last_30m, ts);
    }
}

/// Set the SessionPhase node label (e.g., "PreOpen", "Morning", "Lunch",
/// "Afternoon", "Closing", "PostClose"). Same value broadcast to ALL
/// symbols' sub-KGs (session phase is market-wide).
pub fn update_from_session_phase(
    registry: &mut SubKgRegistry,
    phase_label: &str,
    ts: DateTime<Utc>,
    tick: u64,
) {
    for kg in registry.graphs.values_mut() {
        kg.set_tick(tick, ts);
        kg.set_node_label(NodeId::SessionPhase, phase_label.to_string(), ts);
        kg.set_node_value(NodeId::SessionPhase, Decimal::ONE, ts);
    }
}

/// Per-symbol earnings calendar info.
#[derive(Debug, Clone)]
pub struct EarningsSnapshot {
    pub days_until_next: i32,
    pub in_window: bool, // within ±N days of earnings
}

pub fn update_from_earnings(
    registry: &mut SubKgRegistry,
    earnings: &HashMap<String, EarningsSnapshot>,
    ts: DateTime<Utc>,
    tick: u64,
) {
    for (sym, e) in earnings {
        let kg = registry.upsert(sym, ts);
        kg.set_tick(tick, ts);
        kg.set_node_value(
            NodeId::NextEarningsDays,
            Decimal::from(e.days_until_next as i64),
            ts,
        );
        kg.set_node_value(
            NodeId::InEarningsWindow,
            if e.in_window {
                Decimal::ONE
            } else {
                Decimal::ZERO
            },
            ts,
        );
    }
}

/// Cross-market bridge: HK symbol → US ADR mapping (if exists).
pub fn update_cross_market_bridge(
    registry: &mut SubKgRegistry,
    bridges: &HashMap<String, String>, // sym → counterpart symbol
    ts: DateTime<Utc>,
    tick: u64,
) {
    for (sym, counterpart) in bridges {
        let kg = registry.upsert(sym, ts);
        kg.set_tick(tick, ts);
        kg.set_node_label(NodeId::CrossMarketBridge, counterpart.clone(), ts);
        kg.set_node_value(NodeId::CrossMarketBridge, Decimal::ONE, ts);
    }
}

/// Per-symbol index membership (e.g., "HSI" / "HSCEI" / "SP500").
pub fn update_index_membership(
    registry: &mut SubKgRegistry,
    memberships: &HashMap<String, Vec<String>>,
    ts: DateTime<Utc>,
    tick: u64,
) {
    for (sym, indices) in memberships {
        let kg = registry.upsert(sym, ts);
        kg.set_tick(tick, ts);
        if !indices.is_empty() {
            kg.set_node_label(NodeId::IndexMembership, indices.join(","), ts);
            kg.set_node_value(
                NodeId::IndexMembership,
                Decimal::from(indices.len() as i64),
                ts,
            );
        }
    }
}

/// Per-symbol microstructure snapshot.
#[derive(Debug, Clone, Default)]
pub struct MicrostructureSnapshot {
    pub trade_tape_buy_minus_sell_30s: Decimal,
    pub trade_tape_accel_last_1m: Decimal,
    pub depth_asymmetry_top3: Decimal, // bid_top3 / (bid_top3 + ask_top3)
    pub queue_stability_bid1: Decimal, // ticks bid1 unchanged
    pub queue_stability_ask1: Decimal,
    pub vwap: Decimal,
    pub vwap_deviation_pct: Decimal,
}

pub fn update_from_microstructure(
    registry: &mut SubKgRegistry,
    snaps: &HashMap<String, MicrostructureSnapshot>,
    ts: DateTime<Utc>,
    tick: u64,
) {
    for (sym, m) in snaps {
        let kg = registry.upsert(sym, ts);
        kg.set_tick(tick, ts);
        kg.set_node_value(
            NodeId::TradeTapeBuyMinusSell30s,
            m.trade_tape_buy_minus_sell_30s,
            ts,
        );
        kg.set_node_value(NodeId::TradeTapeAccelLast1m, m.trade_tape_accel_last_1m, ts);
        kg.set_node_value(NodeId::DepthAsymmetryTop3, m.depth_asymmetry_top3, ts);
        kg.set_node_value(NodeId::QueueStabilityBid1, m.queue_stability_bid1, ts);
        kg.set_node_value(NodeId::QueueStabilityAsk1, m.queue_stability_ask1, ts);
        kg.set_node_value(NodeId::Vwap, m.vwap, ts);
        kg.set_node_value(NodeId::VwapDeviationPct, m.vwap_deviation_pct, ts);
    }
}

/// Event flags per symbol.
#[derive(Debug, Clone, Default)]
pub struct EventSnapshot {
    pub big_trade_count_last_1h: u32,
    pub has_halted_today: bool,
    pub volume_spike_fresh: bool,
}

pub fn update_from_events(
    registry: &mut SubKgRegistry,
    events: &HashMap<String, EventSnapshot>,
    ts: DateTime<Utc>,
    tick: u64,
) {
    for (sym, e) in events {
        let kg = registry.upsert(sym, ts);
        kg.set_tick(tick, ts);
        kg.set_node_value(
            NodeId::BigTradeCountLast1h,
            Decimal::from(e.big_trade_count_last_1h as i64),
            ts,
        );
        kg.set_node_value(
            NodeId::HasHaltedToday,
            if e.has_halted_today {
                Decimal::ONE
            } else {
                Decimal::ZERO
            },
            ts,
        );
        kg.set_node_value(
            NodeId::VolumeSpikeFresh,
            if e.volume_spike_fresh {
                Decimal::ONE
            } else {
                Decimal::ZERO
            },
            ts,
        );
    }
}

/// Cross-symbol role within sector cluster.
#[derive(Debug, Clone, Default)]
pub struct RoleSnapshot {
    pub leader_laggard_score: Decimal,     // signed
    pub sector_relative_strength: Decimal, // % deviation from sector
}

pub fn update_from_roles(
    registry: &mut SubKgRegistry,
    roles: &HashMap<String, RoleSnapshot>,
    ts: DateTime<Utc>,
    tick: u64,
) {
    for (sym, r) in roles {
        let kg = registry.upsert(sym, ts);
        kg.set_tick(tick, ts);
        kg.set_node_value(NodeId::LeaderLaggardScore, r.leader_laggard_score, ts);
        kg.set_node_value(
            NodeId::SectorRelativeStrength,
            r.sector_relative_strength,
            ts,
        );
    }
}

/// Per-symbol holding structure (mostly static day-over-day).
#[derive(Debug, Clone, Default)]
pub struct HoldingSnapshot {
    pub insider_holding_pct: Decimal,
    pub institutional_holder_count: u32,
    pub southbound_flow_today: Decimal,
    pub etf_holding_pct: Decimal,
}

pub fn update_from_holdings(
    registry: &mut SubKgRegistry,
    holdings: &HashMap<String, HoldingSnapshot>,
    ts: DateTime<Utc>,
    tick: u64,
) {
    for (sym, h) in holdings {
        let kg = registry.upsert(sym, ts);
        kg.set_tick(tick, ts);
        kg.set_node_value(NodeId::InsiderHoldingPct, h.insider_holding_pct, ts);
        kg.set_node_value(
            NodeId::InstitutionalHolderCount,
            Decimal::from(h.institutional_holder_count as i64),
            ts,
        );
        kg.set_node_value(NodeId::SouthboundFlowToday, h.southbound_flow_today, ts);
        kg.set_node_value(NodeId::EtfHoldingPct, h.etf_holding_pct, ts);
    }
}

/// Option surface → Option NodeKind nodes. Only writes fields that are
/// present on the observation — missing atm_call_iv / skew / ratio leave
/// the corresponding node at its previous value (no zero-poisoning).
pub fn update_from_option_surfaces(
    registry: &mut SubKgRegistry,
    surfaces: &HashMap<String, OptionSurfaceFields>,
    ts: DateTime<Utc>,
    tick: u64,
) {
    for (sym, s) in surfaces {
        let kg = registry.upsert(sym, ts);
        kg.set_tick(tick, ts);
        if let Some(v) = s.atm_call_iv {
            kg.set_node_value(NodeId::OptionAtmCallIv, v, ts);
        }
        if let Some(v) = s.atm_put_iv {
            kg.set_node_value(NodeId::OptionAtmPutIv, v, ts);
        }
        if let Some(v) = s.put_call_skew {
            kg.set_node_value(NodeId::OptionPutCallSkew, v, ts);
        }
        if let Some(v) = s.put_call_oi_ratio {
            kg.set_node_value(NodeId::OptionPutCallOiRatio, v, ts);
        }
        if s.total_oi > 0 {
            kg.set_node_value(NodeId::OptionTotalOi, Decimal::from(s.total_oi), ts);
        }
    }
}

/// Narrow view over OptionSurfaceObservation fields the sub-KG needs.
/// Kept separate from the canonical `OptionSurfaceObservation` type so
/// this module doesn't depend on `ontology::links`.
#[derive(Debug, Clone, Default)]
pub struct OptionSurfaceFields {
    pub atm_call_iv: Option<Decimal>,
    pub atm_put_iv: Option<Decimal>,
    pub put_call_skew: Option<Decimal>,
    pub put_call_oi_ratio: Option<Decimal>,
    pub total_oi: i64,
}

/// Per-symbol classification label (stable enum from state_engine).
pub fn update_from_state(
    registry: &mut SubKgRegistry,
    states: &HashMap<String, String>,
    ts: DateTime<Utc>,
    tick: u64,
) {
    for (sym, label) in states {
        let kg = registry.upsert(sym, ts);
        kg.set_tick(tick, ts);
        kg.set_node_label(NodeId::StateClassification, label.clone(), ts);
        // Keep node "active" so snapshot doesn't skip it
        kg.set_node_value(NodeId::StateClassification, Decimal::ONE, ts);
    }
}

/// Append pre-serialized sub-KG lines to the per-market NDJSON file.
/// 2026-04-29: pure IO step — pairs with
/// [`SubKgRegistry::serialize_active_to_lines`]. Background NDJSON
/// writer task calls this; runtime consumer never blocks on it.
pub fn append_subkg_lines_to_ndjson(market: &str, lines: &[String]) -> std::io::Result<usize> {
    if lines.is_empty() {
        return Ok(0);
    }
    let path = format!(".run/eden-subkg-{}.ndjson", market);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let mut written = 0usize;
    for line in lines {
        writeln!(file, "{}", line)?;
        written += 1;
    }
    Ok(written)
}

// ---------------- Tests ----------------

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;

    #[test]
    fn empty_sub_kg_has_102_fixed_nodes() {
        let kg = SymbolSubKG::new_empty("981.HK".into(), Utc::now());
        // 82 singletons (67 + 5 option + 6 substrate evidence + 2 sector
        // intent V3.2 + 2 KL surprise V4) + 10 bid + 10 ask = 102
        assert_eq!(kg.nodes.len(), 102);
    }

    #[test]
    fn empty_sub_kg_contains_canonical_node_ids() {
        let kg = SymbolSubKG::new_empty("981.HK".into(), Utc::now());
        for id in [
            NodeId::Symbol,
            NodeId::LastPrice,
            NodeId::PressureOrderBook,
            NodeId::IntentAccumulation,
            NodeId::BidLevel(1),
            NodeId::BidLevel(10),
            NodeId::AskLevel(10),
            NodeId::Time30min,
            NodeId::SectorRef,
        ] {
            assert!(kg.nodes.contains_key(&id), "missing {:?}", id);
        }
    }

    #[test]
    fn book_chain_has_18_edges() {
        let kg = SymbolSubKG::new_empty("981.HK".into(), Utc::now());
        let chain: Vec<_> = kg
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::BookChain)
            .collect();
        assert_eq!(chain.len(), 18); // 9 bid + 9 ask
    }

    #[test]
    fn book_opposing_has_10_edges() {
        let kg = SymbolSubKG::new_empty("981.HK".into(), Utc::now());
        let opposing: Vec<_> = kg
            .edges
            .iter()
            .filter(|e| e.kind == EdgeKind::BookOpposing)
            .collect();
        assert_eq!(opposing.len(), 10);
    }

    #[test]
    fn add_broker_creates_node_and_edge() {
        let mut kg = SymbolSubKG::new_empty("981.HK".into(), Utc::now());
        let edges_before = kg.edges.len();
        kg.add_or_update_broker("4828".into(), Some((Side::Bid, 2)), Utc::now());
        assert!(kg.nodes.contains_key(&NodeId::Broker("4828".into())));
        assert_eq!(kg.edges.len() - edges_before, 1);
    }

    #[test]
    fn re_add_same_broker_position_increments_weight() {
        let mut kg = SymbolSubKG::new_empty("981.HK".into(), Utc::now());
        kg.add_or_update_broker("4828".into(), Some((Side::Bid, 2)), Utc::now());
        kg.add_or_update_broker("4828".into(), Some((Side::Bid, 2)), Utc::now());
        kg.add_or_update_broker("4828".into(), Some((Side::Bid, 2)), Utc::now());
        let edges: Vec<_> = kg
            .edges
            .iter()
            .filter(|e| matches!(&e.from, NodeId::Broker(b) if b == "4828"))
            .collect();
        assert_eq!(edges.len(), 1);
        assert_eq!(edges[0].weight, Decimal::from(3));
    }

    #[test]
    fn broker_can_have_edges_to_both_sides() {
        let mut kg = SymbolSubKG::new_empty("981.HK".into(), Utc::now());
        kg.add_or_update_broker("4828".into(), Some((Side::Bid, 1)), Utc::now());
        kg.add_or_update_broker("4828".into(), Some((Side::Ask, 1)), Utc::now());
        let edges: Vec<_> = kg
            .edges
            .iter()
            .filter(|e| matches!(&e.from, NodeId::Broker(b) if b == "4828"))
            .collect();
        assert_eq!(edges.len(), 2);
    }

    #[test]
    fn set_node_value_marks_change() {
        let mut kg = SymbolSubKG::new_empty("981.HK".into(), Utc::now());
        kg.set_tick(7, Utc::now());
        kg.set_node_value(NodeId::LastPrice, dec!(50.75), Utc::now());
        let node = kg.nodes.get(&NodeId::LastPrice).unwrap();
        assert_eq!(node.value, Some(dec!(50.75)));
        assert_eq!(node.age_ticks, 0);
        assert_eq!(node.last_seen_tick, 7);
        assert_eq!(node.freshness, NodeFreshness::Fresh);
        assert_eq!(node.provenance_source, NodeProvenanceSource::MarketQuote);
    }

    #[test]
    fn set_node_value_unchanged_increments_age() {
        let mut kg = SymbolSubKG::new_empty("981.HK".into(), Utc::now());
        kg.set_node_value(NodeId::LastPrice, dec!(50.75), Utc::now());
        kg.set_node_value(NodeId::LastPrice, dec!(50.75), Utc::now());
        kg.set_node_value(NodeId::LastPrice, dec!(50.75), Utc::now());
        let node = kg.nodes.get(&NodeId::LastPrice).unwrap();
        assert_eq!(node.age_ticks, 2);
        assert_eq!(node.freshness, NodeFreshness::Unchanged);
    }

    #[test]
    fn node_provenance_maps_market_capability_for_depth_broker_and_options() {
        let mut kg = SymbolSubKG::new_empty("981.HK".into(), Utc::now());
        kg.set_tick(12, Utc::now());
        kg.set_node_value(NodeId::BidLevel(1), dec!(50.1), Utc::now());
        kg.add_or_update_broker("4828".into(), Some((Side::Bid, 1)), Utc::now());
        kg.set_node_value(NodeId::OptionPutCallOiRatio, dec!(1.2), Utc::now());

        let bid = kg.nodes.get(&NodeId::BidLevel(1)).unwrap();
        let broker = kg.nodes.get(&NodeId::Broker("4828".into())).unwrap();
        let option = kg.nodes.get(&NodeId::OptionPutCallOiRatio).unwrap();

        assert_eq!(bid.provenance_source, NodeProvenanceSource::MarketDepth);
        assert_eq!(bid.market_capability, Some(MarketDataCapability::DepthL2));
        assert_eq!(broker.provenance_source, NodeProvenanceSource::BrokerQueue);
        assert_eq!(
            broker.market_capability,
            Some(MarketDataCapability::BrokerQueue)
        );
        assert_eq!(
            option.provenance_source,
            NodeProvenanceSource::OptionSurface
        );
        assert_eq!(
            option.market_capability,
            Some(MarketDataCapability::OptionSurface)
        );
    }

    #[test]
    fn nodes_of_kind_filters_correctly() {
        let kg = SymbolSubKG::new_empty("981.HK".into(), Utc::now());
        let pressures = kg.nodes_of_kind(NodeKind::Pressure);
        assert_eq!(pressures.len(), 6);
        let intents = kg.nodes_of_kind(NodeKind::Intent);
        assert_eq!(intents.len(), 5);
        let bids = kg.nodes_of_kind(NodeKind::BidDepth);
        assert_eq!(bids.len(), 10);
    }

    #[test]
    fn template_edge_count_is_deterministic() {
        let kg1 = SymbolSubKG::new_empty("X.HK".into(), Utc::now());
        let kg2 = SymbolSubKG::new_empty("Y.HK".into(), Utc::now());
        assert_eq!(kg1.edges.len(), kg2.edges.len());
        // Same number of edges across symbols (template is fixed).
        assert!(kg1.edges.len() > 50);
    }

    #[test]
    fn decay_brokers_zeros_old_presence() {
        let mut kg = SymbolSubKG::new_empty("981.HK".into(), Utc::now());
        kg.add_or_update_broker("4828".into(), None, Utc::now());
        // Bump age by decaying many times
        for _ in 0..15 {
            kg.decay_brokers(10, kg.tick);
        }
        let node = kg.nodes.get(&NodeId::Broker("4828".into())).unwrap();
        assert_eq!(node.value, Some(Decimal::ZERO));
    }

    #[test]
    fn active_node_count_zero_on_empty() {
        let kg = SymbolSubKG::new_empty("X.HK".into(), Utc::now());
        assert_eq!(kg.active_node_count(), 0);
    }

    #[test]
    fn active_node_count_after_setting_values() {
        let mut kg = SymbolSubKG::new_empty("X.HK".into(), Utc::now());
        kg.set_node_value(NodeId::LastPrice, dec!(50.0), Utc::now());
        kg.set_node_value(NodeId::Volume, dec!(1000), Utc::now());
        assert_eq!(kg.active_node_count(), 2);
    }

    #[test]
    fn registry_upsert_creates_on_first_call() {
        let mut reg = SubKgRegistry::new();
        assert!(reg.is_empty());
        reg.upsert("981.HK", Utc::now());
        assert_eq!(reg.len(), 1);
        assert!(reg.get("981.HK").is_some());
    }

    #[test]
    fn registry_upsert_returns_existing_on_second_call() {
        let mut reg = SubKgRegistry::new();
        reg.upsert("981.HK", Utc::now())
            .set_node_value(NodeId::LastPrice, dec!(50.0), Utc::now());
        // Second upsert preserves the value
        let kg = reg.upsert("981.HK", Utc::now());
        assert_eq!(
            kg.nodes.get(&NodeId::LastPrice).unwrap().value,
            Some(dec!(50.0))
        );
        assert_eq!(reg.len(), 1);
    }

    #[test]
    fn updater_quotes_populates_price_and_volume_nodes() {
        let mut reg = SubKgRegistry::new();
        let mut quotes = HashMap::new();
        quotes.insert(
            "981.HK".into(),
            QuoteData {
                last_done: dec!(50.75),
                prev_close: dec!(50.0),
                day_high: dec!(51.0),
                day_low: dec!(49.5),
                volume: dec!(1_000_000),
                turnover: dec!(50_500_000),
            },
        );
        update_from_quotes_depths_brokers(
            &mut reg,
            &quotes,
            &HashMap::new(),
            &HashMap::new(),
            Utc::now(),
            5,
        );
        let kg = reg.get("981.HK").unwrap();
        assert_eq!(kg.tick, 5);
        assert_eq!(
            kg.nodes.get(&NodeId::LastPrice).unwrap().value,
            Some(dec!(50.75))
        );
        assert_eq!(
            kg.nodes.get(&NodeId::Volume).unwrap().value,
            Some(dec!(1_000_000))
        );
    }

    #[test]
    fn updater_depths_populates_book_levels_and_derives_spread_mid() {
        let mut reg = SubKgRegistry::new();
        let mut depths = HashMap::new();
        depths.insert(
            "981.HK".into(),
            DepthData {
                bids: vec![
                    DepthLevel {
                        price: dec!(50.7),
                        volume: 100,
                    },
                    DepthLevel {
                        price: dec!(50.6),
                        volume: 200,
                    },
                ],
                asks: vec![
                    DepthLevel {
                        price: dec!(50.8),
                        volume: 150,
                    },
                    DepthLevel {
                        price: dec!(50.9),
                        volume: 250,
                    },
                ],
            },
        );
        update_from_quotes_depths_brokers(
            &mut reg,
            &HashMap::new(),
            &depths,
            &HashMap::new(),
            Utc::now(),
            1,
        );
        let kg = reg.get("981.HK").unwrap();
        assert_eq!(
            kg.nodes.get(&NodeId::BidLevel(1)).unwrap().value,
            Some(dec!(50.7))
        );
        assert_eq!(
            kg.nodes.get(&NodeId::AskLevel(2)).unwrap().value,
            Some(dec!(50.9))
        );
        assert_eq!(
            kg.nodes.get(&NodeId::Spread).unwrap().value,
            Some(dec!(0.1))
        );
        assert_eq!(
            kg.nodes.get(&NodeId::MidPrice).unwrap().value,
            Some(dec!(50.75))
        );
    }

    #[test]
    fn updater_brokers_creates_broker_nodes_and_edges() {
        let mut reg = SubKgRegistry::new();
        let mut brokers = HashMap::new();
        brokers.insert(
            "981.HK".into(),
            vec![
                BrokerSeat {
                    broker_id: "4828".into(),
                    side: Side::Bid,
                    position: 1,
                },
                BrokerSeat {
                    broker_id: "9876".into(),
                    side: Side::Ask,
                    position: 1,
                },
            ],
        );
        update_from_quotes_depths_brokers(
            &mut reg,
            &HashMap::new(),
            &HashMap::new(),
            &brokers,
            Utc::now(),
            1,
        );
        let kg = reg.get("981.HK").unwrap();
        assert!(kg.nodes.contains_key(&NodeId::Broker("4828".into())));
        assert!(kg.nodes.contains_key(&NodeId::Broker("9876".into())));
        let broker_edges: Vec<_> = kg
            .edges
            .iter()
            .filter(|e| matches!(&e.from, NodeId::Broker(_)))
            .collect();
        assert_eq!(broker_edges.len(), 2);
    }

    #[test]
    fn registry_summary_line_formatted() {
        let mut reg = SubKgRegistry::new();
        reg.upsert("981.HK", Utc::now())
            .set_node_value(NodeId::LastPrice, dec!(50.0), Utc::now());
        reg.upsert("1347.HK", Utc::now())
            .set_node_value(NodeId::Volume, dec!(1000), Utc::now());
        let s = reg.summary_line("hk");
        assert!(s.contains("symbols=2"));
        assert!(s.contains("active_nodes=2"));
    }
}
