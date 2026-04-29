//! Ontology emergence — Y#0 first piece.
//!
//! Eden's existing `TensionDriver` classifier forces every vortex into
//! one of 6 pre-defined buckets (TradeFlowDriven, CapitalFlowDriven,
//! MicrostructureDriven, InstitutionalDriven, BroadStructural,
//! SingleChannel). When the actual data doesn't fit any bucket well,
//! the classifier still picks one — and the forced fit is invisible to
//! the operator.
//!
//! This module tracks raw **vortex fingerprints** independent of the
//! classifier. A fingerprint is `(tense_channels, direction_sign, phase)`.
//! Over time we see which fingerprints recur often; more importantly,
//! we see which fingerprints get classified **inconsistently** (same
//! shape, different driver labels). Inconsistent-but-frequent
//! fingerprints are the seeds of new entity types that don't yet
//! exist in our ontology.
//!
//! Fingerprinting observes and surfaces; when streak thresholds are met
//! (see `ResidualPatternTracker::evaluate_proposals`), a candidate name is
//! emitted in wake (`ontology proposal: ...`). Accepting a proposal into
//! the live ontology (new entity types) remains a future spec.
//!
//! Y spirit here: let the data grow the model. Every observation
//! reshapes the fingerprint distribution; no input is thrown away.

use std::collections::{HashMap, HashSet};

use chrono::{DateTime, Utc};

use crate::ontology::objects::{Market, Symbol};
use crate::pipeline::pressure::reasoning::{AnomalyPhase, TensionDriver};
use crate::pipeline::pressure::{PressureChannel, PressureVortex};

/// Canonical signature for a pressure vortex, independent of the
/// classifier's current labelling. Two vortices with the same
/// fingerprint are structurally interchangeable from pressure-field
/// perspective.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct VortexFingerprint {
    /// Tense channels, sorted for canonical ordering.
    pub channels: Vec<PressureChannel>,
    /// Direction bucket: -1 (negative), 0 (neutral), +1 (positive).
    /// Continuous direction is bucketed so slightly-different magnitudes
    /// don't fragment the fingerprint space.
    pub direction_sign: i8,
    pub phase: AnomalyPhase,
}

impl VortexFingerprint {
    /// Build a fingerprint from a vortex.
    /// Direction bucket threshold: |hour_direction| < 0.05 → 0, else sign.
    pub fn from_vortex(vortex: &PressureVortex, phase: AnomalyPhase) -> Self {
        use rust_decimal::prelude::ToPrimitive;
        let mut channels: Vec<PressureChannel> = vortex.tense_channels.clone();
        channels.sort_by_key(|c| channel_order(*c));
        channels.dedup();
        let dir_f = vortex.hour_direction.to_f64().unwrap_or(0.0);
        let direction_sign = if dir_f > 0.05 {
            1
        } else if dir_f < -0.05 {
            -1
        } else {
            0
        };
        Self {
            channels,
            direction_sign,
            phase,
        }
    }

    /// Human-readable label for wake / log. Example: "[capital_flow,institutional]+/growing"
    pub fn label(&self) -> String {
        let chs: Vec<&str> = self.channels.iter().map(|c| channel_name(*c)).collect();
        let dir = match self.direction_sign {
            1 => "+",
            -1 => "-",
            _ => "0",
        };
        let phase = match self.phase {
            AnomalyPhase::Growing => "growing",
            AnomalyPhase::Peaking => "peaking",
            AnomalyPhase::Fading => "fading",
            AnomalyPhase::New => "new",
        };
        format!("[{}]{}/{}", chs.join(","), dir, phase)
    }
}

fn channel_order(c: PressureChannel) -> u8 {
    match c {
        PressureChannel::OrderBook => 0,
        PressureChannel::CapitalFlow => 1,
        PressureChannel::Institutional => 2,
        PressureChannel::Momentum => 3,
        PressureChannel::Volume => 4,
        PressureChannel::Structure => 5,
    }
}

fn channel_name(c: PressureChannel) -> &'static str {
    match c {
        PressureChannel::OrderBook => "order_book",
        PressureChannel::CapitalFlow => "capital_flow",
        PressureChannel::Institutional => "institutional",
        PressureChannel::Momentum => "momentum",
        PressureChannel::Volume => "volume",
        PressureChannel::Structure => "structure",
    }
}

fn driver_name(d: &TensionDriver) -> &'static str {
    match d {
        TensionDriver::TradeFlowDriven => "trade_flow",
        TensionDriver::CapitalFlowDriven => "capital_flow",
        TensionDriver::MicrostructureDriven => "microstructure",
        TensionDriver::InstitutionalDriven => "institutional",
        TensionDriver::BroadStructural => "broad_structural",
        TensionDriver::SingleChannel { .. } => "single_channel",
    }
}

#[derive(Debug, Clone)]
pub struct FingerprintStats {
    pub count: u64,
    pub first_seen: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub symbols: HashSet<Symbol>,
    /// How many times each driver label was assigned for this
    /// fingerprint. A fingerprint with high count and spread over
    /// multiple driver labels → the classifier is forcing a bad fit.
    pub driver_classifications: HashMap<&'static str, u64>,
}

/// Summary of a single notable residual pattern, suitable for wake
/// emission or future ontology-emergence proposer.
#[derive(Debug, Clone)]
pub struct ResidualPatternSummary {
    pub fingerprint: VortexFingerprint,
    pub count: u64,
    pub distinct_symbols: usize,
    pub driver_label_spread: f64,
    pub dominant_driver: &'static str,
    pub dominant_driver_fraction: f64,
}

/// A fingerprint has crossed the threshold that warrants proposing it
/// as a candidate new entity type. Not an auto-accepted proposal — the
/// wake line makes it visible; operator (human or Claude Code) decides
/// whether to promote it to a real TensionDriver variant.
#[derive(Debug, Clone)]
pub struct OntologyProposal {
    pub fingerprint: VortexFingerprint,
    /// Short auto-generated name suggesting what this pattern might be.
    pub suggested_name: String,
    pub count_at_proposal: u64,
    pub symbols_at_proposal: usize,
    pub driver_spread_at_proposal: f64,
    pub first_seen: DateTime<Utc>,
    pub proposed_at: DateTime<Utc>,
    /// How many ticks the pattern has been *eligible* (above proposal
    /// threshold continuously) before emitting. Anti-noise: a noisy
    /// spike that dies immediately shouldn't produce a proposal.
    pub eligibility_streak: u32,
}

/// Thresholds for emitting a proposal. Set conservative so false
/// proposals don't flood wake.
pub struct ProposalThresholds {
    pub min_count: u64,
    pub min_driver_spread: f64,
    pub min_distinct_symbols: usize,
    pub min_eligibility_streak: u32,
}

impl Default for ProposalThresholds {
    fn default() -> Self {
        Self {
            min_count: 50,
            min_driver_spread: 0.5,
            min_distinct_symbols: 3,
            min_eligibility_streak: 5,
        }
    }
}

/// Per-market residual pattern tracker.
pub struct ResidualPatternTracker {
    market: Market,
    fingerprints: HashMap<VortexFingerprint, FingerprintStats>,
    /// Per-fingerprint streak counter: consecutive ticks this
    /// fingerprint has been above proposal threshold. Resets on
    /// any tick it falls below.
    eligibility_streaks: HashMap<VortexFingerprint, u32>,
    /// Fingerprints that have already been proposed — don't re-emit
    /// the same one every tick. Cleared only if tracker is reset.
    already_proposed: HashSet<VortexFingerprint>,
    thresholds: ProposalThresholds,
}

impl ResidualPatternTracker {
    pub fn new(market: Market) -> Self {
        Self::with_thresholds(market, ProposalThresholds::default())
    }

    pub fn with_thresholds(market: Market, thresholds: ProposalThresholds) -> Self {
        Self {
            market,
            fingerprints: HashMap::new(),
            eligibility_streaks: HashMap::new(),
            already_proposed: HashSet::new(),
            thresholds,
        }
    }

    pub fn market(&self) -> Market {
        self.market
    }

    pub fn total_fingerprints(&self) -> usize {
        self.fingerprints.len()
    }

    pub fn total_observations(&self) -> u64 {
        self.fingerprints.values().map(|s| s.count).sum()
    }

    /// Record a single vortex observation. Caller is expected to pass
    /// the vortex + the classifier's lifecycle phase + the classifier's
    /// driver decision. We don't re-classify here — we just track what
    /// the classifier said, so downstream analysis can surface
    /// inconsistency.
    pub fn observe(
        &mut self,
        vortex: &PressureVortex,
        phase: AnomalyPhase,
        driver: &TensionDriver,
        now: DateTime<Utc>,
    ) {
        let fingerprint = VortexFingerprint::from_vortex(vortex, phase);
        let entry = self
            .fingerprints
            .entry(fingerprint)
            .or_insert_with(|| FingerprintStats {
                count: 0,
                first_seen: now,
                last_seen: now,
                symbols: HashSet::new(),
                driver_classifications: HashMap::new(),
            });
        entry.count += 1;
        entry.last_seen = now;
        entry.symbols.insert(vortex.symbol.clone());
        *entry
            .driver_classifications
            .entry(driver_name(driver))
            .or_insert(0) += 1;
    }

    /// Return the top-k patterns by "residuality": patterns that are
    /// both frequent (count ≥ min_count) AND classified inconsistently
    /// across multiple drivers (normalized entropy ≥ min_spread).
    ///
    /// A fingerprint seen 50 times, all labelled "microstructure" →
    /// NOT residual (classifier is consistent).
    /// A fingerprint seen 50 times, split 30/15/5 across three drivers
    /// → residual (forced fit, suggests missing entity type).
    pub fn top_residual_patterns(&self, k: usize, min_count: u64) -> Vec<ResidualPatternSummary> {
        let mut summaries: Vec<ResidualPatternSummary> = self
            .fingerprints
            .iter()
            .filter(|(_, s)| s.count >= min_count)
            .map(|(fp, s)| summarize(fp, s))
            .filter(|s| s.driver_label_spread > 0.0)
            .collect();
        // Sort by residuality score = spread × log(count), descending.
        summaries.sort_by(|a, b| {
            let score_b = b.driver_label_spread * ((b.count + 1) as f64).ln();
            let score_a = a.driver_label_spread * ((a.count + 1) as f64).ln();
            score_b
                .partial_cmp(&score_a)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        summaries.truncate(k);
        summaries
    }

    /// Format a summary as a single wake line.
    pub fn format_wake_line(summary: &ResidualPatternSummary) -> String {
        format!(
            "ontology gap: {} count={} symbols={} driver_spread={:.2} dominant={}({}%)",
            summary.fingerprint.label(),
            summary.count,
            summary.distinct_symbols,
            summary.driver_label_spread,
            summary.dominant_driver,
            (summary.dominant_driver_fraction * 100.0).round() as i64
        )
    }

    /// Evaluate current fingerprints against thresholds and return
    /// freshly-emitted OntologyProposals. Call this once per tick after
    /// `observe` has been called for that tick's vortices.
    ///
    /// Streak semantics: a fingerprint must be above all thresholds
    /// (count, spread, distinct_symbols) for `min_eligibility_streak`
    /// CONSECUTIVE ticks to emit. This filters out noise spikes —
    /// any tick a fingerprint falls below threshold resets its streak.
    /// Once a proposal has been emitted, it doesn't re-emit (we record
    /// it in `already_proposed`) until tracker is cleared.
    ///
    /// Return value is the proposals newly-emitted this tick.
    pub fn evaluate_proposals(&mut self, now: DateTime<Utc>) -> Vec<OntologyProposal> {
        let mut proposals = Vec::new();
        // Collect fingerprints for eligibility check; avoid borrow conflict.
        let snapshot: Vec<(VortexFingerprint, FingerprintStats)> = self
            .fingerprints
            .iter()
            .map(|(fp, stats)| (fp.clone(), stats.clone()))
            .collect();

        for (fp, stats) in snapshot {
            if self.already_proposed.contains(&fp) {
                continue;
            }
            let summary = summarize(&fp, &stats);
            let eligible = summary.count >= self.thresholds.min_count
                && summary.driver_label_spread >= self.thresholds.min_driver_spread
                && summary.distinct_symbols >= self.thresholds.min_distinct_symbols;

            if eligible {
                let streak = self.eligibility_streaks.entry(fp.clone()).or_insert(0);
                *streak += 1;
                let current_streak = *streak;
                if current_streak >= self.thresholds.min_eligibility_streak {
                    let suggested_name = auto_name_fingerprint(&fp, summary.dominant_driver);
                    proposals.push(OntologyProposal {
                        fingerprint: fp.clone(),
                        suggested_name,
                        count_at_proposal: summary.count,
                        symbols_at_proposal: summary.distinct_symbols,
                        driver_spread_at_proposal: summary.driver_label_spread,
                        first_seen: stats.first_seen,
                        proposed_at: now,
                        eligibility_streak: current_streak,
                    });
                    self.already_proposed.insert(fp);
                }
            } else {
                // Any dip below threshold resets streak.
                self.eligibility_streaks.remove(&fp);
            }
        }

        proposals
    }

    /// Format a proposal as a single wake line.
    pub fn format_proposal_wake_line(proposal: &OntologyProposal) -> String {
        format!(
            "ontology proposal: {} as candidate `{}` (count={}, symbols={}, \
             spread={:.2}, streak={} ticks, first_seen={})",
            proposal.fingerprint.label(),
            proposal.suggested_name,
            proposal.count_at_proposal,
            proposal.symbols_at_proposal,
            proposal.driver_spread_at_proposal,
            proposal.eligibility_streak,
            proposal.first_seen.format("%Y-%m-%d %H:%M"),
        )
    }

    /// Reset proposal state (streaks + already-proposed). Useful for
    /// tests; in production we deliberately do NOT reset between ticks
    /// so streaks span the full session.
    pub fn reset_proposal_state(&mut self) {
        self.eligibility_streaks.clear();
        self.already_proposed.clear();
    }
}

/// Auto-generate a human-readable candidate name from a fingerprint.
/// Examples:
///   ([capital_flow, institutional], +, growing) → "deep_flow_accum_growing"
///   ([order_book, volume], -, peaking) → "microstructure_fade_peaking"
///
/// These names are suggestions for operator review, not authoritative.
fn auto_name_fingerprint(fp: &VortexFingerprint, dominant_driver: &str) -> String {
    let channel_key = if fp.channels.len() == 1 {
        channel_name(fp.channels[0]).to_string()
    } else if fp.channels.len() <= 3 {
        fp.channels
            .iter()
            .map(|c| {
                let name = channel_name(*c);
                name.split('_').next().unwrap_or(name).to_string()
            })
            .collect::<Vec<_>>()
            .join("_")
    } else {
        "broad".to_string()
    };
    let direction = match fp.direction_sign {
        1 => "bull",
        -1 => "bear",
        _ => "neutral",
    };
    let phase = match fp.phase {
        AnomalyPhase::Growing => "growing",
        AnomalyPhase::Peaking => "peaking",
        AnomalyPhase::Fading => "fading",
        AnomalyPhase::New => "new",
    };
    format!(
        "{}_{}_{}_{}",
        channel_key, direction, phase, dominant_driver
    )
}

fn summarize(fp: &VortexFingerprint, stats: &FingerprintStats) -> ResidualPatternSummary {
    let total = stats.count as f64;
    let mut dominant: (&'static str, u64) = ("none", 0);
    for (label, c) in &stats.driver_classifications {
        if *c > dominant.1 {
            dominant = (label, *c);
        }
    }
    // Normalized Shannon entropy over driver labels, ∈ [0, 1].
    // 0 = all classifications the same label (consistent, not residual).
    // 1 = perfectly spread (maximum residuality).
    let mut entropy = 0.0_f64;
    for c in stats.driver_classifications.values() {
        if *c == 0 {
            continue;
        }
        let p = *c as f64 / total;
        entropy -= p * p.ln();
    }
    let n_labels = stats.driver_classifications.len() as f64;
    let normalized = if n_labels > 1.0 {
        entropy / n_labels.ln()
    } else {
        0.0
    };
    let dominant_fraction = if total > 0.0 {
        dominant.1 as f64 / total
    } else {
        0.0
    };
    ResidualPatternSummary {
        fingerprint: fp.clone(),
        count: stats.count,
        distinct_symbols: stats.symbols.len(),
        driver_label_spread: normalized,
        dominant_driver: dominant.0,
        dominant_driver_fraction: dominant_fraction,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    fn mk_vortex(symbol: &str, channels: Vec<PressureChannel>, dir: Decimal) -> PressureVortex {
        PressureVortex {
            symbol: Symbol(symbol.to_string()),
            tension: dec!(0.5),
            cross_channel_conflict: dec!(0.0),
            temporal_divergence: dec!(0.0),
            hour_direction: dir,
            tick_direction: dec!(0.0),
            tense_channels: channels.clone(),
            tense_channel_count: channels.len(),
            edge_violation_source: None,
        }
    }

    #[test]
    fn fingerprint_is_channel_order_invariant() {
        let v1 = mk_vortex(
            "A.HK",
            vec![PressureChannel::Volume, PressureChannel::OrderBook],
            dec!(0.5),
        );
        let v2 = mk_vortex(
            "A.HK",
            vec![PressureChannel::OrderBook, PressureChannel::Volume],
            dec!(0.5),
        );
        let fp1 = VortexFingerprint::from_vortex(&v1, AnomalyPhase::Growing);
        let fp2 = VortexFingerprint::from_vortex(&v2, AnomalyPhase::Growing);
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn fingerprint_direction_bucketed_to_neutral_when_small() {
        let v = mk_vortex(
            "A.HK",
            vec![PressureChannel::Volume],
            dec!(0.01), // below 0.05 threshold
        );
        let fp = VortexFingerprint::from_vortex(&v, AnomalyPhase::Growing);
        assert_eq!(fp.direction_sign, 0);
    }

    #[test]
    fn observe_increments_count_and_tracks_driver() {
        let mut tracker = ResidualPatternTracker::new(Market::Hk);
        let v = mk_vortex(
            "A.HK",
            vec![PressureChannel::Volume, PressureChannel::OrderBook],
            dec!(0.3),
        );
        let now = Utc::now();
        tracker.observe(
            &v,
            AnomalyPhase::Growing,
            &TensionDriver::MicrostructureDriven,
            now,
        );
        tracker.observe(
            &v,
            AnomalyPhase::Growing,
            &TensionDriver::MicrostructureDriven,
            now,
        );
        tracker.observe(
            &v,
            AnomalyPhase::Growing,
            &TensionDriver::BroadStructural,
            now,
        );

        assert_eq!(tracker.total_fingerprints(), 1);
        assert_eq!(tracker.total_observations(), 3);

        let fp = VortexFingerprint::from_vortex(&v, AnomalyPhase::Growing);
        let stats = tracker.fingerprints.get(&fp).unwrap();
        assert_eq!(stats.count, 3);
        assert_eq!(stats.symbols.len(), 1);
        assert_eq!(stats.driver_classifications["microstructure"], 2);
        assert_eq!(stats.driver_classifications["broad_structural"], 1);
    }

    #[test]
    fn consistent_classifier_is_not_residual() {
        let mut tracker = ResidualPatternTracker::new(Market::Hk);
        let v = mk_vortex("A.HK", vec![PressureChannel::Volume], dec!(0.5));
        let now = Utc::now();
        for _ in 0..20 {
            tracker.observe(
                &v,
                AnomalyPhase::Growing,
                &TensionDriver::SingleChannel {
                    channel: PressureChannel::Volume,
                },
                now,
            );
        }
        // All 20 classified as single_channel → spread = 0 → not residual.
        let residual = tracker.top_residual_patterns(5, 5);
        assert!(
            residual.is_empty(),
            "consistent classifications should not surface as residual"
        );
    }

    #[test]
    fn inconsistent_classifier_surfaces_as_residual() {
        let mut tracker = ResidualPatternTracker::new(Market::Hk);
        let v = mk_vortex(
            "A.HK",
            vec![PressureChannel::Volume, PressureChannel::Institutional],
            dec!(0.3),
        );
        let now = Utc::now();
        for _ in 0..10 {
            tracker.observe(
                &v,
                AnomalyPhase::Growing,
                &TensionDriver::TradeFlowDriven,
                now,
            );
        }
        for _ in 0..8 {
            tracker.observe(
                &v,
                AnomalyPhase::Growing,
                &TensionDriver::InstitutionalDriven,
                now,
            );
        }
        for _ in 0..6 {
            tracker.observe(
                &v,
                AnomalyPhase::Growing,
                &TensionDriver::BroadStructural,
                now,
            );
        }
        let residual = tracker.top_residual_patterns(5, 10);
        assert_eq!(residual.len(), 1, "inconsistent pattern should surface");
        assert_eq!(residual[0].count, 24);
        assert!(residual[0].driver_label_spread > 0.5);
    }

    #[test]
    fn min_count_filters_out_rare_patterns() {
        let mut tracker = ResidualPatternTracker::new(Market::Hk);
        let v = mk_vortex("A.HK", vec![PressureChannel::Volume], dec!(0.3));
        let now = Utc::now();
        tracker.observe(
            &v,
            AnomalyPhase::Growing,
            &TensionDriver::TradeFlowDriven,
            now,
        );
        tracker.observe(
            &v,
            AnomalyPhase::Growing,
            &TensionDriver::InstitutionalDriven,
            now,
        );

        // Only 2 observations — below min_count=5.
        let residual = tracker.top_residual_patterns(5, 5);
        assert!(residual.is_empty());
    }

    #[test]
    fn evaluate_proposals_requires_sustained_eligibility() {
        // Fingerprint that IS residual (high count + spread) but hasn't
        // been eligible for min_streak ticks should NOT yet emit a proposal.
        let mut tracker = ResidualPatternTracker::with_thresholds(
            Market::Hk,
            ProposalThresholds {
                min_count: 10,
                min_driver_spread: 0.3,
                min_distinct_symbols: 2,
                min_eligibility_streak: 3,
            },
        );
        let vs: Vec<PressureVortex> = (0..4)
            .map(|i| {
                mk_vortex(
                    &format!("S{}.HK", i),
                    vec![PressureChannel::Volume, PressureChannel::Institutional],
                    dec!(0.3),
                )
            })
            .collect();
        let drivers = [
            &TensionDriver::TradeFlowDriven,
            &TensionDriver::InstitutionalDriven,
            &TensionDriver::BroadStructural,
        ];
        let now = Utc::now();
        for v in &vs {
            for d in drivers {
                tracker.observe(v, AnomalyPhase::Growing, d, now);
            }
        }

        let first_eval = tracker.evaluate_proposals(now);
        assert!(
            first_eval.is_empty(),
            "streak=1, below min_eligibility_streak=3, no proposal yet"
        );
        let second_eval = tracker.evaluate_proposals(now);
        assert!(second_eval.is_empty(), "streak=2, still below");
        let third_eval = tracker.evaluate_proposals(now);
        assert_eq!(third_eval.len(), 1, "streak=3 hit → proposal emits");
        assert_eq!(third_eval[0].eligibility_streak, 3);
        assert_eq!(third_eval[0].count_at_proposal, 12);
        assert_eq!(third_eval[0].symbols_at_proposal, 4);

        // Proposal doesn't re-emit next tick.
        let fourth_eval = tracker.evaluate_proposals(now);
        assert!(
            fourth_eval.is_empty(),
            "already-proposed should not re-emit"
        );
    }

    #[test]
    fn dip_below_threshold_resets_streak() {
        let mut tracker = ResidualPatternTracker::with_thresholds(
            Market::Hk,
            ProposalThresholds {
                min_count: 20,
                min_driver_spread: 0.3,
                min_distinct_symbols: 1,
                min_eligibility_streak: 3,
            },
        );
        let v = mk_vortex(
            "A.HK",
            vec![PressureChannel::Volume, PressureChannel::OrderBook],
            dec!(0.3),
        );
        let now = Utc::now();
        // Not enough count yet — streak stays 0.
        for _ in 0..5 {
            tracker.observe(
                &v,
                AnomalyPhase::Growing,
                &TensionDriver::TradeFlowDriven,
                now,
            );
            tracker.observe(
                &v,
                AnomalyPhase::Growing,
                &TensionDriver::InstitutionalDriven,
                now,
            );
        }
        // count=10, below min 20 → no streak.
        let e1 = tracker.evaluate_proposals(now);
        assert!(e1.is_empty());

        // Push past min_count.
        for _ in 0..10 {
            tracker.observe(
                &v,
                AnomalyPhase::Growing,
                &TensionDriver::TradeFlowDriven,
                now,
            );
            tracker.observe(
                &v,
                AnomalyPhase::Growing,
                &TensionDriver::InstitutionalDriven,
                now,
            );
        }
        // Now eligible — build streak.
        tracker.evaluate_proposals(now); // streak 1
        tracker.evaluate_proposals(now); // streak 2
        let e3 = tracker.evaluate_proposals(now); // streak 3 → emit
        assert_eq!(e3.len(), 1);
    }

    #[test]
    fn auto_name_uses_channel_direction_phase() {
        let fp = VortexFingerprint {
            channels: vec![PressureChannel::CapitalFlow, PressureChannel::Institutional],
            direction_sign: 1,
            phase: AnomalyPhase::Growing,
        };
        let name = auto_name_fingerprint(&fp, "trade_flow");
        assert!(name.contains("bull"));
        assert!(name.contains("growing"));
        assert!(name.contains("trade_flow"));
    }

    #[test]
    fn format_proposal_wake_line_contains_fingerprint_and_name() {
        let proposal = OntologyProposal {
            fingerprint: VortexFingerprint {
                channels: vec![PressureChannel::Volume, PressureChannel::Institutional],
                direction_sign: 1,
                phase: AnomalyPhase::Growing,
            },
            suggested_name: "volume_inst_bull_growing_trade_flow".to_string(),
            count_at_proposal: 72,
            symbols_at_proposal: 14,
            driver_spread_at_proposal: 0.83,
            first_seen: Utc::now(),
            proposed_at: Utc::now(),
            eligibility_streak: 5,
        };
        let line = ResidualPatternTracker::format_proposal_wake_line(&proposal);
        assert!(line.starts_with("ontology proposal:"));
        assert!(line.contains("volume_inst_bull_growing_trade_flow"));
        assert!(line.contains("count=72"));
        assert!(line.contains("streak=5"));
    }

    #[test]
    fn format_wake_line_shows_fingerprint_and_spread() {
        let summary = ResidualPatternSummary {
            fingerprint: VortexFingerprint {
                channels: vec![PressureChannel::Volume, PressureChannel::Institutional],
                direction_sign: 1,
                phase: AnomalyPhase::Growing,
            },
            count: 24,
            distinct_symbols: 8,
            driver_label_spread: 0.89,
            dominant_driver: "trade_flow",
            dominant_driver_fraction: 0.42,
        };
        let line = ResidualPatternTracker::format_wake_line(&summary);
        assert!(line.starts_with("ontology gap:"));
        assert!(line.contains("count=24"));
        assert!(line.contains("symbols=8"));
        assert!(line.contains("driver_spread=0.89"));
        assert!(line.contains("dominant=trade_flow"));
        assert!(line.contains("42%"));
    }
}
