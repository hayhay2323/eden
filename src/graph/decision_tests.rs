use super::*;
use crate::action::narrative::{
    DimensionReading, Direction, NarrativeSnapshot, Regime, SymbolNarrative,
};
use crate::external::polymarket::{PolymarketBias, PolymarketPrior, PolymarketSnapshot};
use crate::graph::graph::BrainGraph;
use crate::pipeline::tension::Dimension;
use crate::ontology::links::*;
use crate::ontology::objects::*;
use crate::ontology::{ActionNode, ActionNodeStage};
use crate::pipeline::dimensions::SymbolDimensions;
use crate::ReasoningScope;
use rust_decimal_macros::dec;

fn sym(s: &str) -> Symbol {
    Symbol(s.into())
}

fn make_store_with_stocks(stocks: Vec<Stock>) -> ObjectStore {
    let mut stock_map = HashMap::new();
    for s in stocks {
        stock_map.insert(s.symbol.clone(), s);
    }
    ObjectStore {
        institutions: HashMap::new(),
        brokers: HashMap::new(),
        stocks: stock_map,
        sectors: HashMap::new(),
        broker_to_institution: HashMap::new(),
        knowledge: std::sync::RwLock::new(crate::ontology::store::AccumulatedKnowledge::empty()),
    }
}

fn make_stock(symbol: &str, lot_size: i32) -> Stock {
    let symbol_id = sym(symbol);
    Stock {
        market: symbol_id.market(),
        symbol: symbol_id,
        name_en: symbol.into(),
        name_cn: String::new(),
        name_hk: String::new(),
        exchange: "SEHK".into(),
        lot_size,
        sector_id: None,
        total_shares: 0,
        circulating_shares: 0,
        eps_ttm: rust_decimal::Decimal::ZERO,
        bps: rust_decimal::Decimal::ZERO,
        dividend_yield: rust_decimal::Decimal::ZERO,
    }
}

fn make_narrative(coherence: Decimal, mean_direction: Decimal) -> SymbolNarrative {
    SymbolNarrative {
        regime: Regime::classify(coherence, mean_direction),
        coherence,
        mean_direction,
        readings: vec![DimensionReading {
            dimension: Dimension::OrderBookPressure,
            value: mean_direction,
            direction: Direction::from_value(mean_direction),
        }],
        agreements: vec![],
        contradictions: vec![],
    }
}

fn make_dims(obp: Decimal, cfd: Decimal, csd: Decimal, id: Decimal) -> SymbolDimensions {
    SymbolDimensions {
        order_book_pressure: obp,
        capital_flow_direction: cfd,
        capital_size_divergence: csd,
        institutional_direction: id,
        ..Default::default()
    }
}

fn empty_links() -> LinkSnapshot {
    LinkSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        broker_queues: vec![],
        calc_indexes: vec![],
        candlesticks: vec![],
        institution_activities: vec![],
        cross_stock_presences: vec![],
        capital_flows: vec![],
        capital_flow_series: vec![],
        capital_breakdowns: vec![],
        market_temperature: None,
        order_books: vec![],
        quotes: vec![],
        trade_activities: vec![],
        intraday: vec![],
    }
}

fn build_brain(
    narratives: HashMap<Symbol, SymbolNarrative>,
    dimensions: HashMap<Symbol, SymbolDimensions>,
    links: &LinkSnapshot,
    store: &ObjectStore,
) -> BrainGraph {
    let narrative = NarrativeSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        narratives,
    };
    let dims = crate::pipeline::dimensions::DimensionSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        dimensions,
    };
    BrainGraph::compute(&narrative, &dims, links, store)
}

// ── Convergence Tests ──

#[test]
fn all_bullish_convergence() {
    let mut narratives = HashMap::new();
    narratives.insert(sym("700.HK"), make_narrative(dec!(0.8), dec!(0.6)));
    narratives.insert(sym("9988.HK"), make_narrative(dec!(0.7), dec!(0.5)));
    narratives.insert(sym("5.HK"), make_narrative(dec!(0.3), dec!(-0.2)));

    let mut dimensions = HashMap::new();
    dimensions.insert(
        sym("700.HK"),
        make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
    );
    dimensions.insert(
        sym("9988.HK"),
        make_dims(dec!(0.4), dec!(0.4), dec!(0.4), dec!(0.4)),
    );
    dimensions.insert(
        sym("5.HK"),
        make_dims(dec!(0.5), dec!(-0.5), dec!(0.5), dec!(-0.5)),
    );

    let mut links = empty_links();
    links.cross_stock_presences.push(CrossStockPresence {
        institution_id: InstitutionId(100),
        symbols: vec![sym("700.HK"), sym("9988.HK")],
        ask_symbols: vec![],
        bid_symbols: vec![sym("700.HK"), sym("9988.HK")],
    });
    links.institution_activities.push(InstitutionActivity {
        symbol: sym("700.HK"),
        institution_id: InstitutionId(100),
        ask_positions: vec![],
        bid_positions: vec![1, 2, 3],
        seat_count: 3,
    });

    let store = make_store_with_stocks(vec![]);
    let brain = build_brain(narratives, dimensions, &links, &store);

    let score = ConvergenceScore::compute(&sym("700.HK"), &brain, None, None).unwrap();
    // All bullish → positive composite
    assert!(score.composite > Decimal::ZERO);
    assert!(score.institutional_alignment > Decimal::ZERO);
    assert!(score.cross_stock_correlation > Decimal::ZERO);
}

#[test]
fn conflicted_signals() {
    let mut narratives = HashMap::new();
    narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
    narratives.insert(sym("9988.HK"), make_narrative(dec!(0.3), dec!(-0.5)));

    let mut dimensions = HashMap::new();
    dimensions.insert(
        sym("700.HK"),
        make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
    );
    dimensions.insert(
        sym("9988.HK"),
        make_dims(dec!(-0.4), dec!(-0.4), dec!(-0.4), dec!(-0.4)),
    );

    let mut links = empty_links();
    links.cross_stock_presences.push(CrossStockPresence {
        institution_id: InstitutionId(100),
        symbols: vec![sym("700.HK"), sym("9988.HK")],
        ask_symbols: vec![sym("700.HK")],
        bid_symbols: vec![],
    });
    links.institution_activities.push(InstitutionActivity {
        symbol: sym("700.HK"),
        institution_id: InstitutionId(100),
        ask_positions: vec![1, 2],
        bid_positions: vec![],
        seat_count: 2,
    });

    let store = make_store_with_stocks(vec![]);
    let brain = build_brain(narratives, dimensions, &links, &store);

    let score = ConvergenceScore::compute(&sym("700.HK"), &brain, None, None).unwrap();
    // Institution selling, correlated stock bearish → negative composite
    assert!(score.composite < Decimal::ZERO);
}

#[test]
fn no_institutions_convergence() {
    let mut narratives = HashMap::new();
    narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));

    let mut dimensions = HashMap::new();
    dimensions.insert(
        sym("700.HK"),
        make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
    );

    let links = empty_links();
    let store = make_store_with_stocks(vec![]);
    let brain = build_brain(narratives, dimensions, &links, &store);

    let score = ConvergenceScore::compute(&sym("700.HK"), &brain, None, None).unwrap();
    assert_eq!(score.institutional_alignment, Decimal::ZERO);
    // No neighbors either, so composite = 0
    assert_eq!(score.composite, Decimal::ZERO);
}

// ── Fingerprint + Degradation Tests ──

#[test]
fn fingerprint_no_degradation() {
    let mut narratives = HashMap::new();
    narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));

    let mut dimensions = HashMap::new();
    dimensions.insert(
        sym("700.HK"),
        make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
    );

    let links = empty_links();
    let store = make_store_with_stocks(vec![]);
    let brain = build_brain(narratives, dimensions, &links, &store);

    let fp = StructuralFingerprint::capture(&sym("700.HK"), &brain, 7, Some(dec!(320))).unwrap();
    // Same brain → no degradation
    let deg = StructuralDegradation::compute(&fp, &brain);
    // dimension_drift should be ~0 (same dims)
    assert!(deg.dimension_drift.abs() < dec!(0.001));
    assert_eq!(deg.institution_retention, Decimal::ONE); // no institutions → 1
    assert_eq!(deg.correlation_retention, Decimal::ONE); // no correlations → 1
}

#[test]
fn full_degradation() {
    // Build entry brain
    let mut narratives = HashMap::new();
    narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
    narratives.insert(sym("9988.HK"), make_narrative(dec!(0.3), dec!(0.1)));

    let mut dimensions = HashMap::new();
    dimensions.insert(
        sym("700.HK"),
        make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
    );
    dimensions.insert(
        sym("9988.HK"),
        make_dims(dec!(0.4), dec!(0.4), dec!(0.4), dec!(0.4)),
    );

    let mut links = empty_links();
    links.cross_stock_presences.push(CrossStockPresence {
        institution_id: InstitutionId(100),
        symbols: vec![sym("700.HK"), sym("9988.HK")],
        ask_symbols: vec![],
        bid_symbols: vec![sym("700.HK"), sym("9988.HK")],
    });
    links.institution_activities.push(InstitutionActivity {
        symbol: sym("700.HK"),
        institution_id: InstitutionId(100),
        ask_positions: vec![],
        bid_positions: vec![1],
        seat_count: 1,
    });

    let store = make_store_with_stocks(vec![]);
    let entry_brain = build_brain(narratives.clone(), dimensions.clone(), &links, &store);
    let mut fp =
        StructuralFingerprint::capture(&sym("700.HK"), &entry_brain, 9, Some(dec!(320))).unwrap();
    fp.entry_composite = dec!(0.5);

    // Build degraded brain — institution gone, dimensions flipped
    let mut narratives2 = HashMap::new();
    narratives2.insert(sym("700.HK"), make_narrative(dec!(-0.3), dec!(-0.5)));

    let mut dimensions2 = HashMap::new();
    dimensions2.insert(
        sym("700.HK"),
        make_dims(dec!(-0.5), dec!(-0.5), dec!(-0.5), dec!(-0.5)),
    );

    let empty = empty_links();
    let degraded_brain = build_brain(narratives2, dimensions2, &empty, &store);

    let deg = StructuralDegradation::compute(&fp, &degraded_brain);
    // Institution gone → retention = 0
    assert_eq!(deg.institution_retention, Decimal::ZERO);
    // Dimensions flipped → drift should be ~2
    assert!(deg.dimension_drift > dec!(1.5));
    // Overall high degradation
    assert!(deg.composite_degradation > dec!(0.5));
}

#[test]
fn degradation_skips_missing_sector_component_in_average() {
    let mut entry_narratives = HashMap::new();
    entry_narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
    let mut entry_dimensions = HashMap::new();
    entry_dimensions.insert(
        sym("700.HK"),
        make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
    );

    let links = empty_links();
    let store = make_store_with_stocks(vec![]);
    let entry_brain = build_brain(entry_narratives, entry_dimensions, &links, &store);
    let fp =
        StructuralFingerprint::capture(&sym("700.HK"), &entry_brain, 10, Some(dec!(320))).unwrap();
    assert_eq!(fp.sector_mean_coherence, None);

    let mut degraded_narratives = HashMap::new();
    degraded_narratives.insert(sym("700.HK"), make_narrative(dec!(-0.3), dec!(-0.5)));
    let mut degraded_dimensions = HashMap::new();
    degraded_dimensions.insert(
        sym("700.HK"),
        make_dims(dec!(-0.5), dec!(-0.5), dec!(-0.5), dec!(-0.5)),
    );
    let degraded_brain = build_brain(degraded_narratives, degraded_dimensions, &links, &store);

    let deg = StructuralDegradation::compute(&fp, &degraded_brain);
    assert_eq!(deg.institution_retention, Decimal::ONE);
    assert_eq!(deg.correlation_retention, Decimal::ONE);
    assert_eq!(deg.sector_coherence_change, Decimal::ZERO);
    assert_eq!(deg.composite_degradation, deg.dimension_drift / dec!(3));
}

// ── Order Suggestion Tests ──

#[test]
fn order_direction_from_composite() {
    let mut narratives = HashMap::new();
    narratives.insert(sym("700.HK"), make_narrative(dec!(0.8), dec!(0.6)));
    narratives.insert(sym("9988.HK"), make_narrative(dec!(0.7), dec!(0.5)));

    let mut dimensions = HashMap::new();
    dimensions.insert(
        sym("700.HK"),
        make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
    );
    dimensions.insert(
        sym("9988.HK"),
        make_dims(dec!(0.4), dec!(0.4), dec!(0.4), dec!(0.4)),
    );

    let mut links = empty_links();
    links.cross_stock_presences.push(CrossStockPresence {
        institution_id: InstitutionId(100),
        symbols: vec![sym("700.HK"), sym("9988.HK")],
        ask_symbols: vec![],
        bid_symbols: vec![sym("700.HK"), sym("9988.HK")],
    });
    links.institution_activities.push(InstitutionActivity {
        symbol: sym("700.HK"),
        institution_id: InstitutionId(100),
        ask_positions: vec![],
        bid_positions: vec![1, 2],
        seat_count: 2,
    });

    let store = make_store_with_stocks(vec![make_stock("700.HK", 100)]);
    let brain = build_brain(narratives, dimensions, &links, &store);
    let snapshot = DecisionSnapshot::compute(&brain, &links, &[], &store, None, None);

    let suggestion = snapshot
        .order_suggestions
        .iter()
        .find(|o| o.symbol == sym("700.HK"));
    assert!(suggestion.is_some());
    let s = suggestion.unwrap();
    assert_eq!(s.direction, OrderDirection::Buy);
    assert_eq!(s.suggested_quantity, 100);
    assert!(s.requires_confirmation);
}

#[test]
fn price_range_from_order_book() {
    let mut narratives = HashMap::new();
    narratives.insert(sym("700.HK"), make_narrative(dec!(0.8), dec!(0.6)));
    narratives.insert(sym("9988.HK"), make_narrative(dec!(0.7), dec!(0.5)));

    let mut dimensions = HashMap::new();
    dimensions.insert(
        sym("700.HK"),
        make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
    );
    dimensions.insert(
        sym("9988.HK"),
        make_dims(dec!(0.4), dec!(0.4), dec!(0.4), dec!(0.4)),
    );

    let mut links = empty_links();
    links.cross_stock_presences.push(CrossStockPresence {
        institution_id: InstitutionId(100),
        symbols: vec![sym("700.HK"), sym("9988.HK")],
        ask_symbols: vec![],
        bid_symbols: vec![sym("700.HK"), sym("9988.HK")],
    });
    links.institution_activities.push(InstitutionActivity {
        symbol: sym("700.HK"),
        institution_id: InstitutionId(100),
        ask_positions: vec![],
        bid_positions: vec![1],
        seat_count: 1,
    });
    links.order_books.push(OrderBookObservation {
        symbol: sym("700.HK"),
        bid_levels: vec![DepthLevel {
            position: 1,
            price: Some(dec!(350)),
            volume: 1000,
            order_num: 10,
        }],
        ask_levels: vec![DepthLevel {
            position: 1,
            price: Some(dec!(351)),
            volume: 800,
            order_num: 8,
        }],
        total_bid_volume: 1000,
        total_ask_volume: 800,
        total_bid_orders: 10,
        total_ask_orders: 8,
        spread: Some(dec!(1)),
        bid_level_count: 1,
        ask_level_count: 1,
        bid_profile: DepthProfile::empty(),
        ask_profile: DepthProfile::empty(),
    });

    let store = make_store_with_stocks(vec![make_stock("700.HK", 100)]);
    let brain = build_brain(narratives, dimensions, &links, &store);
    let snapshot = DecisionSnapshot::compute(&brain, &links, &[], &store, None, None);

    let s = snapshot
        .order_suggestions
        .iter()
        .find(|o| o.symbol == sym("700.HK"))
        .unwrap();
    assert_eq!(s.price_low, Some(dec!(350)));
    assert_eq!(s.price_high, Some(dec!(351)));
}

#[test]
fn confirmation_logic_only_flags_risky_orders() {
    let confident = ConvergenceScore {
        symbol: sym("700.HK"),
        institutional_alignment: dec!(0.7),
        sector_coherence: Some(dec!(0.6)),
        cross_stock_correlation: dec!(0.5),
        composite: dec!(0.6),
        edge_stability: None,
        institutional_edge_age: None,
        new_edge_fraction: None,
        microstructure_confirmation: None,
        component_spread: None,
        temporal_weight: None,
    };
    let policy = ConfirmationPolicy {
        low_confidence_cutoff: dec!(0.4),
        wide_spread_cutoff: dec!(0.01),
    };
    assert!(!requires_manual_confirmation(
        &confident,
        Some(dec!(350)),
        Some(dec!(351)),
        policy,
    ));

    let conflicted = ConvergenceScore {
        cross_stock_correlation: dec!(-0.5),
        ..confident.clone()
    };
    assert!(requires_manual_confirmation(
        &conflicted,
        Some(dec!(350)),
        Some(dec!(351)),
        policy,
    ));
}

#[test]
fn confirmation_policy_derives_cutoffs_from_market_samples() {
    let scores = HashMap::from([
        (
            sym("700.HK"),
            ConvergenceScore {
                symbol: sym("700.HK"),
                institutional_alignment: dec!(0.7),
                sector_coherence: Some(dec!(0.6)),
                cross_stock_correlation: dec!(0.5),
                composite: dec!(0.2),
                edge_stability: None,
                institutional_edge_age: None,
                new_edge_fraction: None,
                microstructure_confirmation: None,
                component_spread: None,
                temporal_weight: None,
            },
        ),
        (
            sym("388.HK"),
            ConvergenceScore {
                symbol: sym("388.HK"),
                institutional_alignment: dec!(0.5),
                sector_coherence: Some(dec!(0.4)),
                cross_stock_correlation: dec!(0.3),
                composite: dec!(0.6),
                edge_stability: None,
                institutional_edge_age: None,
                new_edge_fraction: None,
                microstructure_confirmation: None,
                component_spread: None,
                temporal_weight: None,
            },
        ),
        (
            sym("9988.HK"),
            ConvergenceScore {
                symbol: sym("9988.HK"),
                institutional_alignment: dec!(0.6),
                sector_coherence: Some(dec!(0.5)),
                cross_stock_correlation: dec!(0.2),
                composite: dec!(0.9),
                edge_stability: None,
                institutional_edge_age: None,
                new_edge_fraction: None,
                microstructure_confirmation: None,
                component_spread: None,
                temporal_weight: None,
            },
        ),
    ]);
    let best_bid = HashMap::from([
        (sym("700.HK"), dec!(350)),
        (sym("388.HK"), dec!(100)),
        (sym("9988.HK"), dec!(80)),
    ]);
    let best_ask = HashMap::from([
        (sym("700.HK"), dec!(351)),
        (sym("388.HK"), dec!(100.4)),
        (sym("9988.HK"), dec!(80.8)),
    ]);

    let policy = ConfirmationPolicy::from_market(&scores, &best_bid, &best_ask);

    assert_eq!(policy.low_confidence_cutoff, dec!(0.6));
    assert!(policy.wide_spread_cutoff > dec!(0.003));
    assert!(policy.wide_spread_cutoff < dec!(0.01));
}

#[test]
fn action_node_from_hk_fingerprint_maps_direction_and_market() {
    let symbol = sym("700.HK");
    let fingerprint = StructuralFingerprint {
        symbol: symbol.clone(),
        entry_tick: 0,
        entry_timestamp: OffsetDateTime::UNIX_EPOCH,
        entry_price: Some(dec!(95)),
        entry_composite: dec!(0.6),
        entry_regime: crate::action::narrative::Regime::CoherentBullish,
        institutional_directions: vec![],
        sector_mean_coherence: Some(dec!(0.2)),
        correlated_stocks: vec![],
        entry_dimensions: SymbolDimensions::default(),
    };

    let node = ActionNode::from_hk_fingerprint(&symbol, &fingerprint);

    assert_eq!(node.market, crate::ontology::Market::Hk);
    assert_eq!(node.direction, ActionDirection::Long);
    assert_eq!(node.stage, ActionNodeStage::Monitoring);
}

#[test]
fn action_node_from_hk_position_threads_live_fields() {
    let symbol = sym("700.HK");
    let fingerprint = StructuralFingerprint {
        symbol: symbol.clone(),
        entry_tick: 12,
        entry_timestamp: OffsetDateTime::UNIX_EPOCH,
        entry_price: Some(dec!(100)),
        entry_composite: dec!(-0.6),
        entry_regime: crate::action::narrative::Regime::CoherentBearish,
        institutional_directions: vec![],
        sector_mean_coherence: Some(dec!(0.2)),
        correlated_stocks: vec![],
        entry_dimensions: SymbolDimensions::default(),
    };
    let degradation = StructuralDegradation {
        symbol: symbol.clone(),
        institution_retention: dec!(0.4),
        sector_coherence_change: dec!(-0.1),
        correlation_retention: dec!(0.5),
        dimension_drift: dec!(0.7),
        composite_degradation: dec!(0.6),
    };

    let node = ActionNode::from_hk_position(
        &symbol,
        &fingerprint,
        18,
        Some(dec!(0.35)),
        Some(dec!(90)),
        Some(&degradation),
    );

    assert_eq!(node.direction, ActionDirection::Short);
    assert_eq!(node.current_confidence, dec!(0.35));
    assert_eq!(node.age_ticks, 6);
    assert_eq!(node.entry_price, Some(dec!(100)));
    assert_eq!(node.pnl, Some(dec!(0.1)));
    assert_eq!(node.degradation_score, Some(dec!(0.6)));
    assert!(node.exit_forming);
}

#[test]
fn market_regime_flags_broad_selloff_as_risk_off() {
    let mut links = empty_links();
    links.quotes = vec![
        QuoteObservation {
            symbol: sym("700.HK"),
            last_done: dec!(519),
            prev_close: dec!(550.5),
            open: dec!(545),
            high: dec!(546),
            low: dec!(515),
            volume: 100,
            turnover: dec!(52000),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            market_status: MarketStatus::Normal,
            pre_market: None,
            post_market: None,
        },
        QuoteObservation {
            symbol: sym("9988.HK"),
            last_done: dec!(72.3),
            prev_close: dec!(75.0),
            open: dec!(74.8),
            high: dec!(74.9),
            low: dec!(71.9),
            volume: 100,
            turnover: dec!(7230),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            market_status: MarketStatus::Normal,
            pre_market: None,
            post_market: None,
        },
        QuoteObservation {
            symbol: sym("3690.HK"),
            last_done: dec!(118),
            prev_close: dec!(123),
            open: dec!(122),
            high: dec!(122.5),
            low: dec!(117),
            volume: 100,
            turnover: dec!(11800),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            market_status: MarketStatus::Normal,
            pre_market: None,
            post_market: None,
        },
        QuoteObservation {
            symbol: sym("1810.HK"),
            last_done: dec!(14.8),
            prev_close: dec!(15.2),
            open: dec!(15.1),
            high: dec!(15.1),
            low: dec!(14.6),
            volume: 100,
            turnover: dec!(1480),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            market_status: MarketStatus::Normal,
            pre_market: None,
            post_market: None,
        },
        QuoteObservation {
            symbol: sym("883.HK"),
            last_done: dec!(19.1),
            prev_close: dec!(19.8),
            open: dec!(19.7),
            high: dec!(19.7),
            low: dec!(18.9),
            volume: 100,
            turnover: dec!(1910),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            market_status: MarketStatus::Normal,
            pre_market: None,
            post_market: None,
        },
        QuoteObservation {
            symbol: sym("939.HK"),
            last_done: dec!(5.91),
            prev_close: dec!(6.05),
            open: dec!(6.02),
            high: dec!(6.03),
            low: dec!(5.88),
            volume: 100,
            turnover: dec!(591),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            market_status: MarketStatus::Normal,
            pre_market: None,
            post_market: None,
        },
        QuoteObservation {
            symbol: sym("6060.HK"),
            last_done: dec!(14.96),
            prev_close: dec!(14.5),
            open: dec!(14.6),
            high: dec!(15.1),
            low: dec!(14.4),
            volume: 100,
            turnover: dec!(1496),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            market_status: MarketStatus::Normal,
            pre_market: None,
            post_market: None,
        },
        QuoteObservation {
            symbol: sym("688.HK"),
            last_done: dec!(11.9),
            prev_close: dec!(12.3),
            open: dec!(12.2),
            high: dec!(12.2),
            low: dec!(11.8),
            volume: 100,
            turnover: dec!(1190),
            timestamp: OffsetDateTime::UNIX_EPOCH,
            market_status: MarketStatus::Normal,
            pre_market: None,
            post_market: None,
        },
    ];

    let convergence_scores = HashMap::from([
        (
            sym("700.HK"),
            ConvergenceScore {
                symbol: sym("700.HK"),
                institutional_alignment: dec!(-0.4),
                sector_coherence: Some(dec!(-0.3)),
                cross_stock_correlation: dec!(-0.5),
                composite: dec!(-0.4),
                edge_stability: None,
                institutional_edge_age: None,
                new_edge_fraction: None,
                microstructure_confirmation: None,
                component_spread: None,
                temporal_weight: None,
            },
        ),
        (
            sym("9988.HK"),
            ConvergenceScore {
                symbol: sym("9988.HK"),
                institutional_alignment: dec!(-0.3),
                sector_coherence: Some(dec!(-0.2)),
                cross_stock_correlation: dec!(-0.4),
                composite: dec!(-0.3),
                edge_stability: None,
                institutional_edge_age: None,
                new_edge_fraction: None,
                microstructure_confirmation: None,
                component_spread: None,
                temporal_weight: None,
            },
        ),
        (
            sym("6060.HK"),
            ConvergenceScore {
                symbol: sym("6060.HK"),
                institutional_alignment: dec!(0.2),
                sector_coherence: Some(dec!(0.1)),
                cross_stock_correlation: dec!(0.1),
                composite: dec!(0.15),
                edge_stability: None,
                institutional_edge_age: None,
                new_edge_fraction: None,
                microstructure_confirmation: None,
                component_spread: None,
                temporal_weight: None,
            },
        ),
        (
            sym("688.HK"),
            ConvergenceScore {
                symbol: sym("688.HK"),
                institutional_alignment: dec!(-0.2),
                sector_coherence: Some(dec!(-0.1)),
                cross_stock_correlation: dec!(-0.2),
                composite: dec!(-0.2),
                edge_stability: None,
                institutional_edge_age: None,
                new_edge_fraction: None,
                microstructure_confirmation: None,
                component_spread: None,
                temporal_weight: None,
            },
        ),
    ]);

    let regime = MarketRegimeFilter::compute(&links, &convergence_scores);
    assert_eq!(regime.bias, MarketRegimeBias::RiskOff);
    assert!(regime.blocks(OrderDirection::Buy));
    assert!(!regime.blocks(OrderDirection::Sell));
}

#[test]
fn zero_composite_no_suggestions() {
    let mut narratives = HashMap::new();
    narratives.insert(sym("700.HK"), make_narrative(dec!(0), dec!(0)));

    let mut dimensions = HashMap::new();
    dimensions.insert(sym("700.HK"), make_dims(dec!(0), dec!(0), dec!(0), dec!(0)));

    let links = empty_links();
    let store = make_store_with_stocks(vec![]);
    let brain = build_brain(narratives, dimensions, &links, &store);
    let snapshot = DecisionSnapshot::compute(&brain, &links, &[], &store, None, None);

    // Composite is zero → no suggestion
    let suggestion = snapshot
        .order_suggestions
        .iter()
        .find(|o| o.symbol == sym("700.HK"));
    assert!(suggestion.is_none());
}

#[test]
fn polymarket_market_prior_soft_blocks_when_local_regime_is_neutral() {
    let mut regime = MarketRegimeFilter::neutral();
    regime.apply_polymarket_snapshot(&PolymarketSnapshot {
        fetched_at: OffsetDateTime::UNIX_EPOCH,
        priors: vec![PolymarketPrior {
            slug: "fed-cut".into(),
            label: "Fed cut".into(),
            question: "Will the Fed cut?".into(),
            scope: ReasoningScope::market(),
            target_scopes: vec![],
            bias: PolymarketBias::RiskOn,
            selected_outcome: "Yes".into(),
            probability: dec!(0.72),
            conviction_threshold: dec!(0.60),
            active: true,
            closed: false,
            category: None,
            volume: None,
            liquidity: None,
            end_date: None,
        }],
    });

    assert!(regime.blocks(OrderDirection::Sell));
    assert!(!regime.blocks(OrderDirection::Buy));
    assert!(regime
        .gate_reason(OrderDirection::Sell)
        .unwrap_or_default()
        .contains("external="));
}

#[test]
fn explicit_target_scopes_drive_polymarket_symbol_relevance() {
    let store = make_store_with_stocks(vec![Stock {
        market: crate::ontology::Market::Hk,
        symbol: sym("981.HK"),
        name_en: "SMIC".into(),
        name_cn: String::new(),
        name_hk: String::new(),
        exchange: "SEHK".into(),
        lot_size: 100,
        sector_id: Some(SectorId("semiconductor".into())),
        total_shares: 0,
        circulating_shares: 0,
        eps_ttm: Decimal::ZERO,
        bps: Decimal::ZERO,
        dividend_yield: Decimal::ZERO,
    }]);
    let mut suggestion = OrderSuggestion {
        symbol: sym("981.HK"),
        direction: OrderDirection::Sell,
        convergence: ConvergenceScore {
            symbol: sym("981.HK"),
            institutional_alignment: dec!(-0.5),
            sector_coherence: Some(dec!(-0.4)),
            cross_stock_correlation: dec!(-0.3),
            composite: dec!(-0.55),
            edge_stability: None,
            institutional_edge_age: None,
            new_edge_fraction: None,
            microstructure_confirmation: None,
            component_spread: None,
            temporal_weight: None,
        },
        suggested_quantity: 100,
        price_low: Some(dec!(20)),
        price_high: Some(dec!(20.1)),
        estimated_cost: dec!(0.005),
        heuristic_edge: dec!(0.54),
        requires_confirmation: false,
        convergence_score: dec!(0.55),
        effective_confidence: dec!(0.55),
        external_confirmation: None,
        external_conflict: None,
        external_support_slug: None,
        external_support_probability: None,
        external_conflict_slug: None,
        external_conflict_probability: None,
    };
    let snapshot = PolymarketSnapshot {
        fetched_at: OffsetDateTime::UNIX_EPOCH,
        priors: vec![PolymarketPrior {
            slug: "chip-sanctions".into(),
            label: "AI chip sanctions".into(),
            question: "Will AI chip sanctions tighten?".into(),
            scope: ReasoningScope::Theme("ai_semis".into()),
            target_scopes: vec!["sector:semiconductor".into()],
            bias: PolymarketBias::RiskOff,
            selected_outcome: "Yes".into(),
            probability: dec!(0.66),
            conviction_threshold: dec!(0.60),
            active: true,
            closed: false,
            category: None,
            volume: None,
            liquidity: None,
            end_date: None,
        }],
    };

    let sector_id = store
        .stocks
        .get(&sym("981.HK"))
        .and_then(|stock| stock.sector_id.as_ref());
    apply_external_convergence_to_suggestion(&mut suggestion, &snapshot, sector_id);

    assert!(suggestion.external_confirmation.is_some());
    assert_eq!(
        suggestion.external_support_slug.as_deref(),
        Some("chip-sanctions")
    );
    assert!(suggestion.convergence_score > dec!(0.55));
}
