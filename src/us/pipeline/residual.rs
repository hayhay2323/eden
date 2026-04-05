//! US-specific residual field computation.
//!
//! Adapts the shared ResidualField infrastructure (from `pipeline::residual`)
//! to work with US-specific types (UsConvergenceScore, UsSymbolDimensions).

use std::collections::HashMap;

use rust_decimal::prelude::Signed;
use rust_decimal::Decimal;

use crate::ontology::objects::{SectorId, Symbol};
use crate::pipeline::residual::{
    ResidualDimension, ResidualField, SectorResidualCluster, SymbolResidual,
};
use crate::us::graph::decision::UsConvergenceScore;

/// Compute the US residual field from US-specific convergence scores and quote data.
///
/// Since US doesn't have HK's BrainGraph with explicit stock-to-stock edges,
/// we use convergence_scores + sector_coherence + quote deltas to infer expected behavior.
pub fn compute_us_residual_field(
    convergence_scores: &HashMap<Symbol, UsConvergenceScore>,
    quotes: &[crate::ontology::links::QuoteObservation],
) -> ResidualField {
    // Compute per-symbol price delta from quotes
    let stock_deltas: HashMap<Symbol, Decimal> = quotes
        .iter()
        .filter(|q| q.prev_close > Decimal::ZERO)
        .map(|q| {
            let delta = (q.last_done - q.prev_close) / q.prev_close;
            (q.symbol.clone(), delta)
        })
        .collect();

    // Compute sector expected deltas
    let sector_expected = compute_sector_expected(&stock_deltas);

    let mut residuals = Vec::new();

    for (symbol, convergence) in convergence_scores {
        let observed_delta = stock_deltas.get(symbol).copied().unwrap_or(Decimal::ZERO);
        let sector = crate::ontology::store::symbol_sector(&symbol.0);

        // 1. Convergence residual: composite vs sector expectation
        let expected_convergence = convergence
            .sector_coherence
            .map(|sc| sc * Decimal::new(5, 1))
            .unwrap_or(Decimal::ZERO);
        let convergence_residual = convergence.composite - expected_convergence;

        // 2. Price residual: actual delta vs sector mean × correlation
        let sector_delta = sector
            .as_ref()
            .and_then(|s| sector_expected.get(s))
            .copied()
            .unwrap_or(Decimal::ZERO);
        let cross_factor = convergence
            .cross_stock_correlation
            .abs()
            .max(Decimal::new(1, 1));
        let expected_price = sector_delta * cross_factor;
        let price_residual = observed_delta - expected_price;

        // 3. Flow residual: capital_flow_direction vs dimension_composite direction
        let flow_residual =
            convergence.capital_flow_direction - convergence.dimension_composite * Decimal::new(5, 1);

        // 4. Cross-market residual (US-specific): actual vs cross-market propagation
        let cross_market_residual = convergence
            .cross_market_propagation
            .map(|prop| {
                // If HK propagation says bullish but US is bearish, that's a residual
                observed_delta.signum() * Decimal::new(5, 1) - prop
            })
            .unwrap_or(Decimal::ZERO);

        let components = [
            convergence_residual,
            price_residual,
            flow_residual,
            cross_market_residual,
        ];
        let magnitude = approx_l2_norm(&components);
        let dominant_dimension = dominant_dim(&components);
        let net_direction: Decimal = components.iter().sum();

        if magnitude > Decimal::new(5, 2) {
            residuals.push(SymbolResidual {
                symbol: symbol.clone(),
                sector,
                convergence_residual,
                price_residual,
                flow_residual,
                institutional_residual: cross_market_residual, // US uses cross-market instead of institutional
                magnitude,
                dominant_dimension,
                net_direction,
            });
        }
    }

    residuals.sort_by(|a, b| b.magnitude.cmp(&a.magnitude));

    let clustered_sectors = detect_us_sector_clusters(&residuals);

    // US doesn't have BrainGraph for divergent pair detection,
    // use simple unconnected-sector heuristic
    let divergent_pairs = detect_us_divergent_pairs(&residuals);

    ResidualField {
        residuals,
        clustered_sectors,
        divergent_pairs: divergent_pairs,
    }
}

fn compute_sector_expected(
    stock_deltas: &HashMap<Symbol, Decimal>,
) -> HashMap<SectorId, Decimal> {
    let mut sums: HashMap<SectorId, (Decimal, usize)> = HashMap::new();
    for (symbol, delta) in stock_deltas {
        if let Some(sector) = crate::ontology::store::symbol_sector(&symbol.0) {
            let e = sums.entry(sector).or_insert((Decimal::ZERO, 0));
            e.0 += delta;
            e.1 += 1;
        }
    }
    sums.into_iter()
        .map(|(s, (sum, count))| {
            (
                s,
                if count > 0 {
                    sum / Decimal::from(count as i64)
                } else {
                    Decimal::ZERO
                },
            )
        })
        .collect()
}

fn detect_us_sector_clusters(residuals: &[SymbolResidual]) -> Vec<SectorResidualCluster> {
    let mut by_sector: HashMap<&SectorId, Vec<&SymbolResidual>> = HashMap::new();
    for r in residuals {
        if let Some(sector) = &r.sector {
            by_sector.entry(sector).or_default().push(r);
        }
    }

    let mut clusters = Vec::new();
    for (sector, items) in &by_sector {
        if items.len() < 2 {
            continue;
        }
        let mean_net: Decimal =
            items.iter().map(|r| r.net_direction).sum::<Decimal>() / Decimal::from(items.len() as i64);
        let same_sign = items
            .iter()
            .filter(|r| (r.net_direction > Decimal::ZERO) == (mean_net > Decimal::ZERO))
            .count();
        let coherence = Decimal::from(same_sign as i64) / Decimal::from(items.len() as i64);

        let mut dim_counts = [0usize; 4];
        for r in items {
            match r.dominant_dimension {
                ResidualDimension::Convergence => dim_counts[0] += 1,
                ResidualDimension::Price => dim_counts[1] += 1,
                ResidualDimension::Flow => dim_counts[2] += 1,
                ResidualDimension::Institutional => dim_counts[3] += 1,
            }
        }
        let dims = [
            ResidualDimension::Convergence,
            ResidualDimension::Price,
            ResidualDimension::Flow,
            ResidualDimension::Institutional,
        ];
        let dominant = dims[dim_counts.iter().enumerate().max_by_key(|(_, c)| *c).unwrap().0];

        if coherence >= Decimal::new(6, 1) {
            clusters.push(SectorResidualCluster {
                sector: (*sector).clone(),
                mean_residual: mean_net,
                symbol_count: items.len(),
                coherence,
                dominant_dimension: dominant,
            });
        }
    }
    clusters.sort_by(|a, b| b.symbol_count.cmp(&a.symbol_count));
    clusters
}

fn detect_us_divergent_pairs(
    residuals: &[SymbolResidual],
) -> Vec<crate::pipeline::residual::ResidualDivergence> {
    let mut pairs = Vec::new();
    let top = &residuals[..residuals.len().min(20)];

    for i in 0..top.len() {
        for j in (i + 1)..top.len() {
            let a = &top[i];
            let b = &top[j];
            if (a.net_direction > Decimal::ZERO) == (b.net_direction > Decimal::ZERO) {
                continue;
            }
            // For US, only flag cross-sector divergences (same sector is expected)
            if a.sector == b.sector && a.sector.is_some() {
                continue;
            }
            let strength = (a.net_direction - b.net_direction).abs();
            if strength >= Decimal::new(2, 1) {
                let (pos, neg) = if a.net_direction > b.net_direction {
                    (a, b)
                } else {
                    (b, a)
                };
                pairs.push(crate::pipeline::residual::ResidualDivergence {
                    symbol_a: pos.symbol.clone(),
                    symbol_b: neg.symbol.clone(),
                    residual_a: pos.net_direction,
                    residual_b: neg.net_direction,
                    divergence_strength: strength,
                });
            }
        }
    }
    pairs.sort_by(|a, b| b.divergence_strength.cmp(&a.divergence_strength));
    pairs.truncate(10);
    pairs
}

fn approx_l2_norm(components: &[Decimal]) -> Decimal {
    let l1: Decimal = components.iter().map(|c| c.abs()).sum();
    l1 * Decimal::new(5, 1)
}

fn dominant_dim(components: &[Decimal]) -> ResidualDimension {
    let dims = [
        ResidualDimension::Convergence,
        ResidualDimension::Price,
        ResidualDimension::Flow,
        ResidualDimension::Institutional,
    ];
    let max_idx = components
        .iter()
        .enumerate()
        .max_by_key(|(_, v)| v.abs())
        .map(|(i, _)| i)
        .unwrap_or(0);
    dims[max_idx]
}
