use std::path::PathBuf;

use crate::core::market::{ArtifactKind, ArtifactSpec, MarketId, MarketRegistry};

pub fn artifact_spec(market: MarketId, kind: ArtifactKind) -> ArtifactSpec {
    MarketRegistry::artifact_spec(market, kind)
}

pub fn resolve_artifact_path(market: MarketId, kind: ArtifactKind) -> String {
    MarketRegistry::resolve_artifact_path(market, kind)
}

pub fn resolve_artifact_pathbuf(market: MarketId, kind: ArtifactKind) -> PathBuf {
    PathBuf::from(resolve_artifact_path(market, kind))
}
