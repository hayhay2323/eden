use super::*;

pub(crate) async fn initialize_us_store(
    ctx: &QuoteContext,
    watchlist: &[Symbol],
) -> Arc<ObjectStore> {
    let symbols_vec: Vec<String> = watchlist.iter().map(|s| s.0.clone()).collect();
    let static_infos = match ctx.static_info(symbols_vec).await {
        Ok(infos) => infos,
        Err(e) => {
            eprintln!("Warning: static_info failed: {}", e);
            vec![]
        }
    };

    let stocks: Vec<Stock> = static_infos
        .into_iter()
        .map(|info| {
            let symbol = Symbol(info.symbol.clone());
            Stock {
                market: symbol.market(),
                symbol,
                name_en: info.name_en.clone(),
                name_cn: info.name_cn.clone(),
                name_hk: info.name_hk.clone(),
                exchange: info.exchange.clone(),
                lot_size: info.lot_size,
                sector_id: us_symbol_sector(&info.symbol).map(|s| SectorId(s.into())),
                total_shares: info.total_shares,
                circulating_shares: info.circulating_shares,
                eps_ttm: info.eps_ttm,
                bps: info.bps,
                dividend_yield: info.dividend_yield,
            }
        })
        .collect();

    let stock_map: HashMap<Symbol, Stock> =
        stocks.into_iter().map(|s| (s.symbol.clone(), s)).collect();

    let sectors: HashMap<SectorId, crate::ontology::objects::Sector> = us_sector_names()
        .iter()
        .map(|(id, name)| {
            (
                SectorId(id.to_string()),
                crate::ontology::objects::Sector {
                    id: SectorId(id.to_string()),
                    name: name.to_string(),
                },
            )
        })
        .collect();

    Arc::new(ObjectStore {
        institutions: HashMap::new(),
        brokers: HashMap::new(),
        stocks: stock_map,
        sectors,
        broker_to_institution: HashMap::new(),
        knowledge: std::sync::RwLock::new(crate::ontology::store::AccumulatedKnowledge::empty()),
    })
}

pub(crate) async fn fetch_us_rest_data(
    ctx: &QuoteContext,
    watchlist: &[Symbol],
) -> UsRestSnapshot {
    const BATCH_CONCURRENCY: usize = 8;

    let flow_future = stream::iter(watchlist.iter().cloned())
        .map(|sym| {
            let ctx = ctx.clone();
            async move {
                match ctx.capital_flow(sym.0.clone()).await {
                    Ok(f) => Some((sym, f)),
                    Err(e) => {
                        eprintln!("Warning: capital_flow({}) failed: {}", sym, e);
                        None
                    }
                }
            }
        })
        .buffer_unordered(BATCH_CONCURRENCY)
        .collect::<Vec<_>>();

    let calc_future = async {
        match ctx
            .calc_indexes(
                watchlist.iter().map(|s| s.0.clone()).collect::<Vec<_>>(),
                [
                    CalcIndex::TurnoverRate,
                    CalcIndex::VolumeRatio,
                    CalcIndex::PeTtmRatio,
                    CalcIndex::PbRatio,
                    CalcIndex::Amplitude,
                    CalcIndex::FiveMinutesChangeRate,
                    CalcIndex::DividendRatioTtm,
                ],
            )
            .await
        {
            Ok(indexes) => indexes
                .into_iter()
                .map(|idx| (Symbol(idx.symbol.clone()), idx))
                .collect(),
            Err(e) => {
                eprintln!("Warning: calc_indexes failed: {}", e);
                HashMap::new()
            }
        }
    };

    let quote_future = async {
        match ctx
            .quote(watchlist.iter().map(|s| s.0.clone()).collect::<Vec<_>>())
            .await
        {
            Ok(quotes) => quotes
                .into_iter()
                .map(|quote| (Symbol(quote.symbol.clone()), quote))
                .collect(),
            Err(e) => {
                eprintln!("Warning: US quote batch failed: {}", e);
                HashMap::new()
            }
        }
    };
    let (flow_results, calc_indexes, quotes) = tokio::join!(flow_future, calc_future, quote_future);

    UsRestSnapshot {
        quotes,
        calc_indexes,
        capital_flows: flow_results.into_iter().flatten().collect(),
    }
}
