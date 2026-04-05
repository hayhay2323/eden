use std::collections::HashMap;

use rust_decimal::Decimal;
use time::OffsetDateTime;

use crate::ontology::objects::Symbol;
use crate::pipeline::reasoning::ConvergenceDetail;

/// Per-edge accumulated learning signal from historical outcomes.
/// Runtime持有，跨tick累積。Profitable edges get amplified, losing edges dampened.
#[derive(Debug, Clone, Default)]
pub struct EdgeLearningLedger {
    entries: HashMap<String, EdgeCredit>,
}

#[derive(Debug, Clone)]
pub struct EdgeCredit {
    pub total_credit: Decimal,
    pub sample_count: u32,
    pub mean_credit: Decimal,
    pub last_updated: OffsetDateTime,
}

impl EdgeLearningLedger {
    /// Get the weight multiplier for a given edge. Range: [0.5, 1.5].
    /// Returns 1.0 (neutral) if no learning data exists.
    pub fn weight_multiplier(&self, edge_id: &str) -> Decimal {
        self.entries
            .get(edge_id)
            .map(|credit| {
                Decimal::ONE + credit.mean_credit.clamp(Decimal::new(-5, 1), Decimal::new(5, 1))
            })
            .unwrap_or(Decimal::ONE)
    }

    /// Credit edges based on a resolved outcome's convergence detail.
    ///
    /// Identifies the dominant convergence component and distributes credit
    /// to the corresponding edge type.
    pub fn credit_from_outcome(
        &mut self,
        _symbol: &Symbol,
        net_return: Decimal,
        detail: &ConvergenceDetail,
        now: OffsetDateTime,
        inst_edge_ids: &[String],
        stock_edge_ids: &[String],
        sector_edge_id: Option<&str>,
    ) {
        let inst_abs = detail.institutional_alignment.abs();
        let sector_abs = detail
            .sector_coherence
            .map(|v| v.abs())
            .unwrap_or(Decimal::ZERO);
        let cross_abs = detail.cross_stock_correlation.abs();
        let total_abs = inst_abs + sector_abs + cross_abs;

        if total_abs == Decimal::ZERO {
            return;
        }

        let (target_ids, contribution_ratio) = if inst_abs >= sector_abs && inst_abs >= cross_abs {
            (inst_edge_ids.to_vec(), inst_abs / total_abs)
        } else if cross_abs >= inst_abs && cross_abs >= sector_abs {
            (stock_edge_ids.to_vec(), cross_abs / total_abs)
        } else {
            (
                sector_edge_id
                    .map(|id| vec![id.to_string()])
                    .unwrap_or_default(),
                sector_abs / total_abs,
            )
        };

        let credit = net_return * contribution_ratio;
        for edge_id in target_ids {
            let entry = self.entries.entry(edge_id).or_insert(EdgeCredit {
                total_credit: Decimal::ZERO,
                sample_count: 0,
                mean_credit: Decimal::ZERO,
                last_updated: now,
            });
            entry.total_credit += credit;
            entry.sample_count += 1;
            entry.mean_credit = entry.total_credit / Decimal::from(entry.sample_count);
            entry.last_updated = now;
        }
    }

    /// Decay stale entries. Entries older than 7 days get credit reduced by 5%.
    /// Entries with negligible credit are removed.
    pub fn decay(&mut self, now: OffsetDateTime) {
        let cutoff = now - time::Duration::days(7);
        for entry in self.entries.values_mut() {
            if entry.last_updated < cutoff {
                entry.total_credit *= Decimal::new(95, 2);
                entry.mean_credit = if entry.sample_count > 0 {
                    entry.total_credit / Decimal::from(entry.sample_count)
                } else {
                    Decimal::ZERO
                };
            }
        }
        self.entries
            .retain(|_, entry| entry.total_credit.abs() >= Decimal::new(1, 3));
    }

    /// Number of edges with learning data.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// Build edge fingerprint strings for a symbol's edges in the BrainGraph.
pub fn edge_ids_for_symbol(
    symbol: &Symbol,
    brain: &crate::graph::graph::BrainGraph,
) -> (Vec<String>, Vec<String>, Option<String>) {
    use crate::graph::graph::{EdgeKind, NodeKind};
    use petgraph::visit::EdgeRef;
    use petgraph::Direction as GraphDirection;

    let Some(&stock_idx) = brain.stock_nodes.get(symbol) else {
        return (vec![], vec![], None);
    };

    let mut inst_ids = Vec::new();
    let mut stock_ids = Vec::new();
    let mut sector_id = None;

    for edge in brain
        .graph
        .edges_directed(stock_idx, GraphDirection::Incoming)
    {
        if let EdgeKind::InstitutionToStock(_) = edge.weight() {
            let source = edge.source();
            if let NodeKind::Institution(inst) = &brain.graph[source] {
                inst_ids.push(format!("inst:{}→stock:{}", inst.institution_id, symbol));
            }
        }
    }

    for edge in brain
        .graph
        .edges_directed(stock_idx, GraphDirection::Outgoing)
    {
        match edge.weight() {
            EdgeKind::StockToStock(_) => {
                let target = edge.target();
                if let NodeKind::Stock(neighbor) = &brain.graph[target] {
                    let mut pair = [symbol.to_string(), neighbor.symbol.to_string()];
                    pair.sort();
                    stock_ids.push(format!("stock:{}↔stock:{}", pair[0], pair[1]));
                }
            }
            EdgeKind::StockToSector(_) => {
                let target = edge.target();
                if let NodeKind::Sector(s) = &brain.graph[target] {
                    sector_id = Some(format!("stock:{}→sector:{}", symbol, s.sector_id));
                }
            }
            _ => {}
        }
    }

    (inst_ids, stock_ids, sector_id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_decimal_macros::dec;
    use time::OffsetDateTime;

    fn make_detail(inst: Decimal, sector: Decimal, cross: Decimal) -> ConvergenceDetail {
        ConvergenceDetail {
            institutional_alignment: inst,
            sector_coherence: Some(sector),
            cross_stock_correlation: cross,
            component_spread: None,
            edge_stability: None,
        }
    }

    #[test]
    fn credit_attribution_selects_dominant_component() {
        let mut ledger = EdgeLearningLedger::default();
        let symbol = Symbol("700.HK".into());
        let now = OffsetDateTime::now_utc();
        let detail = make_detail(dec!(0.6), dec!(0.2), dec!(0.1));
        ledger.credit_from_outcome(
            &symbol,
            dec!(0.05),
            &detail,
            now,
            &["inst:1→stock:700.HK".into()],
            &["stock:700.HK↔stock:388.HK".into()],
            Some("stock:700.HK→sector:tech"),
        );
        assert!(ledger.weight_multiplier("inst:1→stock:700.HK") > Decimal::ONE);
        assert_eq!(
            ledger.weight_multiplier("stock:700.HK↔stock:388.HK"),
            Decimal::ONE
        );
        assert_eq!(
            ledger.weight_multiplier("stock:700.HK→sector:tech"),
            Decimal::ONE
        );
    }

    #[test]
    fn weight_multiplier_positive_credit_amplifies() {
        let mut ledger = EdgeLearningLedger::default();
        ledger.entries.insert(
            "test_edge".into(),
            EdgeCredit {
                total_credit: dec!(0.3),
                sample_count: 1,
                mean_credit: dec!(0.3),
                last_updated: OffsetDateTime::now_utc(),
            },
        );
        assert_eq!(ledger.weight_multiplier("test_edge"), dec!(1.3));
    }

    #[test]
    fn weight_multiplier_negative_credit_dampens() {
        let mut ledger = EdgeLearningLedger::default();
        ledger.entries.insert(
            "test_edge".into(),
            EdgeCredit {
                total_credit: dec!(-0.3),
                sample_count: 1,
                mean_credit: dec!(-0.3),
                last_updated: OffsetDateTime::now_utc(),
            },
        );
        assert_eq!(ledger.weight_multiplier("test_edge"), dec!(0.7));
    }

    #[test]
    fn weight_multiplier_capped_at_50_pct() {
        let mut ledger = EdgeLearningLedger::default();
        ledger.entries.insert(
            "test_edge".into(),
            EdgeCredit {
                total_credit: dec!(0.9),
                sample_count: 1,
                mean_credit: dec!(0.9),
                last_updated: OffsetDateTime::now_utc(),
            },
        );
        assert_eq!(ledger.weight_multiplier("test_edge"), dec!(1.5));
    }

    #[test]
    fn decay_reduces_stale_entries() {
        let mut ledger = EdgeLearningLedger::default();
        let now = OffsetDateTime::now_utc();
        let old = now - time::Duration::days(8);
        ledger.entries.insert(
            "stale_edge".into(),
            EdgeCredit {
                total_credit: dec!(0.10),
                sample_count: 1,
                mean_credit: dec!(0.10),
                last_updated: old,
            },
        );
        ledger.decay(now);
        assert!(ledger.weight_multiplier("stale_edge") < dec!(1.10));
    }

    #[test]
    fn no_learning_data_returns_neutral() {
        let ledger = EdgeLearningLedger::default();
        assert_eq!(ledger.weight_multiplier("unknown_edge"), Decimal::ONE);
    }
}
