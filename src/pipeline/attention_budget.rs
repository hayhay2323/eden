use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Attention level determines depth of analysis per symbol.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttentionLevel {
    /// Full deep analysis: all signals, reasoning, hypothesis tracking
    Deep,
    /// Standard analysis: signals + predicate evaluation
    Standard,
    /// Light scan: only check for threshold-breaking changes
    Scan,
    /// Skip entirely this tick (nothing changed)
    Skip,
}

/// Budget allocation for a single symbol.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolBudget {
    pub symbol: String,
    pub attention: AttentionLevel,
    pub priority_score: f64,
    pub reason: String,
}

/// Tracks per-symbol activity to inform budget decisions.
#[derive(Debug, Clone, Default)]
struct SymbolActivity {
    /// Ticks since last signal fired
    ticks_since_signal: u32,
    /// Ticks since last price move above noise threshold
    ticks_since_move: u32,
    /// Number of active hypotheses involving this symbol
    active_hypotheses: u32,
    /// Whether this symbol has an active recommendation
    has_recommendation: bool,
    /// Recent price change magnitude (absolute percentage)
    recent_change_pct: f64,
}

/// The attention budget allocator.
pub struct AttentionBudgetAllocator {
    activity: HashMap<String, SymbolActivity>,
    /// Total symbols budget can allocate "deep" to per tick
    deep_slots: usize,
    /// Total symbols budget can allocate "standard" to per tick
    standard_slots: usize,
}

impl AttentionBudgetAllocator {
    /// Create with slot counts derived from total symbol count.
    /// Deep: top ~5% of symbols, Standard: next ~20%, Scan: rest
    pub fn from_universe_size(total_symbols: usize) -> Self {
        let deep_slots = (total_symbols / 20).max(5); // ~5%, min 5
        let standard_slots = (total_symbols / 5).max(20); // ~20%, min 20
        Self {
            activity: HashMap::new(),
            deep_slots,
            standard_slots,
        }
    }

    /// Update activity tracking for a symbol after a tick.
    pub fn update_activity(
        &mut self,
        symbol: &str,
        signal_fired: bool,
        price_moved: bool,
        change_pct: f64,
        active_hypotheses: u32,
        has_recommendation: bool,
    ) {
        let entry = self.activity.entry(symbol.to_string()).or_default();

        if signal_fired {
            entry.ticks_since_signal = 0;
        } else {
            entry.ticks_since_signal += 1;
        }

        if price_moved {
            entry.ticks_since_move = 0;
        } else {
            entry.ticks_since_move += 1;
        }

        entry.active_hypotheses = active_hypotheses;
        entry.has_recommendation = has_recommendation;
        entry.recent_change_pct = change_pct;
    }

    /// Allocate attention budget for the next tick.
    /// Returns a sorted list with highest priority first.
    pub fn allocate(&self) -> Vec<SymbolBudget> {
        let mut scored: Vec<(String, f64, String)> = self
            .activity
            .iter()
            .map(|(symbol, act)| {
                let (score, reason) = Self::compute_priority(act);
                (symbol.clone(), score, reason)
            })
            .collect();

        // Sort by priority score descending
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        let mut budgets = Vec::with_capacity(scored.len());

        for (i, (symbol, score, reason)) in scored.into_iter().enumerate() {
            let attention = if i < self.deep_slots {
                AttentionLevel::Deep
            } else if i < self.deep_slots + self.standard_slots {
                AttentionLevel::Standard
            } else if score > 0.0 {
                AttentionLevel::Scan
            } else {
                AttentionLevel::Skip
            };

            budgets.push(SymbolBudget {
                symbol,
                attention,
                priority_score: score,
                reason,
            });
        }

        budgets
    }

    /// Compute priority score from activity metrics.
    /// Higher score = more attention needed.
    fn compute_priority(act: &SymbolActivity) -> (f64, String) {
        let mut score = 0.0;
        let mut reasons = Vec::new();

        // Recency of signal (exponential decay)
        if act.ticks_since_signal == 0 {
            score += 10.0;
            reasons.push("active signal");
        } else if act.ticks_since_signal < 5 {
            score += 5.0 / act.ticks_since_signal as f64;
            reasons.push("recent signal");
        }

        // Price movement magnitude
        let change_score = act.recent_change_pct.abs();
        score += change_score;
        if change_score > 2.0 {
            reasons.push("large move");
        }

        // Active hypotheses (each one needs tracking)
        score += act.active_hypotheses as f64 * 2.0;
        if act.active_hypotheses > 0 {
            reasons.push("active hypotheses");
        }

        // Has recommendation (needs monitoring)
        if act.has_recommendation {
            score += 3.0;
            reasons.push("has recommendation");
        }

        // Penalty for prolonged inactivity
        if act.ticks_since_signal > 20 && act.ticks_since_move > 20 {
            score *= 0.1; // Dramatically reduce if dormant
        }

        let reason = if reasons.is_empty() {
            "baseline".to_string()
        } else {
            reasons.join(", ")
        };

        (score, reason)
    }

    /// Get the current attention level for a specific symbol.
    pub fn attention_for(&self, symbol: &str) -> AttentionLevel {
        self.allocate()
            .iter()
            .find(|b| b.symbol == symbol)
            .map(|b| b.attention)
            .unwrap_or(AttentionLevel::Scan)
    }

    /// Summary statistics.
    pub fn summary(&self) -> BudgetSummary {
        let alloc = self.allocate();
        BudgetSummary {
            total: alloc.len(),
            deep: alloc
                .iter()
                .filter(|b| b.attention == AttentionLevel::Deep)
                .count(),
            standard: alloc
                .iter()
                .filter(|b| b.attention == AttentionLevel::Standard)
                .count(),
            scan: alloc
                .iter()
                .filter(|b| b.attention == AttentionLevel::Scan)
                .count(),
            skip: alloc
                .iter()
                .filter(|b| b.attention == AttentionLevel::Skip)
                .count(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BudgetSummary {
    pub total: usize,
    pub deep: usize,
    pub standard: usize,
    pub scan: usize,
    pub skip: usize,
}
