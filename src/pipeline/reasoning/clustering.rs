use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::ontology::reasoning::{
    CaseCluster, Hypothesis, HypothesisTrack, HypothesisTrackStatus, PropagationPath,
    TacticalSetup,
};
use crate::ontology::scope_node_id;

use super::policy::action_priority;

pub(super) fn derive_case_clusters(
    hypotheses: &[Hypothesis],
    propagation_paths: &[PropagationPath],
    setups: &[TacticalSetup],
    tracks: &[HypothesisTrack],
) -> Vec<CaseCluster> {
    #[derive(Default)]
    struct Bucket<'a> {
        setups: Vec<&'a TacticalSetup>,
        tracks: Vec<&'a HypothesisTrack>,
        hypotheses: Vec<&'a Hypothesis>,
        path_ids: Vec<String>,
    }

    let hypothesis_map = hypotheses
        .iter()
        .map(|hypothesis| (hypothesis.hypothesis_id.as_str(), hypothesis))
        .collect::<HashMap<_, _>>();
    let track_map = tracks
        .iter()
        .filter(|track| track.invalidated_at.is_none())
        .map(|track| (track.setup_id.as_str(), track))
        .collect::<HashMap<_, _>>();
    let path_map = propagation_paths
        .iter()
        .map(|path| (path.path_id.as_str(), path))
        .collect::<HashMap<_, _>>();
    let mut buckets: HashMap<(String, String), Bucket<'_>> = HashMap::new();

    for setup in setups {
        let Some(hypothesis) = hypothesis_map.get(setup.hypothesis_id.as_str()).copied() else {
            continue;
        };
        let Some(track) = track_map.get(setup.setup_id.as_str()).copied() else {
            continue;
        };
        let family_key = hypothesis.family_key.clone();
        let linkage_key = cluster_linkage_key(hypothesis, &path_map);
        let bucket = buckets.entry((family_key, linkage_key)).or_default();
        bucket.setups.push(setup);
        bucket.tracks.push(track);
        bucket.hypotheses.push(hypothesis);
        for path_id in &hypothesis.propagation_path_ids {
            if !bucket.path_ids.contains(path_id) {
                bucket.path_ids.push(path_id.clone());
            }
        }
    }

    let mut clusters = buckets
        .into_iter()
        .filter_map(|((family_key, linkage_key), bucket)| {
            let lead_idx = strongest_member_index(&bucket.setups)?;
            let weak_idx = weakest_member_index(&bucket.setups)?;
            let lead_setup = bucket.setups[lead_idx];
            let lead_hypothesis = bucket.hypotheses[lead_idx];
            let weakest_setup = bucket.setups[weak_idx];
            let trend = cluster_trend(&bucket.tracks);
            let member_count = bucket.setups.len();
            let divisor = Decimal::from(member_count as u64);
            let average_confidence = bucket
                .setups
                .iter()
                .map(|setup| setup.confidence)
                .sum::<Decimal>()
                / divisor;
            let average_gap = bucket
                .setups
                .iter()
                .map(|setup| setup.confidence_gap)
                .sum::<Decimal>()
                / divisor;
            let average_edge = bucket
                .setups
                .iter()
                .map(|setup| setup.heuristic_edge)
                .sum::<Decimal>()
                / divisor;
            let title = cluster_title(
                &family_key,
                &linkage_key,
                member_count,
                bucket
                    .path_ids
                    .first()
                    .and_then(|id| path_map.get(id.as_str()).copied()),
            );

            Some(CaseCluster {
                cluster_id: format!("cluster:{}:{}", family_key, linkage_key),
                family_key,
                linkage_key,
                title,
                lead_hypothesis_id: lead_hypothesis.hypothesis_id.clone(),
                lead_statement: lead_hypothesis.statement.clone(),
                trend,
                member_setup_ids: bucket
                    .setups
                    .iter()
                    .map(|setup| setup.setup_id.clone())
                    .collect(),
                member_track_ids: bucket
                    .tracks
                    .iter()
                    .map(|track| track.track_id.clone())
                    .collect(),
                member_scopes: bucket
                    .setups
                    .iter()
                    .map(|setup| setup.scope.clone())
                    .collect(),
                propagation_path_ids: bucket.path_ids,
                strongest_setup_id: lead_setup.setup_id.clone(),
                weakest_setup_id: weakest_setup.setup_id.clone(),
                strongest_title: lead_setup.title.clone(),
                weakest_title: weakest_setup.title.clone(),
                member_count,
                average_confidence: average_confidence.round_dp(4),
                average_gap: average_gap.round_dp(4),
                average_edge: average_edge.round_dp(4),
            })
        })
        .collect::<Vec<_>>();

    clusters.sort_by(|a, b| {
        cluster_trend_priority(a.trend)
            .cmp(&cluster_trend_priority(b.trend))
            .then_with(|| b.average_gap.cmp(&a.average_gap))
            .then_with(|| b.average_edge.cmp(&a.average_edge))
            .then_with(|| b.member_count.cmp(&a.member_count))
            .then_with(|| a.cluster_id.cmp(&b.cluster_id))
    });
    clusters
}

fn cluster_linkage_key(
    hypothesis: &Hypothesis,
    path_map: &HashMap<&str, &PropagationPath>,
) -> String {
    if let Some(path_id) = hypothesis.propagation_path_ids.first() {
        if let Some(path) = path_map.get(path_id.as_str()) {
            if let Some(step) = path.steps.first() {
                return format!(
                    "path:{}->{}",
                    scope_node_id(&step.from),
                    scope_node_id(&step.to)
                );
            }
        }
        return format!("path:{}", path_id);
    }

    match &hypothesis.scope {
        crate::ontology::reasoning::ReasoningScope::Market(_) => "market".into(),
        _ => scope_node_id(&hypothesis.scope),
    }
}

fn strongest_member_index(setups: &[&TacticalSetup]) -> Option<usize> {
    setups
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| {
            action_priority(&a.action)
                .cmp(&action_priority(&b.action))
                .reverse()
                .then_with(|| a.confidence_gap.cmp(&b.confidence_gap))
                .then_with(|| a.heuristic_edge.cmp(&b.heuristic_edge))
                .then_with(|| a.confidence.cmp(&b.confidence))
        })
        .map(|(idx, _)| idx)
}

fn weakest_member_index(setups: &[&TacticalSetup]) -> Option<usize> {
    setups
        .iter()
        .enumerate()
        .min_by(|(_, a), (_, b)| {
            a.confidence_gap
                .cmp(&b.confidence_gap)
                .then_with(|| a.confidence.cmp(&b.confidence))
                .then_with(|| a.heuristic_edge.cmp(&b.heuristic_edge))
        })
        .map(|(idx, _)| idx)
}

fn cluster_trend(tracks: &[&HypothesisTrack]) -> HypothesisTrackStatus {
    let strengthening = tracks
        .iter()
        .filter(|track| track.status == HypothesisTrackStatus::Strengthening)
        .count();
    let weakening = tracks
        .iter()
        .filter(|track| {
            matches!(
                track.status,
                HypothesisTrackStatus::Weakening | HypothesisTrackStatus::Invalidated
            )
        })
        .count();
    let stable = tracks
        .iter()
        .filter(|track| track.status == HypothesisTrackStatus::Stable)
        .count();

    if strengthening > weakening && strengthening >= stable {
        HypothesisTrackStatus::Strengthening
    } else if weakening > strengthening && weakening >= stable {
        HypothesisTrackStatus::Weakening
    } else if tracks
        .iter()
        .all(|track| track.status == HypothesisTrackStatus::New)
    {
        HypothesisTrackStatus::New
    } else {
        HypothesisTrackStatus::Stable
    }
}

fn cluster_trend_priority(status: HypothesisTrackStatus) -> i32 {
    match status {
        HypothesisTrackStatus::Strengthening => 0,
        HypothesisTrackStatus::New => 1,
        HypothesisTrackStatus::Stable => 2,
        HypothesisTrackStatus::Weakening => 3,
        HypothesisTrackStatus::Invalidated => 4,
    }
}

fn family_label(family_key: &str) -> &'static str {
    match family_key {
        "flow" => "Flow",
        "liquidity" => "Liquidity",
        "propagation" => "Propagation",
        "risk" => "Risk",
        _ => "Narrative",
    }
}

pub(crate) fn cluster_title(
    family_key: &str,
    linkage_key: &str,
    member_count: usize,
    path: Option<&PropagationPath>,
) -> String {
    let family = family_label(family_key);
    if let Some(path) = path {
        if member_count <= 1 {
            format!("{} solo case via {}", family, path.summary)
        } else {
            format!("{} cluster x{} via {}", family, member_count, path.summary)
        }
    } else if member_count <= 1 {
        format!("{} solo case around {}", family, linkage_key)
    } else {
        format!("{} cluster x{} around {}", family, member_count, linkage_key)
    }
}
