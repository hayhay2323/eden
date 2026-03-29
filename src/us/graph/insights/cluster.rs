use super::*;

pub(super) fn compute_clusters(
    graph: &UsGraph,
    dims: &UsDimensionSnapshot,
    prev: Option<&UsGraphInsights>,
) -> Vec<UsStockCluster> {
    let stock_syms: Vec<Symbol> = graph.stock_nodes.keys().cloned().collect();
    let n = stock_syms.len();
    if n == 0 {
        return Vec::new();
    }

    let sym_to_local: HashMap<&Symbol, usize> =
        stock_syms.iter().enumerate().map(|(i, s)| (s, i)).collect();

    let mut parent: Vec<usize> = (0..n).collect();

    for (symbol, &node_idx) in &graph.stock_nodes {
        let i = sym_to_local[symbol];
        for edge in graph
            .graph
            .edges_directed(node_idx, GraphDirection::Outgoing)
        {
            if let UsEdgeKind::StockToStock(_) = edge.weight() {
                if let UsNodeKind::Stock(neighbor) = &graph.graph[edge.target()] {
                    if let Some(&j) = sym_to_local.get(&neighbor.symbol) {
                        union(&mut parent, i, j);
                    }
                }
            }
        }
    }

    let mut components: HashMap<usize, Vec<usize>> = HashMap::new();
    for i in 0..n {
        let root = find(&mut parent, i);
        components.entry(root).or_default().push(i);
    }

    let prev_clusters: Vec<HashSet<&Symbol>> = prev
        .map(|p| {
            p.clusters
                .iter()
                .map(|c| c.members.iter().collect::<HashSet<_>>())
                .collect()
        })
        .unwrap_or_default();
    let prev_cluster_ages: Vec<u64> = prev
        .map(|p| p.clusters.iter().map(|c| c.age).collect())
        .unwrap_or_default();

    let mut results = Vec::new();

    for members in components.values() {
        if members.len() < 2 {
            continue;
        }

        let member_syms: Vec<Symbol> = members.iter().map(|&i| stock_syms[i].clone()).collect();

        let directions: Vec<Decimal> = member_syms
            .iter()
            .filter_map(|sym| dims.dimensions.get(sym).map(|d| d.price_momentum))
            .collect();

        if directions.is_empty() {
            continue;
        }

        let positive = directions.iter().filter(|&&d| d > Decimal::ZERO).count();
        let negative = directions.iter().filter(|&&d| d < Decimal::ZERO).count();
        let majority = positive.max(negative);
        let directional_alignment =
            Decimal::from(majority as i64) / Decimal::from(directions.len() as i64);

        if directional_alignment < Decimal::new(6, 1) {
            continue;
        }

        let current_set: HashSet<&Symbol> = member_syms.iter().collect();
        let (stability, matched_age) = if prev_clusters.is_empty() {
            (Decimal::ZERO, 0u64)
        } else {
            let mut best_jaccard = Decimal::ZERO;
            let mut best_age = 0u64;
            for (idx, prev_set) in prev_clusters.iter().enumerate() {
                let intersection = current_set.intersection(prev_set).count();
                let union_size = current_set.union(prev_set).count();
                if union_size > 0 {
                    let j = Decimal::from(intersection as i64) / Decimal::from(union_size as i64);
                    if j > best_jaccard {
                        best_jaccard = j;
                        best_age = prev_cluster_ages[idx];
                    }
                }
            }
            (best_jaccard, best_age)
        };

        let age = if stability > Decimal::new(5, 1) {
            matched_age + 1
        } else {
            1
        };

        if age < 3 && prev.is_some() {
            continue;
        }

        results.push(UsStockCluster {
            members: member_syms,
            directional_alignment,
            stability,
            age,
        });
    }

    results.sort_by(|a, b| b.members.len().cmp(&a.members.len()));
    results
}
