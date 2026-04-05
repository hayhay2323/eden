use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::ontology::objects::{InstitutionId, Symbol};
use crate::pipeline::learning_loop::{ConditionedLearningAdjustment, ReasoningLearningFeedback};

#[derive(Debug, Clone, Default)]
pub struct AccumulatedKnowledge {
    pub institutional_memory: HashMap<(InstitutionId, Symbol), InstitutionSymbolProfile>,
    pub mechanism_priors: HashMap<String, MechanismPrior>,
    pub calibrated_weights: CalibratedWeights,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct InstitutionSymbolProfile {
    pub observation_count: u32,
    pub directional_hit_count: u32,
    pub avg_presence_ticks: Decimal,
    pub last_seen_tick: u64,
    pub directional_bias: Decimal,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MechanismPrior {
    pub hit_rate: Decimal,
    pub sample_count: u32,
    pub regime_hit_rates: HashMap<String, Decimal>,
    pub mean_net_return: Decimal,
}

#[derive(Debug, Clone, Default)]
pub struct CalibratedWeights {
    pub factor_adjustments: HashMap<(String, String), Decimal>,
    pub predicate_adjustments: HashMap<String, Decimal>,
    pub conditioned_adjustments: Vec<ConditionedLearningAdjustment>,
}

impl AccumulatedKnowledge {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn restored_from_calibration(feedback: Option<&ReasoningLearningFeedback>) -> Self {
        let mut knowledge = Self::empty();
        if let Some(feedback) = feedback {
            knowledge.apply_calibration(feedback);
        }
        knowledge
    }

    pub fn apply_calibration(&mut self, feedback: &ReasoningLearningFeedback) {
        self.calibrated_weights.factor_adjustments = feedback.mechanism_factor_lookup();
        self.calibrated_weights.predicate_adjustments = feedback
            .predicate_adjustments
            .iter()
            .map(|adj| (adj.label.clone(), adj.delta))
            .collect();
        self.calibrated_weights.conditioned_adjustments = feedback.conditioned_adjustments.clone();
    }

    pub fn accumulate_institutional_memory(
        &mut self,
        tick_number: u64,
        brain: &crate::graph::graph::BrainGraph,
    ) {
        use crate::graph::graph::{EdgeKind, NodeKind};
        use petgraph::visit::EdgeRef;

        for (&inst_id, &inst_idx) in &brain.institution_nodes {
            for edge in brain.graph.edges(inst_idx) {
                if let EdgeKind::InstitutionToStock(e) = edge.weight() {
                    let target = edge.target();
                    if let NodeKind::Stock(stock_node) = &brain.graph[target] {
                        let key = (inst_id, stock_node.symbol.clone());
                        let profile = self.institutional_memory.entry(key).or_insert(
                            InstitutionSymbolProfile {
                                observation_count: 0,
                                directional_hit_count: 0,
                                avg_presence_ticks: Decimal::ZERO,
                                last_seen_tick: tick_number,
                                directional_bias: Decimal::ZERO,
                            },
                        );
                        profile.observation_count += 1;
                        profile.last_seen_tick = tick_number;
                        // Running average of directional bias
                        let n = Decimal::from(profile.observation_count);
                        profile.directional_bias =
                            profile.directional_bias * (n - Decimal::ONE) / n + e.direction / n;
                    }
                }
            }
        }
    }

    #[cfg(feature = "persistence")]
    pub async fn restore_from(db: &crate::persistence::store::EdenStore, market: &str) -> Self {
        use crate::pipeline::learning_loop::{
            derive_learning_feedback, derive_outcome_learning_context_from_hk_rows,
            derive_outcome_learning_context_from_us_rows, OutcomeLearningContext,
        };

        let mut knowledge = Self::empty();

        if let Ok(assessments) = db
            .recent_case_reasoning_assessments_by_market(market, 200)
            .await
        {
            let outcome_ctx = match market {
                "hk" => {
                    if let Ok(rows) = db.recent_lineage_metric_rows(500).await {
                        derive_outcome_learning_context_from_hk_rows(&rows)
                    } else {
                        OutcomeLearningContext::default()
                    }
                }
                "us" => {
                    if let Ok(rows) = db.recent_us_lineage_metric_rows(500).await {
                        derive_outcome_learning_context_from_us_rows(&rows)
                    } else {
                        OutcomeLearningContext::default()
                    }
                }
                _ => OutcomeLearningContext::default(),
            };
            let feedback = derive_learning_feedback(&assessments, &outcome_ctx);
            knowledge = Self::restored_from_calibration(Some(&feedback));
        }

        eprintln!(
            "  [KNOWLEDGE] Restored: {} factor adjustments, {} predicate adjustments",
            knowledge.calibrated_weights.factor_adjustments.len(),
            knowledge.calibrated_weights.predicate_adjustments.len(),
        );

        knowledge
    }

    pub fn institution_history_bonus(
        &self,
        institution_id: &InstitutionId,
        symbol: &Symbol,
    ) -> Decimal {
        self.institutional_memory
            .get(&(*institution_id, symbol.clone()))
            .map(|profile| {
                if profile.observation_count >= 5 {
                    let hit_rate = Decimal::from(profile.directional_hit_count)
                        / Decimal::from(profile.observation_count);
                    (hit_rate - Decimal::new(5, 1)) * Decimal::new(2, 1)
                } else {
                    Decimal::ZERO
                }
            })
            .unwrap_or(Decimal::ZERO)
    }
}

#[cfg(test)]
mod tests {
    use rust_decimal_macros::dec;

    use super::*;
    use crate::pipeline::learning_loop::{
        LearningAdjustment, MechanismFactorAdjustment, OutcomeLearningContext,
    };

    #[test]
    fn empty_knowledge_has_no_data() {
        let k = AccumulatedKnowledge::empty();
        assert!(k.institutional_memory.is_empty());
        assert!(k.mechanism_priors.is_empty());
        assert!(k.calibrated_weights.factor_adjustments.is_empty());
    }

    #[test]
    fn institution_profile_hit_rate() {
        let profile = InstitutionSymbolProfile {
            observation_count: 10,
            directional_hit_count: 7,
            avg_presence_ticks: dec!(3.5),
            last_seen_tick: 100,
            directional_bias: dec!(0.6),
        };
        let hit_rate =
            Decimal::from(profile.directional_hit_count) / Decimal::from(profile.observation_count);
        assert_eq!(hit_rate, dec!(0.7));
    }

    #[test]
    fn calibrated_weights_default_is_empty() {
        let w = CalibratedWeights::default();
        assert!(w.factor_adjustments.is_empty());
        assert!(w.predicate_adjustments.is_empty());
        assert!(w.conditioned_adjustments.is_empty());
    }

    #[test]
    fn history_bonus_positive_for_high_hit_rate() {
        let mut k = AccumulatedKnowledge::empty();
        let iid = InstitutionId(100);
        let sym = Symbol("700.HK".into());
        k.institutional_memory.insert(
            (iid, sym.clone()),
            InstitutionSymbolProfile {
                observation_count: 10,
                directional_hit_count: 8,
                avg_presence_ticks: dec!(5.0),
                last_seen_tick: 50,
                directional_bias: dec!(0.7),
            },
        );
        let bonus = k.institution_history_bonus(&iid, &sym);
        // hit_rate = 0.8, bonus = (0.8 - 0.5) * 0.2 = 0.06
        assert_eq!(bonus, dec!(0.06));
    }

    #[test]
    fn history_bonus_negative_for_low_hit_rate() {
        let mut k = AccumulatedKnowledge::empty();
        let iid = InstitutionId(200);
        let sym = Symbol("9988.HK".into());
        k.institutional_memory.insert(
            (iid, sym.clone()),
            InstitutionSymbolProfile {
                observation_count: 10,
                directional_hit_count: 2,
                avg_presence_ticks: dec!(3.0),
                last_seen_tick: 40,
                directional_bias: dec!(-0.3),
            },
        );
        let bonus = k.institution_history_bonus(&iid, &sym);
        // hit_rate = 0.2, bonus = (0.2 - 0.5) * 0.2 = -0.06
        assert_eq!(bonus, dec!(-0.06));
    }

    #[test]
    fn history_bonus_zero_when_insufficient_samples() {
        let mut k = AccumulatedKnowledge::empty();
        let iid = InstitutionId(300);
        let sym = Symbol("5.HK".into());
        k.institutional_memory.insert(
            (iid, sym.clone()),
            InstitutionSymbolProfile {
                observation_count: 3,
                directional_hit_count: 3,
                avg_presence_ticks: dec!(2.0),
                last_seen_tick: 10,
                directional_bias: dec!(1.0),
            },
        );
        let bonus = k.institution_history_bonus(&iid, &sym);
        assert_eq!(bonus, Decimal::ZERO);
    }

    #[test]
    fn history_bonus_zero_when_not_found() {
        let k = AccumulatedKnowledge::empty();
        let bonus = k.institution_history_bonus(&InstitutionId(999), &Symbol("FAKE.HK".into()));
        assert_eq!(bonus, Decimal::ZERO);
    }

    #[test]
    fn accumulate_institutional_memory_from_edges() {
        use crate::action::narrative::Regime;
        use crate::graph::graph::{
            BrainGraph, EdgeKind, InstitutionNode, InstitutionToStock, NodeKind, StockNode,
        };
        use crate::ontology::domain::{ProvenanceMetadata, ProvenanceSource};
        use crate::pipeline::dimensions::SymbolDimensions;
        use petgraph::graph::DiGraph;
        use time::OffsetDateTime;

        let mut graph = DiGraph::new();
        let stock_idx = graph.add_node(NodeKind::Stock(StockNode {
            symbol: Symbol("700.HK".into()),
            regime: Regime::CoherentBullish,
            coherence: dec!(0.5),
            mean_direction: dec!(0.3),
            dimensions: SymbolDimensions::default(),
        }));
        let inst_idx = graph.add_node(NodeKind::Institution(InstitutionNode {
            institution_id: InstitutionId(100),
            stock_count: 2,
            bid_stock_count: 1,
            ask_stock_count: 1,
            net_direction: dec!(0.5),
        }));
        graph.add_edge(
            inst_idx,
            stock_idx,
            EdgeKind::InstitutionToStock(InstitutionToStock {
                direction: dec!(0.6),
                seat_count: 3,
                timestamp: OffsetDateTime::UNIX_EPOCH,
                provenance: ProvenanceMetadata {
                    source: ProvenanceSource::Computed,
                    observed_at: OffsetDateTime::UNIX_EPOCH,
                    received_at: Some(OffsetDateTime::UNIX_EPOCH),
                    confidence: Some(dec!(0.8)),
                    trace_id: None,
                    inputs: vec![],
                    note: None,
                },
            }),
        );

        let mut stock_nodes = std::collections::HashMap::new();
        stock_nodes.insert(Symbol("700.HK".into()), stock_idx);
        let mut institution_nodes = std::collections::HashMap::new();
        institution_nodes.insert(InstitutionId(100), inst_idx);

        let brain = BrainGraph {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            graph,
            market_temperature: None,
            stock_nodes,
            institution_nodes,
            sector_nodes: std::collections::HashMap::new(),
        };

        let mut k = AccumulatedKnowledge::empty();
        k.accumulate_institutional_memory(1, &brain);

        let key = (InstitutionId(100), Symbol("700.HK".into()));
        let profile = k.institutional_memory.get(&key).unwrap();
        assert_eq!(profile.observation_count, 1);
        assert_eq!(profile.last_seen_tick, 1);
        assert!(profile.directional_bias > Decimal::ZERO);

        // Second accumulation should increment
        k.accumulate_institutional_memory(2, &brain);
        let profile = k.institutional_memory.get(&key).unwrap();
        assert_eq!(profile.observation_count, 2);
        assert_eq!(profile.last_seen_tick, 2);
        assert_eq!(k.institutional_memory.len(), 1);
    }

    #[test]
    fn apply_calibration_populates_weights() {
        let mut k = AccumulatedKnowledge::empty();
        let feedback = ReasoningLearningFeedback {
            paired_examples: 5,
            reinforced_examples: 3,
            corrected_examples: 2,
            mechanism_adjustments: vec![LearningAdjustment {
                label: "Narrative Failure".into(),
                delta: dec!(-0.04),
                samples: 2,
            }],
            mechanism_factor_adjustments: vec![MechanismFactorAdjustment {
                mechanism: "Narrative Failure".into(),
                factor_key: "state:reflexive_correction".into(),
                factor_label: "Reflexive Correction".into(),
                delta: dec!(-0.02),
                samples: 2,
            }],
            predicate_adjustments: vec![LearningAdjustment {
                label: "Counterevidence Present".into(),
                delta: dec!(0.03),
                samples: 3,
            }],
            conditioned_adjustments: vec![ConditionedLearningAdjustment {
                mechanism: "Narrative Failure".into(),
                scope: "state".into(),
                conditioned_on: "Reflexive Correction".into(),
                delta: dec!(-0.01),
                samples: 1,
            }],
            outcome_context: OutcomeLearningContext::default(),
        };

        k.apply_calibration(&feedback);

        assert_eq!(k.calibrated_weights.factor_adjustments.len(), 1);
        assert_eq!(
            k.calibrated_weights.factor_adjustments[&(
                "Narrative Failure".into(),
                "state:reflexive_correction".into()
            )],
            dec!(-0.02)
        );
        assert_eq!(k.calibrated_weights.predicate_adjustments.len(), 1);
        assert_eq!(
            k.calibrated_weights.predicate_adjustments["Counterevidence Present"],
            dec!(0.03)
        );
        assert_eq!(k.calibrated_weights.conditioned_adjustments.len(), 1);
    }

    #[test]
    fn apply_calibration_replaces_previous_weights_instead_of_accumulating() {
        let mut k = AccumulatedKnowledge::empty();
        let first_feedback = ReasoningLearningFeedback {
            paired_examples: 1,
            reinforced_examples: 1,
            corrected_examples: 0,
            mechanism_adjustments: vec![],
            mechanism_factor_adjustments: vec![MechanismFactorAdjustment {
                mechanism: "First".into(),
                factor_key: "factor:a".into(),
                factor_label: "A".into(),
                delta: dec!(0.1),
                samples: 1,
            }],
            predicate_adjustments: vec![LearningAdjustment {
                label: "Alpha".into(),
                delta: dec!(0.2),
                samples: 1,
            }],
            conditioned_adjustments: vec![],
            outcome_context: OutcomeLearningContext::default(),
        };
        let second_feedback = ReasoningLearningFeedback {
            paired_examples: 2,
            reinforced_examples: 1,
            corrected_examples: 1,
            mechanism_adjustments: vec![],
            mechanism_factor_adjustments: vec![MechanismFactorAdjustment {
                mechanism: "Second".into(),
                factor_key: "factor:b".into(),
                factor_label: "B".into(),
                delta: dec!(-0.3),
                samples: 1,
            }],
            predicate_adjustments: vec![LearningAdjustment {
                label: "Beta".into(),
                delta: dec!(-0.4),
                samples: 1,
            }],
            conditioned_adjustments: vec![],
            outcome_context: OutcomeLearningContext::default(),
        };

        k.apply_calibration(&first_feedback);
        assert_eq!(k.calibrated_weights.factor_adjustments.len(), 1);
        assert!(k
            .calibrated_weights
            .factor_adjustments
            .contains_key(&("First".into(), "factor:a".into())));

        k.apply_calibration(&second_feedback);
        assert_eq!(k.calibrated_weights.factor_adjustments.len(), 1);
        assert!(k
            .calibrated_weights
            .factor_adjustments
            .contains_key(&("Second".into(), "factor:b".into())));
        assert!(!k
            .calibrated_weights
            .factor_adjustments
            .contains_key(&("First".into(), "factor:a".into())));
        assert_eq!(k.calibrated_weights.predicate_adjustments.len(), 1);
        assert!(k
            .calibrated_weights
            .predicate_adjustments
            .contains_key("Beta"));
        assert!(!k
            .calibrated_weights
            .predicate_adjustments
            .contains_key("Alpha"));
    }

    #[test]
    fn restored_from_calibration_is_empty_without_feedback() {
        let knowledge = AccumulatedKnowledge::restored_from_calibration(None);
        assert!(knowledge.institutional_memory.is_empty());
        assert!(knowledge.mechanism_priors.is_empty());
        assert!(knowledge.calibrated_weights.factor_adjustments.is_empty());
        assert!(knowledge
            .calibrated_weights
            .predicate_adjustments
            .is_empty());
    }

    #[test]
    fn restored_from_calibration_applies_feedback() {
        let feedback = ReasoningLearningFeedback {
            paired_examples: 3,
            reinforced_examples: 2,
            corrected_examples: 1,
            mechanism_adjustments: vec![],
            mechanism_factor_adjustments: vec![MechanismFactorAdjustment {
                mechanism: "Narrative Failure".into(),
                factor_key: "state:reflexive_correction".into(),
                factor_label: "Reflexive Correction".into(),
                delta: dec!(-0.02),
                samples: 2,
            }],
            predicate_adjustments: vec![LearningAdjustment {
                label: "Counterevidence Present".into(),
                delta: dec!(0.03),
                samples: 3,
            }],
            conditioned_adjustments: vec![],
            outcome_context: OutcomeLearningContext::default(),
        };

        let knowledge = AccumulatedKnowledge::restored_from_calibration(Some(&feedback));
        assert_eq!(knowledge.calibrated_weights.factor_adjustments.len(), 1);
        assert_eq!(
            knowledge.calibrated_weights.factor_adjustments[&(
                "Narrative Failure".into(),
                "state:reflexive_correction".into()
            )],
            dec!(-0.02)
        );
        assert_eq!(knowledge.calibrated_weights.predicate_adjustments.len(), 1);
        assert_eq!(
            knowledge.calibrated_weights.predicate_adjustments["Counterevidence Present"],
            dec!(0.03)
        );
    }
}
