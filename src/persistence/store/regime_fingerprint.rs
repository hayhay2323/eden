//! EdenStore methods for regime_fingerprint_snapshot.
//!
//! Mirrors persistence/store/belief.rs — same write/latest/range
//! pattern. Naive cosine scan for find_similar (~390 rows/day → fine
//! without dedicated index in v1).

use crate::persistence::regime_fingerprint_snapshot::RegimeFingerprintSnapshot;

use super::super::store_helpers::{upsert_record_checked, StoreError};
use super::EdenStore;

impl EdenStore {
    /// Persist a regime fingerprint snapshot. Overwrites an existing
    /// row with the same record id (mirror of belief snapshot
    /// upsert semantics).
    pub async fn write_regime_fingerprint_snapshot(
        &self,
        snapshot: &RegimeFingerprintSnapshot,
    ) -> Result<(), StoreError> {
        let id = snapshot.record_id();
        upsert_record_checked(&self.db, "regime_fingerprint_snapshot", &id, snapshot).await
    }

    /// Load the most recent fingerprint for the given market, or None
    /// if no snapshot exists.
    pub async fn latest_regime_fingerprint_snapshot(
        &self,
        market: &str,
    ) -> Result<Option<RegimeFingerprintSnapshot>, StoreError> {
        let mut result = self
            .db
            .query(
                "SELECT * FROM regime_fingerprint_snapshot \
                 WHERE market = $market \
                 ORDER BY snapshot_ts DESC \
                 LIMIT 1",
            )
            .bind(("market", market.to_string()))
            .await?;

        let snaps: Vec<RegimeFingerprintSnapshot> = result.take(0)?;
        Ok(snaps.into_iter().next())
    }

    /// List all fingerprint snapshots for a market within a timestamp
    /// range, ordered ascending by snapshot_ts. Used by similarity
    /// queries + future episodic-memory tools.
    pub async fn regime_fingerprint_snapshots_in_range(
        &self,
        market: &str,
        from: &str,
        to: &str,
    ) -> Result<Vec<RegimeFingerprintSnapshot>, StoreError> {
        let mut result = self
            .db
            .query(
                "SELECT * FROM regime_fingerprint_snapshot \
                 WHERE market = $market \
                   AND snapshot_ts >= $from \
                   AND snapshot_ts <= $to \
                 ORDER BY snapshot_ts ASC",
            )
            .bind(("market", market.to_string()))
            .bind(("from", from.to_string()))
            .bind(("to", to.to_string()))
            .await?;

        let snaps: Vec<RegimeFingerprintSnapshot> = result.take(0)?;
        Ok(snaps)
    }

    /// Naive cosine scan for the `limit` most-similar past
    /// fingerprints to `target` within the given market. Uses the
    /// 5-dim universal vector. Caller can pass an optional date range
    /// to bound the scan; without it, the query reads the whole table
    /// (acceptable up to a few hundred thousand rows; v1 expects
    /// ~390/day).
    ///
    /// Returns `Vec<(snapshot, cosine_similarity)>` ordered descending
    /// by similarity. Identical match (cosine == 1.0) is included.
    pub async fn find_similar_regime_fingerprints(
        &self,
        market: &str,
        target: [f64; 5],
        limit: usize,
    ) -> Result<Vec<(RegimeFingerprintSnapshot, f64)>, StoreError> {
        let mut result = self
            .db
            .query(
                "SELECT * FROM regime_fingerprint_snapshot \
                 WHERE market = $market \
                 ORDER BY snapshot_ts DESC",
            )
            .bind(("market", market.to_string()))
            .await?;

        let snaps: Vec<RegimeFingerprintSnapshot> = result.take(0)?;
        let target_norm: f64 = target.iter().map(|v| v * v).sum::<f64>().sqrt();
        if target_norm == 0.0 {
            return Ok(vec![]);
        }

        let mut scored: Vec<(RegimeFingerprintSnapshot, f64)> = snaps
            .into_iter()
            .map(|snap| {
                let other = [
                    snap.stress,
                    snap.synchrony,
                    snap.bull_bias,
                    snap.activity,
                    snap.turn_pressure,
                ];
                let dot: f64 = target.iter().zip(other.iter()).map(|(a, b)| a * b).sum();
                let other_norm: f64 = other.iter().map(|v| v * v).sum::<f64>().sqrt();
                let cosine = if other_norm == 0.0 {
                    0.0
                } else {
                    dot / (target_norm * other_norm)
                };
                (snap, cosine)
            })
            .collect();

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(limit);
        Ok(scored)
    }
}
