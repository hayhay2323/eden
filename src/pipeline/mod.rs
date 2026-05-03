pub mod attention_budget;
pub mod belief;
pub mod belief_field;
// belief_modulation deleted — V2 fold belief into BP NodePrior via
// NodeId::BeliefEntropy + NodeId::BeliefSampleCount.
// broker_alignment_modulation deleted — rule-based direction modulation
pub mod broker_archetype;
pub mod broker_outcome_feedback;
// case_narrative deleted — rule-based storytelling
pub mod counterfactual_planner;
// cross_layer_narrative deleted — rule-based composite narrative
pub mod decision_ledger;
pub mod dimensions;
pub mod direction_flip;
pub mod encoded_tick_frame;
pub mod event_driven_bp;
// evidence_snippet deleted — rule-based string formatting for narratives
pub mod frontier;
pub mod graph_query_backend;
pub mod information_gain;
pub mod institution_archetype;
pub mod intent_belief;
// intent_modulation deleted — rule-based direction modulation
pub mod graph_attention;
pub mod intervention;
pub mod kl_surprise;
pub mod kl_tension;
pub mod latent_world_state;
pub mod learning_loop;
pub mod mechanism_inference;
pub mod replay_backtest;
pub mod sub_kg_emergence;
// modulation_report deleted — V2 has no modulation chain to report.
// BP posterior is the single source of truth for setup.confidence.
pub mod ontology_contract;
pub mod ontology_emergence;
pub mod oscillation;
pub mod outcome_feedback;
// outcome_history deleted — V2 fold outcome history into BP NodePrior
// via NodeId::OutcomeMemory.
pub mod perception;
pub mod predicate_engine;
pub mod prediction_calibration;
pub mod pressure;
pub mod pressure_events;
pub mod raw_events;
pub mod raw_expectation;
pub mod reasoning;
pub mod regime_classifier;
pub mod regime_fingerprint;
pub mod residual;
pub mod runtime_stage_trace;
pub mod signature_replay;
// sector_alignment_modulation deleted — rule-based direction modulation
pub mod action_promotion;
pub mod active_probe;
pub mod cluster_sync;
pub mod consistency_gauge;
pub mod cross_sector_contrast;
pub mod cross_symbol_propagation;
pub mod lead_lag_index;
pub mod loopy_bp;
pub mod regime_analog_index;
pub mod sector_intent;
pub mod sector_kinematics;
pub mod sector_sub_kg;
pub mod sector_to_symbol_propagation;
pub mod symbol_wl_analog_index;
pub mod wl_graph_signature;
// session_quality deleted — rule-tier bucketing (aggressive/normal/defensive)
pub mod signal_velocity;
pub mod signals;
// size_recommendation deleted — rule-based scalar from formula not data
pub mod state_composition;
pub mod state_engine;
pub mod state_labeler;
pub mod structural_causal;
pub mod structural_contrast;
pub mod structural_expectation;
pub mod structural_kinematics;
pub mod structural_persistence;
pub mod symbol_sub_kg;
pub mod temporal_graph_delta;
pub mod tension;
pub mod tick_state_machine;
pub mod visual_graph_frame;
pub mod world;

// closed_loop_tests deleted — wrapped the deleted modulation chain.
// V2 closed-loop validation lives in active_probe forecast tracking.
#[cfg(test)]
mod architecture_invariants_tests;
#[cfg(test)]
mod mechanism_integration_tests;
