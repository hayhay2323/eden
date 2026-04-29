use serde::{Deserialize, Serialize};

/// An active hypothesis being tracked by the reasoning layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActiveHypothesis {
    pub id: String,
    pub label: String,
    pub confidence: f64,
}

/// A summary of a causal chain discovered during reasoning.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CausalChainSummary {
    pub id: String,
    pub description: String,
    pub strength: f64,
}

/// A reference to an investigation that is pending execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingInvestigation {
    pub id: String,
    pub label: String,
}

/// The active reasoning and causal-inference layer.
///
/// Tracks hypotheses currently under evaluation, causal chains
/// discovered so far, and investigations queued for execution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReasoningState {
    pub active_hypotheses: Vec<ActiveHypothesis>,
    pub causal_chains: Vec<CausalChainSummary>,
    pub pending_investigations: Vec<PendingInvestigation>,
}

impl ReasoningState {
    pub fn new() -> Self {
        Self {
            active_hypotheses: Vec::new(),
            causal_chains: Vec::new(),
            pending_investigations: Vec::new(),
        }
    }

    /// Add a new hypothesis to the active set.
    pub fn add_hypothesis(&mut self, id: String, label: String, confidence: f64) {
        self.active_hypotheses.push(ActiveHypothesis {
            id,
            label,
            confidence,
        });
    }

    /// Remove a hypothesis by id and return it, if found.
    pub fn resolve_hypothesis(&mut self, id: &str) -> Option<ActiveHypothesis> {
        if let Some(pos) = self.active_hypotheses.iter().position(|h| h.id == id) {
            Some(self.active_hypotheses.remove(pos))
        } else {
            None
        }
    }

    /// Number of active hypotheses.
    pub fn active_count(&self) -> usize {
        self.active_hypotheses.len()
    }
}

impl Default for ReasoningState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_and_resolve_hypothesis() {
        let mut state = ReasoningState::new();
        state.add_hypothesis("h1".into(), "test hypothesis".into(), 0.75);
        assert_eq!(state.active_count(), 1);

        let resolved = state.resolve_hypothesis("h1");
        assert!(resolved.is_some());
        assert_eq!(resolved.unwrap().id, "h1");
        assert_eq!(state.active_count(), 0);
    }
}
