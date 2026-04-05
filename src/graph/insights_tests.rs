use super::*;
use crate::action::narrative::{
    DimensionReading, Direction, NarrativeSnapshot, Regime, SymbolNarrative,
};
use crate::graph::graph::BrainGraph;
use crate::pipeline::tension::Dimension;
use crate::ontology::links::*;
use crate::ontology::objects::*;
use crate::pipeline::dimensions::{DimensionSnapshot, SymbolDimensions};
use rust_decimal_macros::dec;
use time::OffsetDateTime;

fn sym(s: &str) -> Symbol {
    Symbol(s.into())
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

fn make_store(stocks: Vec<Stock>, sectors: Vec<Sector>) -> ObjectStore {
    let mut stock_map = HashMap::new();
    for s in stocks {
        stock_map.insert(s.symbol.clone(), s);
    }
    let mut sector_map = HashMap::new();
    for s in sectors {
        sector_map.insert(s.id.clone(), s);
    }
    ObjectStore {
        institutions: HashMap::new(),
        brokers: HashMap::new(),
        stocks: stock_map,
        sectors: sector_map,
        broker_to_institution: HashMap::new(),
        knowledge: std::sync::RwLock::new(crate::ontology::store::AccumulatedKnowledge::empty()),
    }
}

fn make_stock(symbol: &str, sector: Option<&str>) -> Stock {
    let symbol_id = sym(symbol);
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
        eps_ttm: Decimal::ZERO,
        bps: Decimal::ZERO,
        dividend_yield: Decimal::ZERO,
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
    let dims = DimensionSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        dimensions,
    };
    BrainGraph::compute(&narrative, &dims, links, store)
}

fn empty_stress() -> MarketStressIndex {
    MarketStressIndex {
        sector_synchrony: Decimal::ZERO,
        pressure_consensus: Decimal::ZERO,
        conflict_intensity_mean: Decimal::ZERO,
        market_temperature_stress: Decimal::ZERO,
        composite_stress: Decimal::ZERO,
    }
}

fn make_insights_with_pressures(pressures: Vec<StockPressure>) -> GraphInsights {
    GraphInsights {
        pressures,
        rotations: vec![],
        clusters: vec![],
        conflicts: vec![],
        inst_rotations: vec![],
        inst_exoduses: vec![],
        shared_holders: vec![],
        stress: empty_stress(),
        institution_stock_counts: HashMap::new(),
        edge_profiles: vec![],
    }
}

fn make_insights_with_clusters(clusters: Vec<StockCluster>) -> GraphInsights {
    GraphInsights {
        pressures: vec![],
        rotations: vec![],
        clusters,
        conflicts: vec![],
        inst_rotations: vec![],
        inst_exoduses: vec![],
        shared_holders: vec![],
        stress: empty_stress(),
        institution_stock_counts: HashMap::new(),
        edge_profiles: vec![],
    }
}

fn make_insights_with_rotations(rotations: Vec<RotationPair>) -> GraphInsights {
    GraphInsights {
        pressures: vec![],
        rotations,
        clusters: vec![],
        conflicts: vec![],
        inst_rotations: vec![],
        inst_exoduses: vec![],
        shared_holders: vec![],
        stress: empty_stress(),
        institution_stock_counts: HashMap::new(),
        edge_profiles: vec![],
    }
}

fn make_empty_insights() -> GraphInsights {
    GraphInsights {
        pressures: vec![],
        rotations: vec![],
        clusters: vec![],
        conflicts: vec![],
        inst_rotations: vec![],
        inst_exoduses: vec![],
        shared_holders: vec![],
        stress: empty_stress(),
        institution_stock_counts: HashMap::new(),
        edge_profiles: vec![],
    }
}

// ── Test 1: Empty graph + no prev → empty insights, no panics ──

#[test]
fn empty_graph_no_prev_empty_insights() {
    let narrative = NarrativeSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        narratives: HashMap::new(),
    };
    let dims = DimensionSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        dimensions: HashMap::new(),
    };
    let links = empty_links();
    let store = make_store(vec![], vec![]);
    let brain = BrainGraph::compute(&narrative, &dims, &links, &store);

    let mut ch = ConflictHistory::new();
    let insights = GraphInsights::compute(&brain, &store, None, &mut ch, 0);
    assert!(insights.pressures.is_empty());
    assert!(insights.rotations.is_empty());
    assert!(insights.clusters.is_empty());
    assert!(insights.conflicts.is_empty());
}

// ── Test 2: StockPressure with prev → correct delta and duration ──

#[test]
fn stock_pressure_with_prev_delta_and_duration() {
    let mut narratives = HashMap::new();
    narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
    narratives.insert(sym("9988.HK"), make_narrative(dec!(0.3), dec!(0.1)));

    let mut dimensions = HashMap::new();
    dimensions.insert(
        sym("700.HK"),
        make_dims(dec!(0.2), dec!(0.1), dec!(0.3), dec!(0.4)),
    );
    dimensions.insert(
        sym("9988.HK"),
        make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
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
    links.institution_activities.push(InstitutionActivity {
        symbol: sym("9988.HK"),
        institution_id: InstitutionId(100),
        ask_positions: vec![1, 2],
        bid_positions: vec![],
        seat_count: 2,
    });

    let store = make_store(vec![], vec![]);
    let brain = build_brain(narratives, dimensions, &links, &store);

    // First tick: no prev
    let mut ch = ConflictHistory::new();
    let insights1 = GraphInsights::compute(&brain, &store, None, &mut ch, 1);
    let p700_1 = insights1
        .pressures
        .iter()
        .find(|p| p.symbol == sym("700.HK"))
        .unwrap();
    assert!(p700_1.net_pressure > Decimal::ZERO);
    assert_eq!(p700_1.pressure_duration, 1);
    assert_eq!(p700_1.pressure_delta, Decimal::ZERO);

    // Second tick: with prev → delta should be 0 (same data), duration increments
    let insights2 = GraphInsights::compute(&brain, &store, Some(&insights1), &mut ch, 2);
    let p700_2 = insights2
        .pressures
        .iter()
        .find(|p| p.symbol == sym("700.HK"))
        .unwrap();
    assert_eq!(p700_2.pressure_delta, Decimal::ZERO);
    assert_eq!(p700_2.pressure_duration, 2); // same direction, incremented
}

// ── Test 3: StockPressure direction flip → duration resets ──

#[test]
fn stock_pressure_direction_flip_resets_duration() {
    // Create a "prev" insight with positive pressure
    let prev_pressure = StockPressure {
        symbol: sym("700.HK"),
        net_pressure: Decimal::new(3, 1), // +0.3
        institution_count: 1,
        buy_inst_count: 1,
        sell_inst_count: 0,
        pressure_delta: Decimal::ZERO,
        pressure_duration: 5, // was going for 5 ticks
        accelerating: false,
    };
    let prev = make_insights_with_pressures(vec![prev_pressure]);

    // Now build a brain where 700.HK has NEGATIVE pressure (flipped)
    let mut narratives = HashMap::new();
    narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));

    let mut dimensions = HashMap::new();
    dimensions.insert(
        sym("700.HK"),
        make_dims(dec!(0.2), dec!(0.1), dec!(0.3), dec!(0.4)),
    );

    let mut links = empty_links();
    links.cross_stock_presences.push(CrossStockPresence {
        institution_id: InstitutionId(100),
        symbols: vec![sym("700.HK")],
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

    let store = make_store(vec![], vec![]);
    let brain = build_brain(narratives, dimensions, &links, &store);
    let mut ch = ConflictHistory::new();
    let insights = GraphInsights::compute(&brain, &store, Some(&prev), &mut ch, 6);

    let p700 = insights
        .pressures
        .iter()
        .find(|p| p.symbol == sym("700.HK"))
        .unwrap();
    assert!(p700.net_pressure < Decimal::ZERO); // flipped to negative
    assert_eq!(p700.pressure_duration, 1); // reset
}

// ── Test 4: Cluster with high stability prev → age increments ──

#[test]
fn cluster_high_stability_age_increments() {
    let prev_cluster = StockCluster {
        members: vec![sym("700.HK"), sym("9988.HK")],
        mean_similarity: dec!(0.8),
        directional_alignment: dec!(0.9),
        cross_sector: false,
        stability: dec!(0.9),
        age: 5,
    };
    let prev = make_insights_with_clusters(vec![prev_cluster]);

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

    let links = empty_links();
    let store = make_store(vec![], vec![]);
    let brain = build_brain(narratives, dimensions, &links, &store);
    let mut ch = ConflictHistory::new();
    let insights = GraphInsights::compute(&brain, &store, Some(&prev), &mut ch, 6);

    // If a cluster forms with the same members, stability should be high and age should increment
    for c in &insights.clusters {
        let has_700 = c.members.contains(&sym("700.HK"));
        let has_9988 = c.members.contains(&sym("9988.HK"));
        if has_700 && has_9988 {
            assert!(
                c.stability > Decimal::new(5, 1),
                "stability should be > 0.5"
            );
            assert!(c.age > 5, "age should have incremented from 5");
        }
    }
}

// ── Test 5: Cluster with low stability → age resets to 1 ──

#[test]
fn cluster_low_stability_age_resets() {
    // Prev cluster had completely different members
    let prev_cluster = StockCluster {
        members: vec![sym("883.HK"), sym("5.HK")],
        mean_similarity: dec!(0.8),
        directional_alignment: dec!(0.9),
        cross_sector: false,
        stability: dec!(0.9),
        age: 10,
    };
    let prev = make_insights_with_clusters(vec![prev_cluster]);

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

    let links = empty_links();
    let store = make_store(vec![], vec![]);
    let brain = build_brain(narratives, dimensions, &links, &store);
    let mut ch = ConflictHistory::new();
    let insights = GraphInsights::compute(&brain, &store, Some(&prev), &mut ch, 11);

    // New cluster (700, 9988) has no overlap with prev (883, 5) → Jaccard = 0 → age = 1
    // age < 3 with prev.is_some() → filtered out
    for c in &insights.clusters {
        if c.members.contains(&sym("700.HK")) && c.members.contains(&sym("9988.HK")) {
            // If it wasn't filtered, age should be 1
            assert_eq!(c.age, 1);
        }
    }
}

// ── Test 6: Cluster with align < 0.6 → filtered out ──

#[test]
fn cluster_low_alignment_filtered() {
    let mut narratives = HashMap::new();
    // One positive, one negative → alignment ~50%
    narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
    narratives.insert(sym("9988.HK"), make_narrative(dec!(0.3), dec!(-0.3)));

    let mut dimensions = HashMap::new();
    // Both have similar magnitudes but opposite signs won't necessarily create edges
    // We need them to be similar enough to form an edge
    dimensions.insert(
        sym("700.HK"),
        make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
    );
    dimensions.insert(
        sym("9988.HK"),
        make_dims(dec!(0.4), dec!(0.4), dec!(0.4), dec!(0.4)),
    );

    let links = empty_links();
    let store = make_store(vec![], vec![]);
    let brain = build_brain(narratives, dimensions, &links, &store);
    let mut ch = ConflictHistory::new();
    let insights = GraphInsights::compute(&brain, &store, None, &mut ch, 1);

    // If a cluster forms with one positive and one negative direction,
    // alignment = max(1,1)/2 = 0.5 which is < 0.6 → filtered
    for c in &insights.clusters {
        assert!(
            c.directional_alignment >= Decimal::new(6, 1),
            "clusters with alignment < 0.6 should be filtered"
        );
    }
}

// ── Test 7: Cluster age < 3 → not reported (when prev exists) ──

#[test]
fn cluster_young_not_reported() {
    // Empty prev → new clusters get age=1, which is < 3 → filtered when prev is Some
    let prev = make_empty_insights();

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

    let links = empty_links();
    let store = make_store(vec![], vec![]);
    let brain = build_brain(narratives, dimensions, &links, &store);
    let mut ch = ConflictHistory::new();
    let insights = GraphInsights::compute(&brain, &store, Some(&prev), &mut ch, 1);

    // All new clusters should be filtered out (age=1 < 3)
    assert!(
        insights.clusters.is_empty(),
        "young clusters should be filtered when prev exists"
    );
}

// ── Test 8: Conflict with prev → age tracks correctly ──

#[test]
fn conflict_age_tracks() {
    let mut narratives = HashMap::new();
    narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
    narratives.insert(sym("9988.HK"), make_narrative(dec!(0.3), dec!(0.1)));
    narratives.insert(sym("3690.HK"), make_narrative(dec!(0.4), dec!(0.2)));

    let mut dimensions = HashMap::new();
    dimensions.insert(
        sym("700.HK"),
        make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
    );
    dimensions.insert(
        sym("9988.HK"),
        make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
    );
    dimensions.insert(
        sym("3690.HK"),
        make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
    );

    let mut links = empty_links();
    links.cross_stock_presences.push(CrossStockPresence {
        institution_id: InstitutionId(100),
        symbols: vec![sym("700.HK"), sym("9988.HK")],
        ask_symbols: vec![],
        bid_symbols: vec![sym("700.HK"), sym("9988.HK")],
    });
    links.cross_stock_presences.push(CrossStockPresence {
        institution_id: InstitutionId(200),
        symbols: vec![sym("700.HK"), sym("9988.HK")],
        ask_symbols: vec![sym("700.HK"), sym("9988.HK")],
        bid_symbols: vec![],
    });
    links.cross_stock_presences.push(CrossStockPresence {
        institution_id: InstitutionId(300),
        symbols: vec![sym("3690.HK"), sym("700.HK")],
        ask_symbols: vec![],
        bid_symbols: vec![sym("3690.HK"), sym("700.HK")],
    });

    links.institution_activities.push(InstitutionActivity {
        symbol: sym("700.HK"),
        institution_id: InstitutionId(100),
        ask_positions: vec![],
        bid_positions: vec![1],
        seat_count: 1,
    });
    links.institution_activities.push(InstitutionActivity {
        symbol: sym("9988.HK"),
        institution_id: InstitutionId(100),
        ask_positions: vec![],
        bid_positions: vec![1],
        seat_count: 1,
    });
    links.institution_activities.push(InstitutionActivity {
        symbol: sym("700.HK"),
        institution_id: InstitutionId(200),
        ask_positions: vec![1],
        bid_positions: vec![],
        seat_count: 1,
    });
    links.institution_activities.push(InstitutionActivity {
        symbol: sym("9988.HK"),
        institution_id: InstitutionId(200),
        ask_positions: vec![1],
        bid_positions: vec![],
        seat_count: 1,
    });
    links.institution_activities.push(InstitutionActivity {
        symbol: sym("3690.HK"),
        institution_id: InstitutionId(300),
        ask_positions: vec![],
        bid_positions: vec![1],
        seat_count: 1,
    });
    links.institution_activities.push(InstitutionActivity {
        symbol: sym("700.HK"),
        institution_id: InstitutionId(300),
        ask_positions: vec![],
        bid_positions: vec![1],
        seat_count: 1,
    });

    let store = make_store(vec![], vec![]);
    let brain = build_brain(narratives, dimensions, &links, &store);

    let mut ch = ConflictHistory::new();
    // Tick 1
    let _insights1 = GraphInsights::compute(&brain, &store, None, &mut ch, 1);
    // Tick 5
    let insights5 = GraphInsights::compute(&brain, &store, None, &mut ch, 5);

    // The 100 vs 200 conflict should have age = 5 - 1 = 4
    if let Some(c) = insights5.conflicts.iter().find(|c| {
        (c.inst_a == InstitutionId(100) && c.inst_b == InstitutionId(200))
            || (c.inst_a == InstitutionId(200) && c.inst_b == InstitutionId(100))
    }) {
        assert_eq!(
            c.conflict_age, 4,
            "conflict age should be tick_now - first_seen"
        );
    }
}

// ── Test 9: Conflict intensity increasing → intensity_delta > 0 ──

#[test]
fn conflict_intensity_increasing() {
    let mut ch = ConflictHistory::new();
    let a = InstitutionId(100);
    let b = InstitutionId(200);

    // First observation: intensity = 0.5
    let (age1, delta1) = ch.update(a, b, dec!(0.5), 1);
    assert_eq!(age1, 0);
    assert_eq!(delta1, Decimal::ZERO);

    // Second observation: intensity = 0.8 (increased)
    let (age2, delta2) = ch.update(a, b, dec!(0.8), 2);
    assert_eq!(age2, 1);
    assert!(
        delta2 > Decimal::ZERO,
        "intensity increased, delta should be positive"
    );
}

// ── Test 10: Rotation with prev → spread_delta correct ──

#[test]
fn rotation_spread_delta() {
    let mut narratives = HashMap::new();
    narratives.insert(sym("700.HK"), make_narrative(dec!(0.8), dec!(0.6)));
    narratives.insert(sym("5.HK"), make_narrative(dec!(0.7), dec!(-0.5)));
    narratives.insert(sym("883.HK"), make_narrative(dec!(0.5), dec!(0.0)));

    let mut dimensions = HashMap::new();
    dimensions.insert(
        sym("700.HK"),
        make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
    );
    dimensions.insert(
        sym("5.HK"),
        make_dims(dec!(-0.5), dec!(-0.5), dec!(-0.5), dec!(-0.5)),
    );
    dimensions.insert(
        sym("883.HK"),
        make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
    );

    let links = empty_links();
    let store = make_store(
        vec![
            make_stock("700.HK", Some("tech")),
            make_stock("5.HK", Some("finance")),
            make_stock("883.HK", Some("energy")),
        ],
        vec![
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
        ],
    );

    let brain = build_brain(narratives, dimensions, &links, &store);
    let mut ch = ConflictHistory::new();

    // First tick: no prev
    let insights1 = GraphInsights::compute(&brain, &store, None, &mut ch, 1);

    // Second tick: same data → spread_delta should be 0
    let insights2 = GraphInsights::compute(&brain, &store, Some(&insights1), &mut ch, 2);

    for r in &insights2.rotations {
        assert_eq!(
            r.spread_delta,
            Decimal::ZERO,
            "same data → spread_delta = 0"
        );
    }
}

// ── Test 11: Rotation widening vs narrowing ──

#[test]
fn rotation_widening_vs_narrowing() {
    // Create a prev with a known spread
    let prev = make_insights_with_rotations(vec![RotationPair {
        from_sector: SectorId("tech".into()),
        to_sector: SectorId("finance".into()),
        spread: dec!(0.5),
        spread_delta: Decimal::ZERO,
        widening: false,
    }]);

    let mut narratives = HashMap::new();
    narratives.insert(sym("700.HK"), make_narrative(dec!(0.8), dec!(0.8))); // tech higher
    narratives.insert(sym("5.HK"), make_narrative(dec!(0.7), dec!(-0.5)));
    narratives.insert(sym("883.HK"), make_narrative(dec!(0.5), dec!(0.0)));

    let mut dimensions = HashMap::new();
    dimensions.insert(
        sym("700.HK"),
        make_dims(dec!(0.5), dec!(0.5), dec!(0.5), dec!(0.5)),
    );
    dimensions.insert(
        sym("5.HK"),
        make_dims(dec!(-0.5), dec!(-0.5), dec!(-0.5), dec!(-0.5)),
    );
    dimensions.insert(
        sym("883.HK"),
        make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
    );

    let links = empty_links();
    let store = make_store(
        vec![
            make_stock("700.HK", Some("tech")),
            make_stock("5.HK", Some("finance")),
            make_stock("883.HK", Some("energy")),
        ],
        vec![
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
        ],
    );

    let brain = build_brain(narratives, dimensions, &links, &store);
    let mut ch = ConflictHistory::new();
    let insights = GraphInsights::compute(&brain, &store, Some(&prev), &mut ch, 2);

    // tech-finance spread is now |0.8 - (-0.5)| = 1.3, prev was 0.5
    // So spread_delta = 1.3 - 0.5 = 0.8 > 0 → widening
    if let Some(r) = insights.rotations.iter().find(|r| {
        r.from_sector == SectorId("tech".into()) && r.to_sector == SectorId("finance".into())
    }) {
        assert!(r.spread_delta > Decimal::ZERO, "spread should be widening");
        assert!(r.widening);
    }
}

// ── Test 12: ConcentrationAlert removed — no concentrations field ──

#[test]
fn no_concentrations_field() {
    let insights = make_empty_insights();
    assert!(insights.pressures.is_empty());
}

// ── Graph-Only Signal Tests ──

#[test]
fn institution_rotation_buy_and_sell() {
    let mut narratives = HashMap::new();
    narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
    narratives.insert(sym("9988.HK"), make_narrative(dec!(0.3), dec!(0.1)));

    let mut dimensions = HashMap::new();
    dimensions.insert(
        sym("700.HK"),
        make_dims(dec!(0.2), dec!(0.1), dec!(0.3), dec!(0.4)),
    );
    dimensions.insert(
        sym("9988.HK"),
        make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
    );

    let mut links = empty_links();
    // Institution 100: buying 700.HK, selling 9988.HK → rotation
    links.cross_stock_presences.push(CrossStockPresence {
        institution_id: InstitutionId(100),
        symbols: vec![sym("700.HK"), sym("9988.HK")],
        ask_symbols: vec![sym("9988.HK")],
        bid_symbols: vec![sym("700.HK")],
    });
    links.institution_activities.push(InstitutionActivity {
        symbol: sym("700.HK"),
        institution_id: InstitutionId(100),
        ask_positions: vec![],
        bid_positions: vec![1, 2],
        seat_count: 2,
    });
    links.institution_activities.push(InstitutionActivity {
        symbol: sym("9988.HK"),
        institution_id: InstitutionId(100),
        ask_positions: vec![1, 2],
        bid_positions: vec![],
        seat_count: 2,
    });

    let store = make_store(vec![], vec![]);
    let brain = build_brain(narratives, dimensions, &links, &store);
    let mut ch = ConflictHistory::new();
    let insights = GraphInsights::compute(&brain, &store, None, &mut ch, 1);

    // Institution 100 should appear in inst_rotations
    assert!(
        !insights.inst_rotations.is_empty(),
        "should detect rotation"
    );
    let rot = insights
        .inst_rotations
        .iter()
        .find(|r| r.institution_id == InstitutionId(100))
        .expect("institution 100 should be rotating");
    assert!(!rot.buy_symbols.is_empty(), "should have buy symbols");
    assert!(!rot.sell_symbols.is_empty(), "should have sell symbols");
}

#[test]
fn institution_rotation_one_sided_no_rotation() {
    let mut narratives = HashMap::new();
    narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));

    let mut dimensions = HashMap::new();
    dimensions.insert(
        sym("700.HK"),
        make_dims(dec!(0.2), dec!(0.1), dec!(0.3), dec!(0.4)),
    );

    let mut links = empty_links();
    // Institution 100: only buying → NOT a rotation
    links.cross_stock_presences.push(CrossStockPresence {
        institution_id: InstitutionId(100),
        symbols: vec![sym("700.HK")],
        ask_symbols: vec![],
        bid_symbols: vec![sym("700.HK")],
    });
    links.institution_activities.push(InstitutionActivity {
        symbol: sym("700.HK"),
        institution_id: InstitutionId(100),
        ask_positions: vec![],
        bid_positions: vec![1],
        seat_count: 1,
    });

    let store = make_store(vec![], vec![]);
    let brain = build_brain(narratives, dimensions, &links, &store);
    let mut ch = ConflictHistory::new();
    let insights = GraphInsights::compute(&brain, &store, None, &mut ch, 1);

    // One-sided institution should NOT appear in rotations
    let rot = insights
        .inst_rotations
        .iter()
        .find(|r| r.institution_id == InstitutionId(100));
    assert!(
        rot.is_none(),
        "one-sided institution should not be in rotations"
    );
}

#[test]
fn institution_exodus_detected() {
    // Simulate prev tick with institution having 5 stocks
    let mut prev_counts = HashMap::new();
    prev_counts.insert(InstitutionId(100), 5usize);
    prev_counts.insert(InstitutionId(200), 3usize);
    let mut prev = make_empty_insights();
    prev.institution_stock_counts = prev_counts;

    // Current tick: institution 100 dropped to 1, institution 200 unchanged
    let mut current = HashMap::new();
    current.insert(InstitutionId(100), 1usize);
    current.insert(InstitutionId(200), 3usize);

    let exoduses = compute_institution_exoduses(&current, Some(&prev));

    // Institution 100 dropped 4 stocks, institution 200 dropped 0
    // With 2 data points: drops = [4], only inst 100 has a drop
    // median of [4] = 4, strict > 4 = nothing passes
    // Actually only 1 institution dropped, so drops = [(100, 5, 1, 4)]
    // median of [4] = 4, > 4 is false. So nothing passes.
    // We need 2+ institutions dropping for the median filter to work.
    // This is correct — a single institution's drop isn't anomalous without comparison.

    // Let's add a third institution with a small drop
    let mut prev2 = make_empty_insights();
    let mut prev_counts2 = HashMap::new();
    prev_counts2.insert(InstitutionId(100), 5);
    prev_counts2.insert(InstitutionId(200), 4);
    prev_counts2.insert(InstitutionId(300), 3);
    prev2.institution_stock_counts = prev_counts2;

    let mut current2 = HashMap::new();
    current2.insert(InstitutionId(100), 1); // dropped 4
    current2.insert(InstitutionId(200), 3); // dropped 1
    current2.insert(InstitutionId(300), 2); // dropped 1

    let exoduses2 = compute_institution_exoduses(&current2, Some(&prev2));
    // drops: [4, 1, 1], sorted: [1, 1, 4], median = 1
    // > 1: only institution 100 (dropped 4)
    assert_eq!(exoduses2.len(), 1);
    assert_eq!(exoduses2[0].institution_id, InstitutionId(100));
    assert_eq!(exoduses2[0].dropped_count, 4);

    // No prev → no exoduses
    let exoduses_no_prev = compute_institution_exoduses(&current, None);
    assert!(exoduses_no_prev.is_empty());

    let _ = exoduses; // suppress warning
}

#[test]
fn shared_holder_cross_sector() {
    let mut narratives = HashMap::new();
    narratives.insert(sym("700.HK"), make_narrative(dec!(0.5), dec!(0.3)));
    narratives.insert(sym("883.HK"), make_narrative(dec!(0.4), dec!(0.2)));
    narratives.insert(sym("5.HK"), make_narrative(dec!(0.3), dec!(0.1)));

    let mut dimensions = HashMap::new();
    dimensions.insert(
        sym("700.HK"),
        make_dims(dec!(0.3), dec!(0.3), dec!(0.3), dec!(0.3)),
    );
    dimensions.insert(
        sym("883.HK"),
        make_dims(dec!(0.2), dec!(0.2), dec!(0.2), dec!(0.2)),
    );
    dimensions.insert(
        sym("5.HK"),
        make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
    );

    let mut links = empty_links();
    // Same two institutions (100, 200) present in BOTH 700.HK(tech) and 883.HK(energy)
    // but only institution 300 in 5.HK(finance)
    for &sym_str in &["700.HK", "883.HK"] {
        for &inst_id in &[100i32, 200] {
            links.cross_stock_presences.push(CrossStockPresence {
                institution_id: InstitutionId(inst_id),
                symbols: vec![sym(sym_str), sym("883.HK")],
                ask_symbols: vec![],
                bid_symbols: vec![sym(sym_str)],
            });
            links.institution_activities.push(InstitutionActivity {
                symbol: sym(sym_str),
                institution_id: InstitutionId(inst_id),
                ask_positions: vec![],
                bid_positions: vec![1],
                seat_count: 1,
            });
        }
    }
    links.cross_stock_presences.push(CrossStockPresence {
        institution_id: InstitutionId(300),
        symbols: vec![sym("5.HK")],
        ask_symbols: vec![],
        bid_symbols: vec![sym("5.HK")],
    });
    links.institution_activities.push(InstitutionActivity {
        symbol: sym("5.HK"),
        institution_id: InstitutionId(300),
        ask_positions: vec![],
        bid_positions: vec![1],
        seat_count: 1,
    });

    let store = make_store(
        vec![
            make_stock("700.HK", Some("tech")),
            make_stock("883.HK", Some("energy")),
            make_stock("5.HK", Some("finance")),
        ],
        vec![
            Sector {
                id: SectorId("tech".into()),
                name: "Tech".into(),
            },
            Sector {
                id: SectorId("energy".into()),
                name: "Energy".into(),
            },
            Sector {
                id: SectorId("finance".into()),
                name: "Finance".into(),
            },
        ],
    );

    let brain = build_brain(narratives, dimensions, &links, &store);
    let mut ch = ConflictHistory::new();
    let insights = GraphInsights::compute(&brain, &store, None, &mut ch, 1);

    // 700.HK(tech) and 883.HK(energy) share institutions {100, 200}
    // This is a cross-sector shared holder anomaly
    // Whether it appears depends on median filtering — with 3 cross-sector pairs,
    // the 700-883 pair should have the highest Jaccard
    for sh in &insights.shared_holders {
        assert_ne!(
            sh.sector_a, sh.sector_b,
            "shared holders must be cross-sector"
        );
        assert!(sh.jaccard > Decimal::ZERO);
    }
}

#[test]
fn stress_index_computed() {
    let narrative = NarrativeSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        narratives: HashMap::new(),
    };
    let dims = DimensionSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        dimensions: HashMap::new(),
    };
    let links = empty_links();
    let store = make_store(vec![], vec![]);
    let brain = BrainGraph::compute(&narrative, &dims, &links, &store);
    let mut ch = ConflictHistory::new();
    let insights = GraphInsights::compute(&brain, &store, None, &mut ch, 1);

    // Empty graph → stress should be zero
    assert_eq!(insights.stress.sector_synchrony, Decimal::ZERO);
    assert_eq!(insights.stress.pressure_consensus, Decimal::ZERO);
    assert_eq!(insights.stress.market_temperature_stress, Decimal::ZERO);
    assert_eq!(insights.stress.composite_stress, Decimal::ZERO);
}

#[test]
fn stress_components_are_clamped_non_negative() {
    let narratives = HashMap::from([
        (sym("700.HK"), make_narrative(dec!(0.5), dec!(2))),
        (sym("5.HK"), make_narrative(dec!(0.5), dec!(-2))),
    ]);
    let dimensions = HashMap::from([
        (
            sym("700.HK"),
            make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
        ),
        (
            sym("5.HK"),
            make_dims(dec!(0.1), dec!(0.1), dec!(0.1), dec!(0.1)),
        ),
    ]);
    let store = make_store(
        vec![
            make_stock("700.HK", Some("tech")),
            make_stock("5.HK", Some("finance")),
        ],
        vec![
            Sector {
                id: SectorId("tech".into()),
                name: "Technology".into(),
            },
            Sector {
                id: SectorId("finance".into()),
                name: "Finance".into(),
            },
        ],
    );
    let brain = build_brain(narratives, dimensions, &empty_links(), &store);
    let pressures = vec![
        StockPressure {
            symbol: sym("700.HK"),
            net_pressure: dec!(3),
            institution_count: 1,
            buy_inst_count: 1,
            sell_inst_count: 0,
            pressure_delta: Decimal::ZERO,
            pressure_duration: 1,
            accelerating: false,
        },
        StockPressure {
            symbol: sym("5.HK"),
            net_pressure: dec!(-3),
            institution_count: 1,
            buy_inst_count: 0,
            sell_inst_count: 1,
            pressure_delta: Decimal::ZERO,
            pressure_duration: 1,
            accelerating: false,
        },
    ];

    let stress = compute_stress_index(&brain, &pressures, &[]);
    assert_eq!(stress.sector_synchrony, Decimal::ZERO);
    assert_eq!(stress.pressure_consensus, Decimal::ZERO);
}

#[test]
fn stress_index_uses_market_temperature() {
    let narrative = NarrativeSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        narratives: HashMap::new(),
    };
    let dims = DimensionSnapshot {
        timestamp: OffsetDateTime::UNIX_EPOCH,
        dimensions: HashMap::new(),
    };
    let mut links = empty_links();
    links.market_temperature = Some(MarketTemperatureObservation {
        temperature: Decimal::from(90),
        valuation: Decimal::from(85),
        sentiment: Decimal::from(80),
        description: "hot".into(),
        timestamp: OffsetDateTime::UNIX_EPOCH,
    });
    let store = make_store(vec![], vec![]);
    let brain = BrainGraph::compute(&narrative, &dims, &links, &store);
    let mut ch = ConflictHistory::new();
    let insights = GraphInsights::compute(&brain, &store, None, &mut ch, 1);

    assert!(insights.stress.market_temperature_stress > Decimal::ZERO);
    assert!(insights.stress.composite_stress > Decimal::ZERO);
}
