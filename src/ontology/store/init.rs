use std::sync::RwLock;

use super::*;
use super::catalog::{define_sectors, symbol_sector};
use super::knowledge::AccumulatedKnowledge;

pub async fn initialize(ctx: &QuoteContext, watchlist: &[&str]) -> Arc<ObjectStore> {
    let participants = ctx
        .participants()
        .await
        .expect("failed to fetch participants");

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
            brokers.insert(
                *bid,
                Broker {
                    id: *bid,
                    institution_id,
                },
            );
            broker_to_institution.insert(*bid, institution_id);
        }
    }

    let sectors: HashMap<SectorId, Sector> = define_sectors()
        .into_iter()
        .map(|s| (s.id.clone(), s))
        .collect();

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
                market: sym.market(),
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
        knowledge: RwLock::new(AccumulatedKnowledge::empty()),
    })
}
