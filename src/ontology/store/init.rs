use std::sync::RwLock;

use super::catalog::{define_sectors, symbol_sector};
use super::knowledge::AccumulatedKnowledge;
use super::*;

pub(crate) fn build_object_store(
    institutions: HashMap<InstitutionId, Institution>,
    brokers: HashMap<BrokerId, Broker>,
    stocks: HashMap<Symbol, Stock>,
    sectors: HashMap<SectorId, Sector>,
    broker_to_institution: HashMap<BrokerId, InstitutionId>,
    knowledge: AccumulatedKnowledge,
) -> Arc<ObjectStore> {
    Arc::new(ObjectStore {
        institutions,
        brokers,
        stocks,
        sectors,
        broker_to_institution,
        knowledge: RwLock::new(knowledge),
    })
}

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

    build_object_store(
        institutions,
        brokers,
        stocks,
        sectors,
        broker_to_institution,
        AccumulatedKnowledge::empty(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_object_store_seeds_empty_knowledge() {
        let store = build_object_store(
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            AccumulatedKnowledge::empty(),
        );

        let knowledge = store.knowledge.read().unwrap();
        assert!(knowledge.institutional_memory.is_empty());
        assert!(knowledge.mechanism_priors.is_empty());
        assert!(knowledge.calibrated_weights.factor_adjustments.is_empty());
    }

    #[test]
    fn build_object_store_preserves_restored_knowledge() {
        let restored = AccumulatedKnowledge::restored_from_calibration(None);
        let store = build_object_store(
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            restored.clone(),
        );

        let knowledge = store.knowledge.read().unwrap();
        assert_eq!(
            knowledge.institutional_memory,
            restored.institutional_memory
        );
        assert_eq!(knowledge.mechanism_priors, restored.mechanism_priors);
        assert_eq!(
            knowledge.calibrated_weights.factor_adjustments,
            restored.calibrated_weights.factor_adjustments
        );
    }
}
