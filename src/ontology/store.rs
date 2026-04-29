use std::collections::HashMap;
use std::sync::Arc;

use longport::quote::QuoteContext;

use super::objects::*;

#[path = "store/catalog.rs"]
mod catalog;
#[path = "store/init.rs"]
mod init;
#[path = "store/knowledge.rs"]
mod knowledge;
#[path = "store/object_store.rs"]
mod object_store;

pub use knowledge::{
    AccumulatedKnowledge, CalibratedWeights, InstitutionSymbolProfile, MechanismPrior,
};

pub use catalog::{
    canonical_sector_id, define_sectors, symbol_sector, us_sector_names, us_symbol_sector,
};
pub use init::initialize;
pub use object_store::ObjectStore;

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn make_institution(min_id: i32, ids: &[i32], name: &str) -> Institution {
        Institution {
            id: InstitutionId(min_id),
            name_en: name.into(),
            name_cn: String::new(),
            name_hk: String::new(),
            broker_ids: ids.iter().map(|&i| BrokerId(i)).collect(),
            class: InstitutionClass::classify_from_brokers(
                &ids.iter().map(|&i| BrokerId(i)).collect(),
            ),
        }
    }

    fn make_stock(symbol: &str, sector: Option<&str>) -> Stock {
        let symbol_id = Symbol(symbol.into());
        Stock {
            market: symbol_id.market(),
            symbol: symbol_id,
            name_en: symbol.into(),
            name_cn: String::new(),
            name_hk: String::new(),
            exchange: "SEHK".into(),
            lot_size: 100,
            sector_id: sector.map(|s| SectorId(s.into())),
            total_shares: 0,
            circulating_shares: 0,
            eps_ttm: rust_decimal::Decimal::ZERO,
            bps: rust_decimal::Decimal::ZERO,
            dividend_yield: rust_decimal::Decimal::ZERO,
        }
    }

    fn test_store() -> ObjectStore {
        let barclays = make_institution(2040, &[2040, 2041, 4497], "Barclays Asia");
        let stock_connect = make_institution(6996, &[6996, 6997], "Stock Connect SH");
        let morgan = make_institution(3000, &[3000, 3001], "Morgan Stanley");

        let stocks = vec![
            make_stock("700.HK", Some("tech")),
            make_stock("9988.HK", Some("tech")),
            make_stock("5.HK", Some("finance")),
            make_stock("883.HK", Some("energy")),
            make_stock("UNKNOWN.HK", None),
        ];

        let sectors = vec![
            Sector {
                id: SectorId("tech".into()),
                name: "Technology".into(),
            },
            Sector {
                id: SectorId("finance".into()),
                name: "Finance".into(),
            },
            Sector {
                id: SectorId("energy".into()),
                name: "Energy".into(),
            },
        ];

        ObjectStore::from_parts(vec![barclays, stock_connect, morgan], stocks, sectors)
    }

    #[test]
    fn lookup_broker_finds_institution() {
        let store = test_store();
        let inst = store.institution_for_broker(&BrokerId(4497)).unwrap();
        assert_eq!(inst.id, InstitutionId(2040));
        assert_eq!(inst.name_en, "Barclays Asia");
    }

    #[test]
    fn lookup_broker_min_id_also_works() {
        let store = test_store();
        let inst = store.institution_for_broker(&BrokerId(2040)).unwrap();
        assert_eq!(inst.id, InstitutionId(2040));
    }

    #[test]
    fn lookup_broker_not_found() {
        let store = test_store();
        assert!(store.institution_for_broker(&BrokerId(9999)).is_none());
    }

    #[test]
    fn brokers_for_barclays() {
        let store = test_store();
        let brokers = store.brokers_for_institution(&InstitutionId(2040));
        let mut ids: Vec<i32> = brokers.iter().map(|b| b.id.0).collect();
        ids.sort();
        assert_eq!(ids, vec![2040, 2041, 4497]);
    }

    #[test]
    fn brokers_for_nonexistent_institution() {
        let store = test_store();
        let brokers = store.brokers_for_institution(&InstitutionId(8888));
        assert!(brokers.is_empty());
    }

    #[test]
    fn tech_stocks() {
        let store = test_store();
        let stocks = store.stocks_in_sector(&SectorId("tech".into()));
        let mut syms: Vec<&str> = stocks.iter().map(|s| s.symbol.0.as_str()).collect();
        syms.sort();
        assert_eq!(syms, vec!["700.HK", "9988.HK"]);
    }

    #[test]
    fn energy_stocks() {
        let store = test_store();
        let stocks = store.stocks_in_sector(&SectorId("energy".into()));
        assert_eq!(stocks.len(), 1);
        assert_eq!(stocks[0].symbol.0, "883.HK");
    }

    #[test]
    fn empty_sector() {
        let store = test_store();
        let stocks = store.stocks_in_sector(&SectorId("consumer".into()));
        assert!(stocks.is_empty());
    }

    #[test]
    fn institution_id_is_min_broker() {
        let store = test_store();
        let inst = store.institutions.get(&InstitutionId(2040)).unwrap();
        let min_broker = inst.broker_ids.iter().map(|b| b.0).min().unwrap();
        assert_eq!(inst.id.0, min_broker);
    }

    #[test]
    fn stock_connect_classified_correctly() {
        let store = test_store();
        let sc = store.institutions.get(&InstitutionId(6996)).unwrap();
        assert_eq!(sc.class, InstitutionClass::StockConnectChannel);
    }

    #[test]
    fn regular_institution_is_unknown() {
        let store = test_store();
        let barclays = store.institutions.get(&InstitutionId(2040)).unwrap();
        assert_eq!(barclays.class, InstitutionClass::Unknown);
    }

    #[test]
    fn all_brokers_point_to_valid_institution() {
        let store = test_store();
        for (bid, iid) in &store.broker_to_institution {
            assert!(
                store.institutions.contains_key(iid),
                "Broker {} points to non-existent institution {}",
                bid,
                iid,
            );
        }
    }

    #[test]
    fn all_institution_brokers_exist_in_broker_map() {
        let store = test_store();
        for inst in store.institutions.values() {
            for bid in &inst.broker_ids {
                assert!(
                    store.brokers.contains_key(bid),
                    "Institution {} claims broker {} but it's not in broker map",
                    inst.id,
                    bid,
                );
            }
        }
    }

    #[test]
    fn symbol_sector_tech() {
        assert_eq!(symbol_sector("700.HK"), Some(SectorId("tech".into())));
        assert_eq!(symbol_sector("9988.HK"), Some(SectorId("tech".into())));
        assert_eq!(symbol_sector("268.HK"), Some(SectorId("tech".into())));
    }

    #[test]
    fn symbol_sector_finance() {
        assert_eq!(symbol_sector("5.HK"), Some(SectorId("finance".into())));
        assert_eq!(symbol_sector("388.HK"), Some(SectorId("finance".into())));
    }

    #[test]
    fn symbol_sector_energy() {
        assert_eq!(symbol_sector("883.HK"), Some(SectorId("energy".into())));
    }

    #[test]
    fn symbol_sector_cross_sector_cleanup() {
        assert_eq!(symbol_sector("1818.HK"), Some(SectorId("materials".into())));
        assert_eq!(symbol_sector("316.HK"), Some(SectorId("logistics".into())));
    }

    #[test]
    fn symbol_sector_unknown() {
        assert_eq!(symbol_sector("FAKE.HK"), None);
    }

    #[test]
    fn sectors_have_unique_ids() {
        let sectors = define_sectors();
        let ids: HashSet<_> = sectors.iter().map(|s| &s.id).collect();
        assert_eq!(ids.len(), sectors.len());
    }

    #[test]
    fn sectors_count() {
        assert_eq!(define_sectors().len(), 17);
    }

    #[test]
    fn object_store_has_empty_knowledge_by_default() {
        let store = test_store();
        let k = store.knowledge.read().unwrap();
        assert!(k.institutional_memory.is_empty());
    }
}
