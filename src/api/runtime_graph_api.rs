use std::io;

use axum::extract::Path;
use axum::Json;

use crate::api::foundation::ApiError;
use crate::core::market::{MarketId, MarketRegistry};
use crate::core::runtime_artifacts::{RuntimeArtifactKind, RuntimeArtifactStore};
use crate::pipeline::graph_query_backend::{EgoGraphResult, GraphQueryBackend, InfluenceResult};
use crate::pipeline::visual_graph_frame::VisualGraphFrame;

pub(super) async fn get_runtime_graph_frame(
    Path(market): Path<String>,
) -> Result<Json<VisualGraphFrame>, ApiError> {
    let market = parse_market(&market)?;
    let frame = load_latest_visual_frame(&RuntimeArtifactStore::default(), market)
        .map_err(|err| ApiError::internal(err.to_string()))?
        .ok_or_else(|| {
            ApiError::service_unavailable("visual graph frame artifact not available")
        })?;
    Ok(Json(frame))
}

pub(super) async fn get_runtime_graph_ego(
    Path((market, symbol)): Path<(String, String)>,
) -> Result<Json<EgoGraphResult>, ApiError> {
    let market = parse_market(&market)?;
    let frame = load_latest_visual_frame(&RuntimeArtifactStore::default(), market)
        .map_err(|err| ApiError::internal(err.to_string()))?
        .ok_or_else(|| {
            ApiError::service_unavailable("visual graph frame artifact not available")
        })?;
    let ego = query_ego_from_frame(&frame, &symbol).ok_or_else(|| {
        ApiError::not_found(format!("symbol not found in visual graph: {symbol}"))
    })?;
    Ok(Json(ego))
}

pub(super) async fn get_runtime_graph_influence(
    Path((market, symbol)): Path<(String, String)>,
) -> Result<Json<InfluenceResult>, ApiError> {
    let market = parse_market(&market)?;
    let frame = load_latest_visual_frame(&RuntimeArtifactStore::default(), market)
        .map_err(|err| ApiError::internal(err.to_string()))?
        .ok_or_else(|| {
            ApiError::service_unavailable("visual graph frame artifact not available")
        })?;
    let backend = GraphQueryBackend::new(&frame);
    Ok(Json(backend.influence(&symbol)))
}

fn parse_market(market: &str) -> Result<MarketId, ApiError> {
    MarketRegistry::by_slug(market)
        .ok_or_else(|| ApiError::bad_request(format!("unknown market: {market}")))
}

fn load_latest_visual_frame(
    store: &RuntimeArtifactStore,
    market: MarketId,
) -> io::Result<Option<VisualGraphFrame>> {
    store
        .read_latest_json_payload::<VisualGraphFrame>(RuntimeArtifactKind::VisualGraphFrame, market)
}

fn query_ego_from_frame(frame: &VisualGraphFrame, symbol: &str) -> Option<EgoGraphResult> {
    GraphQueryBackend::new(frame).ego(symbol)
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    use chrono::Utc;

    use crate::core::market::MarketId;
    use crate::core::runtime_artifacts::{RuntimeArtifactKind, RuntimeArtifactStore};
    use crate::pipeline::loopy_bp::{GraphEdge, NodePrior};
    use crate::pipeline::symbol_sub_kg::{NodeId, SubKgRegistry};
    use crate::pipeline::visual_graph_frame::build_visual_graph_frame;
    use rust_decimal_macros::dec;

    use super::*;

    #[test]
    fn latest_visual_frame_query_reads_enveloped_runtime_artifact() {
        let root = unique_temp_dir();
        let store = RuntimeArtifactStore::new(&root);
        store
            .append_json_line(
                RuntimeArtifactKind::VisualGraphFrame,
                MarketId::Us,
                &sample_frame(1, 0.65),
            )
            .expect("append old frame");
        store
            .append_json_line(
                RuntimeArtifactKind::VisualGraphFrame,
                MarketId::Us,
                &sample_frame(2, 0.82),
            )
            .expect("append latest frame");

        let frame = load_latest_visual_frame(&store, MarketId::Us)
            .expect("load latest frame")
            .expect("frame exists");
        let ego = query_ego_from_frame(&frame, "A.US").expect("ego result");

        assert_eq!(frame.tick, 2);
        assert_eq!(ego.posterior.p_bull, 0.82);

        fs::remove_dir_all(root).ok();
    }

    fn sample_frame(tick: u64, p_bull: f64) -> VisualGraphFrame {
        let now = Utc::now();
        let mut registry = SubKgRegistry::new();
        registry
            .upsert("A.US", now)
            .set_node_value(NodeId::PressureCapitalFlow, dec!(0.7), now);
        registry.upsert("B.US", now);

        let mut priors = HashMap::new();
        priors.insert(
            "A.US".to_string(),
            NodePrior {
                belief: [0.7, 0.2, 0.1],
                observed: true,
            },
        );

        let mut beliefs = HashMap::new();
        beliefs.insert("A.US".to_string(), [p_bull, 0.1, 0.08]);
        let edges = vec![GraphEdge {
            from: "A.US".to_string(),
            to: "B.US".to_string(),
            weight: 0.9,
            kind: crate::pipeline::loopy_bp::BpEdgeKind::StockToStock,
        }];

        build_visual_graph_frame("us", tick, &registry, &edges, &priors, &beliefs, now)
    }

    fn unique_temp_dir() -> std::path::PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock")
            .as_nanos();
        std::env::temp_dir().join(format!("eden-runtime-graph-api-{nanos}"))
    }
}
