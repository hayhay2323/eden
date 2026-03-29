use std::collections::HashMap;

use petgraph::visit::EdgeRef;
use petgraph::Direction as GraphDirection;
use rust_decimal::Decimal;

use crate::math::{clamp_unit_interval, median};
use crate::ontology::objects::{SectorId, Symbol};
use crate::graph::graph::{BrainGraph, EdgeKind, NodeKind};

use super::{GraphInsights, InstitutionalConflict, MarketStressIndex, RotationPair, StockPressure};

pub(super) fn average(values: impl IntoIterator<Item = Decimal>) -> Decimal {
    let values = values.into_iter().collect::<Vec<_>>();
    if values.is_empty() {
        Decimal::ZERO
    } else {
        values.iter().copied().sum::<Decimal>() / Decimal::from(values.len() as i64)
    }
}

pub(super) fn compute_pressures(brain: &BrainGraph, prev: Option<&GraphInsights>) -> Vec<StockPressure> {
    // Build prev pressure map
    let prev_map: HashMap<&Symbol, &StockPressure> = prev
        .map(|p| p.pressures.iter().map(|sp| (&sp.symbol, sp)).collect())
        .unwrap_or_default();

    let mut results = Vec::new();

    for (symbol, &stock_idx) in &brain.stock_nodes {
        let mut weighted_sum = Decimal::ZERO;
        let mut weight_total = Decimal::ZERO;
        let mut buy_count = 0usize;
        let mut sell_count = 0usize;
        let mut inst_count = 0usize;

        for edge in brain
            .graph
            .edges_directed(stock_idx, GraphDirection::Incoming)
        {
            if let EdgeKind::InstitutionToStock(e) = edge.weight() {
                let source = edge.source();
                if let NodeKind::Institution(inst) = &brain.graph[source] {
                    // Weight by breadth: institutions in more stocks have broader signal
                    let w = Decimal::from(inst.stock_count as i64);
                    weighted_sum += e.direction * w;
                    weight_total += w;
                    inst_count += 1;
                    if e.direction > Decimal::ZERO {
                        buy_count += 1;
                    } else if e.direction < Decimal::ZERO {
                        sell_count += 1;
                    }
                }
            }
        }

        if inst_count == 0 {
            continue;
        }

        let net_pressure = if weight_total > Decimal::ZERO {
            weighted_sum / weight_total
        } else {
            Decimal::ZERO
        };

        // Temporal: delta, duration, acceleration
        let (pressure_delta, pressure_duration, accelerating) =
            if let Some(prev_p) = prev_map.get(symbol) {
                let delta = net_pressure - prev_p.net_pressure;
                let prev_delta = prev_p.pressure_delta;
                // Same direction: delta and prev pressure have same sign
                let same_dir = (delta > Decimal::ZERO && prev_p.net_pressure > Decimal::ZERO)
                    || (delta < Decimal::ZERO && prev_p.net_pressure < Decimal::ZERO)
                    || delta == Decimal::ZERO;
                let duration = if same_dir {
                    prev_p.pressure_duration + 1
                } else {
                    1
                };
                let accelerating = delta.abs() > prev_delta.abs();
                (delta, duration, accelerating)
            } else {
                (Decimal::ZERO, 1, false)
            };

        results.push(StockPressure {
            symbol: symbol.clone(),
            net_pressure,
            institution_count: inst_count,
            buy_inst_count: buy_count,
            sell_inst_count: sell_count,
            pressure_delta,
            pressure_duration,
            accelerating,
        });
    }

    results.sort_by(|a, b| b.net_pressure.abs().cmp(&a.net_pressure.abs()));
    results
}

pub(super) fn compute_rotations(brain: &BrainGraph, prev: Option<&GraphInsights>) -> Vec<RotationPair> {
    // Build prev rotation map: (from, to) → spread
    let prev_map: HashMap<(&SectorId, &SectorId), Decimal> = prev
        .map(|p| {
            p.rotations
                .iter()
                .map(|r| ((&r.from_sector, &r.to_sector), r.spread))
                .collect()
        })
        .unwrap_or_default();

    // Collect sector directions
    let sectors: Vec<(SectorId, Decimal)> = brain
        .sector_nodes
        .iter()
        .filter_map(|(sid, &idx)| {
            if let NodeKind::Sector(s) = &brain.graph[idx] {
                Some((sid.clone(), s.mean_direction))
            } else {
                None
            }
        })
        .collect();

    if sectors.len() < 2 {
        return Vec::new();
    }

    // Compute all pairwise spreads
    let mut all_spreads: Vec<(usize, usize, Decimal)> = Vec::new();
    for i in 0..sectors.len() {
        for j in (i + 1)..sectors.len() {
            let spread = sectors[i].1 - sectors[j].1;
            all_spreads.push((i, j, spread));
        }
    }

    // Median absolute spread as cutoff
    let median =
        median(all_spreads.iter().map(|(_, _, s)| s.abs()).collect()).unwrap_or(Decimal::ZERO);

    // Emit pairs above median
    let mut results = Vec::new();
    for (i, j, spread) in &all_spreads {
        if spread.abs() <= median {
            continue;
        }
        // from = higher direction, to = lower direction
        let (from, to) = if *spread > Decimal::ZERO {
            (&sectors[*i], &sectors[*j])
        } else {
            (&sectors[*j], &sectors[*i])
        };

        // Temporal: spread_delta
        let prev_spread = prev_map
            .get(&(&from.0, &to.0))
            .copied()
            .unwrap_or(spread.abs());
        let spread_delta = spread.abs() - prev_spread;
        let widening = spread_delta > Decimal::ZERO;

        results.push(RotationPair {
            from_sector: from.0.clone(),
            to_sector: to.0.clone(),
            spread: spread.abs(),
            spread_delta,
            widening,
        });
    }

    results.sort_by(|a, b| b.spread.cmp(&a.spread));
    results
}

pub(super) fn compute_stress_index(
    brain: &BrainGraph,
    pressures: &[StockPressure],
    conflicts: &[InstitutionalConflict],
) -> MarketStressIndex {
    // 1. Sector synchrony: low std of sector directions = all sectors moving together = stress
    let sector_dirs: Vec<Decimal> = brain
        .sector_nodes
        .values()
        .filter_map(|&idx| {
            if let NodeKind::Sector(s) = &brain.graph[idx] {
                Some(s.mean_direction)
            } else {
                None
            }
        })
        .collect();
    let sector_synchrony = if sector_dirs.len() < 2 {
        Decimal::ZERO
    } else {
        let mean = sector_dirs.iter().sum::<Decimal>() / Decimal::from(sector_dirs.len() as i64);
        let variance = sector_dirs
            .iter()
            .map(|d| (*d - mean) * (*d - mean))
            .sum::<Decimal>()
            / Decimal::from(sector_dirs.len() as i64);
        // synchrony = 1 - std (high when sectors move together)
        (Decimal::ONE - crate::math::decimal_sqrt(variance))
            .max(Decimal::ZERO)
            .min(Decimal::ONE)
    };

    // 2. Pressure consensus: low std of stock pressures = unusual agreement
    let pressure_consensus = if pressures.len() < 2 {
        Decimal::ZERO
    } else {
        let mean = pressures.iter().map(|p| p.net_pressure).sum::<Decimal>()
            / Decimal::from(pressures.len() as i64);
        let variance = pressures
            .iter()
            .map(|p| (p.net_pressure - mean) * (p.net_pressure - mean))
            .sum::<Decimal>()
            / Decimal::from(pressures.len() as i64);
        (Decimal::ONE - crate::math::decimal_sqrt(variance))
            .max(Decimal::ZERO)
            .min(Decimal::ONE)
    };

    // 3. Conflict intensity mean
    let conflict_intensity_mean = if conflicts.is_empty() {
        Decimal::ZERO
    } else {
        let sum: Decimal = conflicts
            .iter()
            .map(|c| (c.direction_a - c.direction_b).abs())
            .sum();
        sum / Decimal::from(conflicts.len() as i64)
    };

    let market_temperature_stress = brain
        .market_temperature
        .as_ref()
        .map(|temp| {
            let temperature_bias = (temp.temperature - Decimal::from(50)).abs() / Decimal::from(50);
            let valuation_bias = (temp.valuation - Decimal::from(50)).abs() / Decimal::from(50);
            let sentiment_bias = (temp.sentiment - Decimal::from(50)).abs() / Decimal::from(50);
            clamp_unit_interval(
                (temperature_bias + valuation_bias + sentiment_bias) / Decimal::from(3),
            )
        })
        .unwrap_or(Decimal::ZERO);

    let conflict_component = clamp_unit_interval(conflict_intensity_mean / Decimal::TWO);
    let composite_stress = average([
        sector_synchrony,
        pressure_consensus,
        conflict_component,
        market_temperature_stress,
    ]);

    MarketStressIndex {
        sector_synchrony,
        pressure_consensus,
        conflict_intensity_mean,
        market_temperature_stress,
        composite_stress,
    }
}
