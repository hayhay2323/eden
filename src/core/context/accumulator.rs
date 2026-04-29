use super::session_memory::SessionMemory;
use std::sync::{Arc, RwLock};

/// Thread-safe session memory accumulator.
/// Designed to be shared across the runtime and updated each tick.
pub struct MemoryAccumulator {
    memory: Arc<RwLock<SessionMemory>>,
}

impl MemoryAccumulator {
    pub fn new() -> Self {
        Self {
            memory: Arc::new(RwLock::new(SessionMemory::new())),
        }
    }

    /// Record a decision made at a given tick.
    pub fn record_decision(&self, tick: u64, symbol: String, action: String, confidence: f64) {
        if let Ok(mut mem) = self.memory.write() {
            mem.record_decision(tick, symbol, action, confidence);
        }
    }

    /// Record a hypothesis outcome (e.g. "confirmed", "rejected", "expired").
    pub fn record_hypothesis_outcome(&self, id: String, label: String, outcome: String) {
        if let Ok(mut mem) = self.memory.write() {
            mem.record_hypothesis_outcome(id, label, outcome);
        }
    }

    /// Record a signal outcome for hit-rate tracking.
    pub fn record_signal(&self, signal_type: String, was_hit: bool) {
        if let Ok(mut mem) = self.memory.write() {
            mem.record_signal_outcome(&signal_type, was_hit);
        }
    }

    /// Get a snapshot of the current memory state.
    pub fn snapshot(&self) -> SessionMemory {
        self.memory
            .read()
            .map(|m| m.clone())
            .unwrap_or_else(|_| SessionMemory::new())
    }

    /// Get the number of decisions recorded.
    pub fn decision_count(&self) -> usize {
        self.memory
            .read()
            .map(|m| m.decision_history.len())
            .unwrap_or(0)
    }

    /// Get signal accuracy for a given signal type.
    pub fn signal_accuracy(&self, signal_type: &str) -> Option<f64> {
        self.memory
            .read()
            .ok()
            .and_then(|m| m.signal_accuracy(signal_type))
    }

    /// Export memory as JSON for persistence.
    pub fn export_json(&self) -> Result<String, String> {
        let mem = self.snapshot();
        serde_json::to_string_pretty(&mem).map_err(|e| e.to_string())
    }

    /// Import memory from JSON.
    pub fn import_json(&self, json: &str) -> Result<(), String> {
        let mem: SessionMemory = serde_json::from_str(json).map_err(|e| e.to_string())?;
        if let Ok(mut current) = self.memory.write() {
            *current = mem;
            Ok(())
        } else {
            Err("Failed to acquire write lock".into())
        }
    }
}

impl Default for MemoryAccumulator {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for MemoryAccumulator {
    fn clone(&self) -> Self {
        Self {
            memory: Arc::clone(&self.memory),
        }
    }
}
