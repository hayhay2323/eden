use super::*;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LensPriority {
    Iceberg,
    Structural,
    Causal,
    Lineage,
}

pub trait SignalLens: Send + Sync {
    fn name(&self) -> &'static str;
    fn priority(&self) -> LensPriority;
    fn observe(&self, ctx: &LensContext<'_>) -> Vec<LensObservation>;
}

#[allow(dead_code)]
pub struct LensContext<'a> {
    pub snapshot: &'a AgentSnapshot,
    pub symbol: &'a AgentSymbolState,
    pub current_transition: Option<&'a AgentTransition>,
    pub current_notice: Option<&'a AgentNotice>,
    pub backward: Option<&'a BackwardInvestigation>,
    pub bias: &'a str,
    pub confidence: Decimal,
    pub best_action: &'a str,
    pub severity: &'a str,
    pub expected_net_alpha: Option<Decimal>,
}

#[derive(Debug, Clone)]
pub struct LensObservation {
    pub lens_name: &'static str,
    pub confidence: Decimal,
    pub why_fragment: String,
    pub invalidation_fragments: Vec<String>,
    #[allow(dead_code)]
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, Default)]
pub struct LensBundle {
    #[allow(dead_code)]
    pub observations: Vec<LensObservation>,
    pub why_fragments: Vec<String>,
    pub invalidation_fragments: Vec<String>,
}
