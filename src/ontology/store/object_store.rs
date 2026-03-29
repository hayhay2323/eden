use std::sync::RwLock;

use super::*;
use super::knowledge::AccumulatedKnowledge;

pub struct ObjectStore {
    pub institutions: HashMap<InstitutionId, Institution>,
    pub brokers: HashMap<BrokerId, Broker>,
    pub stocks: HashMap<Symbol, Stock>,
    pub sectors: HashMap<SectorId, Sector>,
    pub broker_to_institution: HashMap<BrokerId, InstitutionId>,
    pub knowledge: RwLock<AccumulatedKnowledge>,
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

    pub fn stocks_in_market(&self, market: Market) -> Vec<&Stock> {
        self.stocks
            .values()
            .filter(|stock| stock.market == market)
            .collect()
    }

    pub fn sector_name_for_symbol(&self, symbol: &Symbol) -> Option<&str> {
        let sector_id = self.stocks.get(symbol)?.sector_id.as_ref()?;
        self.sectors
            .get(sector_id)
            .map(|sector| sector.name.as_str())
    }
}

impl ObjectStore {
    /// Build an ObjectStore from raw collections.
    /// Used by tests and the replay binary.
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
                broker_map.insert(
                    bid,
                    Broker {
                        id: bid,
                        institution_id: inst.id,
                    },
                );
                b2i.insert(bid, inst.id);
            }
            inst_map.insert(inst.id, inst);
        }

        let stock_map: HashMap<Symbol, Stock> =
            stocks.into_iter().map(|s| (s.symbol.clone(), s)).collect();

        let sector_map: HashMap<SectorId, Sector> =
            sectors.into_iter().map(|s| (s.id.clone(), s)).collect();

        ObjectStore {
            institutions: inst_map,
            brokers: broker_map,
            stocks: stock_map,
            sectors: sector_map,
            broker_to_institution: b2i,
            knowledge: RwLock::new(AccumulatedKnowledge::empty()),
        }
    }
}
