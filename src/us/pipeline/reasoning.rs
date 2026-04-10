use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::ontology::reasoning::{
    Hypothesis, InvestigationSelection, PropagationPath, ReasoningScope, TacticalSetup,
};

#[path = "reasoning/propagation.rs"]
mod propagation;

#[derive(Debug, Clone)]
pub struct UsReasoningSnapshot {
    pub timestamp: OffsetDateTime,
    pub hypotheses: Vec<Hypothesis>,
    pub propagation_paths: Vec<PropagationPath>,
    pub investigation_selections: Vec<InvestigationSelection>,
    pub tactical_setups: Vec<TacticalSetup>,
    pub hypothesis_tracks: Vec<crate::ontology::reasoning::HypothesisTrack>,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UsStructuralRankMetrics {
    pub composite_delta: Decimal,
    pub composite_acceleration: Decimal,
    pub capital_flow_delta: Decimal,
    pub flow_persistence: u64,
    pub flow_reversal: bool,
}

impl UsReasoningSnapshot {
    pub fn empty(timestamp: OffsetDateTime) -> Self {
        Self {
            timestamp,
            hypotheses: Vec::new(),
            propagation_paths: Vec::new(),
            investigation_selections: Vec::new(),
            tactical_setups: Vec::new(),
            hypothesis_tracks: Vec::new(),
        }
    }
}

// ── Helpers (kept for propagation module) ──

fn scope_id(scope: &ReasoningScope) -> String {
    match scope {
        ReasoningScope::Market(_) => "market".into(),
        ReasoningScope::Symbol(s) => s.0.clone(),
        ReasoningScope::Sector(s) => format!("sector:{}", s),
        ReasoningScope::Institution(s) => format!("inst:{}", s),
        ReasoningScope::Theme(s) => format!("theme:{}", s),
        ReasoningScope::Region(s) => format!("region:{}", s),
        ReasoningScope::Custom(s) => format!("custom:{}", s),
    }
}

fn scope_label(scope: &ReasoningScope) -> String {
    match scope {
        ReasoningScope::Market(_) => "US market".into(),
        ReasoningScope::Symbol(s) => s.0.clone(),
        ReasoningScope::Sector(s) => format!("sector {}", s),
        ReasoningScope::Institution(s) => s.to_string(),
        ReasoningScope::Theme(s) => s.to_string(),
        ReasoningScope::Region(s) => s.to_string(),
        ReasoningScope::Custom(s) => s.to_string(),
    }
}
