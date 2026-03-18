use std::collections::HashMap;
use std::sync::Arc;

use longport::quote::QuoteContext;

use super::objects::*;

// ── Sector definitions for our watchlist ──

fn define_sectors() -> Vec<Sector> {
    vec![
        Sector { id: SectorId("tech".into()), name: "Technology".into() },
        Sector { id: SectorId("finance".into()), name: "Finance".into() },
        Sector { id: SectorId("energy".into()), name: "Energy".into() },
        Sector { id: SectorId("telecom".into()), name: "Telecommunications".into() },
        Sector { id: SectorId("property".into()), name: "Property".into() },
        Sector { id: SectorId("consumer".into()), name: "Consumer".into() },
        Sector { id: SectorId("healthcare".into()), name: "Healthcare".into() },
        Sector { id: SectorId("utilities".into()), name: "Utilities".into() },
        Sector { id: SectorId("insurance".into()), name: "Insurance".into() },
        Sector { id: SectorId("auto".into()), name: "Automobile".into() },
    ]
}

/// Hardcoded symbol → sector mapping for our ~20 stock watchlist.
/// Longport static_info has no sector field, so we define this manually.
fn symbol_sector(symbol: &str) -> Option<SectorId> {
    match symbol {
        // Tech
        "700.HK" | "9988.HK" | "3690.HK" | "9618.HK" | "1810.HK"
        | "9888.HK" | "268.HK" => Some(SectorId("tech".into())),
        // Finance
        "5.HK" | "388.HK" | "1398.HK" | "3988.HK" | "939.HK" => {
            Some(SectorId("finance".into()))
        }
        // Energy
        "883.HK" | "857.HK" | "386.HK" => Some(SectorId("energy".into())),
        // Telecom
        "941.HK" => Some(SectorId("telecom".into())),
        // Property
        "16.HK" | "1109.HK" => Some(SectorId("property".into())),
        // Insurance
        "2318.HK" | "1299.HK" => Some(SectorId("insurance".into())),
        // Auto
        "9868.HK" | "2015.HK" => Some(SectorId("auto".into())),
        // Healthcare
        "2269.HK" => Some(SectorId("healthcare".into())),
        _ => None,
    }
}

// ── ObjectStore ──

pub struct ObjectStore {
    pub institutions: HashMap<InstitutionId, Institution>,
    pub brokers: HashMap<BrokerId, Broker>,
    pub stocks: HashMap<Symbol, Stock>,
    pub sectors: HashMap<SectorId, Sector>,
    pub broker_to_institution: HashMap<BrokerId, InstitutionId>,
}

impl ObjectStore {
    pub fn institution_for_broker(&self, broker_id: &BrokerId) -> Option<&Institution> {
        self.broker_to_institution
            .get(broker_id)
            .and_then(|iid| self.institutions.get(iid))
    }

    pub fn brokers_for_institution(&self, institution_id: &InstitutionId) -> Vec<&Broker> {
        self.institutions
            .get(institution_id)
            .map(|inst| {
                inst.broker_ids
                    .iter()
                    .filter_map(|bid| self.brokers.get(bid))
                    .collect()
            })
            .unwrap_or_default()
    }

    pub fn stocks_in_sector(&self, sector_id: &SectorId) -> Vec<&Stock> {
        self.stocks
            .values()
            .filter(|s| s.sector_id.as_ref() == Some(sector_id))
            .collect()
    }
}

// ── Test helper ──

#[cfg(test)]
impl ObjectStore {
    /// Build an ObjectStore from raw data, no API needed.
    pub fn from_parts(
        institutions: Vec<Institution>,
        stocks: Vec<Stock>,
        sectors: Vec<Sector>,
    ) -> Self {
        let mut inst_map = HashMap::new();
        let mut broker_map = HashMap::new();
        let mut b2i = HashMap::new();

        for inst in institutions {
            for &bid in &inst.broker_ids {
                broker_map.insert(bid, Broker { id: bid, institution_id: inst.id });
                b2i.insert(bid, inst.id);
            }
            inst_map.insert(inst.id, inst);
        }

        let stock_map: HashMap<Symbol, Stock> = stocks
            .into_iter()
            .map(|s| (s.symbol.clone(), s))
            .collect();

        let sector_map: HashMap<SectorId, Sector> = sectors
            .into_iter()
            .map(|s| (s.id.clone(), s))
            .collect();

        ObjectStore {
            institutions: inst_map,
            brokers: broker_map,
            stocks: stock_map,
            sectors: sector_map,
            broker_to_institution: b2i,
        }
    }
}

// ── Initialization from Longport API ──

pub async fn initialize(
    ctx: &QuoteContext,
    watchlist: &[&str],
) -> Arc<ObjectStore> {
    // 1. Fetch all HKEX participants → build Institutions + Brokers
    let participants = ctx.participants().await.expect("failed to fetch participants");

    let mut institutions: HashMap<InstitutionId, Institution> = HashMap::new();
    let mut brokers: HashMap<BrokerId, Broker> = HashMap::new();
    let mut broker_to_institution: HashMap<BrokerId, InstitutionId> = HashMap::new();

    for p in &participants {
        let mut broker_ids: std::collections::HashSet<BrokerId> = std::collections::HashSet::new();
        for &raw_id in &p.broker_ids {
            broker_ids.insert(BrokerId(raw_id));
        }

        if broker_ids.is_empty() {
            continue;
        }

        // InstitutionId = min(broker_ids) — stable, deterministic
        let min_id = broker_ids.iter().map(|b| b.0).min().unwrap();
        let institution_id = InstitutionId(min_id);
        let class = InstitutionClass::classify_from_brokers(&broker_ids);

        let institution = Institution {
            id: institution_id,
            name_en: p.name_en.clone(),
            name_cn: p.name_cn.clone(),
            name_hk: p.name_hk.clone(),
            broker_ids: broker_ids.clone(),
            class,
        };

        institutions.insert(institution_id, institution);

        for bid in &broker_ids {
            brokers.insert(*bid, Broker {
                id: *bid,
                institution_id,
            });
            broker_to_institution.insert(*bid, institution_id);
        }
    }

    // 2. Sectors (hardcoded)
    let sectors: HashMap<SectorId, Sector> = define_sectors()
        .into_iter()
        .map(|s| (s.id.clone(), s))
        .collect();

    // 3. Fetch static info for watchlist → build Stocks
    let symbols: Vec<String> = watchlist.iter().map(|s| s.to_string()).collect();
    let static_infos = ctx
        .static_info(symbols)
        .await
        .expect("failed to fetch static_info");

    let mut stocks: HashMap<Symbol, Stock> = HashMap::new();
    for info in &static_infos {
        let sym = Symbol(info.symbol.clone());
        let sector_id = symbol_sector(&info.symbol);
        stocks.insert(
            sym.clone(),
            Stock {
                symbol: sym,
                name_en: info.name_en.clone(),
                name_cn: info.name_cn.clone(),
                name_hk: info.name_hk.clone(),
                exchange: info.exchange.clone(),
                lot_size: info.lot_size,
                sector_id,
                total_shares: info.total_shares,
                circulating_shares: info.circulating_shares,
                eps_ttm: info.eps_ttm,
                bps: info.bps,
                dividend_yield: info.dividend_yield,
            },
        );
    }

    Arc::new(ObjectStore {
        institutions,
        brokers,
        stocks,
        sectors,
        broker_to_institution,
    })
}

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
        Stock {
            symbol: Symbol(symbol.into()),
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
            Sector { id: SectorId("tech".into()), name: "Technology".into() },
            Sector { id: SectorId("finance".into()), name: "Finance".into() },
            Sector { id: SectorId("energy".into()), name: "Energy".into() },
        ];

        ObjectStore::from_parts(
            vec![barclays, stock_connect, morgan],
            stocks,
            sectors,
        )
    }

    // ── institution_for_broker ──

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

    // ── brokers_for_institution ──

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

    // ── stocks_in_sector ──

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

    // ── Institution ID = min(broker_ids) ──

    #[test]
    fn institution_id_is_min_broker() {
        let store = test_store();
        // Barclays has brokers [2040, 2041, 4497], so InstitutionId should be 2040
        let inst = store.institutions.get(&InstitutionId(2040)).unwrap();
        let min_broker = inst.broker_ids.iter().map(|b| b.0).min().unwrap();
        assert_eq!(inst.id.0, min_broker);
    }

    // ── Stock Connect classification ──

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

    // ── Broker → Institution consistency ──

    #[test]
    fn all_brokers_point_to_valid_institution() {
        let store = test_store();
        for (bid, iid) in &store.broker_to_institution {
            assert!(
                store.institutions.contains_key(iid),
                "Broker {} points to non-existent institution {}",
                bid, iid,
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
                    inst.id, bid,
                );
            }
        }
    }

    // ── symbol_sector mapping ──

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
    fn symbol_sector_unknown() {
        assert_eq!(symbol_sector("FAKE.HK"), None);
    }

    // ── define_sectors ──

    #[test]
    fn sectors_have_unique_ids() {
        let sectors = define_sectors();
        let ids: HashSet<_> = sectors.iter().map(|s| &s.id).collect();
        assert_eq!(ids.len(), sectors.len());
    }

    #[test]
    fn sectors_count() {
        assert_eq!(define_sectors().len(), 10);
    }
}
