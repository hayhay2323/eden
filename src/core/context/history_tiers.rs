use serde::{Deserialize, Serialize};
use std::collections::VecDeque;

/// A full-detail tick record (Tier 1 - most recent).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickRecord {
    pub tick: u64,
    pub timestamp: String,
    pub symbols_with_signals: Vec<String>,
    pub regime: Option<String>,
    pub stress: Option<f64>,
    pub decisions_made: Vec<String>,
    pub hypotheses_changed: Vec<String>,
}

/// A compressed summary of multiple ticks (Tier 2).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickBatchSummary {
    pub tick_range: (u64, u64),
    pub tick_count: u32,
    pub timestamp_range: (String, String),
    /// Most active symbols in this batch
    pub top_symbols: Vec<(String, u32)>,
    /// Regime during this batch (most frequent)
    pub dominant_regime: Option<String>,
    /// Stress statistics
    pub stress_mean: Option<f64>,
    pub stress_max: Option<f64>,
    /// Total decisions and hypotheses in batch
    pub total_decisions: u32,
    pub total_hypothesis_changes: u32,
}

/// A session-level summary (Tier 3 - oldest).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub session_date: String,
    pub total_ticks: u64,
    pub validated_hypotheses: Vec<ValidatedHypothesis>,
    pub key_decisions: Vec<KeyDecision>,
    pub regime_transitions: Vec<RegimeTransition>,
    pub signal_hit_rates: Vec<(String, f64)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatedHypothesis {
    pub id: String,
    pub label: String,
    pub outcome: String,
    pub confidence_at_close: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyDecision {
    pub tick: u64,
    pub symbol: String,
    pub action: String,
    pub outcome: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegimeTransition {
    pub tick: u64,
    pub from: String,
    pub to: String,
}

/// Three-tier history store with automatic compression.
///
/// Tier boundaries are derived from the data volume (tick rate) rather than
/// arbitrary constants. The caller supplies the observed tick rate and the
/// desired full-detail window; everything else scales from those inputs.
pub struct TieredHistory {
    /// Tier 1: Full detail, most recent ticks
    recent: VecDeque<TickRecord>,
    tier1_capacity: usize,

    /// Tier 2: Batch summaries of older ticks
    batches: VecDeque<TickBatchSummary>,
    tier2_capacity: usize,
    /// How many tier 1 records to compress into one batch
    batch_size: usize,

    /// Tier 3: Session-level summaries (oldest)
    sessions: Vec<SessionSummary>,
    tier3_capacity: usize,
}

impl TieredHistory {
    /// Create with capacities derived from expected tick rate.
    ///
    /// * `tick_rate` – expected ticks per minute
    /// * `tier1_minutes` – how many minutes of full detail to keep
    ///
    /// Tier 2 batch size equals 5 minutes of ticks (so each batch summarises
    /// a 5-minute window).  Tier 2 holds up to 50 batches (~4 hours of
    /// compressed history).  Tier 3 retains the 30 most recent session
    /// summaries.
    pub fn new(tick_rate: usize, tier1_minutes: usize) -> Self {
        let tier1_capacity = tick_rate * tier1_minutes;
        let batch_size = tick_rate * 5; // 5 minutes per batch
        let tier2_capacity = 50;
        let tier3_capacity = 30;

        Self {
            recent: VecDeque::with_capacity(tier1_capacity),
            tier1_capacity,
            batches: VecDeque::with_capacity(tier2_capacity),
            tier2_capacity,
            batch_size,
            sessions: Vec::new(),
            tier3_capacity,
        }
    }

    /// Push a new tick record. Triggers compression if tier 1 is full.
    pub fn push(&mut self, record: TickRecord) {
        self.recent.push_back(record);

        if self.recent.len() > self.tier1_capacity {
            self.compress_tier1_to_tier2();
        }
    }

    /// Compress oldest tier 1 records into a tier 2 batch.
    fn compress_tier1_to_tier2(&mut self) {
        let drain_count = self.batch_size.min(self.recent.len());
        if drain_count == 0 {
            return;
        }

        let drained: Vec<TickRecord> = self.recent.drain(..drain_count).collect();
        let batch = Self::summarize_batch(&drained);

        self.batches.push_back(batch);

        if self.batches.len() > self.tier2_capacity {
            self.compress_tier2_to_tier3();
        }
    }

    /// Compress oldest tier 2 batches into a tier 3 session summary.
    fn compress_tier2_to_tier3(&mut self) {
        // Take the oldest half of batches and merge into a session summary
        let drain_count = self.batches.len() / 2;
        if drain_count == 0 {
            return;
        }

        let drained: Vec<TickBatchSummary> = self.batches.drain(..drain_count).collect();
        let session = Self::summarize_session(&drained);

        self.sessions.push(session);

        if self.sessions.len() > self.tier3_capacity {
            self.sessions.remove(0); // Drop oldest session
        }
    }

    fn summarize_batch(records: &[TickRecord]) -> TickBatchSummary {
        if records.is_empty() {
            return TickBatchSummary {
                tick_range: (0, 0),
                tick_count: 0,
                timestamp_range: (String::new(), String::new()),
                top_symbols: vec![],
                dominant_regime: None,
                stress_mean: None,
                stress_max: None,
                total_decisions: 0,
                total_hypothesis_changes: 0,
            };
        }

        // Count symbol appearances
        let mut symbol_counts: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();
        let mut regime_counts: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();
        let mut stress_values: Vec<f64> = Vec::new();
        let mut total_decisions = 0u32;
        let mut total_hyp = 0u32;

        for r in records {
            for s in &r.symbols_with_signals {
                *symbol_counts.entry(s.clone()).or_default() += 1;
            }
            if let Some(ref regime) = r.regime {
                *regime_counts.entry(regime.clone()).or_default() += 1;
            }
            if let Some(stress) = r.stress {
                stress_values.push(stress);
            }
            total_decisions += r.decisions_made.len() as u32;
            total_hyp += r.hypotheses_changed.len() as u32;
        }

        let mut top_symbols: Vec<(String, u32)> = symbol_counts.into_iter().collect();
        top_symbols.sort_by(|a, b| b.1.cmp(&a.1));
        top_symbols.truncate(10);

        let dominant_regime = regime_counts
            .into_iter()
            .max_by_key(|(_, count)| *count)
            .map(|(regime, _)| regime);

        let stress_mean = if stress_values.is_empty() {
            None
        } else {
            Some(stress_values.iter().sum::<f64>() / stress_values.len() as f64)
        };
        let stress_max = stress_values
            .iter()
            .cloned()
            .fold(None, |max: Option<f64>, v| {
                Some(max.map_or(v, |m: f64| m.max(v)))
            });

        TickBatchSummary {
            tick_range: (records.first().unwrap().tick, records.last().unwrap().tick),
            tick_count: records.len() as u32,
            timestamp_range: (
                records.first().unwrap().timestamp.clone(),
                records.last().unwrap().timestamp.clone(),
            ),
            top_symbols,
            dominant_regime,
            stress_mean,
            stress_max,
            total_decisions,
            total_hypothesis_changes: total_hyp,
        }
    }

    fn summarize_session(batches: &[TickBatchSummary]) -> SessionSummary {
        let total_ticks: u64 = batches.iter().map(|b| b.tick_count as u64).sum();
        let session_date = batches
            .first()
            .map(|b| b.timestamp_range.0.clone())
            .unwrap_or_default();

        SessionSummary {
            session_date,
            total_ticks,
            validated_hypotheses: vec![],
            key_decisions: vec![],
            regime_transitions: vec![],
            signal_hit_rates: vec![],
        }
    }

    // --- Accessors ---

    /// Get recent full-detail records.
    pub fn recent_ticks(&self) -> &VecDeque<TickRecord> {
        &self.recent
    }

    /// Get batch summaries.
    pub fn batch_summaries(&self) -> &VecDeque<TickBatchSummary> {
        &self.batches
    }

    /// Get session summaries.
    pub fn session_summaries(&self) -> &[SessionSummary] {
        &self.sessions
    }

    /// Total records across all tiers.
    pub fn total_records(&self) -> usize {
        self.recent.len() + self.batches.len() + self.sessions.len()
    }

    /// Tier capacities for diagnostics.
    pub fn capacities(&self) -> (usize, usize, usize) {
        (
            self.tier1_capacity,
            self.tier2_capacity,
            self.tier3_capacity,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_record(tick: u64) -> TickRecord {
        TickRecord {
            tick,
            timestamp: format!("t{}", tick),
            symbols_with_signals: vec!["AAPL".into()],
            regime: Some("bullish".into()),
            stress: Some(0.5),
            decisions_made: vec![],
            hypotheses_changed: vec![],
        }
    }

    #[test]
    fn tier1_compression_triggers() {
        // Small capacity: 10 ticks, batch size 5
        let mut history = TieredHistory::new(5, 2); // 5 ticks/min * 2 min = 10 capacity
        for i in 0..15 {
            history.push(make_record(i));
        }
        // Should have compressed some to tier 2
        assert!(!history.batch_summaries().is_empty());
        assert!(history.recent_ticks().len() <= 10);
    }

    #[test]
    fn batch_summary_correct() {
        let records: Vec<TickRecord> = (0..5).map(make_record).collect();
        let batch = TieredHistory::summarize_batch(&records);
        assert_eq!(batch.tick_count, 5);
        assert_eq!(batch.tick_range, (0, 4));
        assert_eq!(batch.dominant_regime, Some("bullish".into()));
    }
}
