#[cfg(feature = "coordinator")]
use crate::cases::CaseMarket;
#[cfg(feature = "coordinator")]
use crate::core::coordinator::{CoordinatorEvent, CoordinatorSnapshot, MarketCoordinator};
#[cfg(feature = "coordinator")]
use axum::Json;
#[cfg(feature = "coordinator")]
use rust_decimal::prelude::ToPrimitive;
#[cfg(feature = "coordinator")]
use time::OffsetDateTime;

/// Returns the latest coordinator snapshot.
#[cfg(feature = "coordinator")]
pub(super) async fn get_coordinator_snapshot() -> Json<CoordinatorSnapshot> {
    let mut coordinator = MarketCoordinator::new();

    if let Ok(snapshot) = crate::cases::load_snapshot(CaseMarket::Hk).await {
        coordinator.update_hk(
            Some(snapshot.market_regime.bias.clone()),
            snapshot.stress.composite_stress.to_f64(),
        );
        coordinator.handle_event(CoordinatorEvent::HkUpdate {
            tick: snapshot.tick,
            timestamp: snapshot.timestamp.clone(),
        });
    }

    if let Ok(snapshot) = crate::cases::load_snapshot(CaseMarket::Us).await {
        coordinator.update_us(
            Some(snapshot.market_regime.bias.clone()),
            snapshot.stress.composite_stress.to_f64(),
        );
        coordinator.handle_event(CoordinatorEvent::UsUpdate {
            tick: snapshot.tick,
            timestamp: snapshot.timestamp.clone(),
        });
    }

    let snapshot = if coordinator.state().both_markets_active() {
        coordinator.analyze()
    } else {
        CoordinatorSnapshot {
            generated_at: String::new(),
            hk_tick: coordinator.state().hk_tick,
            us_tick: coordinator.state().us_tick,
            divergences: Vec::new(),
            cross_market_hypotheses: Vec::new(),
        }
    }
    .with_generated_at(OffsetDateTime::now_utc());

    Json(snapshot)
}
