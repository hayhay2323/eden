use serde::{Deserialize, Serialize};
use std::fmt;

/// Why the tick loop continued instead of completing normally.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TickTransition {
    /// Normal completion
    Completed,
    /// Data was stale/delayed, retrying with fresh data
    StaleDataRetry { symbol: String, attempt: u32 },
    /// Signal conflict detected, re-running reasoning with relaxed constraints
    ConflictEscalation { conflicting_signals: Vec<String> },
    /// Partial processing — some symbols processed, rest deferred
    PartialCompletion { processed: usize, deferred: usize },
    /// Recovery from error — skip failed symbol, continue rest
    ErrorRecovery {
        failed_symbol: String,
        error: String,
    },
    /// Budget exhausted — stop early, highest priority symbols already done
    BudgetExhausted { processed: usize, remaining: usize },
    /// Diminishing returns — recent ticks produced no new insights
    DiminishingReturns { consecutive_no_change: u32 },
}

impl fmt::Display for TickTransition {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Completed => write!(f, "Completed"),
            Self::StaleDataRetry { symbol, attempt } => {
                write!(f, "StaleDataRetry({symbol}, attempt {attempt})")
            }
            Self::ConflictEscalation {
                conflicting_signals,
            } => {
                write!(
                    f,
                    "ConflictEscalation({} signals)",
                    conflicting_signals.len()
                )
            }
            Self::PartialCompletion {
                processed,
                deferred,
            } => write!(
                f,
                "PartialCompletion({processed} done, {deferred} deferred)"
            ),
            Self::ErrorRecovery {
                failed_symbol,
                error,
            } => write!(f, "ErrorRecovery({failed_symbol}: {error})"),
            Self::BudgetExhausted {
                processed,
                remaining,
            } => write!(f, "BudgetExhausted({processed} done, {remaining} left)"),
            Self::DiminishingReturns {
                consecutive_no_change,
            } => write!(f, "DiminishingReturns({consecutive_no_change} idle ticks)"),
        }
    }
}

/// The result of processing a single tick.
#[derive(Debug, Clone)]
pub struct TickOutcome {
    pub tick: u64,
    pub transition: TickTransition,
    pub symbols_analyzed: usize,
    pub signals_fired: usize,
    pub hypotheses_updated: usize,
    pub duration_ms: u64,
}

/// A symbol scheduled for retry on a subsequent tick.
#[derive(Debug, Clone)]
pub struct PendingRetry {
    pub symbol: String,
    pub reason: String,
    pub attempts: u32,
    pub max_attempts: u32,
}

/// Tracks state across ticks for recovery decisions.
///
/// Runtimes (HK, US) create one instance and call [`record_outcome`] after each
/// tick.  The machine accumulates statistics that drive adaptive decisions such
/// as "skip deep analysis when nothing is changing" or "retry symbols that
/// previously failed".
pub struct TickStateMachine {
    /// Count of consecutive ticks with no new signals or hypothesis changes.
    consecutive_idle_ticks: u32,
    /// Rolling window of recent tick durations (ms) for budget estimation.
    recent_durations: Vec<u64>,
    /// Maximum size of the duration window.
    max_duration_window: usize,
    /// Symbols that failed in the previous tick (candidates for retry).
    #[allow(dead_code)]
    pending_retries: Vec<PendingRetry>,
    /// Total ticks processed since creation.
    total_ticks: u64,
    /// Recent transitions kept for debugging / introspection.
    recent_transitions: Vec<TickTransition>,
    /// Cap on how many transitions we keep.
    max_transition_history: usize,
}

impl TickStateMachine {
    pub fn new() -> Self {
        Self {
            consecutive_idle_ticks: 0,
            recent_durations: Vec::new(),
            max_duration_window: 20,
            pending_retries: Vec::new(),
            total_ticks: 0,
            recent_transitions: Vec::new(),
            max_transition_history: 50,
        }
    }

    // --- core lifecycle ---------------------------------------------------

    /// Record the outcome of a tick and update internal bookkeeping.
    pub fn record_outcome(&mut self, outcome: &TickOutcome) {
        self.total_ticks += 1;

        // Track durations (rolling window).
        self.recent_durations.push(outcome.duration_ms);
        if self.recent_durations.len() > self.max_duration_window {
            self.recent_durations.remove(0);
        }

        // Track idle ticks.
        if outcome.signals_fired == 0 && outcome.hypotheses_updated == 0 {
            self.consecutive_idle_ticks += 1;
        } else {
            self.consecutive_idle_ticks = 0;
        }

        // Track transitions.
        self.recent_transitions.push(outcome.transition.clone());
        if self.recent_transitions.len() > self.max_transition_history {
            self.recent_transitions.remove(0);
        }
    }

    // --- adaptive queries -------------------------------------------------

    /// Returns `true` when the recent tick history suggests that deep analysis
    /// is unlikely to produce new insights.
    ///
    /// Heuristic: if consecutive idle ticks exceed 75 % of the observation
    /// window (minimum 3 ticks observed), recommend skipping.  The 75 %
    /// threshold comes from the rolling window itself — not an arbitrary
    /// number — it means "three quarters of recent evidence says nothing is
    /// happening".
    pub fn should_skip_deep_analysis(&self) -> bool {
        let window = self.recent_durations.len().min(10);
        if window < 3 {
            return false;
        }
        let idle_ratio = self.consecutive_idle_ticks as f64 / window as f64;
        idle_ratio > 0.75
    }

    // --- accessors --------------------------------------------------------

    /// Average tick duration from the rolling window, or `None` if no ticks
    /// have been recorded yet.
    pub fn average_duration_ms(&self) -> Option<u64> {
        if self.recent_durations.is_empty() {
            return None;
        }
        let sum: u64 = self.recent_durations.iter().sum();
        Some(sum / self.recent_durations.len() as u64)
    }

    /// Number of consecutive ticks that produced zero signals and zero
    /// hypothesis updates.
    pub fn consecutive_idle(&self) -> u32 {
        self.consecutive_idle_ticks
    }

    /// Total ticks processed since creation.
    pub fn total_ticks(&self) -> u64 {
        self.total_ticks
    }

    /// Snapshot of recent transition history (most recent last).
    pub fn recent_transitions(&self) -> &[TickTransition] {
        &self.recent_transitions
    }
}

impl Default for TickStateMachine {
    fn default() -> Self {
        Self::new()
    }
}
