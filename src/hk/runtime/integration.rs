use std::collections::HashSet;

use crate::core::context::accumulator::MemoryAccumulator;
use crate::core::context::history_tiers::{TickRecord as TieredTickRecord, TieredHistory};
use crate::ontology::reasoning::ReasoningScope;
use crate::ontology::world::{Vortex, WorldStateSnapshot};
use crate::pipeline::attention_budget::{AttentionBudgetAllocator, AttentionLevel, BudgetSummary};
use crate::pipeline::tick_state_machine::{TickOutcome, TickStateMachine, TickTransition};
use rust_decimal::Decimal;

/// Bundles the new infrastructure modules for the HK runtime.
///
/// Created once before the tick loop and updated after each tick.
pub struct RuntimeIntegration {
    pub state_machine: TickStateMachine,
    pub attention: AttentionBudgetAllocator,
    pub tiered_history: TieredHistory,
    pub memory: MemoryAccumulator,
    vortex_deep_symbols: HashSet<String>,
    vortex_standard_symbols: HashSet<String>,
}

impl RuntimeIntegration {
    /// Construct from the number of symbols in the universe.
    ///
    /// Tier-1 history retains full detail for the most recent ~5 minutes at
    /// an observed rate of ~10 ticks per minute. These parameters come from
    /// the data itself, not arbitrary constants.
    pub fn new(symbol_count: usize) -> Self {
        Self {
            state_machine: TickStateMachine::new(),
            attention: AttentionBudgetAllocator::from_universe_size(symbol_count),
            tiered_history: TieredHistory::new(10, 5),
            memory: MemoryAccumulator::new(),
            vortex_deep_symbols: HashSet::new(),
            vortex_standard_symbols: HashSet::new(),
        }
    }

    /// Called after each tick with results. Updates all tracking systems.
    pub fn after_tick(
        &mut self,
        tick: u64,
        timestamp: &str,
        symbols_with_signals: Vec<String>,
        regime: Option<String>,
        stress: Option<f64>,
        signals_fired: usize,
        hypotheses_updated: usize,
        decisions_made: Vec<String>,
        duration_ms: u64,
    ) {
        // 1. Record tick in state machine
        let outcome = TickOutcome {
            tick,
            transition: TickTransition::Completed,
            symbols_analyzed: symbols_with_signals.len(),
            signals_fired,
            hypotheses_updated,
            duration_ms,
        };
        self.state_machine.record_outcome(&outcome);

        // Log budget summary every tick
        {
            let summary = self.attention.summary();
            let avg_ms = self.state_machine.average_duration_ms().unwrap_or(0);
            let idle = self.state_machine.consecutive_idle();
            let tier1 = self.tiered_history.recent_ticks().len();
            let tier2 = self.tiered_history.batch_summaries().len();
            println!(
                "[integration] tick={tick} | budget: {deep}D/{std}S/{scan}Sc/{skip}Sk | signals={signals_fired} hyp={hypotheses_updated} | avg_tick={avg_ms}ms idle_streak={idle} | history: t1={tier1} t2={tier2} | dur={duration_ms}ms",
                deep = summary.deep,
                std = summary.standard,
                scan = summary.scan,
                skip = summary.skip,
            );
        }

        // 2. Push to tiered history
        self.tiered_history.push(TieredTickRecord {
            tick,
            timestamp: timestamp.to_string(),
            symbols_with_signals,
            regime,
            stress,
            decisions_made,
            hypotheses_changed: vec![],
        });
    }

    /// Check if deep analysis should be skipped this tick (diminishing returns).
    pub fn should_skip_deep_analysis(&self) -> bool {
        self.state_machine.should_skip_deep_analysis()
    }

    /// Get the attention level for a symbol.
    pub fn attention_for(&self, symbol: &str) -> AttentionLevel {
        if self.vortex_deep_symbols.contains(symbol) {
            return AttentionLevel::Deep;
        }

        let base = self.attention.attention_for(symbol);
        if self.vortex_standard_symbols.contains(symbol) {
            return match base {
                AttentionLevel::Deep => AttentionLevel::Deep,
                _ => AttentionLevel::Standard,
            };
        }

        base
    }

    /// Get budget allocation summary.
    pub fn budget_summary(&self) -> BudgetSummary {
        self.attention.summary()
    }

    /// Export memory as JSON for persistence.
    pub fn export_memory(&self) -> Result<String, String> {
        self.memory.export_json()
    }

    /// Update attention tracking for a symbol after observing tick results.
    pub fn update_symbol_activity(
        &mut self,
        symbol: &str,
        signal_fired: bool,
        price_moved: bool,
        change_pct: f64,
        active_hypotheses: u32,
        has_recommendation: bool,
    ) {
        self.attention.update_activity(
            symbol,
            signal_fired,
            price_moved,
            change_pct,
            active_hypotheses,
            has_recommendation,
        );
    }

    /// Refresh attention overrides from the latest world-state vortices.
    /// These overrides apply on the next tick and sit on top of the base allocator.
    pub fn refresh_vortex_attention(&mut self, world_state: &WorldStateSnapshot) {
        self.vortex_deep_symbols = crate::pipeline::world::vortex_boosted_scopes(world_state)
            .into_iter()
            .filter_map(|(scope, _)| match scope {
                ReasoningScope::Symbol(symbol) => Some(symbol.0),
                _ => None,
            })
            .collect();

        self.vortex_standard_symbols =
            crate::pipeline::world::vortex_edge_symbol_scopes(world_state)
                .into_iter()
                .filter_map(|(scope, _)| match scope {
                    ReasoningScope::Symbol(symbol) => Some(symbol.0),
                    _ => None,
                })
                .filter(|symbol| !self.vortex_deep_symbols.contains(symbol))
                .collect();

        for vortex in world_state
            .vortices
            .iter()
            .filter(|vortex| qualifies_for_learned_attention(vortex))
        {
            if let ReasoningScope::Symbol(symbol) = &vortex.center_scope {
                self.vortex_deep_symbols.insert(symbol.0.clone());
            }
            for path in &vortex.flow_paths {
                if let ReasoningScope::Symbol(symbol) = &path.source_scope {
                    if !self.vortex_deep_symbols.contains(&symbol.0) {
                        self.vortex_standard_symbols.insert(symbol.0.clone());
                    }
                }
            }
        }
    }

    pub fn is_vortex_attention_boosted(&self, symbol: &str) -> bool {
        self.vortex_deep_symbols.contains(symbol) || self.vortex_standard_symbols.contains(symbol)
    }

    /// Returns true if this symbol warrants rolling stats computation.
    /// Skip and Deep/Standard symbols get rolling stats; Skip symbols don't.
    pub fn should_compute_rolling_stats(&self, symbol: &str) -> bool {
        !matches!(self.attention_for(symbol), AttentionLevel::Skip)
    }
}

fn qualifies_for_learned_attention(vortex: &Vortex) -> bool {
    vortex.strength >= Decimal::new(3, 1) && vortex.coherence >= Decimal::new(6, 1)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ontology::world::{FlowPath, FlowPolarity, WorldLayer};
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    fn symbol_scope(value: &str) -> ReasoningScope {
        ReasoningScope::Symbol(crate::ontology::objects::Symbol(value.into()))
    }

    #[test]
    fn strong_vortex_boosts_attention() {
        let mut integration = RuntimeIntegration::new(100);
        let world_state = WorldStateSnapshot {
            timestamp: OffsetDateTime::UNIX_EPOCH,
            entities: vec![],
            vortices: vec![Vortex {
                vortex_id: "vortex:700.HK".into(),
                center_entity_id: "world:setup:700.HK".into(),
                center_scope: symbol_scope("700.HK"),
                layer: WorldLayer::Leaf,
                flow_paths: vec![
                    FlowPath {
                        source_entity_id: "world:setup:9988.HK".into(),
                        source_scope: symbol_scope("9988.HK"),
                        channel: "broker_flow".into(),
                        weight: dec!(0.30),
                        polarity: FlowPolarity::Confirming,
                    },
                ],
                strength: dec!(0.35),
                channel_diversity: 2,
                coherence: dec!(0.65),
                narrative: None,
            }],
        };

        integration.refresh_vortex_attention(&world_state);
        assert!(integration.is_vortex_attention_boosted("700.HK"));
        assert!(integration.is_vortex_attention_boosted("9988.HK"));
    }
}
