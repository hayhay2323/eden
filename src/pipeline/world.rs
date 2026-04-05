use std::collections::HashMap;

use crate::external::polymarket::{PolymarketBias, PolymarketPrior, PolymarketSnapshot};
use crate::graph::decision::DecisionSnapshot;
use crate::graph::insights::GraphInsights;
use crate::ontology::reasoning::{HypothesisTrack, TacticalSetup};
use crate::ontology::world::{
    BackwardReasoningSnapshot, EntityState, FlowPath, FlowPolarity, Vortex, WorldLayer,
    WorldStateSnapshot,
};
use crate::ontology::{ProvenanceMetadata, ProvenanceSource, ReasoningScope};
use crate::pipeline::reasoning::ReasoningSnapshot;
use crate::pipeline::signals::{DerivedSignalSnapshot, EventSnapshot, SignalScope};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use crate::math::clamp_unit_interval;

#[path = "world/backward.rs"]
mod backward;
use backward::{derive_backward_reasoning, scope_key, world_layer_priority, world_provenance};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorldSnapshots {
    pub world_state: WorldStateSnapshot,
    pub backward_reasoning: BackwardReasoningSnapshot,
}

impl WorldSnapshots {
    pub fn derive(
        events: &EventSnapshot,
        derived_signals: &DerivedSignalSnapshot,
        insights: &GraphInsights,
        decision: &DecisionSnapshot,
        reasoning: &ReasoningSnapshot,
        polymarket: Option<&PolymarketSnapshot>,
        previous_backward_reasoning: Option<&BackwardReasoningSnapshot>,
    ) -> Self {
        let world_state = derive_world_state(
            events,
            derived_signals,
            insights,
            decision,
            reasoning,
            polymarket,
        );
        let backward_reasoning =
            derive_backward_reasoning(reasoning, &world_state, previous_backward_reasoning);
        Self {
            world_state,
            backward_reasoning,
        }
    }
}

pub fn derive_with_backward_confirmation(
    events: &EventSnapshot,
    derived_signals: &DerivedSignalSnapshot,
    insights: &GraphInsights,
    decision: &DecisionSnapshot,
    reasoning: &mut ReasoningSnapshot,
    previous_setups: &[TacticalSetup],
    previous_tracks: &[HypothesisTrack],
    polymarket: Option<&PolymarketSnapshot>,
    previous_backward_reasoning: Option<&BackwardReasoningSnapshot>,
) -> WorldSnapshots {
    let provisional = WorldSnapshots::derive(
        events,
        derived_signals,
        insights,
        decision,
        reasoning,
        polymarket,
        previous_backward_reasoning,
    );
    if crate::pipeline::reasoning::apply_backward_confirmation_gate(
        reasoning,
        previous_setups,
        previous_tracks,
        &provisional.backward_reasoning,
    ) {
        WorldSnapshots::derive(
            events,
            derived_signals,
            insights,
            decision,
            reasoning,
            polymarket,
            previous_backward_reasoning,
        )
    } else {
        provisional
    }
}

fn derive_world_state(
    events: &EventSnapshot,
    derived_signals: &DerivedSignalSnapshot,
    insights: &GraphInsights,
    decision: &DecisionSnapshot,
    reasoning: &ReasoningSnapshot,
    polymarket: Option<&PolymarketSnapshot>,
) -> WorldStateSnapshot {
    let mut entities = Vec::new();
    let mut sectors: HashMap<crate::ontology::SectorId, Vec<&crate::ontology::Hypothesis>> =
        HashMap::new();
    let polymarket_priors = polymarket
        .map(|snapshot| {
            snapshot
                .priors
                .iter()
                .filter(|prior| prior.active && !prior.closed && prior.is_material())
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let market_driver = strongest_market_prior(&polymarket_priors);
    let market_regime = external_market_regime(
        &polymarket_priors,
        !reasoning.case_clusters.is_empty(),
        insights.stress.composite_stress,
    );

    for hypothesis in &reasoning.hypotheses {
        if let ReasoningScope::Sector(sector) = &hypothesis.scope {
            sectors.entry(sector.clone()).or_default().push(hypothesis);
        }
    }

    entities.push(EntityState {
        entity_id: "world:market".into(),
        scope: ReasoningScope::market(),
        layer: WorldLayer::Forest,
        provenance: world_provenance(
            reasoning.timestamp,
            "world:market",
            [
                format!(
                    "market_stress:{}",
                    insights.stress.composite_stress.round_dp(4)
                ),
                format!("clusters:{}", reasoning.case_clusters.len()),
                market_driver
                    .as_ref()
                    .map(|prior| format!("polymarket:{}", prior.slug))
                    .unwrap_or_else(|| "polymarket:none".into()),
            ],
            "market canopy",
            insights.stress.composite_stress,
        ),
        label: "Market canopy".into(),
        regime: market_regime,
        // confidence represents how confident we are in our market assessment,
        // not the stress level itself. With data flowing and multiple signal sources,
        // confidence is high regardless of whether the market is calm or stressed.
        confidence: {
            let signal_count = derived_signals
                .signals
                .iter()
                .filter(|signal| matches!(signal.value.scope, SignalScope::Market))
                .count();
            let cluster_count = reasoning.case_clusters.len();
            // More data sources → higher confidence in assessment
            clamp_unit_interval(
                dec!(0.50)
                    + Decimal::from(signal_count.min(6) as i64) * dec!(0.05)
                    + Decimal::from(cluster_count.min(4) as i64) * dec!(0.025),
            )
        },
        local_support: derived_signals
            .signals
            .iter()
            .filter(|signal| matches!(signal.value.scope, SignalScope::Market))
            .map(|signal| signal.value.strength.abs())
            .sum::<Decimal>()
            .min(Decimal::ONE),
        propagated_support: reasoning
            .propagation_paths
            .iter()
            .filter(|path| path.steps.len() > 1)
            .map(|path| path.confidence)
            .max()
            .unwrap_or(Decimal::ZERO),
        drivers: vec![
            format!(
                "market stress={}",
                insights.stress.composite_stress.round_dp(3)
            ),
            format!("clusters={}", reasoning.case_clusters.len()),
            market_driver
                .map(|prior| prior.driver_text())
                .unwrap_or_else(|| "polymarket=none".into()),
        ],
    });

    let sector_narratives = extract_sector_narratives(events);

    for (sector, hypotheses) in sectors {
        let Some(top) = hypotheses
            .iter()
            .max_by(|a, b| a.confidence.cmp(&b.confidence))
            .copied()
        else {
            continue;
        };

        let narrative = sector_narratives.get(&sector.0);
        let regime = if let Some(narrative) = narrative {
            format!(
                "sector {} {}: {}",
                sector, narrative.dominant_driver, narrative.summary
            )
        } else {
            top.statement.clone()
        };
        let mut drivers = top.expected_observations.clone();
        if let Some(narrative) = narrative {
            drivers.push(format!("cause={}", narrative.dominant_driver));
            if let Some(ref label) = narrative.dominant_label {
                drivers.push(format!("narrative={}", label));
            }
            drivers.push(format!(
                "evidence={}/{} events attributed",
                narrative.attributed_count, narrative.total_events
            ));
        }

        entities.push(EntityState {
            entity_id: format!("world:sector:{}", sector),
            scope: ReasoningScope::Sector(sector.clone()),
            layer: WorldLayer::Trunk,
            provenance: top
                .provenance
                .clone()
                .with_trace_id(format!("world:sector:{}", sector))
                .with_note("sector trunk"),
            label: format!("Sector {}", sector),
            regime,
            confidence: top.confidence,
            local_support: top.local_support_weight,
            propagated_support: top.propagated_support_weight,
            drivers,
        });
    }

    for setup in reasoning
        .tactical_setups
        .iter()
        .filter(|setup| matches!(setup.scope, ReasoningScope::Symbol(_)))
        .take(12)
    {
        if let Some(hypothesis) = reasoning
            .hypotheses
            .iter()
            .find(|hypothesis| hypothesis.hypothesis_id == setup.hypothesis_id)
        {
            entities.push(EntityState {
                entity_id: format!("world:{}", setup.setup_id),
                scope: setup.scope.clone(),
                layer: WorldLayer::Leaf,
                provenance: setup
                    .provenance
                    .clone()
                    .with_trace_id(format!("world:{}", setup.setup_id))
                    .with_note("leaf case"),
                label: setup.title.clone(),
                regime: setup.action.clone(),
                confidence: setup.confidence,
                local_support: hypothesis.local_support_weight,
                propagated_support: hypothesis.propagated_support_weight,
                drivers: hypothesis
                    .evidence
                    .iter()
                    .take(2)
                    .map(|item| item.statement.clone())
                    .collect(),
            });
        }
    }

    for rotation in insights.rotations.iter().take(6) {
        entities.push(EntityState {
            entity_id: format!(
                "world:rotation:{}:{}",
                rotation.from_sector, rotation.to_sector
            ),
            scope: ReasoningScope::Custom(
                format!("{}->{}", rotation.from_sector, rotation.to_sector).into(),
            ),
            layer: WorldLayer::Branch,
            provenance: world_provenance(
                reasoning.timestamp,
                &format!(
                    "world:rotation:{}:{}",
                    rotation.from_sector, rotation.to_sector
                ),
                [format!(
                    "rotation:{}:{}",
                    rotation.from_sector, rotation.to_sector
                )],
                "rotation branch",
                rotation.spread.abs().min(Decimal::ONE),
            ),
            label: format!(
                "Rotation {} -> {}",
                rotation.from_sector, rotation.to_sector
            ),
            regime: if rotation.widening {
                "widening-rotation".into()
            } else {
                "narrowing-rotation".into()
            },
            confidence: rotation.spread.abs().min(Decimal::ONE),
            local_support: Decimal::ZERO,
            propagated_support: rotation.spread.abs().min(Decimal::ONE),
            drivers: vec![format!(
                "spread_delta={}",
                rotation.spread_delta.round_dp(3)
            )],
        });
    }

    append_polymarket_entities(&mut entities, reasoning.timestamp, &polymarket_priors);

    entities.sort_by(|a, b| {
        world_layer_priority(a.layer)
            .cmp(&world_layer_priority(b.layer))
            .then_with(|| b.confidence.cmp(&a.confidence))
            .then_with(|| a.entity_id.cmp(&b.entity_id))
    });

    let vortices = detect_vortices(&entities, events, reasoning);
    let _ = decision;
    WorldStateSnapshot {
        timestamp: reasoning.timestamp,
        entities,
        vortices,
    }
}

/// Detect vortices: convergence points where multiple independent causal paths meet.
///
/// A vortex forms when a branch/trunk-level entity receives flow from multiple
/// leaf-level entities through distinct channels. The algorithm:
/// 1. Identify candidate centers (Branch/Trunk/Forest entities with drivers)
/// 2. For each center, find leaf entities whose scope is contained in the center's scope
/// 3. Each feeding leaf contributes a FlowPath with its channel and polarity
/// 4. Score the vortex by path count, channel diversity, and coherence
fn detect_vortices(
    entities: &[EntityState],
    events: &EventSnapshot,
    reasoning: &ReasoningSnapshot,
) -> Vec<Vortex> {
    let _ = events;
    let leaf_entities: Vec<&EntityState> = entities
        .iter()
        .filter(|e| e.layer == WorldLayer::Leaf)
        .collect();
    let center_candidates: Vec<&EntityState> = entities
        .iter()
        .filter(|e| {
            matches!(
                e.layer,
                WorldLayer::Branch | WorldLayer::Trunk | WorldLayer::Forest
            )
        })
        .filter(|e| !e.drivers.is_empty() || e.confidence > dec!(0.5))
        .collect();

    let hypothesis_lookup: HashMap<String, &crate::ontology::Hypothesis> = reasoning
        .hypotheses
        .iter()
        .map(|h| (scope_key(&h.scope), h))
        .collect();

    let mut vortices = Vec::new();

    for center in &center_candidates {
        let center_scope_key = scope_key(&center.scope);
        let mut flow_paths = Vec::new();
        let mut channels: std::collections::HashSet<String> = std::collections::HashSet::new();

        for leaf in &leaf_entities {
            if !leaf_feeds_center(leaf, center) {
                continue;
            }
            let channel = infer_channel(leaf, center);
            let polarity = infer_polarity(leaf, center, &hypothesis_lookup);
            channels.insert(channel.clone());
            flow_paths.push(FlowPath {
                source_entity_id: leaf.entity_id.clone(),
                source_scope: leaf.scope.clone(),
                channel,
                weight: leaf.confidence,
                polarity,
            });
        }

        if flow_paths.len() < 2 {
            continue;
        }

        let confirming = flow_paths
            .iter()
            .filter(|p| p.polarity == FlowPolarity::Confirming)
            .count();
        let contradicting = flow_paths
            .iter()
            .filter(|p| p.polarity == FlowPolarity::Contradicting)
            .count();
        let total = flow_paths.len();
        let coherence = if total > 0 {
            let dominant = confirming.max(contradicting);
            Decimal::from(dominant as u64) / Decimal::from(total as u64)
        } else {
            Decimal::ZERO
        };

        let weight_sum: Decimal = flow_paths.iter().map(|p| p.weight).sum();
        let path_count_factor = Decimal::from(flow_paths.len().min(10) as u64) / dec!(10);
        let diversity_factor = Decimal::from(channels.len().min(5) as u64) / dec!(5);
        let strength =
            clamp_unit_interval(weight_sum / dec!(3) * path_count_factor * diversity_factor);

        vortices.push(Vortex {
            vortex_id: format!("vortex:{}", center_scope_key),
            center_entity_id: center.entity_id.clone(),
            center_scope: center.scope.clone(),
            layer: center.layer,
            flow_paths,
            strength,
            channel_diversity: channels.len(),
            coherence,
            narrative: None,
        });
    }

    vortices.sort_by(|a, b| b.strength.cmp(&a.strength));
    vortices.truncate(10);
    vortices
}

fn qualifies_for_attention_boost(vortex: &Vortex) -> bool {
    vortex.strength >= dec!(0.3) && vortex.coherence >= dec!(0.6)
}

fn leaf_feeds_center(leaf: &EntityState, center: &EntityState) -> bool {
    match (&leaf.scope, &center.scope) {
        (ReasoningScope::Symbol(_), ReasoningScope::Sector(sector)) => {
            leaf.drivers.iter().any(|d| d.contains(&sector.0)) || leaf.entity_id.contains(&sector.0)
        }
        (ReasoningScope::Symbol(_), ReasoningScope::Market(_)) => true,
        (ReasoningScope::Sector(_), ReasoningScope::Market(_)) => true,
        (ReasoningScope::Symbol(_), ReasoningScope::Theme(theme)) => {
            leaf.drivers.iter().any(|d| d.contains(&theme.0))
        }
        _ => false,
    }
}

fn infer_channel(leaf: &EntityState, _center: &EntityState) -> String {
    if leaf
        .drivers
        .iter()
        .any(|d| d.contains("broker") || d.contains("flow"))
    {
        return "broker_flow".into();
    }
    if leaf
        .drivers
        .iter()
        .any(|d| d.contains("volume") || d.contains("spike"))
    {
        return "volume".into();
    }
    if leaf
        .drivers
        .iter()
        .any(|d| d.contains("gap") || d.contains("price"))
    {
        return "price_action".into();
    }
    if leaf
        .drivers
        .iter()
        .any(|d| d.contains("catalyst") || d.contains("event"))
    {
        return "catalyst".into();
    }
    if leaf
        .drivers
        .iter()
        .any(|d| d.contains("propagat") || d.contains("peer"))
    {
        return "propagation".into();
    }
    "structure".into()
}

fn infer_polarity(
    leaf: &EntityState,
    center: &EntityState,
    hypothesis_lookup: &HashMap<String, &crate::ontology::Hypothesis>,
) -> FlowPolarity {
    let leaf_hyp = hypothesis_lookup.get(&scope_key(&leaf.scope));
    let center_hyp = hypothesis_lookup.get(&scope_key(&center.scope));
    match (leaf_hyp, center_hyp) {
        (Some(lh), Some(ch)) => {
            let leaf_bullish = lh.local_support_weight > lh.local_contradict_weight;
            let center_bullish = ch.local_support_weight > ch.local_contradict_weight;
            if leaf_bullish == center_bullish {
                FlowPolarity::Confirming
            } else {
                FlowPolarity::Contradicting
            }
        }
        _ => FlowPolarity::Ambiguous,
    }
}

/// Extract scopes that sit at the center of strong vortices.
/// These can be used by the attention budget allocator to upgrade
/// associated symbols from Standard to Deep.
pub fn vortex_boosted_scopes(world_state: &WorldStateSnapshot) -> Vec<(ReasoningScope, Decimal)> {
    world_state
        .vortices
        .iter()
        .filter(|v| qualifies_for_attention_boost(v))
        .map(|v| (v.center_scope.clone(), v.strength))
        .collect()
}

/// Extract symbol scopes that feed into strong vortices.
/// These symbols sit on the edge of a convergence structure and should
/// receive at least standard attention on the following tick.
pub fn vortex_edge_symbol_scopes(
    world_state: &WorldStateSnapshot,
) -> Vec<(ReasoningScope, Decimal)> {
    let mut strongest = HashMap::<ReasoningScope, Decimal>::new();

    for vortex in world_state
        .vortices
        .iter()
        .filter(|v| qualifies_for_attention_boost(v))
    {
        for path in &vortex.flow_paths {
            if let ReasoningScope::Symbol(_) = &path.source_scope {
                strongest
                    .entry(path.source_scope.clone())
                    .and_modify(|current| {
                        if vortex.strength > *current {
                            *current = vortex.strength;
                        }
                    })
                    .or_insert(vortex.strength);
            }
        }
    }

    let mut scopes = strongest.into_iter().collect::<Vec<_>>();
    scopes.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.label().cmp(&b.0.label())));
    scopes
}

fn strongest_market_prior(priors: &[PolymarketPrior]) -> Option<&PolymarketPrior> {
    priors
        .iter()
        .filter(|prior| matches!(prior.scope, ReasoningScope::Market(_)))
        .max_by(|a, b| a.probability.cmp(&b.probability))
}

fn external_market_regime(
    priors: &[PolymarketPrior],
    has_clusters: bool,
    market_stress: Decimal,
) -> String {
    let default_regime = if market_stress >= Decimal::new(45, 2) {
        "stress-dominant"
    } else if has_clusters {
        "narrative-fragmenting"
    } else {
        "locally-driven"
    };

    let strongest_risk_off = priors
        .iter()
        .filter(|prior| matches!(prior.scope, ReasoningScope::Market(_)))
        .filter(|prior| prior.bias == PolymarketBias::RiskOff)
        .max_by(|a, b| a.probability.cmp(&b.probability));
    if let Some(prior) = strongest_risk_off {
        if prior.probability >= Decimal::new(65, 2) {
            return format!("event-risk-off ({})", prior.label);
        }
    }

    let strongest_risk_on = priors
        .iter()
        .filter(|prior| matches!(prior.scope, ReasoningScope::Market(_)))
        .filter(|prior| prior.bias == PolymarketBias::RiskOn)
        .max_by(|a, b| a.probability.cmp(&b.probability));
    if let Some(prior) = strongest_risk_on {
        if prior.probability >= Decimal::new(65, 2) {
            return format!("event-risk-on ({})", prior.label);
        }
    }

    default_regime.into()
}

fn append_polymarket_entities(
    entities: &mut Vec<EntityState>,
    timestamp: time::OffsetDateTime,
    priors: &[PolymarketPrior],
) {
    for prior in priors.iter().take(6) {
        let provenance =
            ProvenanceMetadata::new(ProvenanceSource::External("polymarket".into()), timestamp)
                .with_trace_id(format!("world:polymarket:{}", prior.slug))
                .with_inputs([
                    format!("polymarket:{}", prior.slug),
                    format!("outcome:{}", prior.selected_outcome),
                ])
                .with_note("external event prior");
        entities.push(EntityState {
            entity_id: format!("world:polymarket:{}", prior.slug),
            scope: prior.scope.clone(),
            layer: polymarket_layer(&prior.scope),
            provenance: provenance.clone(),
            label: format!("Polymarket {}", prior.label),
            regime: format!("{} {}", prior.bias.as_str(), prior.selected_outcome),
            confidence: prior.probability,
            local_support: Decimal::ZERO,
            propagated_support: prior.probability,
            drivers: vec![
                prior.question.clone(),
                format!(
                    "probability={:.0}%",
                    (prior.probability * Decimal::new(100, 0)).round_dp(0)
                ),
                prior
                    .category
                    .clone()
                    .map(|category| format!("category={}", category))
                    .unwrap_or_else(|| "category=unknown".into()),
            ],
        });

        for target_scope in prior.parsed_target_scopes() {
            if target_scope == prior.scope {
                continue;
            }
            entities.push(EntityState {
                entity_id: format!(
                    "world:polymarket:{}:{}",
                    prior.slug,
                    scope_key(&target_scope)
                ),
                scope: target_scope.clone(),
                layer: polymarket_layer(&target_scope),
                provenance: provenance
                    .clone()
                    .with_trace_id(format!(
                        "world:polymarket:{}:{}",
                        prior.slug,
                        scope_key(&target_scope)
                    ))
                    .with_note("external event target"),
                label: format!("Polymarket target {}", prior.label),
                regime: format!("{} {}", prior.bias.as_str(), prior.selected_outcome),
                confidence: prior.probability,
                local_support: Decimal::ZERO,
                propagated_support: prior.probability,
                drivers: vec![
                    format!("source={}", prior.slug),
                    format!("target={}", scope_key(&target_scope)),
                    prior.question.clone(),
                ],
            });
        }
    }
}

fn polymarket_layer(scope: &ReasoningScope) -> WorldLayer {
    match scope {
        ReasoningScope::Market(_) => WorldLayer::Branch,
        ReasoningScope::Sector(_) | ReasoningScope::Region(_) => WorldLayer::Trunk,
        ReasoningScope::Theme(_) | ReasoningScope::Custom(_) | ReasoningScope::Institution(_) => {
            WorldLayer::Branch
        }
        ReasoningScope::Symbol(_) => WorldLayer::Leaf,
    }
}

struct SectorNarrative {
    dominant_driver: String,
    dominant_label: Option<String>,
    summary: String,
    attributed_count: usize,
    total_events: usize,
}

fn extract_sector_narratives(events: &EventSnapshot) -> HashMap<String, SectorNarrative> {
    let mut sector_data: HashMap<String, Vec<(&str, Option<&str>)>> = HashMap::new();

    for event in &events.events {
        let sector_id = match &event.value.scope {
            SignalScope::Sector(sector) => Some(sector.0.as_str()),
            _ => None,
        };
        let Some(sector_id) = sector_id else {
            continue;
        };

        let mut driver: Option<&str> = None;
        let mut label: Option<&str> = None;
        for input in &event.provenance.inputs {
            if let Some(rest) = input.strip_prefix("attr:driver=") {
                driver = Some(rest);
            }
            if let Some(rest) = input.strip_prefix("attr:label=") {
                label = Some(rest);
            }
        }
        if let Some(driver) = driver {
            sector_data
                .entry(sector_id.to_string())
                .or_default()
                .push((driver, label));
        }
    }

    let mut result = HashMap::new();
    for (sector_id, attributions) in sector_data {
        let total_events = attributions.len();
        if total_events == 0 {
            continue;
        }

        let mut driver_counts: HashMap<&str, usize> = HashMap::new();
        let mut label_counts: HashMap<&str, usize> = HashMap::new();
        for (driver, label) in &attributions {
            *driver_counts.entry(driver).or_default() += 1;
            if let Some(label) = label {
                *label_counts.entry(label).or_default() += 1;
            }
        }

        let dominant_driver = driver_counts
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(driver, _)| driver.to_string())
            .unwrap_or_else(|| "unknown".into());
        let dominant_label = label_counts
            .iter()
            .max_by_key(|(_, count)| *count)
            .map(|(label, _)| label.to_string());

        let summary = match dominant_driver.as_str() {
            "macro_wide" => {
                if let Some(ref label) = dominant_label {
                    format!(
                        "driven by macro-level forces ({}), expect broad sector impact",
                        label
                    )
                } else {
                    "driven by macro-level forces, expect broad sector impact".into()
                }
            }
            "sector_wide" => {
                if let Some(ref label) = dominant_label {
                    format!(
                        "sector-specific pressure from {}, peers should co-move",
                        label
                    )
                } else {
                    "sector-specific pressure detected, peers should co-move".into()
                }
            }
            "company_specific" => {
                "company-specific events dominate, sector propagation unlikely".into()
            }
            _ => format!("{} activity detected", dominant_driver),
        };

        result.insert(
            sector_id,
            SectorNarrative {
                dominant_driver,
                dominant_label,
                summary,
                attributed_count: total_events,
                total_events,
            },
        );
    }
    result
}

#[cfg(test)]
#[path = "world_tests.rs"]
mod tests;
