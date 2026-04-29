use std::collections::HashMap;

use rust_decimal::Decimal;

use crate::ontology::objects::Symbol;

pub type CounterpartMoves = HashMap<Symbol, Decimal>;

#[derive(Debug, Clone, Default)]
pub struct HkToUsBridgeData {
    pub signals: Vec<crate::bridges::hk_to_us::CrossMarketSignal>,
    pub hk_counterpart_moves: CounterpartMoves,
}

#[derive(Debug, Clone, Default)]
pub struct UsToHkBridgeData {
    pub signals: Vec<crate::bridges::us_to_hk::UsToHkSignal>,
    pub us_counterpart_moves: CounterpartMoves,
}
