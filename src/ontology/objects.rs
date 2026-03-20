use std::collections::HashSet;
use std::fmt;

use serde::{Deserialize, Serialize};

// ── ID newtypes ──

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BrokerId(pub i32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct InstitutionId(pub i32);

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Symbol(pub String);

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SectorId(pub String);

impl fmt::Display for BrokerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "B{}", self.0)
    }
}

impl fmt::Display for InstitutionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "I{}", self.0)
    }
}

impl fmt::Display for Symbol {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl fmt::Display for SectorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

// ── Classification ──

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstitutionClass {
    InvestmentBank,
    RetailBroker,
    MarketMaker,
    StockConnectChannel,
    Unknown,
}

impl InstitutionClass {
    /// Only Stock Connect (broker IDs 6996-6999) can be programmatically identified.
    /// All others start as Unknown and are reclassified through behavioral observation.
    pub fn classify_from_brokers(broker_ids: &HashSet<BrokerId>) -> Self {
        let is_stock_connect = broker_ids.iter().any(|b| (6996..=6999).contains(&b.0));
        if is_stock_connect {
            Self::StockConnectChannel
        } else {
            Self::Unknown
        }
    }
}

// ── Objects ──

#[derive(Debug, Clone)]
pub struct Institution {
    pub id: InstitutionId,
    pub name_en: String,
    pub name_cn: String,
    pub name_hk: String,
    pub broker_ids: HashSet<BrokerId>,
    pub class: InstitutionClass,
}

#[derive(Debug, Clone)]
pub struct Broker {
    pub id: BrokerId,
    pub institution_id: InstitutionId,
}

#[derive(Debug, Clone)]
pub struct Stock {
    pub symbol: Symbol,
    pub name_en: String,
    pub name_cn: String,
    pub name_hk: String,
    pub exchange: String,
    pub lot_size: i32,
    pub sector_id: Option<SectorId>,
    // Fundamentals from static_info
    pub total_shares: i64,
    pub circulating_shares: i64,
    pub eps_ttm: rust_decimal::Decimal,
    pub bps: rust_decimal::Decimal,
    pub dividend_yield: rust_decimal::Decimal,
}

#[derive(Debug, Clone)]
pub struct Sector {
    pub id: SectorId,
    pub name: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Display ──

    #[test]
    fn broker_id_display() {
        assert_eq!(BrokerId(4497).to_string(), "B4497");
    }

    #[test]
    fn institution_id_display() {
        assert_eq!(InstitutionId(2040).to_string(), "I2040");
    }

    #[test]
    fn symbol_display() {
        assert_eq!(Symbol("700.HK".into()).to_string(), "700.HK");
    }

    #[test]
    fn sector_id_display() {
        assert_eq!(SectorId("tech".into()).to_string(), "tech");
    }

    // ── InstitutionClass ──

    #[test]
    fn classify_stock_connect_single() {
        let ids = HashSet::from([BrokerId(6996)]);
        assert_eq!(
            InstitutionClass::classify_from_brokers(&ids),
            InstitutionClass::StockConnectChannel,
        );
    }

    #[test]
    fn classify_stock_connect_mixed() {
        // 6997 is Stock Connect, 1000 is not — presence of any SC broker triggers it
        let ids = HashSet::from([BrokerId(1000), BrokerId(6997)]);
        assert_eq!(
            InstitutionClass::classify_from_brokers(&ids),
            InstitutionClass::StockConnectChannel,
        );
    }

    #[test]
    fn classify_stock_connect_all_four() {
        let ids = HashSet::from([
            BrokerId(6996),
            BrokerId(6997),
            BrokerId(6998),
            BrokerId(6999),
        ]);
        assert_eq!(
            InstitutionClass::classify_from_brokers(&ids),
            InstitutionClass::StockConnectChannel,
        );
    }

    #[test]
    fn classify_unknown_regular_broker() {
        let ids = HashSet::from([BrokerId(4497), BrokerId(2040)]);
        assert_eq!(
            InstitutionClass::classify_from_brokers(&ids),
            InstitutionClass::Unknown,
        );
    }

    #[test]
    fn classify_unknown_empty() {
        let ids = HashSet::new();
        assert_eq!(
            InstitutionClass::classify_from_brokers(&ids),
            InstitutionClass::Unknown,
        );
    }

    #[test]
    fn classify_boundary_not_stock_connect() {
        // 6995 and 7000 are outside the 6996-6999 range
        let ids = HashSet::from([BrokerId(6995), BrokerId(7000)]);
        assert_eq!(
            InstitutionClass::classify_from_brokers(&ids),
            InstitutionClass::Unknown,
        );
    }

    // ── Equality / Hash ──

    #[test]
    fn broker_id_equality() {
        assert_eq!(BrokerId(100), BrokerId(100));
        assert_ne!(BrokerId(100), BrokerId(101));
    }

    #[test]
    fn symbol_equality() {
        assert_eq!(Symbol("700.HK".into()), Symbol("700.HK".into()));
        assert_ne!(Symbol("700.HK".into()), Symbol("9988.HK".into()));
    }

    #[test]
    fn broker_id_hashset() {
        let mut set = HashSet::new();
        set.insert(BrokerId(100));
        set.insert(BrokerId(100)); // duplicate
        set.insert(BrokerId(200));
        assert_eq!(set.len(), 2);
    }
}
