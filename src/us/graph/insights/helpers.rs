use super::*;

pub(super) fn average(values: impl IntoIterator<Item = Decimal>) -> Decimal {
    let v: Vec<Decimal> = values.into_iter().collect();
    if v.is_empty() {
        Decimal::ZERO
    } else {
        v.iter().copied().sum::<Decimal>() / Decimal::from(v.len() as i64)
    }
}

pub(super) fn std_dev(values: &[Decimal]) -> Decimal {
    if values.len() < 2 {
        return Decimal::ZERO;
    }
    let mean = values.iter().copied().sum::<Decimal>() / Decimal::from(values.len() as i64);
    let variance = values
        .iter()
        .map(|v| (*v - mean) * (*v - mean))
        .sum::<Decimal>()
        / Decimal::from(values.len() as i64);
    crate::math::decimal_sqrt(variance)
}

pub(super) fn median_decimal(mut values: Vec<Decimal>) -> Decimal {
    if values.is_empty() {
        return Decimal::ZERO;
    }
    values.sort();
    values[values.len() / 2]
}

pub(super) fn collect_sector_member_composites(
    graph: &UsGraph,
    sector_id: &SectorId,
    dims: &UsDimensionSnapshot,
) -> Vec<Decimal> {
    let &sector_idx = match graph.sector_nodes.get(sector_id) {
        Some(idx) => idx,
        None => return Vec::new(),
    };

    graph
        .graph
        .edges_directed(sector_idx, GraphDirection::Incoming)
        .filter_map(|edge| {
            if let UsEdgeKind::StockToSector(_) = edge.weight() {
                if let UsNodeKind::Stock(s) = &graph.graph[edge.source()] {
                    if let Some(d) = dims.dimensions.get(&s.symbol) {
                        let composite = average([
                            d.capital_flow_direction,
                            d.price_momentum,
                            d.volume_profile,
                            d.pre_post_market_anomaly,
                            d.valuation,
                        ]);
                        return Some(composite);
                    }
                }
            }
            None
        })
        .collect()
}

pub(super) fn canonical_sector_key(a: SectorId, b: SectorId) -> (SectorId, SectorId) {
    if a.0 <= b.0 {
        (a, b)
    } else {
        (b, a)
    }
}

pub(super) fn find(parent: &mut Vec<usize>, mut x: usize) -> usize {
    while parent[x] != x {
        parent[x] = parent[parent[x]];
        x = parent[x];
    }
    x
}

pub(super) fn union(parent: &mut Vec<usize>, a: usize, b: usize) {
    let ra = find(parent, a);
    let rb = find(parent, b);
    if ra != rb {
        parent[ra] = rb;
    }
}
