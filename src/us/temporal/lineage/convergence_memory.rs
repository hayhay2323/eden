use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::ontology::reasoning::{
    direction_from_setup, Hypothesis, TacticalDirection, TacticalSetup,
};

use super::super::buffer::UsTickHistory;

// V2 Pass 2: CONVERGENCE_HYPOTHESIS_KEY / LATENT_VORTEX_KEY constants
// deleted. Family marker recognition now uses
// `hypothesis_id.contains(":convergence_hypothesis:" / ":latent_vortex:")`.

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UsConvergenceOutcomeFingerprint {
    pub setup_id: String,
    pub family: String,
    pub symbol: String,
    pub entry_tick: u64,
    pub resolved_tick: u64,
    pub channel_signature: String,
    pub dominant_channels: Vec<String>,
    pub channel_diversity: usize,
    pub strength: Decimal,
    pub coherence: Decimal,
    pub net_return: Decimal,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UsConvergenceSuccessPattern {
    pub channel_signature: String,
    pub dominant_channels: Vec<String>,
    pub top_family: String,
    pub samples: usize,
    pub mean_net_return: Decimal,
    pub mean_strength: Decimal,
    pub mean_coherence: Decimal,
    pub mean_channel_diversity: Decimal,
}

pub fn compute_us_successful_convergence_fingerprints(
    history: &UsTickHistory,
    min_lag: u64,
) -> Vec<UsConvergenceOutcomeFingerprint> {
    let records = history.latest_n(history.len());
    if records.is_empty() {
        return Vec::new();
    }

    let current_tick = records
        .last()
        .map(|record| record.tick_number)
        .unwrap_or_default();
    let _by_tick = records
        .iter()
        .copied()
        .map(|record| (record.tick_number, record))
        .collect::<HashMap<_, _>>();
    let mut seen = std::collections::HashSet::new();

    let mut fingerprints = records
        .iter()
        .flat_map(|record| {
            record
                .tactical_setups
                .iter()
                .map(move |setup| (*record, setup))
        })
        .filter(|(_, setup)| seen.insert(setup.setup_id.clone()))
        .filter_map(|(entry_record, setup)| {
            let hypothesis = entry_record.hypotheses.iter().find(|hypothesis| {
                hypothesis.hypothesis_id == setup.hypothesis_id
                    && is_us_topology_hypothesis(hypothesis)
            })?;
            let symbol = match &setup.scope {
                crate::ontology::reasoning::ReasoningScope::Symbol(symbol) => symbol.clone(),
                _ => return None,
            };
            let entry_price = entry_record
                .signals
                .get(&symbol)
                .and_then(|signal| signal.mark_price)
                .filter(|price| *price > Decimal::ZERO)?;
            if current_tick < entry_record.tick_number + min_lag {
                return None;
            }

            let future_records = records
                .iter()
                .copied()
                .filter(|record| record.tick_number > entry_record.tick_number)
                .collect::<Vec<_>>();
            let direction = setup_direction(setup);
            let mut peak_return = Decimal::ZERO;
            let mut peak_tick = entry_record.tick_number + min_lag;

            for record in future_records
                .iter()
                .filter(|record| record.tick_number >= entry_record.tick_number + min_lag)
            {
                let price = record
                    .signals
                    .get(&symbol)
                    .and_then(|signal| signal.mark_price)
                    .filter(|price| *price > Decimal::ZERO)?;
                let raw = (price - entry_price) / entry_price;
                let oriented = if direction < 0 { -raw } else { raw };
                if oriented > peak_return {
                    peak_return = oriented;
                    peak_tick = record.tick_number;
                }
            }

            if peak_return <= Decimal::ZERO {
                return None;
            }

            let metadata = hypothesis_vortex_metadata(hypothesis);
            let dominant_channels = metadata
                .dominant_channels
                .filter(|channels| !channels.is_empty())
                .unwrap_or_else(|| inferred_channels_from_hypothesis(hypothesis));
            if dominant_channels.is_empty() {
                return None;
            }

            Some(UsConvergenceOutcomeFingerprint {
                setup_id: setup.setup_id.clone(),
                family: hypothesis.family_label.clone(),
                symbol: symbol.0,
                entry_tick: entry_record.tick_number,
                resolved_tick: peak_tick,
                channel_signature: dominant_channels.join("|"),
                dominant_channels,
                channel_diversity: metadata
                    .channel_diversity
                    .unwrap_or_else(|| hypothesis_channel_count(hypothesis)),
                strength: metadata.strength.unwrap_or(hypothesis.confidence),
                coherence: metadata.coherence.unwrap_or(hypothesis.confidence),
                net_return: peak_return,
            })
        })
        .collect::<Vec<_>>();

    fingerprints.sort_by(|left, right| {
        right
            .resolved_tick
            .cmp(&left.resolved_tick)
            .then_with(|| right.net_return.cmp(&left.net_return))
            .then_with(|| left.setup_id.cmp(&right.setup_id))
    });
    fingerprints
}

pub fn compute_us_convergence_success_patterns(
    history: &UsTickHistory,
    min_lag: u64,
) -> Vec<UsConvergenceSuccessPattern> {
    #[derive(Default)]
    struct PatternAccumulator {
        samples: usize,
        sum_net_return: Decimal,
        sum_strength: Decimal,
        sum_coherence: Decimal,
        sum_channel_diversity: Decimal,
        family_counts: HashMap<String, usize>,
        dominant_channels: Vec<String>,
    }

    let mut acc = HashMap::<String, PatternAccumulator>::new();
    for fingerprint in compute_us_successful_convergence_fingerprints(history, min_lag) {
        let entry = acc
            .entry(fingerprint.channel_signature.clone())
            .or_default();
        entry.samples += 1;
        entry.sum_net_return += fingerprint.net_return;
        entry.sum_strength += fingerprint.strength;
        entry.sum_coherence += fingerprint.coherence;
        entry.sum_channel_diversity += Decimal::from(fingerprint.channel_diversity as i64);
        entry.dominant_channels = fingerprint.dominant_channels.clone();
        *entry
            .family_counts
            .entry(fingerprint.family.clone())
            .or_insert(0) += 1;
    }

    let mut patterns = acc
        .into_iter()
        .map(
            |(channel_signature, accumulator)| UsConvergenceSuccessPattern {
                channel_signature,
                dominant_channels: accumulator.dominant_channels,
                top_family: accumulator
                    .family_counts
                    .into_iter()
                    .max_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)))
                    .map(|(family, _)| family)
                    .unwrap_or_else(|| "Convergence Hypothesis".into()),
                samples: accumulator.samples,
                mean_net_return: mean(accumulator.sum_net_return, accumulator.samples),
                mean_strength: mean(accumulator.sum_strength, accumulator.samples),
                mean_coherence: mean(accumulator.sum_coherence, accumulator.samples),
                mean_channel_diversity: mean(
                    accumulator.sum_channel_diversity,
                    accumulator.samples,
                ),
            },
        )
        .collect::<Vec<_>>();

    patterns.sort_by(|left, right| {
        right
            .mean_net_return
            .cmp(&left.mean_net_return)
            .then_with(|| right.samples.cmp(&left.samples))
            .then_with(|| right.mean_strength.cmp(&left.mean_strength))
            .then_with(|| left.channel_signature.cmp(&right.channel_signature))
    });
    patterns
}

pub fn us_convergence_hypothesis_matches_pattern(
    hypothesis: &Hypothesis,
    pattern: &UsConvergenceSuccessPattern,
) -> bool {
    if !matches!(
        hypothesis.kind,
        Some(crate::ontology::reasoning::HypothesisKind::ConvergenceHypothesis)
    ) {
        return false;
    }
    us_topology_hypothesis_matches_pattern(hypothesis, pattern)
}

pub fn us_topology_hypothesis_matches_pattern(
    hypothesis: &Hypothesis,
    pattern: &UsConvergenceSuccessPattern,
) -> bool {
    if !is_us_topology_hypothesis(hypothesis) {
        return false;
    }
    let channels = hypothesis_vortex_metadata(hypothesis)
        .dominant_channels
        .filter(|channels| !channels.is_empty())
        .unwrap_or_else(|| inferred_channels_from_hypothesis(hypothesis));
    if channels.is_empty() || pattern.dominant_channels.is_empty() {
        return false;
    }
    let overlap = channels
        .iter()
        .filter(|channel| pattern.dominant_channels.contains(channel))
        .count();
    let required_overlap = channels
        .len()
        .min(pattern.dominant_channels.len())
        .min(2)
        .max(1);
    overlap >= required_overlap
}

fn is_us_topology_hypothesis(hypothesis: &Hypothesis) -> bool {
    matches!(
        hypothesis.kind,
        Some(crate::ontology::reasoning::HypothesisKind::ConvergenceHypothesis)
            | Some(crate::ontology::reasoning::HypothesisKind::LatentVortex)
    )
}

#[derive(Default)]
struct HypothesisVortexMetadata {
    dominant_channels: Option<Vec<String>>,
    channel_diversity: Option<usize>,
    strength: Option<Decimal>,
    coherence: Option<Decimal>,
}

fn hypothesis_vortex_metadata(hypothesis: &Hypothesis) -> HypothesisVortexMetadata {
    let mut metadata = HypothesisVortexMetadata::default();
    let Some(note) = hypothesis.provenance.note.as_deref() else {
        return metadata;
    };
    for part in note.split(';').map(str::trim) {
        let Some((key, value)) = part.split_once('=') else {
            continue;
        };
        match key.trim() {
            "dominant_channels" => {
                let channels = value
                    .split('|')
                    .map(str::trim)
                    .filter(|item| !item.is_empty())
                    .map(ToOwned::to_owned)
                    .collect::<Vec<_>>();
                if !channels.is_empty() {
                    metadata.dominant_channels = Some(channels);
                }
            }
            "channel_diversity" => {
                metadata.channel_diversity = value.trim().parse::<usize>().ok();
            }
            "vortex_strength" => {
                metadata.strength = value.trim().parse::<Decimal>().ok();
            }
            "coherence" => {
                metadata.coherence = value.trim().parse::<Decimal>().ok();
            }
            _ => {}
        }
    }
    metadata
}

fn inferred_channels_from_hypothesis(hypothesis: &Hypothesis) -> Vec<String> {
    let mut channels = hypothesis
        .evidence
        .iter()
        .filter_map(|item| {
            item.statement
                .rsplit_once(" via ")
                .map(|(_, channel)| channel)
        })
        .map(str::trim)
        .filter(|channel| !channel.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>();
    channels.sort();
    channels.dedup();
    channels
}

fn hypothesis_channel_count(hypothesis: &Hypothesis) -> usize {
    let channels = inferred_channels_from_hypothesis(hypothesis);
    if channels.is_empty() {
        0
    } else {
        channels.len()
    }
}

fn setup_direction(setup: &TacticalSetup) -> i8 {
    match direction_from_setup(setup) {
        Some(TacticalDirection::Short) => -1,
        Some(TacticalDirection::Long) | None => 1,
    }
}

fn mean(total: Decimal, count: usize) -> Decimal {
    if count == 0 {
        Decimal::ZERO
    } else {
        total / Decimal::from(count as i64)
    }
}

// ---------------------------------------------------------------------------
// US Candidate Mechanism evaluation
// ---------------------------------------------------------------------------

use crate::persistence::candidate_mechanism::CandidateMechanismRecord;

const US_PROMOTION_MIN_SAMPLES: usize = 3;

pub fn evaluate_us_candidate_mechanisms(
    patterns: &[UsConvergenceSuccessPattern],
    existing: &[CandidateMechanismRecord],
    current_tick: u64,
    now_rfc3339: &str,
) -> Vec<CandidateMechanismRecord> {
    let mut results = Vec::new();
    let existing_by_id: HashMap<&str, &CandidateMechanismRecord> = existing
        .iter()
        .map(|mech| (mech.mechanism_id.as_str(), mech))
        .collect();

    for pattern in patterns {
        let mech_id = CandidateMechanismRecord::mechanism_key(
            "us",
            "convergence",
            "center",
            &pattern.channel_signature,
        );

        let promotion_min_return = Decimal::new(1, 2); // 0.01
        if pattern.samples < US_PROMOTION_MIN_SAMPLES
            || pattern.mean_net_return < promotion_min_return
        {
            if let Some(existing_mech) = existing_by_id.get(mech_id.as_str()) {
                let mut updated = (*existing_mech).clone();
                updated.last_seen_tick = current_tick;
                updated.samples = pattern.samples as u64;
                updated.mean_net_return = pattern.mean_net_return;
                updated.mean_strength = pattern.mean_strength;
                updated.mean_coherence = pattern.mean_coherence;
                updated.mean_channel_diversity = pattern.mean_channel_diversity;
                updated.updated_at = now_rfc3339.to_string();
                results.push(updated);
            }
            continue;
        }

        if let Some(existing_mech) = existing_by_id.get(mech_id.as_str()) {
            let mut updated = (*existing_mech).clone();
            updated.last_seen_tick = current_tick;
            updated.samples = pattern.samples as u64;
            updated.mean_net_return = pattern.mean_net_return;
            updated.mean_strength = pattern.mean_strength;
            updated.mean_coherence = pattern.mean_coherence;
            updated.mean_channel_diversity = pattern.mean_channel_diversity;
            updated.dominant_channels = pattern.dominant_channels.clone();
            updated.top_family = pattern.top_family.clone();
            updated.updated_at = now_rfc3339.to_string();

            if updated.should_promote_to_live() {
                updated.mode = "live".into();
            } else if updated.should_promote_to_assist() {
                updated.mode = "assist".into();
            }
            if updated.should_decay(current_tick) {
                if let Some(demoted) = updated.demoted_mode() {
                    updated.mode = demoted.into();
                    updated.consecutive_misses = 0;
                }
            }
            results.push(updated);
        } else {
            results.push(CandidateMechanismRecord {
                mechanism_id: mech_id,
                market: "us".to_string(),
                center_kind: "convergence".into(),
                role: "center".into(),
                channel_signature: pattern.channel_signature.clone(),
                dominant_channels: pattern.dominant_channels.clone(),
                top_family: pattern.top_family.clone(),
                samples: pattern.samples as u64,
                mean_net_return: pattern.mean_net_return,
                mean_strength: pattern.mean_strength,
                mean_coherence: pattern.mean_coherence,
                mean_channel_diversity: pattern.mean_channel_diversity,
                mode: "shadow".into(),
                promoted_at_tick: current_tick,
                last_seen_tick: current_tick,
                last_hit_tick: None,
                consecutive_misses: 0,
                post_promotion_hits: 0,
                post_promotion_misses: 0,
                post_promotion_net_return: Decimal::ZERO,
                created_at: now_rfc3339.to_string(),
                updated_at: now_rfc3339.to_string(),
            });
        }
    }

    for existing_mech in existing {
        let already_handled = results
            .iter()
            .any(|mech| mech.mechanism_id == existing_mech.mechanism_id);
        if !already_handled {
            let mut updated = existing_mech.clone();
            updated.consecutive_misses += 1;
            updated.updated_at = now_rfc3339.to_string();
            if updated.should_decay(current_tick) {
                if let Some(demoted) = updated.demoted_mode() {
                    updated.mode = demoted.into();
                    updated.consecutive_misses = 0;
                }
            }
            results.push(updated);
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::domain::{DerivedSignal, Event, ProvenanceMetadata, ProvenanceSource};
    use crate::ontology::objects::Symbol;
    use crate::ontology::reasoning::{default_case_horizon, DecisionLineage, ReasoningScope};
    use crate::us::graph::decision::UsMarketRegimeBias;
    use crate::us::pipeline::signals::{UsDerivedSignalRecord, UsEventRecord, UsSignalScope};
    use crate::us::temporal::record::{UsSymbolSignals, UsTickRecord};
    use rust_decimal_macros::dec;
    use std::collections::HashMap;
    use time::OffsetDateTime;

    fn prov(tag: &str) -> ProvenanceMetadata {
        ProvenanceMetadata::new(ProvenanceSource::Computed, OffsetDateTime::UNIX_EPOCH)
            .with_trace_id(tag)
            .with_inputs([tag.to_string()])
    }

    fn signal(price: Decimal, composite: Decimal) -> UsSymbolSignals {
        UsSymbolSignals {
            mark_price: Some(price),
            composite,
            composite_delta: Decimal::ZERO,
            composite_acceleration: Decimal::ZERO,
            capital_flow_direction: Decimal::ZERO,
            capital_flow_delta: Decimal::ZERO,
            flow_persistence: 0,
            flow_reversal: false,
            price_momentum: Decimal::ZERO,
            volume_profile: Decimal::ZERO,
            pre_post_market_anomaly: Decimal::ZERO,
            valuation: Decimal::ZERO,
            pre_market_delta: Decimal::ZERO,
        }
    }

    fn tick(tick_number: u64, price: Decimal, include_setup: bool) -> UsTickRecord {
        let symbol = Symbol("BABA.US".into());
        let mut signals = HashMap::new();
        signals.insert(symbol.clone(), signal(price, dec!(0.6)));

        let hypothesis = Hypothesis {
            hypothesis_id: "hyp:BABA.US:convergence_hypothesis".into(),
            kind: Some(crate::ontology::reasoning::HypothesisKind::ConvergenceHypothesis),
            family_label: "Convergence Hypothesis".into(),
            provenance: prov("hyp:BABA.US").with_note(
                "family=Convergence Hypothesis; vortex_strength=0.48; channel_diversity=3; coherence=0.72; dominant_channels=cross-market|pre-market|sector rotation",
            ),
            scope: ReasoningScope::Symbol(symbol.clone()),
            statement: "BABA.US shows an emergent convergence vortex".into(),
            confidence: dec!(0.7),
            local_support_weight: dec!(0.5),
            local_contradict_weight: Decimal::ZERO,
            propagated_support_weight: dec!(0.2),
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![],
            invalidation_conditions: vec![],
            propagation_path_ids: vec![],
            expected_observations: vec![],
        };
        let setup = TacticalSetup {
            setup_id: "setup:BABA.US:review".into(),
            hypothesis_id: hypothesis.hypothesis_id.clone(),
            runner_up_hypothesis_id: None,
            provenance: prov("setup:BABA.US"),
            lineage: DecisionLineage::default(),
            scope: ReasoningScope::Symbol(symbol),
            title: "Long BABA.US".into(),
            action: "review".into(),
            direction: None,
            horizon: default_case_horizon(),
            confidence: dec!(0.7),
            confidence_gap: dec!(0.15),
            heuristic_edge: dec!(0.1),
            convergence_score: Some(dec!(0.5)),
            convergence_detail: None,
            workflow_id: None,
            entry_rationale: "vortex".into(),
            causal_narrative: None,
            risk_notes: vec!["family=convergence_hypothesis".into()],
            review_reason_code: None,
            policy_verdict: None,
        };

        UsTickRecord {
            tick_number,
            timestamp: OffsetDateTime::UNIX_EPOCH + time::Duration::seconds(tick_number as i64),
            signals,
            cross_market_signals: vec![],
            events: vec![Event::new(
                UsEventRecord {
                    scope: UsSignalScope::Symbol(Symbol("BABA.US".into())),
                    kind: crate::us::pipeline::signals::UsEventKind::CrossMarketDivergence,
                    magnitude: dec!(0.5),
                    summary: "cross-market".into(),
                },
                prov("event:BABA.US"),
            )],
            derived_signals: vec![DerivedSignal::new(
                UsDerivedSignalRecord {
                    scope: UsSignalScope::Symbol(Symbol("BABA.US".into())),
                    kind: crate::us::pipeline::signals::UsDerivedSignalKind::CrossMarketPropagation,
                    strength: dec!(0.45),
                    summary: "cross-market propagation".into(),
                },
                prov("signal:BABA.US"),
            )],
            hypotheses: if include_setup {
                vec![hypothesis]
            } else {
                vec![]
            },
            tactical_setups: if include_setup { vec![setup] } else { vec![] },
            market_regime: UsMarketRegimeBias::Neutral,
        }
    }

    #[test]
    fn computes_us_convergence_success_patterns_from_profitable_history() {
        let mut history = UsTickHistory::new(32);
        history.push(tick(1, dec!(100), true));
        for tick_number in 2..=15 {
            history.push(tick(
                tick_number,
                dec!(100) + Decimal::from(tick_number as i64),
                false,
            ));
        }

        let fingerprints = compute_us_successful_convergence_fingerprints(&history, 5);
        assert_eq!(fingerprints.len(), 1);
        assert_eq!(
            fingerprints[0].channel_signature,
            "cross-market|pre-market|sector rotation"
        );

        let patterns = compute_us_convergence_success_patterns(&history, 5);
        assert_eq!(patterns.len(), 1);
        assert_eq!(patterns[0].top_family, "Convergence Hypothesis");
        assert!(patterns[0].mean_net_return > Decimal::ZERO);
    }

    #[test]
    fn matches_us_convergence_hypothesis_to_pattern() {
        let hypothesis = Hypothesis {
            hypothesis_id: "hyp:BABA.US:convergence_hypothesis".into(),
            kind: Some(crate::ontology::reasoning::HypothesisKind::ConvergenceHypothesis),
            family_label: "Convergence Hypothesis".into(),
            provenance: prov("hyp:BABA.US").with_note(
                "dominant_channels=cross-market|pre-market|sector rotation; channel_diversity=3; vortex_strength=0.44; coherence=0.70",
            ),
            scope: ReasoningScope::Symbol(Symbol("BABA.US".into())),
            statement: "BABA.US shows an emergent convergence vortex".into(),
            confidence: dec!(0.7),
            local_support_weight: dec!(0.5),
            local_contradict_weight: Decimal::ZERO,
            propagated_support_weight: dec!(0.2),
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![],
            invalidation_conditions: vec![],
            propagation_path_ids: vec![],
            expected_observations: vec![],
        };
        let pattern = UsConvergenceSuccessPattern {
            channel_signature: "cross-market|pre-market|sector rotation".into(),
            dominant_channels: vec![
                "cross-market".into(),
                "pre-market".into(),
                "sector rotation".into(),
            ],
            top_family: "Convergence Hypothesis".into(),
            samples: 2,
            mean_net_return: dec!(0.03),
            mean_strength: dec!(0.45),
            mean_coherence: dec!(0.70),
            mean_channel_diversity: dec!(3),
        };

        assert!(us_convergence_hypothesis_matches_pattern(
            &hypothesis,
            &pattern
        ));
    }

    #[test]
    fn matches_latent_vortex_to_topology_pattern() {
        let hypothesis = Hypothesis {
            hypothesis_id: "hyp:BABA.US:latent_vortex".into(),
            kind: Some(crate::ontology::reasoning::HypothesisKind::LatentVortex),
            family_label: "Latent Vortex".into(),
            provenance: prov("hyp:BABA.US").with_note(
                "family=Latent Vortex; vortex_strength=0.34; channel_diversity=2; coherence=0.64; dominant_channels=cross-market|structure",
            ),
            scope: ReasoningScope::Symbol(Symbol("BABA.US".into())),
            statement: "BABA.US is forming a topology-first vortex".into(),
            confidence: dec!(0.46),
            local_support_weight: dec!(0.35),
            local_contradict_weight: Decimal::ZERO,
            propagated_support_weight: dec!(0.18),
            propagated_contradict_weight: Decimal::ZERO,
            evidence: vec![],
            invalidation_conditions: vec![],
            propagation_path_ids: vec![],
            expected_observations: vec![],
        };
        let pattern = UsConvergenceSuccessPattern {
            channel_signature: "cross-market|structure".into(),
            dominant_channels: vec!["cross-market".into(), "structure".into()],
            top_family: "Latent Vortex".into(),
            samples: 3,
            mean_net_return: dec!(0.04),
            mean_strength: dec!(0.36),
            mean_coherence: dec!(0.65),
            mean_channel_diversity: dec!(2),
        };

        assert!(us_topology_hypothesis_matches_pattern(
            &hypothesis,
            &pattern
        ));
    }
}
