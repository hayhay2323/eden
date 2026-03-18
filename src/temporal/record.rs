use std::collections::HashMap;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;
use time::serde::rfc3339;

use crate::ontology::links::TradeActivity;
use crate::ontology::objects::Symbol;

/// Compact snapshot of one pipeline tick's key signals.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TickRecord {
    pub tick_number: u64,
    #[serde(with = "rfc3339")]
    pub timestamp: OffsetDateTime,
    pub signals: HashMap<Symbol, SymbolSignals>,
}

/// Per-symbol signals captured at one tick.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymbolSignals {
    pub composite: Decimal,
    pub institutional_alignment: Decimal,
    pub sector_coherence: Option<Decimal>,
    pub cross_stock_correlation: Decimal,
    pub order_book_pressure: Decimal,
    pub capital_flow_direction: Decimal,
    pub capital_size_divergence: Decimal,
    pub institutional_direction: Decimal,
    pub depth_structure_imbalance: Decimal,
    pub bid_top3_ratio: Decimal,
    pub ask_top3_ratio: Decimal,
    pub bid_best_ratio: Decimal,
    pub ask_best_ratio: Decimal,
    pub spread: Option<Decimal>,
    pub trade_count: usize,
    pub trade_volume: i64,
    pub buy_volume: i64,
    pub sell_volume: i64,
    pub vwap: Option<Decimal>,
    pub composite_degradation: Option<Decimal>,
    pub institution_retention: Option<Decimal>,
}

impl TickRecord {
    pub fn capture(
        tick_number: u64,
        timestamp: OffsetDateTime,
        convergence: &HashMap<Symbol, crate::graph::decision::ConvergenceScore>,
        dimensions: &HashMap<Symbol, crate::pipeline::dimensions::SymbolDimensions>,
        order_books: &[crate::ontology::links::OrderBookObservation],
        trade_activities: &[TradeActivity],
        degradations: &HashMap<Symbol, crate::graph::decision::StructuralDegradation>,
    ) -> Self {
        let mut signals = HashMap::new();

        let ob_map: HashMap<&Symbol, &crate::ontology::links::OrderBookObservation> =
            order_books.iter().map(|ob| (&ob.symbol, ob)).collect();
        let ta_map: HashMap<&Symbol, &TradeActivity> =
            trade_activities.iter().map(|ta| (&ta.symbol, ta)).collect();

        for (symbol, conv) in convergence {
            let dims = dimensions.get(symbol);
            let ob = ob_map.get(symbol);
            let ta = ta_map.get(symbol);
            let deg = degradations.get(symbol);

            signals.insert(
                symbol.clone(),
                SymbolSignals {
                    composite: conv.composite,
                    institutional_alignment: conv.institutional_alignment,
                    sector_coherence: conv.sector_coherence,
                    cross_stock_correlation: conv.cross_stock_correlation,
                    order_book_pressure: dims.map(|d| d.order_book_pressure).unwrap_or(Decimal::ZERO),
                    capital_flow_direction: dims.map(|d| d.capital_flow_direction).unwrap_or(Decimal::ZERO),
                    capital_size_divergence: dims.map(|d| d.capital_size_divergence).unwrap_or(Decimal::ZERO),
                    institutional_direction: dims.map(|d| d.institutional_direction).unwrap_or(Decimal::ZERO),
                    depth_structure_imbalance: dims.map(|d| d.depth_structure_imbalance).unwrap_or(Decimal::ZERO),
                    bid_top3_ratio: ob.map(|o| o.bid_profile.top3_volume_ratio).unwrap_or(Decimal::ZERO),
                    ask_top3_ratio: ob.map(|o| o.ask_profile.top3_volume_ratio).unwrap_or(Decimal::ZERO),
                    bid_best_ratio: ob.map(|o| o.bid_profile.best_level_ratio).unwrap_or(Decimal::ZERO),
                    ask_best_ratio: ob.map(|o| o.ask_profile.best_level_ratio).unwrap_or(Decimal::ZERO),
                    spread: ob.and_then(|o| o.spread),
                    trade_count: ta.map(|t| t.trade_count).unwrap_or(0),
                    trade_volume: ta.map(|t| t.total_volume).unwrap_or(0),
                    buy_volume: ta.map(|t| t.buy_volume).unwrap_or(0),
                    sell_volume: ta.map(|t| t.sell_volume).unwrap_or(0),
                    vwap: ta.map(|t| t.vwap).filter(|v| *v != Decimal::ZERO),
                    composite_degradation: deg.map(|d| d.composite_degradation),
                    institution_retention: deg.map(|d| d.institution_retention),
                },
            );
        }

        TickRecord { tick_number, timestamp, signals }
    }
}
