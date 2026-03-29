use std::collections::HashMap;

use crate::external::polymarket::{PolymarketBias, PolymarketPrior, PolymarketSnapshot};
use crate::graph::decision::DecisionSnapshot;
use crate::graph::insights::GraphInsights;
use crate::ontology::world::{
    BackwardReasoningSnapshot, EntityState, WorldLayer, WorldStateSnapshot,
};
use crate::ontology::{
    ProvenanceMetadata, ProvenanceSource, ReasoningScope,
};
use crate::pipeline::reasoning::ReasoningSnapshot;
use crate::pipeline::signals::{DerivedSignalSnapshot, EventSnapshot, SignalScope};
use rust_decimal::Decimal;
use rust_decimal_macros::dec;
use serde::{Deserialize, Serialize};

use crate::math::clamp_unit_interval;

#[path = "world/backward.rs"]
mod backward;
use backward::{
    derive_backward_reasoning, scope_key, world_layer_priority, world_provenance,
};

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

    for (sector, hypotheses) in sectors {
        let Some(top) = hypotheses
            .iter()
            .max_by(|a, b| a.confidence.cmp(&b.confidence))
            .copied()
        else {
            continue;
        };
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
            regime: top.statement.clone(),
            confidence: top.confidence,
            local_support: top.local_support_weight,
            propagated_support: top.propagated_support_weight,
            drivers: top.expected_observations.clone(),
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

    let _ = (events, decision); // reserved for richer world-state updates
    WorldStateSnapshot {
        timestamp: reasoning.timestamp,
        entities,
    }
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


#[cfg(test)]
#[path = "world_tests.rs"]
mod tests;
