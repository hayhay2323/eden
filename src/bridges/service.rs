use time::OffsetDateTime;

use crate::bridges::hk_to_us::{
    compute_cross_market_signals, compute_hk_counterpart_moves, minutes_since_hk_close,
    read_hk_snapshot,
};
use crate::bridges::types::{HkToUsBridgeData, UsToHkBridgeData};
use crate::bridges::us_to_hk::{
    compute_us_counterpart_moves, compute_us_to_hk_signals, minutes_since_us_close,
    read_us_snapshot,
};
use crate::core::artifact_repository::resolve_artifact_path;
use crate::core::market::{ArtifactKind, MarketId};

#[derive(Debug, Default, Clone, Copy)]
pub struct FileSystemBridgeService;

impl FileSystemBridgeService {
    pub async fn load_hk_to_us(&self, now: OffsetDateTime) -> HkToUsBridgeData {
        let hk_path = resolve_artifact_path(MarketId::Hk, ArtifactKind::BridgeSnapshot);
        let hk_fallback_path = resolve_artifact_path(MarketId::Hk, ArtifactKind::LiveSnapshot);

        let snapshot = match read_hk_snapshot(&hk_path).await {
            Ok(snapshot) => Ok(snapshot),
            Err(_) => read_hk_snapshot(&hk_fallback_path).await,
        };

        match snapshot {
            Ok(snapshot) => {
                let minutes = minutes_since_hk_close(now);
                HkToUsBridgeData {
                    signals: compute_cross_market_signals(&snapshot, minutes),
                    hk_counterpart_moves: compute_hk_counterpart_moves(&snapshot),
                }
            }
            Err(_) => HkToUsBridgeData::default(),
        }
    }

    pub async fn load_us_to_hk(&self, now: OffsetDateTime) -> UsToHkBridgeData {
        let us_path = resolve_artifact_path(MarketId::Us, ArtifactKind::LiveSnapshot);

        match read_us_snapshot(&us_path).await {
            Ok(snapshot) => {
                let minutes = minutes_since_us_close(now);
                UsToHkBridgeData {
                    signals: compute_us_to_hk_signals(&snapshot, minutes),
                    us_counterpart_moves: compute_us_counterpart_moves(&snapshot),
                }
            }
            Err(_) => UsToHkBridgeData::default(),
        }
    }
}
