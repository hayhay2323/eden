//! Signature replay — historical pattern memory for forward outcomes.
//!
//! When a symbol's local sub-KG signature matches a signature we've seen
//! before, we can ask: "in past instances of this signature, what
//! happened to the belief over the next K ticks?" That's pattern memory
//! — the missing piece between eden's static perception and predictive
//! perception.
//!
//! This skeleton writes per-tick observations (signature_hash, symbol,
//! tick, belief) to .run/eden-signature-obs-{market}.ndjson and provides
//! a reader that, given the current signatures, looks up historical
//! occurrences and computes mean forward-belief-change at horizons +5,
//! +30, +100.
//!
//! Effective once 100+ ticks of history accumulate per signature. Early
//! session: read returns empty (no history yet). The skeleton is in
//! place so accumulation begins immediately.

use std::collections::HashMap;
use std::io::Write;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

use crate::pipeline::loopy_bp::{N_STATES, STATE_BEAR, STATE_BULL};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignatureObservation {
    pub ts: DateTime<Utc>,
    pub market: String,
    pub tick: u64,
    pub symbol: String,
    pub signature_hash: String,
    pub belief: [f64; N_STATES],
}

/// What happened the last time(s) this signature appeared, summarised.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SignatureReplay {
    pub symbol: String,
    pub signature_hash: String,
    pub historical_visits: u32,
    /// Mean (p_bull - p_bear) change at +5 ticks across historical instances.
    pub mean_forward_belief_5tick: f64,
    pub n_5tick: u32,
    /// Mean change at +30 ticks.
    pub mean_forward_belief_30tick: f64,
    pub n_30tick: u32,
}

/// Internal NDJSON record shape for wl-signatures stream.
#[derive(Debug, serde::Deserialize)]
struct RawWlSignature {
    symbol: String,
    signature_hash: String,
}

/// Convenience entrypoint for runtime wiring: reads the latest
/// per-symbol signatures from the wl-signatures NDJSON tail, joins them
/// with the supplied beliefs, writes observations for future replay,
/// and returns the current replay lookup. One-shot call from runtime.
pub fn observe_and_replay(
    market: &str,
    tick: u64,
    beliefs: &HashMap<String, [f64; N_STATES]>,
    max_replays: usize,
) -> Vec<SignatureReplay> {
    let current = read_latest_signatures(market);
    if current.is_empty() {
        return Vec::new();
    }
    // Write observations for future ticks to replay against
    let _ = write_signature_observations(market, tick, &current, beliefs);
    // Compute current replays from accumulated history
    read_signature_replays(market, &current, tick, max_replays)
}

/// Read-only snapshot helper for agent perception surfaces. Unlike
/// [`observe_and_replay`], this does not append observations; it only
/// joins the latest symbol signatures with accumulated history.
pub fn read_latest_signature_replays(
    market: &str,
    tick: u64,
    max_replays: usize,
) -> Vec<SignatureReplay> {
    let current = read_latest_signatures(market);
    if current.is_empty() {
        return Vec::new();
    }
    read_signature_replays(market, &current, tick, max_replays)
}

fn read_latest_signatures(market: &str) -> HashMap<String, String> {
    let wl_path = format!(".run/eden-wl-signatures-{}.ndjson", market);
    let wl_path = std::path::Path::new(&wl_path);
    // Tail-read recent wl-signatures (~latest tick worth)
    let raw: Vec<RawWlSignature> = crate::live_snapshot::tail_records(wl_path, 256 * 1024, 1000);
    // Use the most-recent signature per symbol (later overwrites earlier)
    raw.into_iter()
        .map(|sig| (sig.symbol, sig.signature_hash))
        .collect()
}

/// Write per-symbol signature observations so future ticks can query
/// "what did this signature do last time".
pub fn write_signature_observations(
    market: &str,
    tick: u64,
    signatures: &HashMap<String, String>, // symbol → signature_hash
    beliefs: &HashMap<String, [f64; N_STATES]>,
) -> std::io::Result<()> {
    let path = format!(".run/eden-signature-obs-{}.ndjson", market);
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let now = Utc::now();
    for (symbol, hash) in signatures {
        let belief = match beliefs.get(symbol) {
            Some(b) => *b,
            None => continue,
        };
        let obs = SignatureObservation {
            ts: now,
            market: market.to_string(),
            tick,
            symbol: symbol.clone(),
            signature_hash: hash.clone(),
            belief,
        };
        let line = serde_json::to_string(&obs)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        writeln!(file, "{}", line)?;
    }
    Ok(())
}

/// Look up signature replay for current symbols.
///
/// For each (symbol, signature) in `current`:
///   1. Find all PAST observations in the file with same signature_hash.
///   2. For each past tick T, find the same symbol's belief at T+5 and
///      T+30 (also from the file or bp-marginals).
///   3. Compute mean (p_bull - p_bear) change.
///
/// Skeleton implementation: tail-reads up to 50k recent observations.
/// Scales to ~100 ticks worth at 500 symbols. For longer history, swap
/// in indexed storage.
pub fn read_signature_replays(
    market: &str,
    current: &HashMap<String, String>, // symbol → signature_hash
    current_tick: u64,
    max_replays: usize,
) -> Vec<SignatureReplay> {
    let path = format!(".run/eden-signature-obs-{}.ndjson", market);
    let path = std::path::Path::new(&path);
    // Tail read: 4 MB ~ 50k records ~ 100 ticks * 500 symbols
    let raw: Vec<SignatureObservation> =
        crate::live_snapshot::tail_records(path, 4 * 1024 * 1024, 50000);

    // Index by (signature_hash, symbol) → list of (tick, belief)
    let mut by_sig_sym: HashMap<(String, String), Vec<(u64, [f64; N_STATES])>> = HashMap::new();
    for obs in raw {
        by_sig_sym
            .entry((obs.signature_hash.clone(), obs.symbol.clone()))
            .or_default()
            .push((obs.tick, obs.belief));
    }

    let mut out: Vec<SignatureReplay> = Vec::new();
    for (symbol, hash) in current {
        let key = (hash.clone(), symbol.clone());
        let history = match by_sig_sym.get(&key) {
            Some(h) if !h.is_empty() => h,
            _ => continue,
        };
        // Past instances: those with tick < current_tick - 5 (so we have
        // at least 5 ticks of forward outcome to read)
        let past_instances: Vec<&(u64, [f64; N_STATES])> = history
            .iter()
            .filter(|(t, _)| *t + 5 <= current_tick)
            .collect();
        if past_instances.is_empty() {
            continue;
        }

        // For each past instance, look up the same symbol's belief at
        // T+5 and T+30 (in the same flat history list, not index by sig)
        let symbol_history: Vec<(u64, [f64; N_STATES])> = by_sig_sym
            .iter()
            .filter(|((_, s), _)| s == symbol)
            .flat_map(|(_, v)| v.iter().copied())
            .collect();

        let mut deltas_5: Vec<f64> = Vec::new();
        let mut deltas_30: Vec<f64> = Vec::new();
        for (t_then, belief_then) in &past_instances {
            let then_signed = belief_then[STATE_BULL] - belief_then[STATE_BEAR];
            let plus5 = symbol_history
                .iter()
                .find(|(t, _)| *t >= *t_then + 5 && *t < *t_then + 7);
            let plus30 = symbol_history
                .iter()
                .find(|(t, _)| *t >= *t_then + 30 && *t < *t_then + 32);
            if let Some((_, b5)) = plus5 {
                deltas_5.push((b5[STATE_BULL] - b5[STATE_BEAR]) - then_signed);
            }
            if let Some((_, b30)) = plus30 {
                deltas_30.push((b30[STATE_BULL] - b30[STATE_BEAR]) - then_signed);
            }
        }

        if deltas_5.is_empty() && deltas_30.is_empty() {
            continue;
        }
        let n5 = deltas_5.len() as u32;
        let n30 = deltas_30.len() as u32;
        let m5 = if n5 > 0 {
            deltas_5.iter().sum::<f64>() / n5 as f64
        } else {
            0.0
        };
        let m30 = if n30 > 0 {
            deltas_30.iter().sum::<f64>() / n30 as f64
        } else {
            0.0
        };
        out.push(SignatureReplay {
            symbol: symbol.clone(),
            signature_hash: hash.clone(),
            historical_visits: past_instances.len() as u32,
            mean_forward_belief_5tick: m5,
            n_5tick: n5,
            mean_forward_belief_30tick: m30,
            n_30tick: n30,
        });
    }
    // Surface most-replay-confident first (highest visits × abs(mean delta))
    out.sort_by(|a, b| {
        let score_a = a.historical_visits as f64 * a.mean_forward_belief_5tick.abs();
        let score_b = b.historical_visits as f64 * b.mean_forward_belief_5tick.abs();
        score_b
            .partial_cmp(&score_a)
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    out.truncate(max_replays);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_returns_empty_when_no_history() {
        let current: HashMap<String, String> = [("X.US".to_string(), "abc".to_string())]
            .into_iter()
            .collect();
        let out = read_signature_replays("test_zzz_no_history", &current, 100, 10);
        assert!(out.is_empty());
    }

    #[test]
    fn replay_signature_struct_serde_roundtrip() {
        let original = SignatureReplay {
            symbol: "AAPL.US".to_string(),
            signature_hash: "deadbeef".to_string(),
            historical_visits: 3,
            mean_forward_belief_5tick: 0.05,
            n_5tick: 3,
            mean_forward_belief_30tick: 0.12,
            n_30tick: 2,
        };
        let json = serde_json::to_string(&original).expect("ser");
        let recovered: SignatureReplay = serde_json::from_str(&json).expect("de");
        assert_eq!(original, recovered);
    }
}
