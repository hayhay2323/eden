use super::*;

pub(super) fn compute_broker_queues(raw: &RawSnapshot) -> Vec<BrokerQueueEntry> {
    let mut entries = Vec::new();

    for (symbol, sec_brokers) in &raw.brokers {
        for broker_group in &sec_brokers.ask_brokers {
            for &broker_id in &broker_group.broker_ids {
                entries.push(BrokerQueueEntry {
                    symbol: symbol.clone(),
                    broker_id: BrokerId(broker_id),
                    side: Side::Ask,
                    position: broker_group.position,
                });
            }
        }
        for broker_group in &sec_brokers.bid_brokers {
            for &broker_id in &broker_group.broker_ids {
                entries.push(BrokerQueueEntry {
                    symbol: symbol.clone(),
                    broker_id: BrokerId(broker_id),
                    side: Side::Bid,
                    position: broker_group.position,
                });
            }
        }
    }

    entries
}

pub(super) fn compute_institution_activities(
    broker_queues: &[BrokerQueueEntry],
    store: &ObjectStore,
) -> Vec<InstitutionActivity> {
    let mut map: HashMap<
        (Symbol, InstitutionId),
        (Vec<i32>, Vec<i32>, std::collections::HashSet<BrokerId>),
    > = HashMap::new();

    for entry in broker_queues {
        let institution_id = match store.broker_to_institution.get(&entry.broker_id) {
            Some(&iid) => iid,
            None => continue,
        };

        let key = (entry.symbol.clone(), institution_id);
        let record = map
            .entry(key)
            .or_insert_with(|| (Vec::new(), Vec::new(), std::collections::HashSet::new()));

        match entry.side {
            Side::Ask => record.0.push(entry.position),
            Side::Bid => record.1.push(entry.position),
        }
        record.2.insert(entry.broker_id);
    }

    map.into_iter()
        .map(
            |((symbol, institution_id), (ask_positions, bid_positions, broker_ids))| {
                InstitutionActivity {
                    symbol,
                    institution_id,
                    ask_positions,
                    bid_positions,
                    seat_count: broker_ids.len(),
                }
            },
        )
        .collect()
}

pub(super) fn compute_cross_stock_presences(
    activities: &[InstitutionActivity],
) -> Vec<CrossStockPresence> {
    let mut map: HashMap<InstitutionId, (Vec<Symbol>, Vec<Symbol>, Vec<Symbol>)> = HashMap::new();

    for act in activities {
        let entry = map
            .entry(act.institution_id)
            .or_insert_with(|| (Vec::new(), Vec::new(), Vec::new()));
        if !entry.0.contains(&act.symbol) {
            entry.0.push(act.symbol.clone());
        }
        if !act.ask_positions.is_empty() && !entry.1.contains(&act.symbol) {
            entry.1.push(act.symbol.clone());
        }
        if !act.bid_positions.is_empty() && !entry.2.contains(&act.symbol) {
            entry.2.push(act.symbol.clone());
        }
    }

    map.into_iter()
        .filter(|(_, (symbols, _, _))| symbols.len() >= 2)
        .map(
            |(institution_id, (symbols, ask_symbols, bid_symbols))| CrossStockPresence {
                institution_id,
                symbols,
                ask_symbols,
                bid_symbols,
            },
        )
        .collect()
}
