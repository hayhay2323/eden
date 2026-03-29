use super::*;

pub(super) fn compute_rotations(
    graph: &UsGraph,
    dims: &UsDimensionSnapshot,
    prev: Option<&UsGraphInsights>,
) -> Vec<UsSectorRotation> {
    let prev_map: HashMap<(SectorId, SectorId), Decimal> = prev
        .map(|p| {
            p.rotations
                .iter()
                .map(|r| {
                    let key = canonical_sector_key(r.sector_a.clone(), r.sector_b.clone());
                    (key, r.spread)
                })
                .collect()
        })
        .unwrap_or_default();

    let sectors: Vec<(SectorId, Decimal)> = graph
        .sector_nodes
        .iter()
        .filter_map(|(sid, &idx)| {
            if let UsNodeKind::Sector(_) = &graph.graph[idx] {
                let member_composites = collect_sector_member_composites(graph, sid, dims);
                if member_composites.is_empty() {
                    None
                } else {
                    let mean = average(member_composites);
                    Some((sid.clone(), mean))
                }
            } else {
                None
            }
        })
        .collect();

    if sectors.len() < 2 {
        return Vec::new();
    }

    let mut all_pairs: Vec<(usize, usize, Decimal)> = Vec::new();
    for i in 0..sectors.len() {
        for j in (i + 1)..sectors.len() {
            let spread = (sectors[i].1 - sectors[j].1).abs();
            all_pairs.push((i, j, spread));
        }
    }

    let abs_spreads: Vec<Decimal> = all_pairs.iter().map(|(_, _, s)| *s).collect();
    let median = median_decimal(abs_spreads);

    let mut results = Vec::new();
    for (i, j, spread) in &all_pairs {
        if *spread <= median {
            continue;
        }

        let (sector_a, sector_b) = if sectors[*i].1 >= sectors[*j].1 {
            (sectors[*i].0.clone(), sectors[*j].0.clone())
        } else {
            (sectors[*j].0.clone(), sectors[*i].0.clone())
        };

        let key = canonical_sector_key(sector_a.clone(), sector_b.clone());
        let prev_spread = prev_map.get(&key).copied().unwrap_or(*spread);
        let spread_delta = *spread - prev_spread;
        let widening = spread_delta > Decimal::ZERO;

        results.push(UsSectorRotation {
            sector_a,
            sector_b,
            spread: *spread,
            spread_delta,
            widening,
        });
    }

    results.sort_by(|a, b| b.spread.cmp(&a.spread));
    results
}
