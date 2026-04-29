use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A summary of a validated hypothesis.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HypothesisSummary {
    pub id: String,
    pub label: String,
    /// "confirmed", "rejected", or "expired".
    pub outcome: String,
}

/// A record of a decision made during the session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DecisionRecord {
    pub tick: u64,
    pub symbol: String,
    pub action: String,
    pub confidence: f64,
}

/// Memory that persists across ticks within a single trading session.
///
/// Tracks the outcomes of hypotheses, the decisions made, and the
/// per-signal-type accuracy so the system can adapt within a session.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionMemory {
    /// Hypotheses that reached a terminal state during this session.
    pub validated_hypotheses: Vec<HypothesisSummary>,
    /// Chronological log of decisions made.
    pub decision_history: Vec<DecisionRecord>,
    /// Per-signal-type (hits, total) counters.
    pub signal_hit_rate: HashMap<String, (u32, u32)>,
}

impl SessionMemory {
    /// Start with an empty session memory.
    pub fn new() -> Self {
        Self {
            validated_hypotheses: Vec::new(),
            decision_history: Vec::new(),
            signal_hit_rate: HashMap::new(),
        }
    }

    /// Record a decision taken during this session.
    pub fn record_decision(&mut self, tick: u64, symbol: String, action: String, confidence: f64) {
        self.decision_history.push(DecisionRecord {
            tick,
            symbol,
            action,
            confidence,
        });
    }

    /// Record the outcome of a hypothesis.
    pub fn record_hypothesis_outcome(&mut self, id: String, label: String, outcome: String) {
        self.validated_hypotheses
            .push(HypothesisSummary { id, label, outcome });
    }

    /// Record a signal hit or miss for the given signal type.
    pub fn record_signal_outcome(&mut self, signal_type: &str, hit: bool) {
        let entry = self
            .signal_hit_rate
            .entry(signal_type.to_string())
            .or_insert((0, 0));
        if hit {
            entry.0 += 1;
        }
        entry.1 += 1;
    }

    /// Accuracy for a given signal type, or `None` if no observations exist.
    pub fn signal_accuracy(&self, signal_type: &str) -> Option<f64> {
        self.signal_hit_rate
            .get(signal_type)
            .and_then(|&(hits, total)| {
                if total == 0 {
                    None
                } else {
                    Some(hits as f64 / total as f64)
                }
            })
    }

    /// Total number of decisions recorded.
    pub fn decision_count(&self) -> usize {
        self.decision_history.len()
    }
}

impl Default for SessionMemory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_decisions() {
        let mut mem = SessionMemory::new();
        mem.record_decision(1, "0700.HK".into(), "buy".into(), 0.8);
        assert_eq!(mem.decision_count(), 1);
    }

    #[test]
    fn signal_accuracy_tracking() {
        let mut mem = SessionMemory::new();
        mem.record_signal_outcome("momentum", true);
        mem.record_signal_outcome("momentum", true);
        mem.record_signal_outcome("momentum", false);
        let accuracy = mem.signal_accuracy("momentum").expect("should have data");
        assert!(
            (accuracy - 2.0 / 3.0).abs() < 1e-9,
            "Expected ~66.7%, got {:.4}%",
            accuracy * 100.0
        );
    }

    #[test]
    fn empty_signal_returns_none() {
        let mem = SessionMemory::new();
        assert!(mem.signal_accuracy("unknown_signal").is_none());
    }
}
