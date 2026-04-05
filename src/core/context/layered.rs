use serde::{Deserialize, Serialize};

use super::live_context::LiveContext;
use super::reasoning_state::ReasoningState;
use super::session_memory::SessionMemory;
use super::static_context::StaticContext;

/// The full layered context aggregating all four context tiers.
///
/// - **static_ctx**: invariants for the session (market, universe, date).
/// - **live**: per-tick snapshot of market state.
/// - **memory**: cross-tick session memory (decisions, hypothesis outcomes).
/// - **reasoning**: active causal reasoning state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LayeredContext {
    pub static_ctx: StaticContext,
    pub live: LiveContext,
    pub memory: SessionMemory,
    pub reasoning: ReasoningState,
}

impl LayeredContext {
    /// Build a new layered context from its constituent layers.
    pub fn new(
        static_ctx: StaticContext,
        live: LiveContext,
        memory: SessionMemory,
        reasoning: ReasoningState,
    ) -> Self {
        Self {
            static_ctx,
            live,
            memory,
            reasoning,
        }
    }

    /// Convenience builder: create a context with fresh memory and reasoning
    /// from just the static and live layers.
    pub fn from_static_and_live(static_ctx: StaticContext, live: LiveContext) -> Self {
        Self {
            static_ctx,
            live,
            memory: SessionMemory::new(),
            reasoning: ReasoningState::new(),
        }
    }

    /// Current tick number from the live layer.
    pub fn tick(&self) -> u64 {
        self.live.tick_count
    }

    /// Market identifier from the static layer.
    pub fn market(&self) -> &str {
        &self.static_ctx.market
    }

    /// Session date from the static layer.
    pub fn session_date(&self) -> &str {
        &self.static_ctx.session_date
    }

    /// Number of symbols in the universe.
    pub fn universe_size(&self) -> usize {
        self.static_ctx.universe_size()
    }

    /// Number of decisions made so far this session.
    pub fn decisions_made(&self) -> usize {
        self.memory.decision_count()
    }

    /// Number of active hypotheses in the reasoning layer.
    pub fn active_hypotheses(&self) -> usize {
        self.reasoning.active_count()
    }

    /// Signal accuracy from session memory for a given signal type.
    pub fn signal_accuracy(&self, signal_type: &str) -> Option<f64> {
        self.memory.signal_accuracy(signal_type)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn layered_context_builds() {
        let static_ctx = StaticContext::new(
            "HK".into(),
            vec!["0700.HK".into(), "0005.HK".into(), "9988.HK".into()],
            HashMap::new(),
            "2026-03-31".into(),
        );
        let live = LiveContext::new(42, "2026-03-31T10:00:00Z".into());

        let ctx = LayeredContext::from_static_and_live(static_ctx, live);

        assert_eq!(ctx.market(), "HK");
        assert_eq!(ctx.tick(), 42);
        assert_eq!(ctx.universe_size(), 3);
    }
}
